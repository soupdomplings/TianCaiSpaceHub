use std::{sync::Weak, time::Duration};

use anyhow::Result;
use reqwest::Client;
use tokio::sync::Mutex;

use crate::{app_state::AppState, config::OutboundProxyConfig};

#[derive(Clone, PartialEq, Eq)]
struct NetworkSettings {
    proxy: OutboundProxyConfig,
    local_port: Option<u16>,
}

enum SettingsSource {
    Hub(Weak<AppState>),
    #[cfg(test)]
    Fixture(Mutex<NetworkSettings>),
}

struct CachedClients {
    settings: NetworkSettings,
    auth: Client,
    mcp: Client,
}

/// Shares only connection pools. Login generations and local MCP access stay in Runtime.
pub(crate) struct NvwaHttpClients {
    source: SettingsSource,
    cache: Mutex<Option<CachedClients>>,
}

impl NvwaHttpClients {
    pub(crate) fn new(state: Weak<AppState>) -> Self {
        Self {
            source: SettingsSource::Hub(state),
            cache: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            source: SettingsSource::Fixture(Mutex::new(NetworkSettings {
                proxy: OutboundProxyConfig {
                    mode: crate::config::OutboundProxyMode::Direct,
                    url: String::new(),
                },
                local_port: None,
            })),
            cache: Mutex::new(None),
        }
    }

    pub(crate) async fn auth(&self) -> Result<Client> {
        self.client(true).await
    }

    pub(crate) async fn mcp(&self) -> Result<Client> {
        self.client(false).await
    }

    async fn settings(&self) -> Result<NetworkSettings> {
        match &self.source {
            SettingsSource::Hub(state) => {
                let state = state
                    .upgrade()
                    .ok_or_else(|| anyhow::anyhow!("Hub 网络配置当前不可用"))?;
                let config = state.config.lock().await;
                Ok(NetworkSettings {
                    proxy: config.outbound_proxy.clone(),
                    local_port: config.local_listen_port(),
                })
            }
            #[cfg(test)]
            SettingsSource::Fixture(settings) => Ok(settings.lock().await.clone()),
        }
    }

    async fn client(&self, authentication: bool) -> Result<Client> {
        // Every outbound request reads current in-memory settings. Replacing
        // pools leaves admitted requests running with their existing client.
        let settings = self.settings().await?;
        let mut cache = self.cache.lock().await;
        if cache.as_ref().is_none_or(|c| c.settings != settings) {
            let build = |builder| {
                crate::outbound_http::apply_async_proxy(
                    builder,
                    &settings.proxy,
                    settings.local_port,
                )
                .and_then(|builder| builder.build().map_err(Into::into))
            };
            let auth = build(
                Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(30)),
            )
            .map_err(|_| anyhow::anyhow!("无法初始化 NVWA 认证连接，请检查 Hub 连接与网络设置"))?;
            let mcp = build(
                Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(20))
                    .read_timeout(Duration::from_secs(300))
                    .timeout(Duration::from_secs(300)),
            )
            .map_err(|_| anyhow::anyhow!("无法初始化 NVWA MCP 连接，请检查 Hub 连接与网络设置"))?;
            *cache = Some(CachedClients {
                settings,
                auth,
                mcp,
            });
        }
        let clients = cache.as_ref().expect("HTTP clients were initialized");
        Ok(if authentication {
            clients.auth.clone()
        } else {
            clients.mcp.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutboundProxyMode;
    use axum::{Router, response::Redirect, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (url, task)
    }

    async fn configure(http: &NvwaHttpClients, mode: OutboundProxyMode, url: &str) {
        let SettingsSource::Fixture(settings) = &http.source else {
            panic!("fixture settings required");
        };
        settings.lock().await.proxy = OutboundProxyConfig {
            mode,
            url: url.into(),
        };
    }

    #[tokio::test]
    async fn current_proxy_applies_to_both_pools_without_stale_fallback_or_recreation() {
        let (origin, origin_task) = serve(Router::new().fallback(|| async { "origin" })).await;
        let (proxy, proxy_task) = serve(Router::new().fallback(|| async { "proxy" })).await;
        let http = NvwaHttpClients::for_test();
        for (mode, proxy_url, expected) in [
            (OutboundProxyMode::Custom, proxy.as_str(), "proxy"),
            (OutboundProxyMode::Direct, "", "origin"),
        ] {
            configure(&http, mode, proxy_url).await;
            for client in [http.auth().await.unwrap(), http.mcp().await.unwrap()] {
                let text = client
                    .get(&origin)
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert_eq!(text, expected);
            }
        }
        configure(&http, OutboundProxyMode::Custom, "ftp://fixture.invalid").await;
        assert!(http.auth().await.is_err());
        assert!(http.mcp().await.is_err());
        configure(&http, OutboundProxyMode::Direct, "").await;
        assert!(http.auth().await.is_ok());
        assert!(http.mcp().await.is_ok());
        origin_task.abort();
        proxy_task.abort();
    }

    #[tokio::test]
    async fn auth_and_mcp_do_not_follow_redirects_with_credentials() {
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = hits.clone();
        let router = Router::new()
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/target") }),
            )
            .route(
                "/target",
                get(move || {
                    let counted = counted.clone();
                    async move {
                        counted.fetch_add(1, Ordering::SeqCst);
                        "should not be reached"
                    }
                }),
            );
        let (url, task) = serve(router).await;
        let http = NvwaHttpClients::for_test();
        for client in [http.auth().await.unwrap(), http.mcp().await.unwrap()] {
            let response = client
                .get(format!("{url}/redirect"))
                .header("authorization", "fixture-credential")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn changing_proxy_keeps_inflight_mcp_request_running_once() {
        let origin_hits = Arc::new(AtomicUsize::new(0));
        let count_origin = origin_hits.clone();
        let (origin, origin_task) = serve(Router::new().fallback(move || {
            let counted = count_origin.clone();
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                "new direct request"
            }
        }))
        .await;
        let proxy_hits = Arc::new(AtomicUsize::new(0));
        let count_proxy = proxy_hits.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let proxy_entered = entered.clone();
        let proxy_release = release.clone();
        let (proxy, proxy_task) = serve(Router::new().fallback(move || {
            let counted = count_proxy.clone();
            let entered = proxy_entered.clone();
            let release = proxy_release.clone();
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                entered.notify_one();
                release.notified().await;
                "original proxied request"
            }
        }))
        .await;
        let http = NvwaHttpClients::for_test();
        configure(&http, OutboundProxyMode::Custom, &proxy).await;
        let old_client = http.mcp().await.unwrap();
        let old_url = origin.clone();
        let old_request = tokio::spawn(async move {
            old_client
                .post(old_url)
                .body("fixture-original-request")
                .send()
                .await?
                .text()
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        configure(&http, OutboundProxyMode::Direct, "").await;
        let current = http
            .mcp()
            .await
            .unwrap()
            .post(&origin)
            .body("fixture-new-request")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(current, "new direct request");
        assert!(!old_request.is_finished());
        release.notify_one();
        let original = tokio::time::timeout(Duration::from_secs(5), old_request)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(original, "original proxied request");
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
        assert_eq!(origin_hits.load(Ordering::SeqCst), 1);
        origin_task.abort();
        proxy_task.abort();
    }
}
