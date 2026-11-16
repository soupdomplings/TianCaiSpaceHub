//! The unmodified, versioned EasyTier runtime is installed only after consent.
use std::{
    io::{Cursor, Read, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::{Child, Command};

use super::{NetworkConfig, NetworkSecrets, SERVICE_PORT};

pub(super) const VERSION: &str = "2.6.4";
const OWNER: &str = "tiancai-hub-easytier-v1";
const MAX_ARCHIVE: usize = 160 * 1024 * 1024;
const MAX_BINARY: u64 = 192 * 1024 * 1024;

pub(super) fn platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("windows-x86_64"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        ("macos", "x86_64") => Ok("macos-x86_64"),
        ("macos", "aarch64") => Ok("macos-aarch64"),
        _ => anyhow::bail!("远程连接组件目前支持 Windows / macOS 的 x64 和 Arm64"),
    }
}

fn native(name: &str) -> String {
    format!("{name}{}", if cfg!(windows) { ".exe" } else { "" })
}

pub(super) fn package_dir(root: &Path) -> Result<PathBuf> {
    Ok(root
        .join("runtime")
        .join(format!("easytier-{}-v{VERSION}", platform()?)))
}

#[derive(Deserialize, Serialize)]
struct Manifest {
    owner: String,
    version: String,
    platform: String,
}

pub(super) fn ready(root: &Path) -> bool {
    let Ok(directory) = package_dir(root) else {
        return false;
    };
    let Ok(bytes) = std::fs::read(directory.join("hub-owner.json")) else {
        return false;
    };
    let Ok(marker) = serde_json::from_slice::<Manifest>(&bytes) else {
        return false;
    };
    marker.owner == OWNER
        && marker.version == VERSION
        && platform().is_ok_and(|v| marker.platform == v)
        && directory.join(native("easytier-core")).is_file()
        && directory.join(native("easytier-cli")).is_file()
}

pub(super) async fn prepare(root: &Path) -> Result<()> {
    if ready(root) {
        return Ok(());
    }
    let directory = package_dir(root)?;
    ensure!(
        !directory.exists(),
        "自管组网目录不完整，请停用后移除 Hub 安装的组件再准备"
    );
    let url = format!(
        "https://github.com/EasyTier/EasyTier/releases/download/v{VERSION}/easytier-{}-v{VERSION}.zip",
        platform()?
    );
    let response = crate::outbound_http::get()
        .get(&url)
        .timeout(Duration::from_secs(180))
        .send()
        .await
        .context("无法下载远程连接组件，请检查网络后重试")?
        .error_for_status()
        .context("官方组件下载失败")?;
    ensure!(
        response
            .content_length()
            .is_none_or(|v| v <= MAX_ARCHIVE as u64),
        "远程连接组件包体积异常"
    );
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("远程连接组件下载中断，请重试")?
    {
        ensure!(
            bytes.len() + chunk.len() <= MAX_ARCHIVE,
            "远程连接组件包体积异常"
        );
        bytes.extend_from_slice(&chunk);
    }
    let parent = directory.parent().context("组件目录无效")?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".prepare-")
        .tempdir_in(parent)?;
    unpack(&bytes, staging.path())?;
    std::fs::write(
        staging.path().join("LICENSE-EasyTier"),
        include_bytes!("EasyTier-LICENSE"),
    )?;
    std::fs::write(
        staging.path().join("SOURCE.txt"),
        format!(
            "EasyTier v{VERSION}\nhttps://github.com/EasyTier/EasyTier/tree/v{VERSION}\nUnmodified official binaries; LGPL-3.0.\n"
        ),
    )?;
    std::fs::write(
        staging.path().join("hub-owner.json"),
        serde_json::to_vec_pretty(&Manifest {
            owner: OWNER.into(),
            version: VERSION.into(),
            platform: platform()?.into(),
        })?,
    )?;
    std::fs::rename(staging.path(), &directory).context("无法保存远程连接组件")?;
    Ok(())
}

fn unpack(bytes: &[u8], output: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("组件压缩包无法读取")?;
    let required = [native("easytier-core"), native("easytier-cli")];
    for name in &required {
        let mut found = false;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let Some(path) = entry.enclosed_name() else {
                anyhow::bail!("组件包包含越界路径");
            };
            ensure!(
                entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                "组件包包含链接"
            );
            if path
                .file_name()
                .is_none_or(|value| value != std::ffi::OsStr::new(name))
            {
                continue;
            }
            ensure!(
                !found && entry.size() <= MAX_BINARY && !entry.is_dir(),
                "组件包主程序内容异常"
            );
            let destination = output.join(name);
            let mut file = std::fs::File::create(&destination)?;
            let copied = std::io::copy(&mut (&mut entry).take(MAX_BINARY + 1), &mut file)?;
            ensure!(copied <= MAX_BINARY, "组件程序体积异常");
            file.flush()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o700))?;
            }
            found = true;
        }
        ensure!(found, "官方组件包缺少 {name}");
    }
    Ok(())
}

