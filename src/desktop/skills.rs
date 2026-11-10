use super::transaction;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 2048;
const MAX_TOTAL: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
}

pub struct SkillPackage {
    pub metadata: SkillMetadata,
    pub files: BTreeMap<PathBuf, Vec<u8>>,
    pub fingerprint: String,
}

pub fn metadata(bytes: &[u8]) -> Result<SkillMetadata> {
    ensure!(bytes.len() <= 256 * 1024, "SKILL.md 过大");
    let text = std::str::from_utf8(bytes)
        .context("SKILL.md 需要使用 UTF-8")?
        .trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    ensure!(
        lines.next() == Some("---"),
        "SKILL.md 缺少名称和说明，请检查开头的 YAML 元数据"
    );
    let mut header = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line == "---" {
            closed = true;
            break;
        }
        header.push(line);
    }
    ensure!(closed, "SKILL.md 的 YAML 元数据缺少结束的 ---");
    let header = header.join("\n");
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&header).context("SKILL.md 元数据无效")?;
    let name = value
        .get("name")
        .and_then(serde_yaml_ng::Value::as_str)
        .context("SKILL.md 缺少 name")?
        .trim();
    ensure!(
        !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control),
        "Skill 名称无效"
    );
    let description = value
        .get("description")
        .and_then(serde_yaml_ng::Value::as_str)
        .unwrap_or("")
        .trim();
    Ok(SkillMetadata {
        name: name.into(),
        description: description.into(),
    })
}

pub fn from_directory(path: &Path) -> Result<SkillPackage> {
    transaction::reject_links(path)?;
    ensure!(path.is_dir(), "请选择包含 SKILL.md 的文件夹");
    let mut files = BTreeMap::new();
    let mut pending = vec![path.to_path_buf()];
    let mut total = 0;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).context("无法读取 Skill 目录")? {
            let entry = entry?;
            let current = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            transaction::reject_links(&current)?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(current);
                ensure!(pending.len() <= MAX_FILES, "Skill 子目录过多");
            } else if kind.is_file() {
                let bytes = transaction::read(&current)?.context("Skill 文件在导入期间被移除")?;
                total += bytes.len();
                ensure!(
                    files.len() < MAX_FILES && total <= MAX_TOTAL,
                    "Skill 文件超过 2048 个或完整内容超过 64 MiB，原目录未修改"
                );
                let relative = current.strip_prefix(path)?.to_path_buf();
                ensure!(relative.components().count() <= 32, "Skill 目录层级过深");
                files.insert(relative, bytes);
            } else {
                bail!("Skill 包含特殊文件，未导入");
            }
        }
    }
    package(files)
}

pub fn from_zip(path: &Path, subdirectory: Option<&str>) -> Result<SkillPackage> {
    let bytes = transaction::read(path)?.context("ZIP 文件不存在")?;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("不是有效的 ZIP 文件")?;
    ensure!(archive.len() <= MAX_FILES * 2, "ZIP 条目过多");
    let mut files = BTreeMap::new();
    let mut total = 0;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let relative = file.enclosed_name().context("ZIP 包含越界路径，未导入")?;
        ensure!(
            !relative
                .components()
                .any(|v| v.as_os_str().to_string_lossy().contains([':', '\\'])),
            "ZIP 路径无效"
        );
        ensure!(
            file.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "ZIP 包含符号链接，未导入"
        );
        if file.is_dir() || relative.components().any(|v| v.as_os_str() == ".git") {
            continue;
        }
        ensure!(
            file.size() <= transaction::MAX_FILE_BYTES as u64,
            "ZIP 中单个文件过大"
        );
        let mut content = Vec::new();
        file.by_ref()
            .take(transaction::MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut content)?;
        total += content.len();
        ensure!(
            total <= MAX_TOTAL && files.len() < MAX_FILES,
            "解压内容超过限制"
        );
        ensure!(
            files.insert(relative, content).is_none(),
            "ZIP 包含重复文件，未导入"
        );
    }
    let candidates: Vec<_> = files
        .keys()
        .filter(|p| p.file_name().is_some_and(|v| v == "SKILL.md"))
        .map(|p| p.parent().unwrap_or(Path::new("")).to_path_buf())
        .collect();
    let base = if let Some(sub) = subdirectory.filter(|s| !s.trim().is_empty()) {
        let sub = PathBuf::from(sub);
        ensure!(candidates.contains(&sub), "所选 ZIP 子目录没有 SKILL.md");
        sub
    } else {
        ensure!(
            candidates.len() == 1,
            "ZIP 含有 {} 个 Skill，请在高级设置填写要导入的包内目录",
            candidates.len()
        );
        candidates[0].clone()
    };
    package(
        files
            .into_iter()
            .filter_map(|(p, v)| p.strip_prefix(&base).ok().map(|r| (r.to_path_buf(), v)))
            .collect(),
    )
}

pub fn from_files(files: BTreeMap<PathBuf, Vec<u8>>) -> Result<SkillPackage> {
    for (path, bytes) in &files {
        ensure!(
            !path.is_absolute()
                && !path.as_os_str().is_empty()
                && path.components().count() <= 32
                && path
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                && !path.to_string_lossy().contains([':', '\\'])
                && bytes.len() <= transaction::MAX_FILE_BYTES,
            "Skill 包含越界路径或过大文件"
        );
    }
    ensure!(
        files.len() <= MAX_FILES && files.values().map(Vec::len).sum::<usize>() <= MAX_TOTAL,
        "Skill 完整包超过读取上限"
    );
    package(files)
}
fn package(files: BTreeMap<PathBuf, Vec<u8>>) -> Result<SkillPackage> {
    let metadata = metadata(
        files
            .get(Path::new("SKILL.md"))
            .context("所选文件夹没有 SKILL.md")?,
    )?;
    let mut hash = Sha256::new();
    for (path, bytes) in &files {
        hash.update(path.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update([0]);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(SkillPackage {
        metadata,
        files,
        fingerprint: hex::encode(hash.finalize()),
    })
}

pub fn folder_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_".contains(c) {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-');
    if name.is_empty() {
        "skill".into()
    } else {
        name.chars().take(64).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_package_can_read_a_resource_larger_than_eight_mib() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("SKILL.md"),
            "---\nname: fixture\ndescription: test\n---\nFixture",
        )
        .unwrap();
        std::fs::File::create(dir.path().join("asset.bin"))
            .unwrap()
            .set_len(9 * 1024 * 1024)
            .unwrap();
        let package = from_directory(dir.path()).unwrap();
        assert_eq!(package.files[Path::new("asset.bin")].len(), 9 * 1024 * 1024);
    }
    #[test]
    fn package_keeps_relative_resources_and_detects_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("SKILL.md"),
            "---\nname: fixture\ndescription: test\n---\nRead references/a.md",
        )
        .unwrap();
        fs::create_dir(dir.path().join("references")).unwrap();
        fs::write(dir.path().join("references/a.md"), "first").unwrap();
        let first = from_directory(dir.path()).unwrap();
        assert!(first.files.contains_key(Path::new("references/a.md")));
        fs::write(dir.path().join("references/a.md"), "second").unwrap();
        assert_ne!(
            first.fingerprint,
            from_directory(dir.path()).unwrap().fingerprint
        );
    }
    #[test]
    fn archive_traversal_is_rejected() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("../SKILL.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fixture").unwrap();
        zip.finish().unwrap();
        assert!(from_zip(&path, None).is_err());
    }
}
