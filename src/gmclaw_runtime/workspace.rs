//! Explicit external-model workspace startup. The renderer override lives only
//! in this CDP connection; installed files and official account records stay native.
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use url::Url;

use crate::{
    config::AppConfig,
    desktop::renderer_cdp::{Cdp, MAX_BYTES},
    gmclaw_config::GmClawConfigStatus,
};

const READY_TIMEOUT: Duration = Duration::from_secs(30);
static ENHANCE_LOCK: Mutex<()> = Mutex::const_new(());
static SESSION: Mutex<Option<WorkspaceSession>> = Mutex::const_new(None);

struct WorkspaceSession {
    task: JoinHandle<()>,
    options: watch::Sender<WorkspaceOptions>,
    socket: Url,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceOptions {
    external_model_ids: Vec<String>,
    preferred_model_id: String,
}

pub(crate) fn workspace_options(
    config: &AppConfig,
    preferred: Option<&str>,
) -> Result<WorkspaceOptions> {
    select_options(config, crate::gmclaw_config::load(config)?, preferred)
}

fn select_options(
    config: &AppConfig,
    status: GmClawConfigStatus,
    preferred: Option<&str>,
) -> Result<WorkspaceOptions> {
    if let Some(error) = status.error.as_deref() {
        anyhow::bail!("无法读取已保存的天工外部模型：{error}");
    }
    let entries: Vec<_> = status
        .entries
        .iter()
        .filter(|entry| {
            entry.configured
                && config.ai_gateway.providers.iter().any(|provider| {
                    crate::ai_gateway::config::gmclaw_provider_name(&entry.entry_id).as_deref()
                        == Some(provider.name.as_str())
                        && provider.enabled
                        && provider.tiangong_builtin.is_none()
                })
        })
        .collect();
    ensure!(
        !entries.is_empty(),
        "请先在天工 Work 的模型页保存一个可用的外部模型，再使用增强启动"
    );
    let preferred_model_id = preferred
        .and_then(|id| entries.iter().find(|entry| entry.model_id == id))
        .or_else(|| entries.iter().find(|entry| entry.active))
        .unwrap_or(&entries[0])
        .model_id
        .clone();
    Ok(WorkspaceOptions {
        external_model_ids: entries.iter().map(|entry| entry.model_id.clone()).collect(),
        preferred_model_id,
    })
}

pub(crate) async fn enhance_workspace(options: WorkspaceOptions) -> Result<()> {
    let _guard = ENHANCE_LOCK.lock().await;
    let installation = tokio::task::spawn_blocking(super::discover).await??;
    let executable = installation.executable.context("未找到天工 Work 安装")?;
    let socket = super::display::target(&executable, &installation.process_ids).await?;
    let mut cdp = Cdp::connect(&socket, "天工").await?;
    let probe = cdp.evaluate(PROBE).await?;
    let installed = probe["installed"] == true;
    if installed {
        ensure!(
            probe["ready"] == true,
            "天工增强工作区仍在准备，请查看天工页面提示后重试"
        );
        update_models(&mut cdp, &options).await?;
        let previous = SESSION.lock().await;
        if let Some(session) = previous.as_ref()
            && session.socket == socket
            && !session.task.is_finished()
        {
            session.options.send_replace(options);
            let _ = cdp.socket.close(None).await;
            return Ok(());
        }
    }
    // A normal workspace can contain drafts or active tasks. Only an unopened
    // login/startup view is refreshed automatically to load the memory adapter.
    ensure!(
        installed || (probe["workspaceOpen"] != true && probe["hasInput"] != true),
        "天工已在工作区，本次保留当前任务和草稿；首次启用免登录模式，请先退出天工官方账号回到登录页，再点增强启动"
    );
    let entries = probe["entries"]
        .as_array()
        .context("天工启动界面尚未准备，请稍后重试增强启动")?;
    ensure!(entries.len() == 1, "天工启动脚本结构不兼容，已保留普通启动");
    let entry = entries[0].as_str().context("天工启动脚本地址无效")?;
    let page = probe["page"].as_str().context("天工启动页面地址无效")?;
    ensure!(
        same_renderer_entry(page, entry),
        "天工启动脚本不属于当前安装"
    );
    let entry = entry.to_owned();
    let mut previous = SESSION.lock().await;
    if let Some(session) = previous.take() {
        session.task.abort();
    }
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (options_tx, options_rx) = watch::channel(options);
    let task = tokio::spawn(async move {
        let outcome = tokio::time::timeout(READY_TIMEOUT, async {
            cdp.call("Network.enable", json!({})).await?;
            cdp.call("Network.setCacheDisabled", json!({"cacheDisabled":true})).await?;
            cdp.call("Fetch.enable", json!({"patterns":[{"urlPattern":entry,"requestStage":"Response","resourceType":"Script"}]})).await?;
            if installed {
                // Reattach after a Hub restart without reloading an active workspace.
                ensure!(cdp.evaluate(PROBE).await?["ready"] == true,
                    "天工工作区已切换，请重新点击增强启动");
                Ok(())
            } else {
                // Check again immediately before refreshing, without changing a draft.
                let probe = cdp.evaluate(PROBE).await?;
                ensure!(probe["workspaceOpen"] != true && probe["hasInput"] != true,
                    "天工已进入工作区，本次保留任务和草稿；请正常退出后增强启动");
                cdp.call("Page.reload", json!({"ignoreCache":true})).await?;
                let options = options_rx.borrow().clone();
                wait_ready(&mut cdp, &options).await
            }
        }).await.unwrap_or_else(|_| Err(anyhow::anyhow!(
            "天工增强工作区准备超时，请查看天工页面提示；可以重新使用增强启动"
        )));
        if outcome.is_ok() {
            let _ = ready_tx.send(outcome);
            // Reapply only when this renderer requests its entry again (for
            // example a manual refresh). No polling or periodic source scans.
            while let Ok(event) = cdp.event().await {
                let options = options_rx.borrow().clone();
                if event["method"] == "Fetch.requestPaused"
                    && adapt_response(&mut cdp, &event["params"], &options)
                        .await
                        .is_err()
                {
                    break;
                }
            }
        } else {
            let _ = cdp.call("Fetch.disable", json!({})).await;
            let _ = cdp
                .call("Network.setCacheDisabled", json!({"cacheDisabled":false}))
                .await;
            let _ = cdp.socket.close(None).await;
            let _ = ready_tx.send(outcome);
            return;
        }
        let _ = cdp.call("Fetch.disable", json!({})).await;
        let _ = cdp
            .call("Network.setCacheDisabled", json!({"cacheDisabled":false}))
            .await;
        let _ = cdp.socket.close(None).await;
    });
    *previous = Some(WorkspaceSession {
        task,
        options: options_tx,
        socket,
    });
    drop(previous);
    ready_rx.await.context("天工增强启动连接已结束，请重试")?
}

async fn update_models(cdp: &mut Cdp, options: &WorkspaceOptions) -> Result<()> {
    let expression = format!(
        "window.__tiancaiWorkspace?.updateModels?.({}) === true",
        serde_json::to_string(options)?
    );
    ensure!(
        cdp.evaluate(&expression).await? == true,
        "天工增强工作区的模型列表未能更新，请正常退出天工后重新增强启动"
    );
    Ok(())
}

async fn wait_ready(cdp: &mut Cdp, options: &WorkspaceOptions) -> Result<()> {
    let mut adapted = false;
    let mut timer = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            event = cdp.event() => {
                let event = event?;
                if event["method"] == "Fetch.requestPaused" {
                    adapt_response(cdp, &event["params"], options).await?;
                    adapted = true;
                }
            }
            _ = timer.tick(), if adapted => {
                let value = cdp.evaluate(
                    "({ready:window.__tiancaiWorkspace?.version===1&&window.__tiancaiWorkspace.ready===true,failed:!!window.__tiancaiWorkspace?.error})"
                ).await?;
                if value["ready"] == true { return Ok(()); }
                ensure!(value["failed"] != true,
                    "天工本地工作区加载未完成，请在天工页面查看提示并重试加载");
            }
        }
    }
}

