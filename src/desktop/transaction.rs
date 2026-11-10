//! Compare-and-swap file changes with cross-process locks and protected undo.
//! Snapshots can contain credentials, so they use the existing OS secret store.
use crate::nvwa::secrets::SecretStore;
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const CHUNK_BYTES: usize = 48 * 1024;

pub fn fingerprint(bytes: Option<&[u8]>) -> String {
    bytes
        .map(|v| hex::encode(Sha256::digest(v)))
        .unwrap_or_else(|| "missing".into())
}

pub fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    reject_links(path)?;
    let file = match File::open(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("无法读取配置，请检查文件权限"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_FILE_BYTES,
        "文件超过 32 MiB 的安全读取上限；请在“查看配置文件”核对是否选中了日志或缓存，原文件未修改"
    );
    Ok(Some(bytes))
}

pub fn reject_links(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && !path.components().any(|v| matches!(v, Component::ParentDir)),
        "配置路径必须是完整路径"
    );
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "配置路径包含链接，请选择实际目录"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        meta.file_attributes() & 0x400 == 0,
                        "配置路径包含重解析点，请选择实际目录"
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    reject_links(path)?;
    ensure!(bytes.len() <= MAX_FILE_BYTES, "配置文件过大");
    let parent = path.parent().context("配置路径缺少目录")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    super::permissions::owner_only(temp.path())?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?.sync_all()?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: PathBuf,
    pub expected: String,
    pub after: Option<Vec<u8>>,
}

