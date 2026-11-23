// Included in the remote_sessions module to enforce the same attachment catalog as the web.
use super::{files, types::Scope, valid_client};
use crate::app_state::SharedState;
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::io::SeekFrom;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub(crate) async fn read(
    state: &SharedState,
    client: &str,
    session: &str,
    id: &str,
    offset: u64,
) -> Result<Value> {
    valid_client(client)?;
    let (_, mut file) = files::open_registered(
        state,
        &Scope::new(client.to_owned(), session.to_owned()),
        id,
    )
    .await?;
    let size = file.metadata().await?.len();
    ensure!(offset <= size, "文件发生变化，请重新下载");
    file.seek(SeekFrom::Start(offset)).await?;
    let mut bytes = vec![0; (size - offset).min(192 * 1024) as usize];
    file.read_exact(&mut bytes).await?;
    let next = offset + bytes.len() as u64;
    Ok(json!({"chunk":URL_SAFE_NO_PAD.encode(bytes),"nextOffset":(next<size).then_some(next)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn reads_consecutive_chunks_only_from_the_registered_session() {
        let (dir, state) = crate::remote_network::mobile_test_fixture();
        let path = dir.path().join("result.bin");
        let mut content = vec![3; 192 * 1024];
        content.extend_from_slice(b"last");
        std::fs::write(&path, &content).unwrap();
        let record = files::associate(
            &state,
            &Scope::new("codex:fixture", "chat"),
            &path,
            "artifact",
        )
        .await
        .unwrap()
        .unwrap();
        let first = read(&state, "codex:fixture", "chat", &record.id, 0)
            .await
            .unwrap();
        assert_eq!(first["nextOffset"], 192 * 1024);
        let last = read(&state, "codex:fixture", "chat", &record.id, 192 * 1024)
            .await
            .unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD
                .decode(last["chunk"].as_str().unwrap())
                .unwrap(),
            b"last"
        );
        assert_eq!(last["nextOffset"], Value::Null);
        assert!(
            read(&state, "codex:fixture", "other", &record.id, 0)
                .await
                .is_err()
        );
    }
}