pub(super) fn uninstall(root: &Path) -> Result<()> {
    let directory = package_dir(root)?;
    if !directory.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(directory.join("hub-owner.json"))
        .context("目录缺少 Hub 安装标记，不能自动移除")?;
    let marker: Manifest =
        serde_json::from_slice(&bytes).context("目录缺少有效 Hub 安装标记，不能自动移除")?;
    ensure!(
        marker.owner == OWNER && marker.platform == platform()? && marker.version == VERSION,
        "此目录不属于本 Hub 安装，不能移除"
    );
    crate::desktop::transaction::reject_links(&directory)?;
    std::fs::remove_dir_all(directory).context("无法移除组件，请确认没有正在运行的组网任务")?;
    Ok(())
}

pub(super) struct Running {
    pub child: Child,
    pub tree: crate::local_assistance::process::ProcessTree,
    pub directory: tempfile::TempDir,
    pub rpc_port: u16,
    pub socks_port: u16,
}

#[derive(Deserialize, Serialize)]
struct ActiveOwner {
    owner: String,
    pid: Option<u32>,
}

/// Reclaim only a marked, already-exited Hub instance. Live or unrecognised
/// directories stay untouched; normal stop removes its TempDir immediately.
pub(super) fn recover_inactive(root: &Path) -> Result<()> {
    let active = root.join("active");
    if !active.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&active)? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with("hub-") || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let directory = entry.path();
        crate::desktop::transaction::reject_links(&directory)?;
        let Some(marker) = std::fs::read(directory.join("hub-owner.json"))
            .ok()
            .and_then(|v| serde_json::from_slice::<ActiveOwner>(&v).ok())
        else {
            continue;
        };
        if marker.owner != OWNER {
            continue;
        }
        if marker
            .pid
            .is_some_and(|pid| crate::local_assistance::process::alive(pid).unwrap_or(true))
        {
            continue;
        }
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

/// Both inbound and forwarding chains are constrained: no_tun maps virtual-IP
/// traffic to host loopback, so a listener's bind address alone is insufficient.
pub(super) fn configuration(
    config: &NetworkConfig,
    secrets: &NetworkSecrets,
    rpc_port: u16,
    socks_port: u16,
) -> Result<String> {
    let ip: Ipv4Addr = config.ipv4.parse().context("虚拟地址无效")?;
    ensure!(ip.octets()[..2] == [10, 203], "虚拟地址超出 Hub 网络范围");
    let mut document = toml::map::Map::new();
    document.insert(
        "hostname".into(),
        toml::Value::String(format!("tch-{}", &config.device_id.replace('-', "")[..28])),
    );
    document.insert(
        "instance_name".into(),
        toml::Value::String("TianCaiSpaceHub remote".into()),
    );
    document.insert(
        "instance_id".into(),
        toml::Value::String(config.device_id.clone()),
    );
    document.insert("ipv4".into(), toml::Value::String(format!("{ip}/16")));
    document.insert("dhcp".into(), toml::Value::Boolean(false));
    document.insert(
        "listeners".into(),
        toml::Value::Array(
            ["tcp://0.0.0.0:0", "udp://0.0.0.0:0"]
                .map(|s| toml::Value::String(s.into()))
                .to_vec(),
        ),
    );
    document.insert(
        "socks5_proxy".into(),
        toml::Value::String(format!("socks5://127.0.0.1:{socks_port}")),
    );
    document.insert(
        "network_identity".into(),
        toml::Value::try_from(
            json!({"network_name":config.network_name,"network_secret":secrets.network_secret}),
        )?,
    );
    document.insert(
        "peer".into(),
        toml::Value::try_from(json!([{"uri":config.relay_url}]))?,
    );
    document.insert("flags".into(), toml::Value::try_from(json!({"no_tun":true,"use_smoltcp":true,"enable_encryption":true,"enable_ipv6":false,"enable_exit_node":false,"relay_network_whitelist":"","enable_relay_foreign_network_kcp":false,"enable_relay_foreign_network_quic":false,"relay_all_peer_rpc":false}))?);
    let rule = json!({"name":"hub-remote-https","priority":1000,"action":1,"protocol":1,"ports":[SERVICE_PORT.to_string()],"enabled":true,"stateful":true,"destination_ips":[format!("{ip}/32")]});
    document.insert("acl".into(), toml::Value::try_from(json!({"acl_v1":{"chains":[
        {"name":"hub-inbound","chain_type":1,"enabled":true,"default_action":2,"rules":[rule.clone()]},
        {"name":"hub-forward","chain_type":3,"enabled":true,"default_action":2,"rules":[rule]}
    ]}}))?);
    // RPC lives in a separate core CLI option and is never admitted by the ACL.
    let _ = rpc_port;
    Ok(toml::to_string(&toml::Value::Table(document))?)
}