async fn adapt_response(cdp: &mut Cdp, params: &Value, options: &WorkspaceOptions) -> Result<()> {
    let id = params["requestId"]
        .as_str()
        .context("天工增强启动响应缺少标识")?;
    let operation = async {
        ensure!(params["responseStatusCode"] == 200, "天工启动脚本读取失败");
        let body = cdp
            .call("Fetch.getResponseBody", json!({"requestId":id}))
            .await?;
        let body_text = body["body"].as_str().context("天工启动脚本内容缺失")?;
        let source = if body["base64Encoded"] == true {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(body_text)?)
                .context("天工启动脚本编码不兼容")?
        } else {
            body_text.to_owned()
        };
        let expression = format!(
            "({})({},{})",
            include_str!("workspace-adapter.js"),
            serde_json::to_string(&source)?,
            serde_json::to_string(options)?
        );
        let value = cdp.evaluate(&expression).await?;
        ensure!(
            value["adapted"] == true,
            "当前天工版本的工作区启动结构暂不兼容，已保留原启动；请使用普通启动"
        );
        let source = value["source"].as_str().context("天工工作区适配结果缺失")?;
        ensure!(source.len() <= MAX_BYTES / 2, "天工启动脚本超过支持大小");
        let headers: Vec<Value> = params["responseHeaders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|header| {
                !matches!(
                    header["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "content-length" | "content-encoding" | "etag"
                )
            })
            .cloned()
            .collect();
        cdp.call(
            "Fetch.fulfillRequest",
            json!({
                "requestId":id,"responseCode":200,"responseHeaders":headers,
                "body":base64::engine::general_purpose::STANDARD.encode(source.as_bytes())
            }),
        )
        .await?;
        Ok(())
    }
    .await;
    if operation.is_err() {
        // Always release a paused original response on incompatibility/error.
        let _ = cdp
            .call("Fetch.continueRequest", json!({"requestId":id}))
            .await;
    }
    operation
}

