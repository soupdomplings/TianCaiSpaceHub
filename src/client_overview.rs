//! Secret-free summaries for the selected desktop integration. Reading this
//! view never provisions credentials, launches a desktop app, or creates tasks.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use axum::http::{HeaderMap, header::AUTHORIZATION};

use crate::{config::AppConfig, gmclaw_runtime::GmClawConnectionState};

// A desktop configuration can be locked by another process. Timed-out reads
// must not accumulate a new blocked filesystem worker on every GUI refresh.
static MODEL_READ: Mutex<()> = Mutex::new(());

// Model access is HTTP, not the Harness control connection. Retain only
// evidence observed in this Hub process, tied to the current desktop process
// set and local route. Neither saved settings nor upstream success is proof
// that the desktop reached this Hub instance.
static MODEL_ACTIVITY: LazyLock<Mutex<HashMap<String, ModelActivity>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const MAX_MODEL_ACTIVITY: usize = 256;

struct ModelActivity {
    desktop_process_ids: Vec<u32>,
    observed_at: Instant,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientOverview {
    pub gmclaw: ClientOverviewEntry,
    pub workbuddy: ClientOverviewEntry,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientOverviewEntry {
    pub model: ClientModelStatus,
    pub bridge: ClientBridgeStatus,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientModelStatus {
    /// Indicates that the client's configuration exists, not process liveness.
    pub installed: bool,
    pub configured: bool,
    pub count: usize,
    pub error: Option<String>,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub current_local_route: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientBridgeStatus {
    pub enabled: bool,
    pub state: String,
    pub detail: String,
}

fn unavailable_model() -> ClientModelStatus {
    ClientModelStatus {
        error: Some("暂时无法读取模型接入状态，请刷新后重试".into()),
        ..Default::default()
    }
}

fn activity_key(config: &AppConfig, entry_id: &str) -> Option<String> {
    if !config.ai_gateway.enabled {
        return None;
    }
    let address = config.bind.parse::<SocketAddr>().ok()?;
    if address.port() == 0 || !(address.ip().is_loopback() || address.ip().is_unspecified()) {
        return None;
    }
    let name = crate::ai_gateway::config::gmclaw_provider_name(entry_id)?;
    let mut providers = config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.name.eq_ignore_ascii_case(&name));
    let provider = providers.next()?;
    if providers.next().is_some() || !provider.enabled {
        return None;
    }
    Some(format!("{address}|{name}"))
}

fn current_local_url(config: &AppConfig, entry_id: &str, local_url: &str) -> bool {
    let Ok(address) = config.bind.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(url) = url::Url::parse(local_url) else {
        return false;
    };
    let path = if entry_id == crate::gmclaw_config::GMCLAW_LEGACY_ENTRY_ID {
        "/ai-gateway/gmclaw/v1".to_owned()
    } else {
        format!("/ai-gateway/gmclaw/{entry_id}/v1")
    };
    url.scheme() == "http"
        && url.port_or_known_default() == Some(address.port())
        && match url.host_str() {
            Some("localhost") => address.ip().is_loopback() || address.ip().is_unspecified(),
            Some(host) => host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| {
                    ip.is_loopback()
                        && ip.is_ipv6() == address.is_ipv6()
                        && (address.ip().is_unspecified() || ip == address.ip())
                }),
            None => false,
        }
        && url.path() == path
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && activity_key(config, entry_id).is_some()
}

/// Called after a request has reached a valid dedicated model route. This
/// checks the generated local client marker without changing gateway request
/// acceptance. A failed upstream call still proves the local HTTP connection.
pub(crate) async fn record_model_activity(
    config: &AppConfig,
    entry_id: Option<&str>,
    headers: &HeaderMap,
) {
    let generated_client = headers
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("bearer")
                && token.trim() == crate::gmclaw_config::GMCLAW_LOCAL_KEY
        });
    if !generated_client {
        return;
    }
    let entry_id = entry_id.unwrap_or(crate::gmclaw_config::GMCLAW_LEGACY_ENTRY_ID);
    let Some(key) = activity_key(config, entry_id) else {
        return;
    };
    let Ok(process_ids) = tokio::time::timeout(
        Duration::from_millis(1800),
        crate::gmclaw_runtime::desktop_process_ids(),
    )
    .await
    else {
        return;
    };
    if process_ids.is_empty() {
        return;
    }
    let Ok(mut activity) = MODEL_ACTIVITY.lock() else {
        return;
    };
    if activity.len() >= MAX_MODEL_ACTIVITY && !activity.contains_key(&key) {
        if let Some(oldest) = activity
            .iter()
            .min_by_key(|(_, value)| value.observed_at)
            .map(|(key, _)| key.clone())
        {
            activity.remove(&oldest);
        }
    }
    activity.insert(
        key,
        ModelActivity {
            desktop_process_ids: process_ids,
            observed_at: Instant::now(),
        },
    );
}

