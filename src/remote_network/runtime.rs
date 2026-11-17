//! EasyTier runs inside an isolated, Hub-owned Tokio runtime. Configuration and
//! network secrets stay in memory; no CLI, management RPC or child process runs.
use std::{
    collections::HashSet,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use easytier::{
    common::config::TomlConfigLoader, instance::instance::Instance, proto::api::instance::Route,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::oneshot;

use super::{NetworkConfig, NetworkSecrets, SERVICE_PORT};

pub(super) const VERSION: &str = "2.6.4";
const OWNER: &str = "tiancai-hub-easytier-v1";

pub(super) fn platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("windows-x86_64"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        ("macos", "x86_64") => Ok("macos-x86_64"),
        ("macos", "aarch64") => Ok("macos-aarch64"),
        _ => anyhow::bail!("远程连接组件目前支持 Windows / macOS 的 x64 和 Arm64"),
    }
}

fn package_dir(root: &Path) -> Result<PathBuf> {
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

pub(super) fn ready(_root: &Path) -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// A previous Hub version may have downloaded an owned binary package. It is
/// never loaded by this version, but can still be explicitly removed.
pub(super) fn legacy_installed(root: &Path) -> bool {
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
}

pub(super) async fn prepare(root: &Path) -> Result<()> {
    ensure!(ready(root), "内置组网目前仅支持 Windows / macOS");
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
    pub socks_port: u16,
    worker: Option<thread::JoinHandle<()>>,
    stop: Option<oneshot::Sender<()>>,
    latest: Arc<Mutex<WorkerState>>,
}

impl Running {
    pub(super) fn is_running(&self) -> bool {
        self.worker.as_ref().is_some_and(|v| !v.is_finished())
            && self.latest.lock().is_ok_and(|v| v.running)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // Also covers cancellation while launch is awaiting readiness. Normal
        // shutdown additionally joins the owner thread before reporting stop.
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[derive(Default)]
struct WorkerState {
    running: bool,
    snapshot: PeerSnapshot,
    error: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct ActiveOwner {
    owner: String,
    pid: Option<u32>,
}

/// Reclaim only a marked, already-exited Hub instance. Live or unrecognised
/// directories stay untouched. Embedded runs no longer create these files.
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
    socks_port: u16,
) -> Result<String> {
    let ip: Ipv4Addr = config.ipv4.parse().context("虚拟地址无效")?;
    ensure!(ip.octets()[..2] == [10, 203], "虚拟地址超出 Hub 网络范围");
    let device_id = uuid::Uuid::parse_str(&config.device_id).context("设备身份无效")?;
    let mut document = toml::map::Map::new();
    document.insert(
        "hostname".into(),
        toml::Value::String(format!("tch-{}", &device_id.simple().to_string()[..28])),
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
    document.insert("flags".into(), toml::Value::try_from(json!({"no_tun":true,"use_smoltcp":true,"enable_encryption":true,"encryption_algorithm":"aes-gcm","enable_ipv6":false,"enable_exit_node":false,"relay_network_whitelist":"","enable_relay_foreign_network_kcp":false,"enable_relay_foreign_network_quic":false,"relay_all_peer_rpc":false,"accept_dns":false,"enable_udp_broadcast_relay":false}))?);
    let rule = json!({"name":"hub-remote-https","priority":1000,"action":1,"protocol":1,"ports":[SERVICE_PORT.to_string()],"enabled":true,"stateful":true,"destination_ips":[format!("{ip}/32")]});
    document.insert("acl".into(), toml::Value::try_from(json!({"acl_v1":{"chains":[
        {"name":"hub-inbound","chain_type":1,"enabled":true,"default_action":2,"rules":[rule.clone()]},
        {"name":"hub-forward","chain_type":3,"enabled":true,"default_action":2,"rules":[rule]}
    ]}}))?);
    Ok(toml::to_string(&toml::Value::Table(document))?)
}

pub(super) async fn launch(
    root: &Path,
    config: &NetworkConfig,
    secrets: &NetworkSecrets,
) -> Result<Running> {
    ensure!(ready(root), "内置组网目前仅支持 Windows / macOS");
    let socks_reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let socks_port = socks_reservation.local_addr()?.port();
    let configuration = configuration(config, secrets, socks_port)?;
    let latest = Arc::new(Mutex::new(WorkerState::default()));
    let (stop_tx, stop_rx) = oneshot::channel();
    let (started_tx, started_rx) = oneshot::channel();
    let worker_state = latest.clone();
    let worker = thread::Builder::new()
        .name("hub-remote-network".into())
        .spawn(move || {
            worker_main(
                configuration,
                socks_reservation,
                stop_rx,
                started_tx,
                worker_state,
            )
        })
        .context("无法创建内置组网后台")?;
    let running = Running {
        socks_port,
        worker: Some(worker),
        stop: Some(stop_tx),
        latest,
    };
    let started = match tokio::time::timeout(Duration::from_secs(20), started_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(anyhow::anyhow!("内置组网启动已退出，请重新启用")),
        Err(_) => Err(anyhow::anyhow!("内置组网启动超时，请检查节点设置后重试")),
    };
    match started {
        Ok(()) => Ok(running),
        Err(error) => {
            stop(running).await?;
            Err(error)
        }
    }
}

fn worker_main(
    configuration: String,
    socks_reservation: std::net::TcpListener,
    stop: oneshot::Receiver<()>,
    started: oneshot::Sender<Result<()>>,
    latest: Arc<Mutex<WorkerState>>,
) {
    let mut started = Some(started);
    let result = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("hub-network-io")
        .enable_all()
        .build()
    {
        Ok(runtime) => {
            let result = runtime.block_on(run_instance(
                configuration,
                socks_reservation,
                stop,
                &mut started,
                &latest,
            ));
            // Upstream owns some detached async tasks. Keeping them in this
            // runtime makes stopping independent of Hub's main task executor.
            runtime.shutdown_timeout(Duration::from_secs(3));
            result
        }
        Err(error) => Err(error).context("无法初始化内置组网后台"),
    };
    let error = result.err().map(|v| format!("{v:#}"));
    if let Ok(mut state) = latest.lock() {
        state.running = false;
        state.snapshot = PeerSnapshot::default();
        state.error = error.clone();
    }
    if let Some(started) = started {
        let _ = started.send(Err(anyhow::anyhow!(
            error.unwrap_or_else(|| "内置组网启动已取消".into())
        )));
    }
}

async fn run_instance(
    configuration: String,
    socks_reservation: std::net::TcpListener,
    mut stop: oneshot::Receiver<()>,
    started: &mut Option<oneshot::Sender<Result<()>>>,
    latest: &Arc<Mutex<WorkerState>>,
) -> Result<()> {
    // The upstream parser attaches the full input to parse errors. Report a
    // safe message here because the input contains the private network secret.
    let loader = TomlConfigLoader::new_from_str(&configuration)
        .map_err(|_| anyhow::anyhow!("内置组网配置无法读取，请重新加入个人网络"))?;
    drop(configuration);
    let mut instance = Instance::new(loader);
    drop(socks_reservation);
    let result = async {
        tokio::select! {
            biased;
            _ = &mut stop => return Ok(()),
            result = instance.run() => result.context("内置组网启动失败")?,
        }
        {
            let mut state = latest
                .lock()
                .map_err(|_| anyhow::anyhow!("组网状态读取失败"))?;
            state.running = true;
        }
        if started.take().is_some_and(|v| v.send(Ok(())).is_err()) {
            return Ok(());
        }
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = &mut stop => break,
                _ = interval.tick() => {},
            }
            let snapshot = tokio::select! {
                biased;
                _ = &mut stop => break,
                snapshot = read_peers(&instance) => snapshot,
            };
            let mut state = latest
                .lock()
                .map_err(|_| anyhow::anyhow!("组网状态读取失败"))?;
            state.snapshot = snapshot;
        }
        Ok(())
    }
    .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), instance.clear_resources()).await;
    drop(instance);
    result
}

pub(super) async fn stop(mut running: Running) -> Result<()> {
    if let Some(stop) = running.stop.take() {
        let _ = stop.send(());
    }
    if let Some(worker) = running.worker.take() {
        tokio::task::spawn_blocking(move || worker.join())
            .await
            .context("等待内置组网退出失败")?
            .map_err(|_| anyhow::anyhow!("内置组网后台意外退出"))?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct DiscoveredPeer {
    pub ipv4: String,
    pub hostname: String,
    pub direct: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PeerSnapshot {
    pub peers: Vec<DiscoveredPeer>,
    pub connected: bool,
}

pub(super) fn snapshot(running: &Running) -> Result<PeerSnapshot> {
    let state = running
        .latest
        .lock()
        .map_err(|_| anyhow::anyhow!("组网状态读取失败"))?;
    if let Some(error) = &state.error {
        anyhow::bail!("{error}");
    }
    ensure!(
        state.running && running.worker.as_ref().is_some_and(|v| !v.is_finished()),
        "内置组网后台已停止，请重新启用"
    );
    Ok(state.snapshot.clone())
}

async fn read_peers(instance: &Instance) -> PeerSnapshot {
    let manager = instance.get_peer_manager();
    let connected = !manager
        .get_peer_map()
        .list_peers_with_conn()
        .await
        .is_empty()
        || !manager
            .get_foreign_network_client()
            .get_peer_map()
            .list_peers_with_conn()
            .await
            .is_empty();
    let routes = manager.list_routes().await;
    let direct = routes
        .iter()
        .filter(|v| manager.has_directly_connected_conn(v.peer_id))
        .map(|v| v.peer_id)
        .collect();
    route_snapshot(manager.my_peer_id(), routes, &direct, connected)
}

fn route_snapshot(
    local_peer_id: u32,
    routes: Vec<Route>,
    direct: &HashSet<u32>,
    connected: bool,
) -> PeerSnapshot {
    let peers = if connected {
        routes
            .into_iter()
            .filter(|v| v.peer_id != local_peer_id && v.cost > 0)
            .filter_map(|v| {
                let ipv4: Ipv4Addr = v.ipv4_addr?.address?.into();
                if ipv4.octets()[..2] != [10, 203] {
                    return None;
                }
                Some(DiscoveredPeer {
                    ipv4: ipv4.to_string(),
                    hostname: if v.hostname.is_empty() {
                        "Hub".into()
                    } else {
                        v.hostname
                    },
                    direct: direct.contains(&v.peer_id),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    PeerSnapshot { peers, connected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easytier::common::config::ConfigLoader;

    #[tokio::test]
    async fn embedded_readiness_and_legacy_prepare_create_no_package_or_runtime() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            ready(directory.path()),
            cfg!(any(windows, target_os = "macos"))
        );
        if ready(directory.path()) {
            prepare(directory.path()).await.unwrap();
        }
        assert!(!legacy_installed(directory.path()));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn no_tun_configuration_admits_only_remote_https_in_both_chains() {
        let mut config = NetworkConfig::default();
        config.network_name = "hub-fixture".into();
        let mut secrets = NetworkSecrets::default();
        secrets.network_secret = "test-secret".into();
        let document = configuration(&config, &secrets, 5556).unwrap();
        let value: toml::Value = document.parse().unwrap();
        let loader = TomlConfigLoader::new_from_str(&document).unwrap();
        assert_eq!(loader.get_id().to_string(), config.device_id);
        assert!(loader.get_flags().no_tun);
        assert!(loader.get_flags().use_smoltcp);
        assert!(loader.get_flags().enable_encryption);
        assert_eq!(loader.get_flags().encryption_algorithm, "aes-gcm");
        assert!(loader.get_acl().is_some());
        assert_eq!(value["flags"]["no_tun"].as_bool(), Some(true));
        assert_eq!(value["flags"]["relay_network_whitelist"].as_str(), Some(""));
        for chain in value["acl"]["acl_v1"]["chains"].as_array().unwrap() {
            assert_eq!(chain["default_action"].as_integer(), Some(2));
            assert_eq!(
                chain["rules"][0]["ports"][0].as_str(),
                Some(SERVICE_PORT.to_string().as_str())
            );
        }
        assert!(!document.contains("3847"));
        assert!(!document.contains("rpc_portal"));
    }

    fn fixture_route(id: u32, ipv4: &str, cost: i32, hostname: &str) -> Route {
        Route {
            peer_id: id,
            ipv4_addr: Some(easytier::proto::common::Ipv4Inet {
                address: Some(ipv4.parse::<Ipv4Addr>().unwrap().into()),
                network_length: 16,
            }),
            cost,
            hostname: hostname.into(),
            ..Default::default()
        }
    }

    #[test]
    fn discovered_status_does_not_invent_connectivity() {
        let routes = vec![
            fixture_route(1, "10.203.0.1", 1, "local"),
            fixture_route(2, "10.203.0.2", 1, "tch-fixture"),
            fixture_route(3, "10.203.0.3", 2, "tch-relayed"),
            fixture_route(4, "192.168.0.1", 1, "other"),
            fixture_route(5, "10.203.0.5", 0, "unreachable"),
            Route {
                peer_id: 6,
                cost: 1,
                ..Default::default()
            },
        ];
        let snapshot = route_snapshot(1, routes.clone(), &HashSet::from([2, 4]), true);
        assert!(snapshot.connected);
        assert_eq!(snapshot.peers.len(), 2);
        assert_eq!(snapshot.peers[0].ipv4, "10.203.0.2");
        assert_eq!(snapshot.peers[0].hostname, "tch-fixture");
        assert!(snapshot.peers[0].direct);
        assert!(!snapshot.peers[1].direct);
        let disconnected = route_snapshot(1, routes, &HashSet::from([2, 4]), false);
        assert!(!disconnected.connected);
        assert!(disconnected.peers.is_empty());
        let relay_only = route_snapshot(1, Vec::new(), &HashSet::new(), true);
        assert!(relay_only.connected);
        assert!(relay_only.peers.is_empty());
    }

    #[tokio::test]
    async fn stop_signals_and_joins_owner_without_starting_a_network() {
        let (stop_tx, stop_rx) = oneshot::channel();
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let worker = thread::spawn(move || {
            let _ = stop_rx.blocking_recv();
            worker_stopped.store(true, std::sync::atomic::Ordering::Release);
        });
        let running = Running {
            socks_port: 0,
            worker: Some(worker),
            stop: Some(stop_tx),
            latest: Arc::new(Mutex::new(WorkerState {
                running: true,
                ..Default::default()
            })),
        };
        assert!(running.is_running());
        assert!(!snapshot(&running).unwrap().connected);
        stop(running).await.unwrap();
        assert!(stopped.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn dropping_launch_handle_cancels_the_owned_background() {
        let (stop_tx, stop_rx) = oneshot::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = stop_rx.blocking_recv();
            let _ = done_tx.send(());
        });
        drop(Running {
            socks_port: 0,
            worker: Some(worker),
            stop: Some(stop_tx),
            latest: Arc::new(Mutex::new(WorkerState::default())),
        });
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
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
        std::fs::write(
            package.join("hub-owner.json"),
            serde_json::to_vec(&Manifest {
                owner: OWNER.into(),
                version: VERSION.into(),
                platform: platform().unwrap().into(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(legacy_installed(directory.path()));
        uninstall(directory.path()).unwrap();
        assert!(!package.exists());
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