pub(super) async fn launch(
    root: &Path,
    config: &NetworkConfig,
    secrets: &NetworkSecrets,
) -> Result<Running> {
    ensure!(ready(root), "远程连接组件未准备，请先确认安装");
    let rpc_reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let socks_reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let rpc_port = rpc_reservation.local_addr()?.port();
    let socks_port = socks_reservation.local_addr()?.port();
    let sessions = root.join("active");
    std::fs::create_dir_all(&sessions)?;
    let directory = tempfile::Builder::new()
        .prefix("hub-")
        .tempdir_in(sessions)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let configuration_path = directory.path().join("network.toml");
    std::fs::write(
        directory.path().join("hub-owner.json"),
        serde_json::to_vec(&ActiveOwner {
            owner: OWNER.into(),
            pid: None,
        })?,
    )?;
    crate::desktop::transaction::atomic_write(
        &configuration_path,
        configuration(config, secrets, rpc_port, socks_port)?.as_bytes(),
    )?;
    let program = package_dir(root)?.join(native("easytier-core"));
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new(&program);
    #[cfg(target_os = "macos")]
    let mut command = {
        // Positional arguments avoid shell interpolation of file paths. The
        // tiny parent watcher also stops the owned component after a Hub crash.
        const WATCH_PARENT: &str = r#"parent_pid=$1
shift
"$@" &
component_pid=$!
(
  while kill -0 "$parent_pid" 2>/dev/null; do sleep 1; done
  kill "$component_pid" 2>/dev/null
) &
watcher_pid=$!
trap 'kill "$component_pid" "$watcher_pid" 2>/dev/null' EXIT HUP INT TERM
wait "$component_pid"
status=$?
kill "$watcher_pid" 2>/dev/null
wait "$watcher_pid" 2>/dev/null
exit "$status"
"#;
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", WATCH_PARENT, "hub-network-parent"])
            .arg(std::process::id().to_string())
            .arg(&program);
        command
    };
    // An existing standalone EasyTier installation must not inject config or
    // listeners into the Hub-owned instance through ET_* environment options.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("ET_") {
            command.env_remove(key);
        }
    }
    command
        .arg("-c")
        .arg(&configuration_path)
        .arg("--rpc-portal")
        .arg(format!("127.0.0.1:{rpc_port}"))
        .arg("--rpc-portal-whitelist")
        .arg("127.0.0.1/32")
        .current_dir(directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    #[cfg(target_os = "macos")]
    command.process_group(0);
    drop(rpc_reservation);
    drop(socks_reservation);
    let mut child = command.spawn().context("无法启动远程连接组件")?;
    let tree = match crate::local_assistance::process::ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill().await;
            return Err(error.context("无法管理远程连接后台"));
        }
    };
    std::fs::write(
        directory.path().join("hub-owner.json"),
        serde_json::to_vec(&ActiveOwner {
            owner: OWNER.into(),
            pid: child.id(),
        })?,
    )?;
    for _ in 0..40 {
        if child.try_wait()?.is_some() {
            anyhow::bail!("远程连接组件启动后退出，请检查节点设置与端口占用");
        }
        if tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, rpc_port))
            .await
            .is_ok()
            && tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, socks_port))
                .await
                .is_ok()
        {
            return Ok(Running {
                child,
                tree,
                directory,
                rpc_port,
                socks_port,
            });
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let _ = child.kill().await;
    anyhow::bail!("远程连接组件未能就绪，请检查网络后重试")
}

pub(super) async fn stop(mut runtime: Running) -> Result<()> {
    let _ = runtime.child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(3), runtime.child.wait()).await;
    if let Err(error) = runtime.tree.stop_and_confirm().await {
        let directory = runtime.directory.keep();
        return Err(error.context(format!(
            "远程连接后台退出尚未确认，暂时保留运行记录 {}",
            directory.display()
        )));
    }
    drop(runtime.directory);
    Ok(())
}

#[derive(Clone)]
pub(super) struct DiscoveredPeer {
    pub ipv4: String,
    pub hostname: String,
    pub direct: bool,
}