fn model_summaries(config: &AppConfig) -> (ClientModelStatus, ClientModelStatus, Vec<String>) {
    let Ok(_guard) = MODEL_READ.try_lock() else {
        return (unavailable_model(), unavailable_model(), Vec::new());
    };
    let mut gmclaw_routes = Vec::new();
    let gmclaw = crate::gmclaw_config::load(config)
        .map(|status| {
            gmclaw_routes = status
                .entries
                .iter()
                .filter(|entry| {
                    entry.configured && current_local_url(config, &entry.entry_id, &entry.local_url)
                })
                .filter_map(|entry| activity_key(config, &entry.entry_id))
                .collect();
            ClientModelStatus {
                installed: status.exists,
                configured: status.entries.iter().any(|entry| entry.configured),
                count: status
                    .entries
                    .iter()
                    .filter(|entry| entry.configured)
                    .count(),
                error: status
                    .error
                    .filter(|_| status.exists)
                    .map(|_| "天工模型配置需要检查，请查看接入页".into()),
                current_local_route: !gmclaw_routes.is_empty(),
                ..Default::default()
            }
        })
        .unwrap_or_else(|_| unavailable_model());
    let workbuddy = crate::workbuddy_config::load_selected(config, None)
        .map(|status| ClientModelStatus {
            installed: status.exists,
            configured: status.entries.iter().any(|entry| entry.configured),
            count: status
                .entries
                .iter()
                .filter(|entry| entry.configured)
                .count(),
            error: status
                .error
                .map(|_| "WorkBuddy 模型配置需要检查，请查看接入页".into()),
            ..Default::default()
        })
        .unwrap_or_else(|_| unavailable_model());
    (gmclaw, workbuddy, gmclaw_routes)
}