impl FileChange {
    pub fn plan(path: PathBuf, before: Option<&[u8]>, after: Option<Vec<u8>>) -> Self {
        Self {
            path,
            expected: fingerprint(before),
            after,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct UndoFile {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct Undo {
    version: u32,
    files: Vec<UndoFile>,
}

pub struct TransactionStore {
    root: PathBuf,
    secrets: SecretStore,
}

impl TransactionStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        reject_links(&root)?;
        fs::create_dir_all(root.join("locks"))?;
        Ok(Self {
            secrets: SecretStore::new(root.join("protected-undo"))?,
            root,
        })
    }
    fn locks(&self, paths: &[PathBuf]) -> Result<Vec<File>> {
        let mut names: Vec<_> = paths
            .iter()
            .map(|v| {
                // Windows paths with different case refer to the same lock.
                let name = v.to_string_lossy().to_string();
                let name = if cfg!(windows) {
                    name.to_lowercase()
                } else {
                    name
                };
                hex::encode(Sha256::digest(name.as_bytes()))
            })
            .collect();
        names.sort();
        names.dedup();
        let mut locks = Vec::new();
        for name in names {
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(self.root.join("locks").join(name))?;
            lock.lock_exclusive()?;
            locks.push(lock);
        }
        Ok(locks)
    }
    pub fn apply(&self, changes: Vec<FileChange>) -> Result<String> {
        self.apply_as(&uuid::Uuid::new_v4().to_string(), changes)
    }
    pub fn apply_as(&self, id: &str, changes: Vec<FileChange>) -> Result<String> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "更改身份无效");
        ensure!(
            !changes.is_empty() && changes.len() <= 2056,
            "没有可应用的配置更改或文件过多"
        );
        let paths: Vec<_> = changes.iter().map(|v| v.path.clone()).collect();
        let _locks = self.locks(&paths)?;
        let mut undo = Undo {
            version: 1,
            files: Vec::new(),
        };
        let mut seen = std::collections::HashSet::new();
        for change in &changes {
            ensure!(
                seen.insert(if cfg!(windows) {
                    change.path.to_string_lossy().to_lowercase()
                } else {
                    change.path.to_string_lossy().into_owned()
                }),
                "同一物理配置不能重复写入"
            );
            let before = read(&change.path)?;
            ensure!(
                fingerprint(before.as_deref()) == change.expected,
                "配置已在其他地方修改，请重新读取后再试"
            );
            undo.files.push(UndoFile {
                path: change.path.clone(),
                before,
                after: fingerprint(change.after.as_deref()),
            });
        }
        // Write the undo record before touching any client file. A crash leaves
        // a recoverable record; restore still checks the actual after-image.
        self.save_undo(id, &undo)?;
        for (index, change) in changes.iter().enumerate() {
            let result = (|| -> Result<()> {
                ensure!(
                    fingerprint(read(&change.path)?.as_deref()) == change.expected,
                    "客户端刚刚修改了配置，请重新读取"
                );
                replace(&change.path, change.after.as_deref())?;
                ensure!(
                    fingerprint(read(&change.path)?.as_deref()) == undo.files[index].after,
                    "客户端在保存期间修改了配置，请核对后重试"
                );
                Ok(())
            })();
            if let Err(error) = result {
                let mut restored = true;
                for previous in undo.files[..=index].iter().rev() {
                    let current = read(&previous.path)?;
                    if fingerprint(current.as_deref()) == previous.after {
                        restored &= replace(&previous.path, previous.before.as_deref()).is_ok();
                    } else if current != previous.before {
                        restored = false;
                    }
                }
                if !restored {
                    bail!("{error}；部分文件已发生后续修改，未覆盖它们。恢复记录：{id}");
                }
                return Err(error);
            }
        }
        Ok(id.to_owned())
    }
    fn save_undo(&self, id: &str, undo: &Undo) -> Result<()> {
        let bytes = serde_json::to_vec(undo)?;
        ensure!(bytes.len() <= MAX_FILE_BYTES * 4, "更改过大，请分批使用");
        let count = bytes.len().div_ceil(CHUNK_BYTES);
        for (i, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            self.secrets.set(&format!("desktop-undo:{id}:{i}"), chunk)?;
        }
        self.secrets
            .set(&format!("desktop-undo:{id}"), count.to_string().as_bytes())?;
        Ok(())
    }
    pub fn restore(&self, id: &str) -> Result<()> {
        self.apply(self.restore_plan(id)?)?;
        Ok(())
    }
    pub fn restore_plan(&self, id: &str) -> Result<Vec<FileChange>> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "恢复记录无效");
        let count = self
            .secrets
            .get(&format!("desktop-undo:{id}"))?
            .context("恢复记录不存在")?;
        let count: usize = std::str::from_utf8(&count)?.parse()?;
        ensure!(
            count > 0 && count <= (MAX_FILE_BYTES * 4).div_ceil(CHUNK_BYTES),
            "恢复记录损坏"
        );
        let mut bytes = Vec::new();
        for i in 0..count {
            bytes.extend(
                self.secrets
                    .get(&format!("desktop-undo:{id}:{i}"))?
                    .context("恢复记录不完整")?,
            );
        }
        let undo: Undo = serde_json::from_slice(&bytes)?;
        ensure!(undo.version == 1, "恢复记录版本不受支持");
        undo.files
            .into_iter()
            .map(|v| {
                let current = read(&v.path)?;
                ensure!(
                    fingerprint(current.as_deref()) == v.after || current == v.before,
                    "客户端已在更改后被独立修改，未覆盖"
                );
                Ok(FileChange {
                    path: v.path,
                    expected: fingerprint(current.as_deref()),
                    after: v.before,
                })
            })
            .collect()
    }
    pub fn put_record<T: Serialize>(&self, id: &str, value: &T) -> Result<()> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "操作身份无效");
        let bytes = serde_json::to_vec(value)?;
        ensure!(bytes.len() <= MAX_FILE_BYTES * 4, "操作记录过大");
        let count = bytes.len().div_ceil(CHUNK_BYTES);
        for (i, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            self.secrets
                .set(&format!("desktop-record:{id}:{i}"), chunk)?;
        }
        self.secrets.set(
            &format!("desktop-record:{id}"),
            count.to_string().as_bytes(),
        )?;
        Ok(())
    }
    pub fn get_record<T: serde::de::DeserializeOwned>(&self, id: &str) -> Result<T> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "操作身份无效");
        let count = self
            .secrets
            .get(&format!("desktop-record:{id}"))?
            .context("操作恢复记录不存在")?;
        let count: usize = std::str::from_utf8(&count)?.parse()?;
        ensure!(
            count > 0 && count <= (MAX_FILE_BYTES * 4).div_ceil(CHUNK_BYTES),
            "操作恢复记录损坏"
        );
        let mut bytes = Vec::new();
        for i in 0..count {
            bytes.extend(
                self.secrets
                    .get(&format!("desktop-record:{id}:{i}"))?
                    .context("操作恢复记录不完整")?,
            );
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
}

fn replace(path: &Path, bytes: Option<&[u8]>) -> Result<()> {
    match bytes {
        Some(bytes) => atomic_write(path, bytes),
        None => {
            reject_links(path)?;
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    #[test]
    fn transaction_conflict_and_protected_restore() {
        let dir = tempfile::tempdir().unwrap();
        let store = TransactionStore::new(dir.path().join("state")).unwrap();
        let path = dir.path().join("client.json");
        fs::write(&path, b"before").unwrap();
        let stale = FileChange::plan(path.clone(), Some(b"old"), Some(b"bad".to_vec()));
        assert!(store.apply(vec![stale]).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"before");
        let id = store
            .apply(vec![FileChange::plan(
                path.clone(),
                Some(b"before"),
                Some(b"after".to_vec()),
            )])
            .unwrap();
        fs::write(&path, b"user-change").unwrap();
        assert!(store.restore(&id).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"user-change");
        fs::write(&path, b"after").unwrap();
        store.restore(&id).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"before");
    }
}