pub(super) struct PeerSnapshot {
    pub peers: Vec<DiscoveredPeer>,
    pub connected: bool,
}

pub(super) async fn peers(root: &Path, rpc_port: u16) -> Result<PeerSnapshot> {
    let mut command = Command::new(package_dir(root)?.join(native("easytier-cli")));
    command
        .args(["-p", &format!("127.0.0.1:{rpc_port}"), "-o", "json", "peer"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(4), command.output())
        .await
        .context("读取网络节点超时")??;
    ensure!(output.status.success(), "无法读取组网节点状态");
    let values: Value =
        serde_json::from_slice(&output.stdout).context("组网组件返回的节点格式不兼容")?;
    let connected = values
        .as_array()
        .context("组网组件返回的节点格式不兼容")?
        .iter()
        .any(|v| {
            v["cost"]
                .as_str()
                .is_some_and(|cost| cost == "p2p" || cost.starts_with("relay("))
        });
    Ok(PeerSnapshot {
        peers: parse_peers(&values)?,
        connected,
    })
}

fn parse_peers(values: &Value) -> Result<Vec<DiscoveredPeer>> {
    let values = values.as_array().context("组网组件返回的节点格式不兼容")?;
    Ok(values
        .iter()
        .filter_map(|v| {
            let cost = v["cost"].as_str()?;
            if cost == "Local" {
                return None;
            }
            let ipv4 = v["ipv4"].as_str()?.parse::<Ipv4Addr>().ok()?;
            if ipv4.octets()[..2] != [10, 203] {
                return None;
            }
            Some(DiscoveredPeer {
                ipv4: ipv4.to_string(),
                hostname: v["hostname"].as_str().unwrap_or("Hub").into(),
                direct: cost == "p2p",
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_tun_configuration_admits_only_remote_https_in_both_chains() {
        let mut config = NetworkConfig::default();
        config.network_name = "hub-fixture".into();
        let mut secrets = NetworkSecrets::default();
        secrets.network_secret = "test-secret".into();
        let value: toml::Value = configuration(&config, &secrets, 5555, 5556)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(value["flags"]["no_tun"].as_bool(), Some(true));
        assert_eq!(value["flags"]["relay_network_whitelist"].as_str(), Some(""));
        for chain in value["acl"]["acl_v1"]["chains"].as_array().unwrap() {
            assert_eq!(chain["default_action"].as_integer(), Some(2));
            assert_eq!(
                chain["rules"][0]["ports"][0].as_str(),
                Some(SERVICE_PORT.to_string().as_str())
            );
        }
        assert!(
            !configuration(&config, &secrets, 5555, 5556)
                .unwrap()
                .contains("3847")
        );
    }
    #[test]
    fn discovered_status_does_not_invent_connectivity() {
        let peers = parse_peers(&json!([
            {"cost":"Local","ipv4":"10.203.0.1"},
            {"cost":"p2p","ipv4":"10.203.0.2","hostname":"tch-fixture"},
            {"cost":"relay(2)","ipv4":"10.203.0.3","hostname":"tch-relayed"},
            {"cost":"p2p","ipv4":"192.168.0.1","hostname":"other"}
        ]))
        .unwrap();
        assert_eq!(peers.len(), 2);
        assert!(peers[0].direct);
        assert!(!peers[1].direct);
    }

    #[test]
    fn uninstall_refuses_a_directory_without_our_owner_marker() {
        let directory = tempfile::tempdir().unwrap();
        let package = package_dir(directory.path()).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        let sentinel = package.join("keep-user-file.txt");
        std::fs::write(&sentinel, "unowned").unwrap();
        assert!(uninstall(directory.path()).is_err());
        assert!(sentinel.is_file());
    }

    #[test]
    fn recovery_reclaims_only_owned_exited_directories() {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join("active");
        let owned = active.join("hub-owned");
        let unknown = active.join("hub-unknown");
        let live = active.join("hub-live");
        for directory in [&owned, &unknown, &live] {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(directory.join("network.toml"), "fixture").unwrap();
        }
        for (directory, pid) in [(&owned, None), (&live, Some(std::process::id()))] {
            std::fs::write(
                directory.join("hub-owner.json"),
                serde_json::to_vec(&ActiveOwner {
                    owner: OWNER.into(),
                    pid,
                })
                .unwrap(),
            )
            .unwrap();
        }
        recover_inactive(root.path()).unwrap();
        assert!(!owned.exists());
        assert!(unknown.join("network.toml").is_file());
        assert!(live.join("network.toml").is_file());
    }
}