pub async fn snapshot(config: AppConfig) -> ClientOverview {
    let model_config = config.clone();
    let model_task = tokio::task::spawn_blocking(move || model_summaries(&model_config));
    let (models, runtime, desktop_process_ids) = tokio::join!(
        tokio::time::timeout(Duration::from_millis(1800), model_task),
        tokio::time::timeout(
            Duration::from_millis(1800),
            crate::gmclaw_runtime::overview_status(&config.gmclaw_bridge)
        ),
        tokio::time::timeout(
            Duration::from_millis(1800),
            crate::gmclaw_runtime::desktop_process_ids()
        ),
    );
    let (mut gmclaw, workbuddy, gmclaw_routes) = models
        .ok()
        .and_then(Result::ok)
        .unwrap_or_else(|| (unavailable_model(), unavailable_model(), Vec::new()));
    let desktop_process_ids = desktop_process_ids.unwrap_or_default();
    gmclaw.running = !desktop_process_ids.is_empty();
    if let Ok(mut activity) = MODEL_ACTIVITY.lock() {
        // A closed/replaced desktop, removed route or changed Hub listener
        // invalidates evidence. Logs persisted by an earlier Hub never enter
        // this map, and Harness authorization does not enter it either.
        activity.retain(|key, value| {
            gmclaw.running
                && value.desktop_process_ids == desktop_process_ids
                && (gmclaw.error.is_some() || gmclaw_routes.contains(key))
        });
        gmclaw.connected =
            gmclaw.configured && gmclaw_routes.iter().any(|key| activity.contains_key(key));
    }
    let (state, detail) = match runtime {
        Ok(status) => (
            match status.state {
                GmClawConnectionState::NotConfigured => "not_configured",
                GmClawConnectionState::NotRunning => "not_running",
                GmClawConnectionState::Unverified => "unverified",
                GmClawConnectionState::Connected => "connected",
                GmClawConnectionState::AuthFailed => "auth_failed",
                GmClawConnectionState::Error => "error",
            },
            status.detail,
        ),
        Err(_) => ("unverified", "连接检查尚未完成，请稍后刷新".into()),
    };
    ClientOverview {
        gmclaw: ClientOverviewEntry {
            model: gmclaw,
            bridge: ClientBridgeStatus {
                enabled: config.gmclaw_bridge.enabled,
                state: state.into(),
                detail,
            },
        },
        workbuddy: ClientOverviewEntry {
            model: workbuddy,
            bridge: ClientBridgeStatus {
                enabled: false,
                state: "unsupported".into(),
                detail: "WorkBuddy 外部任务执行暂不支持；模型接入可独立使用".into(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_gateway::config::ProviderConfig;

    fn model_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.bind = "127.0.0.1:3847".into();
        config.ai_gateway.enabled = true;
        config.ai_gateway.providers.push(ProviderConfig {
            name: "gmclaw:entry-a".into(),
            enabled: true,
            ..Default::default()
        });
        config
    }

    #[test]
    fn previous_overview_fields_do_not_claim_a_connection() {
        let model: ClientModelStatus = serde_json::from_value(serde_json::json!({
            "installed": true,
            "configured": true,
            "count": 2,
            "error": null,
        }))
        .unwrap();
        assert!(!model.running);
        assert!(!model.connected);
        assert!(!model.current_local_route);
    }

    #[test]
    fn local_model_connection_is_specific_to_the_hub_listener_and_entry() {
        let mut config = model_config();
        let endpoint = "http://127.0.0.1:3847/ai-gateway/gmclaw/entry-a/v1";
        assert!(current_local_url(&config, "entry-a", endpoint));
        for endpoint in [
            "http://127.0.0.1:3848/ai-gateway/gmclaw/entry-a/v1",
            "http://[::1]:3847/ai-gateway/gmclaw/entry-a/v1",
            "http://127.0.0.2:3847/ai-gateway/gmclaw/entry-a/v1",
            "http://127.0.0.1:3847/ai-gateway/gmclaw/entry-b/v1",
            "http://127.0.0.1:3847/ai-gateway/gmclaw/entry-a/v1?query=1",
            "https://127.0.0.1:3847/ai-gateway/gmclaw/entry-a/v1",
        ] {
            assert!(!current_local_url(&config, "entry-a", endpoint));
        }
        let original = activity_key(&config, "entry-a").unwrap();
        config.bind = "127.0.0.1:3848".into();
        assert_ne!(activity_key(&config, "entry-a").unwrap(), original);
        config.ai_gateway.providers[0].enabled = false;
        assert!(activity_key(&config, "entry-a").is_none());
    }

    #[test]
    fn ambiguous_routes_cannot_prove_a_model_connection() {
        let mut config = model_config();
        let mut duplicate = config.ai_gateway.providers[0].clone();
        duplicate.name = "GMCLAW:entry-a".into();
        config.ai_gateway.providers.push(duplicate);
        assert!(activity_key(&config, "entry-a").is_none());
    }
}
