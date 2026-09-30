//! Bounded, in-memory handoff. No ticket spool files or unauthenticated HTTP API.
use super::ImportLink;
use std::{
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    time::Duration,
};

pub struct Inbox {
    pub receiver: Receiver<ImportLink>,
}

#[cfg(windows)]
mod security;

#[cfg(windows)]
fn pipe_name() -> String {
    use sha2::{Digest, Sha256};
    let user = security::current_user_sid().unwrap_or_default();
    let test_suffix = if cfg!(test) {
        format!(".test.{}", std::process::id())
    } else {
        String::new()
    };
    format!(
        r"\\.\pipe\TianCaiSpaceHub.Import.v1.{}{test_suffix}",
        hex::encode(Sha256::digest(user.as_bytes()))
    )
}

pub fn start() -> Result<Inbox, String> {
    let (sender, receiver) = sync_channel(16);
    #[cfg(windows)]
    start_windows(sender)?;
    #[cfg(not(windows))]
    drop(sender);
    Ok(Inbox { receiver })
}

#[cfg(windows)]
fn start_windows(sender: SyncSender<ImportLink>) -> Result<(), String> {
    let (ready_tx, ready_rx) = sync_channel(1);
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            let mut server = match security::create_server(true, &pipe_name()) {
                Ok(server) => server,
                Err(_) => {
                    let _ = ready_tx.send(false);
                    return;
                }
            };
            let _ = ready_tx.send(true);
            let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
            loop {
                if server.connect().await.is_err() {
                    break;
                }
                // Keep an instance alive while accepting the next connection.
                let next = match security::create_server(false, &pipe_name()) {
                    Ok(next) => next,
                    Err(_) => break,
                };
                let connected = std::mem::replace(&mut server, next);
                let sender = sender.clone();
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ =
                        tokio::time::timeout(Duration::from_secs(3), receive(connected, &sender))
                            .await;
                });
            }
        });
    });
    if ready_rx.recv_timeout(Duration::from_secs(3)) != Ok(true) {
        return Err(
            "网页导入接收服务启动失败，请关闭其他 Hub 后重试 / Import receiver unavailable".into(),
        );
    }
    Ok(())
}

#[cfg(windows)]
async fn receive(
    mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    sender: &SyncSender<ImportLink>,
) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let len = pipe.read_u32().await? as usize;
    if len > super::MAX_LINK_BYTES {
        return Ok(());
    }
    let mut data = vec![0; len];
    pipe.read_exact(&mut data).await?;
    // Parse a canonical link again rather than trusting the sending process.
    let accepted = std::str::from_utf8(&data)
        .ok()
        .and_then(|raw| ImportLink::parse(raw, super::allow_local_development()).ok())
        .is_some_and(|link| sender.try_send(link).is_ok());
    pipe.write_u8(u8::from(accepted)).await?;
    Ok(())
}

pub fn forward(link: &ImportLink) -> Result<(), String> {
    #[cfg(windows)]
    {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::windows::named_pipe::ClientOptions,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "无法转交导入请求 / Cannot forward import")?;
        let mut url = url::Url::parse("tiancaispacehub://import/v1").unwrap();
        url.query_pairs_mut()
            .append_pair("origin", &link.origin)
            .append_pair("ticket", &link.ticket);
        let data = url.as_str().as_bytes();
        return runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(12), async {
                loop {
                    match ClientOptions::new().open(pipe_name()) {
                        Ok(mut pipe) => {
                            security::verify_server_user(&pipe).map_err(|_| ())?;
                            pipe.write_u32(data.len() as u32).await.map_err(|_| ())?;
                            pipe.write_all(data).await.map_err(|_| ())?;
                            return match pipe.read_u8().await { Ok(1) => Ok(()), _ => Err(()) };
                        }
                        Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                    }
                }
            }).await.map_err(|_| ())?
        }).map_err(|_| "现有 Hub 未接收导入请求：请关闭旧版或等待启动完成，再从网页重新发起 / Existing Hub did not accept the import".into());
    }
    #[cfg(not(windows))]
    {
        let _ = link;
        Err("此平台的网页导入尚未发布 / Web import is not released on this platform".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn transfers_to_existing_instance_without_disk_spool() {
        let inbox = super::start().unwrap();
        let link = crate::external_import::ImportLink {
            origin: crate::external_import::OFFICIAL_ORIGIN.into(),
            ticket: "a".repeat(43),
        };
        super::forward(&link).unwrap();
        assert_eq!(
            inbox
                .receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            link
        );
    }
}
