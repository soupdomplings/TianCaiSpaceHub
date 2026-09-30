//! macOS CLI/secondary-process handoff. The socket contains no persisted tickets.
use super::{ImportLink, SyncSender};
use fs2::FileExt;
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, mpsc::sync_channel},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

const UNAVAILABLE: &str =
    "网页导入接收服务启动失败，请关闭其他 Hub 后重试 / Import receiver unavailable";
const FORWARD_FAILED: &str = "现有 Hub 未接收导入请求：请关闭旧版或等待启动完成，再从网页重新发起 / Existing Hub did not accept the import";

fn user_id() -> u32 {
    // geteuid has no preconditions and does not expose account credentials.
    unsafe { libc::geteuid() }
}

fn directory() -> PathBuf {
    let suffix = if cfg!(test) {
        format!("-test-{}", std::process::id())
    } else {
        String::new()
    };
    // A fixed short path stays below Darwin's 104-byte Unix socket path limit.
    // The sticky /tmp parent and the checked 0700 user directory isolate users.
    PathBuf::from(format!("/tmp/tiancaispacehub-import-{}{suffix}", user_id()))
}

fn private_directory(path: &Path, create: bool) -> io::Result<()> {
    if create {
        match DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != user_id() || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(())
}

struct Endpoint {
    path: PathBuf,
    inode: u64,
    _lock: File,
}

impl Endpoint {
    fn bind() -> io::Result<(Self, UnixListener)> {
        let directory = directory();
        private_directory(&directory, true)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(directory.join("receiver.lock"))?;
        let metadata = lock.metadata()?;
        if !metadata.is_file() || metadata.uid() != user_id() || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        FileExt::try_lock_exclusive(&lock)?;
        let path = directory.join("v1.sock");
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.file_type().is_socket() || metadata.uid() != user_id() {
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                // The exclusive lock proves no current receiver owns this path.
                fs::remove_file(&path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        let endpoint = Self {
            inode: fs::symlink_metadata(&path)?.ino(),
            path,
            _lock: lock,
        };
        fs::set_permissions(&endpoint.path, fs::Permissions::from_mode(0o600))?;
        Ok((endpoint, listener))
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| {
            m.file_type().is_socket() && m.ino() == self.inode && m.uid() == user_id()
        }) {
            let _ = fs::remove_file(&self.path);
        }
        // Releasing the lock happens after removing only our own socket inode.
    }
}

pub(super) fn start(sender: SyncSender<ImportLink>) -> Result<(), String> {
    let (ready_tx, ready_rx) = sync_channel(1);
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            let Ok((_endpoint, listener)) = Endpoint::bind() else {
                return;
            };
            let _ = ready_tx.send(true);
            let slots = Arc::new(tokio::sync::Semaphore::new(16));
            while let Ok((stream, _)) = listener.accept().await {
                if !stream.peer_cred().is_ok_and(|peer| peer.uid() == user_id()) {
                    continue;
                }
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let sender = sender.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ =
                        tokio::time::timeout(Duration::from_secs(3), receive(stream, sender)).await;
                });
            }
        });
    });
    if ready_rx.recv_timeout(Duration::from_secs(3)) != Ok(true) {
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}

async fn receive(mut stream: UnixStream, sender: SyncSender<ImportLink>) -> io::Result<()> {
    let len = stream.read_u32().await? as usize;
    if len > super::super::MAX_LINK_BYTES {
        return Ok(());
    }
    let mut data = vec![0; len];
    stream.read_exact(&mut data).await?;
    let accepted = std::str::from_utf8(&data)
        .ok()
        .and_then(|raw| ImportLink::parse(raw).ok())
        .is_some_and(|link| sender.try_send(link).is_ok());
    stream.write_u8(u8::from(accepted)).await
}

pub(super) fn forward(link: &ImportLink) -> Result<(), String> {
    let mut url = url::Url::parse("tiancaispacehub://import/v1").unwrap();
    url.query_pairs_mut()
        .append_pair("origin", &link.origin)
        .append_pair("ticket", &link.ticket);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| FORWARD_FAILED)?;
    runtime
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(12), async {
                let directory = directory();
                loop {
                    match private_directory(&directory, false) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                        Err(_) => return Err(()),
                    }
                    match UnixStream::connect(directory.join("v1.sock")).await {
                        Ok(mut stream) => {
                            if !stream.peer_cred().is_ok_and(|peer| peer.uid() == user_id()) {
                                return Err(());
                            }
                            stream
                                .write_u32(url.as_str().len() as u32)
                                .await
                                .map_err(|_| ())?;
                            stream
                                .write_all(url.as_str().as_bytes())
                                .await
                                .map_err(|_| ())?;
                            // Never replay after sending: an ACK loss may still mean accepted.
                            return match stream.read_u8().await {
                                Ok(1) => Ok(()),
                                _ => Err(()),
                            };
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                            ) =>
                        {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(_) => return Err(()),
                    }
                }
            })
            .await
            .map_err(|_| ())?
        })
        .map_err(|_| FORWARD_FAILED.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_shared_or_symlinked_handoff_directories() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        private_directory(&private, true).unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_directory(&private, false).is_err());
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&private, &alias).unwrap();
        assert!(private_directory(&alias, true).is_err());
    }

    #[tokio::test]
    async fn rejects_oversized_and_malformed_handoffs_without_enqueueing() {
        for bytes in [
            vec![b'x'; super::super::super::MAX_LINK_BYTES + 1],
            b"invalid-link".to_vec(),
        ] {
            let (server, mut client) = UnixStream::pair().unwrap();
            let (sender, receiver) = sync_channel(1);
            let task = tokio::spawn(receive(server, sender));
            client.write_u32(bytes.len() as u32).await.unwrap();
            if bytes.len() <= super::super::super::MAX_LINK_BYTES {
                client.write_all(&bytes).await.unwrap();
                assert_eq!(client.read_u8().await.unwrap(), 0);
            }
            task.await.unwrap().unwrap();
            assert!(receiver.try_recv().is_err());
        }
    }
}
