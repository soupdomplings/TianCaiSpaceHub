use super::{activity::Activity, clients::ClientId};
use serde::Serialize;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRuntime {
    pub client: ClientId,
    pub opened: Option<bool>,
    pub configured: Option<bool>,
    pub model_count: Option<usize>,
    pub requests: Vec<Activity>,
    pub mcps: Vec<ConfiguredMcp>,
}
#[derive(Clone, Serialize)]
pub struct ConfiguredMcp {
    pub id: String,
    pub name: String,
}
static READ_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
type Cache = (String, std::time::Instant, Vec<ClientRuntime>);
static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
pub fn snapshot(
    path: &std::path::Path,
    config: &crate::config::AppConfig,
    requests: &[Activity],
) -> Vec<ClientRuntime> {
    let key = format!(
        "{}:{:?}:{}:{}",
        path.display(),
        config.revision,
        config.bind,
        config.ai_gateway.enabled
    );
    let cached = CACHE.lock().ok().and_then(|s| {
        s.as_ref()
            .filter(|(k, at, _)| *k == key && at.elapsed() < std::time::Duration::from_secs(2))
            .map(|(_, _, r)| r.clone())
    });
    if let Some(rows) = cached {
        return attach_requests(rows, requests);
    }
    let Ok(_guard) = READ_GATE.try_lock() else {
        return ClientId::ALL
            .into_iter()
            .map(|client| ClientRuntime {
                client,
                opened: None,
                configured: None,
                model_count: None,
                requests: requests
                    .iter()
                    .filter(|r| r.client == Some(client))
                    .cloned()
                    .collect(),
                mcps: vec![],
            })
            .collect();
    };
    let names = process_names();
    let library = super::resources::Library::new(path)
        .and_then(|lib| lib.load().map(|catalog| (lib, catalog)));
    let targets = library
        .as_ref()
        .ok()
        .and_then(|(lib, catalog)| lib.targets(catalog).ok());
    let rows: Vec<_> = ClientId::ALL
        .into_iter()
        .map(|client| {
            let opened = client_opened(client, names.as_deref());
            let model_count = match client {
                ClientId::Codex => {
                    let status = crate::codex_app_config::inspect_codex_app_config(
                        None,
                        &config.remote_control_base_url(),
                    );
                    if status.config_path.exists() && status.config_error.is_some() {
                        None
                    } else {
                        Some(usize::from(status.provider_ok))
                    }
                }
                ClientId::WorkBuddy => crate::workbuddy_config::load_selected(config, None)
                    .ok()
                    .filter(|s| s.error.is_none())
                    .map(|s| s.entries.iter().filter(|e| e.configured).count()),
                ClientId::Tiangong => crate::gmclaw_config::load(config)
                    .ok()
                    .filter(|s| s.error.is_none())
                    .map(|s| s.entries.iter().filter(|e| e.configured).count()),
                _ => targets.as_ref().and_then(|targets| {
                    let states = config
                        .ai_gateway
                        .providers
                        .iter()
                        .filter(|p| p.enabled)
                        .filter_map(|p| p.desktop_binding.as_ref())
                        .filter(|b| b.client == client)
                        .map(|b| {
                            targets
                                .iter()
                                .find(|t| t.id == b.target_id)
                                .ok_or_else(|| anyhow::anyhow!("配置位置不存在"))
                                .and_then(|t| {
                                    if client == ClientId::WebStorm && t.model_mode == "native" {
                                        super::webstorm_native::using(
                                            b,
                                            t,
                                            config.local_listen_port().unwrap_or(3210),
                                        )
                                    } else {
                                        super::models::using(b, t)
                                    }
                                })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()
                        .ok()?;
                    Some(states.into_iter().filter(|v| *v).count())
                }),
            };
            let configured = model_count.map(|n| n > 0 && config.ai_gateway.enabled);
            // This is configuration evidence only. It never starts MCP, probes
            // a server or queries a client bridge. Reuse the two-second cache.
            let mcps = library
                .as_ref()
                .ok()
                .zip(targets.as_ref())
                .map(|((lib, catalog), targets)| {
                    catalog
                        .resources
                        .iter()
                        .filter(|r| r.kind == super::resources::ResourceKind::Mcp)
                        .filter(|r| {
                            targets.iter().any(|t| {
                                t.client == client
                                    && t.present
                                    && if client == ClientId::Tiangong {
                                        catalog.uses.iter().any(|u| {
                                            !u.pending
                                                && u.resource_id == r.id
                                                && u.target_id == t.id
                                        })
                                    } else {
                                        lib.is_using(r, t).unwrap_or(false)
                                    }
                            })
                        })
                        .map(|r| ConfiguredMcp {
                            id: r.id.clone(),
                            name: r.name.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            ClientRuntime {
                client,
                opened,
                configured,
                model_count,
                requests: requests
                    .iter()
                    .filter(|r| r.client == Some(client))
                    .cloned()
                    .collect(),
                mcps,
            }
        })
        .collect();
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((key, std::time::Instant::now(), rows.clone()));
    }
    rows
}
fn client_opened(client: ClientId, names: Option<&[String]>) -> Option<bool> {
    let patterns: &[&str] = match client {
        ClientId::Codex => &["codex.exe", "codex", "chatgpt.exe", "chatgpt"],
        ClientId::WorkBuddy => &["workbuddy.exe", "workbuddy"],
        ClientId::Tiangong => &[
            "tiangong.exe",
            "tiangong-desktop.exe",
            "天工work.exe",
            "tiangong",
        ],
        ClientId::VsCode => &["code.exe", "code"],
        ClientId::DeepSeekHarness => &[
            "deepseek harness.exe",
            "deepseek harness",
            "deepseek-harness.exe",
            "deepseek-harness",
            "dsh.exe",
            "dsh",
        ],
        ClientId::ZCode => &["zcode.exe", "zcode"],
        ClientId::WebStorm => &["webstorm64.exe", "webstorm.exe", "webstorm"],
    };
    names.map(|names| {
        names.iter().any(|name| {
            patterns
                .iter()
                .any(|pattern| name.eq_ignore_ascii_case(pattern))
        })
    })
}
pub(crate) fn attach_requests(
    mut rows: Vec<ClientRuntime>,
    requests: &[Activity],
) -> Vec<ClientRuntime> {
    for row in &mut rows {
        row.requests = requests
            .iter()
            .filter(|r| r.client == Some(row.client))
            .cloned()
            .collect();
    }
    rows
}
#[cfg(windows)]
fn process_names() -> Option<Vec<String>> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut names = Vec::new();
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            let length = entry
                .szExeFile
                .iter()
                .position(|v| *v == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..length]).to_lowercase());
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        Some(names)
    }
}
#[cfg(target_os = "macos")]
fn process_names() -> Option<Vec<String>> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|p| std::path::Path::new(p.trim()).file_name())
            .map(|s| s.to_string_lossy().to_lowercase())
            .collect(),
    )
}
#[cfg(not(any(windows, target_os = "macos")))]
fn process_names() -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_desktop_process_with_spaces_is_opened() {
        let names = vec!["explorer.exe".into(), "DeepSeek Harness.exe".into()];
        assert_eq!(
            client_opened(ClientId::DeepSeekHarness, Some(&names)),
            Some(true)
        );
        assert_eq!(client_opened(ClientId::Codex, Some(&names)), Some(false));
        let names = vec!["DeepSeek Harness".into()];
        assert_eq!(
            client_opened(ClientId::DeepSeekHarness, Some(&names)),
            Some(true)
        );
    }

    #[test]
    fn harness_legacy_process_names_still_match_without_substring_false_positives() {
        for name in ["deepseek-harness.exe", "deepseek-harness", "dsh.exe", "dsh"] {
            assert_eq!(
                client_opened(ClientId::DeepSeekHarness, Some(&[name.into()])),
                Some(true),
                "{name}"
            );
        }
        let unrelated = [
            "DeepSeek Harness Helper.exe",
            "DeepSeek Harness.exe.bak",
            "deepseek.exe",
            "dsh-service.exe",
        ]
        .map(String::from);
        assert_eq!(
            client_opened(ClientId::DeepSeekHarness, Some(&unrelated)),
            Some(false)
        );
    }

    #[test]
    fn missing_process_snapshot_stays_unknown_and_empty_snapshot_stays_closed() {
        for client in ClientId::ALL {
            assert_eq!(client_opened(client, None), None);
            assert_eq!(client_opened(client, Some(&[])), Some(false));
        }
    }

    #[test]
    fn other_editors_match_main_process_names_without_matching_other_clients() {
        for (client, main, other) in [
            (ClientId::VsCode, "Code.exe", "Code - Insiders.exe"),
            (ClientId::ZCode, "ZCode.exe", "ZCode Helper.exe"),
            (ClientId::WebStorm, "webstorm64.exe", "webstorm64.exe.bak"),
            (ClientId::WebStorm, "webstorm.exe", "webstorm-toolbox.exe"),
        ] {
            assert_eq!(client_opened(client, Some(&[main.into()])), Some(true));
            assert_eq!(client_opened(client, Some(&[other.into()])), Some(false));
        }
    }
}
