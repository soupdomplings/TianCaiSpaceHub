//! Only uploaded or native-message-associated files can be downloaded. The
//! browser never chooses a host path. This catalog is not conversation history.
use super::types::{RemoteFile, Scope};
use crate::app_state::SharedState;
use anyhow::{Context, Result, ensure};
use axum::{body::Bytes, http::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
#[path = "delivery_links.rs"]
mod delivery;
pub(super) use delivery::{has_local_link, project_links};

pub(super) const MAX_UPLOAD: usize = 32 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct StoredFile {
    pub public: RemoteFile,
    pub path: PathBuf,
    /// Resolved path at registration. Older entries retain their recorded lexical path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pinned_path: Option<PathBuf>,
    /// Only conversational links discovered in a designated output directory set this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_root: Option<PathBuf>,
    #[serde(default)]
    pub pending: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Catalog {
    files: Vec<StoredFile>,
}

fn root(state: &SharedState, scope: &Scope) -> PathBuf {
    let key = hex::encode(Sha256::digest(
        format!("{}\0{}", scope.client, scope.session).as_bytes(),
    ));
    state.config_path.with_extension("remote-files").join(key)
}
fn read_catalog(root: &Path) -> Result<Catalog> {
    let path = root.join("index.json");
    if !path.exists() {
        return Ok(Catalog::default());
    }
    serde_json::from_slice(&std::fs::read(path)?).context("无法读取此会话的附件记录")
}
fn save_catalog(root: &Path, catalog: &Catalog) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec(catalog)?)?;
    file.persist(root.join("index.json")).map_err(|e| e.error)?;
    Ok(())
}
pub(super) async fn upload(
    state: &SharedState,
    scope: &Scope,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<RemoteFile> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_UPLOAD,
        "附件大小须在 1 字节至 32 MB 之间"
    );
    let encoded = headers
        .get("x-file-name")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("attachment");
    let name =
        url::form_urlencoded::parse(format!("name={}", encoded.replace('+', "%2B")).as_bytes())
            .next()
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "attachment".into());
    ensure!(
        !name.is_empty()
            && name.len() <= 255
            && !name.contains(['/', '\\'])
            && !name.chars().any(char::is_control),
        "附件文件名无效"
    );
    let mime = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let _gate = state.remote_sessions.file_gate.lock().await;
    let root = root(state, scope);
    let scope = scope.clone();
    tokio::task::spawn_blocking(move || {
        let mut catalog = read_catalog(&root)?;
        let id = uuid::Uuid::new_v4().to_string();
        let dir = root.join(&id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        std::fs::write(&path, &bytes)?;
        let public = RemoteFile {
            id: id.clone(),
            name,
            size: bytes.len() as u64,
            mime_type: mime,
            url: scope.file_url(&id),
            kind: "attachment".into(),
        };
        catalog.files.push(StoredFile {
            public: public.clone(),
            pinned_path: Some(path.canonicalize()?),
            output_root: None,
            path,
            pending: true,
        });
        save_catalog(&root, &catalog)?;
        Ok(public)
    })
    .await?
}
pub(super) async fn selected(
    state: &SharedState,
    scope: &Scope,
    ids: &[String],
) -> Result<Vec<StoredFile>> {
    ensure!(ids.len() <= 20, "每轮最多添加 20 个附件");
    let _gate = state.remote_sessions.file_gate.lock().await;
    let root = root(state, scope);
    let ids = ids.to_vec();
    tokio::task::spawn_blocking(move || {
        let catalog = read_catalog(&root)?;
        ids.iter()
            .map(|id| {
                catalog
                    .files
                    .iter()
                    .find(|f| f.public.id == *id)
                    .cloned()
                    .context("附件已不存在，请重新选择文件")
            })
            .collect()
    })
    .await?
}
pub(super) async fn lookup(state: &SharedState, scope: &Scope, id: &str) -> Result<StoredFile> {
    selected(state, scope, &[id.to_owned()])
        .await?
        .into_iter()
        .next()
        .context("附件不存在")
}
pub(super) async fn open_registered(
    state: &SharedState,
    scope: &Scope,
    id: &str,
) -> Result<(StoredFile, tokio::fs::File)> {
    let stored = lookup(state, scope, id).await?;
    let (stored, file) = tokio::task::spawn_blocking(move || {
        let file = delivery::open_checked(&stored)?;
        Ok::<_, anyhow::Error>((stored, file))
    })
    .await??;
    Ok((stored, tokio::fs::File::from_std(file)))
}
pub(super) async fn associate(
    state: &SharedState,
    scope: &Scope,
    path: &Path,
    kind: &str,
) -> Result<Option<RemoteFile>> {
    if !path.is_absolute() || !path.is_file() {
        return Ok(None);
    }
    let path = path.to_path_buf();
    let root = root(state, scope);
    let scope = scope.clone();
    let kind = kind.to_owned();
    let _gate = state.remote_sessions.file_gate.lock().await;
    tokio::task::spawn_blocking(move || {
        let mut catalog = read_catalog(&root)?;
        if let Some(file) = catalog.files.iter().find(|f| f.path == path) {
            return Ok(Some(file.public.clone()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let public = RemoteFile {
            id: id.clone(),
            size: std::fs::metadata(&path)?.len(),
            mime_type: mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string(),
            name,
            url: scope.file_url(&id),
            kind,
        };
        catalog.files.push(StoredFile {
            public: public.clone(),
            pinned_path: Some(path.canonicalize()?),
            output_root: None,
            path,
            pending: false,
        });
        save_catalog(&root, &catalog)?;
        Ok(Some(public))
    })
    .await?
}
pub(super) fn native_descriptor(file: &StoredFile) -> Value {
    json!({"file_id":file.public.id,"file_name":file.public.name,"file_path":file.path,
        "file_size":file.public.size,"mime_type":file.public.mime_type,
        "file_type":if file.public.mime_type.starts_with("image/") {"image"} else {"other"}})
}

pub(super) async fn mark_sent(
    state: &SharedState,
    scope: &Scope,
    ids: &[String],
) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let _gate = state.remote_sessions.file_gate.lock().await;
    let root = root(state, scope);
    let ids = ids.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut catalog = read_catalog(&root)?;
        let mut pending = Vec::new();
        for file in &mut catalog.files {
            if ids.contains(&file.public.id) && file.pending {
                pending.push(file.public.id.clone());
                file.pending = false;
            }
        }
        save_catalog(&root, &catalog)?;
        Ok(pending)
    })
    .await?
}
pub(super) async fn restore_pending(
    state: &SharedState,
    scope: &Scope,
    ids: &[String],
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let _gate = state.remote_sessions.file_gate.lock().await;
    let root = root(state, scope);
    let ids = ids.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut catalog = read_catalog(&root)?;
        for file in &mut catalog.files {
            if ids.contains(&file.public.id) {
                file.pending = true;
            }
        }
        save_catalog(&root, &catalog)
    })
    .await?
}
pub(super) async fn remove_pending(state: &SharedState, scope: &Scope, id: &str) -> Result<()> {
    let _gate = state.remote_sessions.file_gate.lock().await;
    let root = root(state, scope);
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut catalog = read_catalog(&root)?;
        let file = catalog
            .files
            .iter()
            .find(|f| f.public.id == id)
            .context("附件已不存在")?;
        ensure!(file.pending, "已发送附件和原生产物需要随原会话保留");
        let owned = root.join(&id);
        ensure!(
            file.path.parent() == Some(owned.as_path()) && owned.parent() == Some(root.as_path()),
            "附件不属于此 Hub 上传目录"
        );
        if file.path.exists() {
            std::fs::remove_file(&file.path)?;
        }
        if owned.is_dir() {
            std::fs::remove_dir(&owned)?;
        }
        catalog.files.retain(|f| f.public.id != id);
        save_catalog(&root, &catalog)
    })
    .await?
}
