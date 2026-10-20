use super::*;
use std::sync::{LazyLock, Mutex};
pub(super) const NAMES: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];
#[derive(Clone)]
pub(super) struct Probe {
    pub patchable: bool,
    pub message: String,
}
#[derive(Clone)]
struct Cached {
    home: PathBuf,
    asar_hash: String,
    at: std::time::Instant,
    result: Probe,
}
static CACHE: LazyLock<Mutex<Option<Cached>>> = LazyLock::new(|| Mutex::new(None));
pub(super) fn check(home: &Path, resources: &Path, apply: bool) -> Result<Probe> {
    let asar_hash = digest(&resources.join("app.asar"))?;
    if apply {
        let cache = CACHE
            .lock()
            .map_err(|_| anyhow::anyhow!("代理检查状态暂时不可用"))?;
        return Ok(cache.as_ref().filter(|c|c.home==home&&c.asar_hash==asar_hash&&c.at.elapsed()<Duration::from_secs(600)).map(|c|c.result.clone()).unwrap_or(Probe{patchable:false,message:"请先在 ChatGPT/Codex 仍运行时点击“检查”，确认子进程是否丢失代理；检查后再退出客户端修复。未改动代理设置。".into()}));
    }
    let items: Vec<Value> = {
        #[cfg(windows)]
        {
            let folder = tempfile::tempdir()?;
            let script = folder.path().join("probe.ps1");
            fs::write(
                &script,
                include_bytes!("../../assets/codex-repair/probe-proxy-windows.ps1"),
            )?;
            serde_json::from_slice(&system(
                Path::new("powershell.exe"),
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    script.to_str().context("探测路径无效")?,
                ],
                None,
                Duration::from_secs(30),
            )?)?
        }
        #[cfg(target_os = "macos")]
        {
            mac_probe()?
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Vec::new()
        }
    };
    let roots = items
        .iter()
        .filter(|v| v["root"] == true && v["readable"] == true)
        .collect::<Vec<_>>();
    let children = items
        .iter()
        .filter(|v| v["root"] == false && v["readable"] == true)
        .collect::<Vec<_>>();
    let root_names = roots
        .iter()
        .flat_map(|v| {
            v["names"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
        })
        .filter(|k| NAMES.contains(k))
        .collect::<std::collections::BTreeSet<_>>();
    let lost = root_names
        .iter()
        .filter(|key| {
            children.iter().any(|child| {
                !child["names"]
                    .as_array()
                    .is_some_and(|names| names.iter().any(|n| n.as_str() == Some(**key)))
            })
        })
        .copied()
        .collect::<Vec<_>>();
    let patchable = !lost.is_empty();
    let message = if roots.is_empty() {
        "没有读到运行中的 ChatGPT/Codex 根进程环境；请保持客户端打开再检查。没有据此判断代理丢失。"
            .into()
    } else if root_names.is_empty() {
        "客户端根进程没有已配置的代理环境变量。本项不会擅自填写代理地址；请先确认客户端的正常网络配置。".into()
    } else if children.is_empty() {
        "根进程已存在代理环境变量，但尚未读到工具子进程环境。请在客户端使用相应工具后重新检查。"
            .into()
    } else if patchable {
        format!(
            "确认根进程已有、工具子进程缺失的变量：{}。可退出客户端后修复继承生成器。",
            lost.join("、")
        )
    } else {
        "已读取根进程及工具子进程，未发现代理变量继承丢失。".into()
    };
    let result = Probe { patchable, message };
    *CACHE
        .lock()
        .map_err(|_| anyhow::anyhow!("代理检查状态暂时不可用"))? = Some(Cached {
        home: home.into(),
        asar_hash,
        at: std::time::Instant::now(),
        result: result.clone(),
    });
    Ok(result)
}
#[cfg(target_os = "macos")]
fn mac_probe() -> Result<Vec<Value>> {
    let bytes = system(
        Path::new("/bin/ps"),
        &["-axo", "pid=,ppid=,comm="],
        None,
        Duration::from_secs(10),
    )?;
    let text = String::from_utf8_lossy(&bytes);
    let mut inventory = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(pid), Some(parent)) = (
            parts.next().and_then(|s| s.parse::<u32>().ok()),
            parts.next().and_then(|s| s.parse::<u32>().ok()),
        ) {
            inventory.push((pid, parent, parts.collect::<Vec<_>>().join(" ")));
        }
    }
    let root = |s: &str| {
        [
            "ChatGPT.app/Contents/MacOS/ChatGPT",
            "Codex.app/Contents/MacOS/Codex",
        ]
        .iter()
        .any(|p| s.ends_with(p))
    };
    let mut ids = inventory
        .iter()
        .filter(|(_, _, s)| root(s))
        .map(|(p, _, _)| *p)
        .collect::<std::collections::HashSet<_>>();
    for _ in 0..24 {
        let next = inventory
            .iter()
            .filter(|(_, pp, _)| ids.contains(pp))
            .map(|(p, _, _)| *p)
            .collect::<Vec<_>>();
        let old = ids.len();
        ids.extend(next);
        if ids.len() == old {
            break;
        }
    }
    let mut out = Vec::new();
    for (pid, _, name) in inventory {
        if !ids.contains(&pid)
            || (!root(&name) && !name.contains("node_repl") && !name.ends_with("/node"))
        {
            continue;
        }
        let raw = system(
            Path::new("/bin/ps"),
            &["eww", "-p", &pid.to_string(), "-o", "command="],
            None,
            Duration::from_secs(5),
        );
        let readable = raw.is_ok();
        let raw = raw.unwrap_or_default();
        let text = String::from_utf8_lossy(&raw);
        let names = NAMES
            .into_iter()
            .filter(|key| {
                text.split_whitespace().any(|v| {
                    v.strip_prefix(key)
                        .is_some_and(|tail| tail.starts_with('='))
                })
            })
            .collect::<Vec<_>>();
        out.push(json!({"pid":pid,"root":root(&name),"readable":readable,"names":names}));
    }
    Ok(out)
}