const PROBE: &str = r#"({
 installed: window.__tiancaiWorkspace?.version === 1,
 ready: window.__tiancaiWorkspace?.ready === true,
 workspaceOpen: !!document.querySelector('.app-main'),
 hasInput: [...document.querySelectorAll('textarea,input:not([type="hidden"])')].some(e => !!e.value?.trim()),
 entries: [...document.querySelectorAll('script[type="module"][src]')].map(e => e.src),
 page: location.href
})"#;

fn same_renderer_entry(page: &str, entry: &str) -> bool {
    let (Ok(page), Ok(entry)) = (Url::parse(page), Url::parse(entry)) else {
        return false;
    };
    page.scheme() == "file"
        && entry.scheme() == "file"
        && page.host_str() == entry.host_str()
        && entry.query().is_none()
        && entry.fragment().is_none()
        && page.path().ends_with("/index.html")
        && entry
            .path()
            .starts_with(page.path().trim_end_matches("index.html"))
        && entry.path().ends_with(".js")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn entry_must_stay_in_the_verified_renderer_directory() {
        let page = "file:///D:/App/resources/app.asar/out/renderer/index.html";
        assert!(same_renderer_entry(
            page,
            "file:///D:/App/resources/app.asar/out/renderer/assets/main.js"
        ));
        for entry in [
            "https://example.invalid/main.js",
            "file:///D:/Other/main.js",
            "file:///D:/App/resources/app.asar/out/main/main.js",
            "file:///D:/App/resources/app.asar/out/renderer/../main/main.js",
            "file:///D:/App/resources/app.asar/out/renderer/assets/main.js?other=1",
        ] {
            assert!(!same_renderer_entry(page, entry));
        }
    }

    fn fixture(configured: bool) -> (AppConfig, GmClawConfigStatus) {
        let mut config = AppConfig::default();
        config
            .ai_gateway
            .providers
            .push(crate::ai_gateway::config::ProviderConfig {
                name: "gmclaw".into(),
                enabled: true,
                models: vec!["fixture-model".into()],
                ..Default::default()
            });
        let status = serde_json::from_value(json!({
            "entries":[{
                "entryId":"legacy","modelId":"tiancaispacehub",
                "model":{"localUrl":"http://127.0.0.1:3847/ai-gateway/gmclaw/v1",
                    "modelName":"fixture-model","maxTokens":8192,"temperature":0.7,"contextWindow":128000},
                "parameters":{},
                "active":false,"configured":configured,"localUrl":"http://127.0.0.1:3847/ai-gateway/gmclaw/v1"
            }],
            "path":"fixture","exists":true,"schemaSupported":true,
            "backupPath":"fixture-backup","backupExists":false,"configured":configured,
            "active":false,"localUrl":"fixture","restartRequired":false,"revision":"fixture"
        })).unwrap();
        (config, status)
    }

    #[test]
    fn external_mode_requires_configured_enabled_non_builtin_hub_models() {
        let (mut config, status) = fixture(true);
        let options = select_options(&config, status.clone(), None).unwrap();
        assert_eq!(options.external_model_ids, ["tiancaispacehub"]);
        assert_eq!(options.preferred_model_id, "tiancaispacehub");
        config.ai_gateway.providers[0].enabled = false;
        assert!(select_options(&config, status.clone(), None).is_err());
        config.ai_gateway.providers[0].enabled = true;
        config.ai_gateway.providers[0].tiangong_builtin =
            Some(crate::desktop::tiangong_models::BuiltinBinding {
                model_id: "tiangong-builtin".into(),
                upstream_model: "fixture-model".into(),
                credential_ref: "fixture-reference".into(),
                parameters: None,
            });
        assert!(select_options(&config, status, None).is_err());
        let (config, status) = fixture(false);
        assert!(select_options(&config, status, None).is_err());
    }

    #[test]
    fn enhanced_preflight_keeps_native_read_failure_details() {
        let (config, mut status) = fixture(false);
        status.error = Some("模型数据库当前不可读".into());
        let error = select_options(&config, status, None)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("模型数据库当前不可读"));
        assert!(!error.contains("请先"));
    }

    #[test]
    fn preferred_model_cannot_select_another_clients_provider() {
        let (mut config, status) = fixture(true);
        config.ai_gateway.providers[0].name = "workbuddy".into();
        assert!(select_options(&config, status.clone(), Some("tiancaispacehub")).is_err());
        config.ai_gateway.providers[0].name = "gmclaw".into();
        let options = select_options(&config, status, Some("tiangong-builtin")).unwrap();
        assert_eq!(options.preferred_model_id, "tiancaispacehub");
        assert!(
            serde_json::to_value(options)
                .unwrap()
                .get("apiKey")
                .is_none()
        );
    }

    async fn mock_cdp(replies: Vec<(&'static str, Value)>) -> (Cdp, JoinHandle<Vec<Value>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut requests = Vec::new();
            for (method, result) in replies {
                let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                    panic!("expected text");
                };
                let request: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(request["method"], method);
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"result":result}).to_string(),
                    ))
                    .await
                    .unwrap();
                requests.push(request);
            }
            requests
        });
        (
            Cdp::connect(&Url::parse(&format!("ws://{address}")).unwrap(), "天工")
                .await
                .unwrap(),
            task,
        )
    }

    #[tokio::test]
    async fn existing_workspace_updates_models_without_reload_and_requires_acknowledgement() {
        for accepted in [true, false] {
            let (mut cdp, server) = mock_cdp(vec![(
                "Runtime.evaluate",
                json!({"result":{"value":accepted}}),
            )])
            .await;
            let (config, status) = fixture(true);
            let options = select_options(&config, status, None).unwrap();
            assert_eq!(update_models(&mut cdp, &options).await.is_ok(), accepted);
            let requests = server.await.unwrap();
            assert_eq!(requests.len(), 1);
            let expression = requests[0]["params"]["expression"].as_str().unwrap();
            assert!(expression.contains("updateModels"));
            assert!(expression.contains("\"externalModelIds\":[\"tiancaispacehub\"]"));
            assert!(!expression.contains("Page.reload"));
        }
    }

    #[tokio::test]
    async fn incompatible_renderer_releases_original_response_without_fulfillment() {
        let (mut cdp,server)=mock_cdp(vec![
            ("Fetch.getResponseBody",json!({"body":"fixture original","base64Encoded":false})),
            ("Runtime.evaluate",json!({"result":{"value":{"adapted":false,"source":"fixture original","reason":"unsupported"}}})),
            ("Fetch.continueRequest",json!({}))
        ]).await;
        let (config, status) = fixture(true);
        let options = select_options(&config, status, None).unwrap();
        assert!(
            adapt_response(
                &mut cdp,
                &json!({"requestId":"fixture","responseStatusCode":200}),
                &options
            )
            .await
            .is_err()
        );
        assert_eq!(server.await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn adapted_script_fulfillment_keeps_headers_without_stale_length_or_encoding() {
        let (mut cdp, server) = mock_cdp(vec![
            (
                "Fetch.getResponseBody",
                json!({"body":"Zml4dHVyZSBvcmlnaW5hbA==","base64Encoded":true}),
            ),
            (
                "Runtime.evaluate",
                json!({"result":{"value":{"adapted":true,"source":"fixture adapted","reason":""}}}),
            ),
            ("Fetch.fulfillRequest", json!({})),
        ])
        .await;
        let (config, status) = fixture(true);
        let options = select_options(&config, status, None).unwrap();
        adapt_response(&mut cdp,&json!({
            "requestId":"fixture","responseStatusCode":200,
            "responseHeaders":[{"name":"Content-Type","value":"text/javascript"},
                {"name":"Content-Length","value":"16"},{"name":"Content-Encoding","value":"gzip"}]
        }),&options).await.unwrap();
        let requests = server.await.unwrap();
        let params = &requests[2]["params"];
        assert_eq!(
            params["responseHeaders"],
            json!([{"name":"Content-Type","value":"text/javascript"}])
        );
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(params["body"].as_str().unwrap())
                .unwrap(),
            b"fixture adapted"
        );
    }
}
