//! Optional ACP runtime. Installation is an explicit user action, never startup.
use super::{targets, transaction};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub const OPENCODE_VERSION: &str = "1.2.21";
pub fn opencode() -> Result<std::path::PathBuf> {
    Ok(targets::home()?
        .join(".tiancaispace-hub/components/opencode")
        .join(OPENCODE_VERSION)
        .join(if cfg!(windows) {
            "opencode.exe"
        } else {
            "opencode"
        }))
}
fn archive() -> (&'static str, &'static str) {
    if cfg!(windows) {
        (
            "opencode-windows-x64-baseline.zip",
            "f75aff647b908943e65cdc7426a84975e62f8f0f51e81f97a2621bb6788d4d79",
        )
    } else if cfg!(target_arch = "aarch64") {
        (
            "opencode-darwin-arm64.zip",
            "432d53995797c496b3fb34dd11426bca82dd8e5741f5ba5096d505f895c39e28",
        )
    } else {
        (
            "opencode-darwin-x64-baseline.zip",
            "4c08bb4cc793f8f16c983b011842ae8ac2c7bd585f57d973e1497e7f8280f9b6",
        )
    }
}
fn extract(bytes: Vec<u8>, wanted: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    ensure!(zip.len() <= 128, "组件文件数量异常");
    let mut found = None;
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        ensure!(file.enclosed_name().is_some(), "组件包含越界路径");
        let name = file.name().replace('\\', "/");
        ensure!(
            !name.starts_with('/') && !name.contains(':') && name.split('/').all(|p| p != ".."),
            "组件包含越界路径"
        );
        ensure!(
            file.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "组件包含链接"
        );
        ensure!(names.insert(name.to_lowercase()), "组件包含重复路径");
        total = total.saturating_add(file.size());
        ensure!(total <= 200 * 1024 * 1024, "组件解压超过容量限制");
        if !file.is_dir() && name.rsplit('/').next() == Some(wanted) {
            ensure!(found.is_none(), "组件包含重复程序");
            let mut output = Vec::new();
            file.take(200 * 1024 * 1024 + 1).read_to_end(&mut output)?;
            ensure!(output.len() <= 200 * 1024 * 1024, "组件解压超过容量限制");
            found = Some(output);
        }
    }
    found.context("下载包中没有 ACP 程序")
}
pub fn status() -> Result<Value> {
    let path = opencode()?;
    let identity = path.with_extension("identity.json");
    let marker =
        transaction::read(&identity)?.and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    transaction::reject_links(&path)?;
    let installed = if let Some(marker) =
        marker.filter(|v| v["version"] == OPENCODE_VERSION && v["archiveSha256"] == archive().1)
    {
        if path.is_file() {
            use std::io::Read;
            let mut file = std::fs::File::open(&path)?;
            let mut hash = Sha256::new();
            let mut buf = [0u8; 65536];
            let mut size = 0usize;
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                size += n;
                ensure!(size <= 200 * 1024 * 1024, "ACP 组件大小异常，请重新安装");
                hash.update(&buf[..n]);
            }
            size > 1024 * 1024 && marker["binarySha256"] == hex::encode(hash.finalize())
        } else {
            false
        }
    } else {
        false
    };
    Ok(json!({"opencode":{"installed":installed,"version":OPENCODE_VERSION,"path":path}}))
}
pub async fn install(proxy: &crate::config::OutboundProxyConfig) -> Result<Value> {
    ensure!(
        cfg!(any(windows, target_os = "macos")),
        "此组件仅用于 Windows 和 macOS"
    );
    let (name, expected) = archive();
    let url = format!(
        "https://github.com/anomalyco/opencode/releases/download/v{OPENCODE_VERSION}/{name}"
    );
    let client =
        crate::external_import::client::build_client(proxy, None).map_err(anyhow::Error::msg)?;
    // Release assets redirect to GitHub's dedicated storage. Only these two
    // origins are accepted; credentials are not attached to either request.
    let mut response = client.get(&url).send().await?;
    if response.status().is_redirection() {
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .context("组件下载跳转缺少地址")?;
        let u = url::Url::parse(location)?;
        ensure!(
            u.scheme() == "https"
                && matches!(
                    u.host_str(),
                    Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
                ),
            "组件下载跳转地址无效"
        );
        response = client.get(u).send().await?;
    }
    ensure!(
        response.status().is_success(),
        "ACP 组件下载失败，请检查网络后重试"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 100 * 1024 * 1024,
            "组件下载超过容量限制"
        );
        bytes.extend(chunk);
    }
    ensure!(
        hex::encode(Sha256::digest(&bytes)) == expected,
        "组件校验不一致，未安装，请稍后重试"
    );
    tokio::task::spawn_blocking(move||->Result<Value>{
        let destination=opencode()?;
        let binary=extract(bytes,if cfg!(windows){"opencode.exe"}else{"opencode"})?;
        // The shared configuration writer caps config files at 8 MiB. Runtime
        // binaries use a separate atomic replacement with a checked destination.
        transaction::reject_links(&destination)?;let parent=destination.parent().unwrap();std::fs::create_dir_all(parent)?;let mut temp=tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write;temp.write_all(&binary)?;temp.as_file().sync_all()?;
        #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;temp.as_file().set_permissions(std::fs::Permissions::from_mode(0o700))?;}
        temp.persist(&destination).map_err(|e|e.error)?;
        transaction::atomic_write(&destination.with_extension("identity.json"),&serde_json::to_vec(&json!({"version":OPENCODE_VERSION,"archiveSha256":expected,"binarySha256":hex::encode(Sha256::digest(&binary))}))?)?;status()
    }).await.context("组件安装中断，请重新读取")?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package(names: &[&str]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in names {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fixture-not-executable").unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn acp_archive_rejects_unsafe_unselected_entries_and_ambiguous_binaries() {
        assert_eq!(
            extract(package(&["bundle/opencode.exe"]), "opencode.exe").unwrap(),
            b"fixture-not-executable"
        );
        assert!(extract(package(&["opencode"]), "opencode").is_ok());
        for names in [
            vec!["opencode.exe", "../outside"],
            vec!["opencode.exe", "C:/outside"],
            vec!["a/opencode.exe", "b/opencode.exe"],
            vec!["opencode.exe", "..\\outside"],
        ] {
            assert!(extract(package(&names), "opencode.exe").is_err());
        }
    }
}
