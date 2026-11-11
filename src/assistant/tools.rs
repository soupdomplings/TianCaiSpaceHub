//! Narrow local configuration tools. Secrets and human consent stay in Hub.
use crate::{
    ai_gateway::config::{ProviderConfig, provider_route_id},
    app_state::SharedState,
    config::AppConfig,
    desktop::{clients::ClientId, models, resources::Library},
};
use anyhow::{Context, Result, bail, ensure};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::sync::oneshot;

pub(super) const WORK_GUIDE: &str = include_str!("guide.md");

#[derive(Default)]
pub(crate) struct ToolState {
    generation: u64,
    credentials: HashMap<String, Credential>,
    deletion: Option<Pending<bool>>,
    credential: Option<Pending<String>>,
    navigation: Option<Value>,
}
struct Credential {
    secret: String,
    mask: String,
    label: String,
    source: Option<crate::external_import::ImportSource>,
}
struct Pending<T> {
    nonce: String,
    view: Value,
    response: oneshot::Sender<T>,
}

pub(crate) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/assistant/confirm", post(confirm))
        .route("/api/desktop/assistant/credential", post(credential))
        .route(
            "/api/desktop/assistant/navigation/ack",
            post(navigation_ack),
        )
}
fn trusted(headers: &HeaderMap) -> Result<()> {
    ensure!(
        headers.get("origin").is_none()
            && headers
                .get("x-tiancai-desktop")
                .and_then(|v| v.to_str().ok())
                == Some("1"),
        "请从天才搭档界面操作"
    );
    Ok(())
}
fn response(value: Result<Value>) -> Response {
    match value {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(json!({"error":e.to_string()}))).into_response(),
    }
}
pub(crate) async fn public_status(state: &SharedState) -> Value {
    let tools = state.assistant.tools.lock().await;
    json!({"pendingDeletion":tools.deletion.as_ref().map(|p|&p.view),
        "pendingCredential":tools.credential.as_ref().map(|p|&p.view),
        "credentialReferences":tools.credentials.iter().map(|(reference,c)|json!({"reference":reference,"mask":c.mask,"label":c.label})).collect::<Vec<_>>(),
        "navigation":tools.navigation})
}
pub(crate) async fn clear(state: &SharedState) {
    let mut tools = state.assistant.tools.lock().await;
    let generation = tools.generation.wrapping_add(1);
    // Dropping the senders cancels waiting tools as well as clearing the form.
    *tools = ToolState {
        generation,
        ..Default::default()
    };
}
pub(crate) async fn cancel_pending(state: &SharedState) {
    let mut tools = state.assistant.tools.lock().await;
    tools.generation = tools.generation.wrapping_add(1);
    tools.deletion = None;
    tools.credential = None;
    tools.navigation = None;
}
#[derive(Deserialize)]
struct Confirmation {
    nonce: String,
    approved: bool,
}
async fn confirm(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<Confirmation>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            let mut tools = state.assistant.tools.lock().await;
            ensure!(
                tools
                    .deletion
                    .as_ref()
                    .is_some_and(|p| p.nonce == input.nonce),
                "此删除确认已结束，请核对当前对话"
            );
            let pending = tools.deletion.take().unwrap();
            let _ = pending.response.send(input.approved);
            Ok(json!({"accepted":true}))
        }
        .await,
    )
}
#[derive(Deserialize)]
struct CredentialInput {
    nonce: Option<String>,
    label: Option<String>,
    #[serde(default)]
    secret: String,
    #[serde(default)]
    cancelled: bool,
}
async fn credential(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<CredentialInput>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            if input.cancelled {
                let mut tools = state.assistant.tools.lock().await;
                ensure!(
                    input.nonce.as_ref().is_some_and(|nonce| tools
                        .credential
                        .as_ref()
                        .is_some_and(|p| p.nonce == *nonce)),
                    "凭据表单已结束，请核对当前对话"
                );
                tools.credential = None;
                return Ok(json!({"cancelled":true}));
            }
            ensure!(
                !input.secret.trim().is_empty() && input.secret.len() <= 16 * 1024,
                "请输入有效凭据"
            );
            let mut tools = state.assistant.tools.lock().await;
            if let Some(nonce) = &input.nonce {
                ensure!(
                    tools.credential.as_ref().is_some_and(|p| p.nonce == *nonce),
                    "凭据表单已结束，请重新打开"
                );
            }
            let reference = format!("credential:{}", uuid::Uuid::new_v4());
            let mask = mask(&input.secret);
            let label = input
                .label
                .unwrap_or_else(|| "模型渠道凭据".into())
                .chars()
                .take(100)
                .collect::<String>();
            tools.credentials.insert(
                reference.clone(),
                Credential {
                    secret: input.secret,
                    mask: mask.clone(),
                    label,
                    source: None,
                },
            );
            if input.nonce.is_some() {
                if let Some(pending) = tools.credential.take() {
                    let _ = pending.response.send(reference.clone());
                }
            }
            Ok(json!({"reference":reference,"mask":mask}))
        }
        .await,
    )
}
#[derive(Deserialize)]
struct NavigationAck {
    id: String,
}
async fn navigation_ack(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<NavigationAck>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            let mut tools = state.assistant.tools.lock().await;
            if tools
                .navigation
                .as_ref()
                .is_some_and(|v| v["id"] == input.id)
            {
                tools.navigation = None;
            }
            Ok(json!({"ok":true}))
        }
        .await,
    )
}
pub(super) fn mask(secret: &str) -> String {
    if secret.chars().count() < 8 {
        "••••••••".into()
    } else {
        format!(
            "••••••••{}",
            secret
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        )
    }
}
pub(super) fn spec(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
}
pub(crate) fn definitions() -> Vec<Value> {
    let mut definitions = vec![
        spec(
            "hub_read_configuration",
            "读取 Hub 渠道、客户端目标、独立模型身份及有效参数。指定 client 可读取该端当前配置；修改前读取、保存后回读核对。凭据只返回引用和掩码。",
            json!({"client":{"type":"string","enum":["codex","workbuddy","tiangong","vscode","zcode","deepseek-harness","webstorm"]}}),
            &[],
        ),
        spec(
            "hub_save_channel",
            "添加或修改 ordinary=true 的普通渠道。name 同名修改、换名会新增；只传要改的字段并保留已有密钥。models/modelAliases 增补，删除走删除工具。保存后回读；不自动更新已有客户端副本。",
            json!({"name":{"type":"string","description":"准确渠道名称，不是显示别名或重命名操作"},"settings":{"type":"object","properties":{"providerType":{"type":"string","enum":["open_ai_responses","deepseek_responses","kimi_responses","grok_responses","chat_completions","anthropic_messages"],"description":"上游API协议；普通OpenAI Responses为open_ai_responses"},"baseUrl":{"type":"string","description":"HTTP/HTTPS服务基址，凭据不能放在URL中"},"modelsUrl":{"type":["string","null"]},"compatibility":{"type":["string","null"]},"models":{"type":"array","items":{"type":"string"},"description":"上游接受的真实模型ID列表"},"modelAliases":{"type":"object","additionalProperties":{"type":"string"},"description":"Hub可见别名到上游真实模型ID的映射"},"weight":{"type":"integer","minimum":1,"description":"路由优先级；越高越优先，不是百分比"},"timeoutSecs":{"type":"integer","minimum":1,"maximum":86400},"enabled":{"type":"boolean"},"chatDisableReasoning":{"type":"boolean"},"zaiAccessMode":{"type":"string","enum":["api","coding_plan"]},"promptCacheRetention":{"type":["string","null"]}},"additionalProperties":false},"credentialReference":{"type":"string","description":"使用hub_request_credential或hub_squad_prepare_api_key返回的reference；省略保留原密钥"}}),
            &["name", "settings"],
        ),
        spec(
            "hub_add_model",
            "给指定普通渠道添加模型或别名，可指定 upstreamModel；不探测或调用真实模型。",
            json!({"channel":{"type":"string"},"model":{"type":"string"},"upstreamModel":{"type":"string"}}),
            &["channel", "model"],
        ),
        spec(
            "hub_configure_client",
            "把指定渠道模型复制到读取结果的 targetId，创建独立客户端配置。返回 model/entryId/id 是后续操作的实际身份；保存后指定client回读。不会启动客户端或测试模型。",
            json!({"targetId":{"type":"string"},"channel":{"type":"string"},"model":{"type":"string"},"label":{"type":"string"}}),
            &["targetId", "channel", "model"],
        ),
        spec(
            "hub_set_model_parameters",
            "修改已保存模型的 supportedParameters。普通副本用 id，天工/WorkBuddy用entryId，Codex动态模型用model。先读parameterHelp、保存后回读；能力字段不等于会话默认思考强度。",
            json!({"client":{"type":"string","enum":["codex","workbuddy","tiangong","vscode","zcode","deepseek-harness","webstorm"]},"id":{"type":"string"},"entryId":{"type":"string"},"model":{"type":"string"},"parameters":{"type":"object","properties":parameter_properties(),"additionalProperties":false}}),
            &["client", "parameters"],
        ),
        spec(
            "hub_delete_configurations",
            "集中删除明确列出的渠道、渠道内模型、客户端模型。Hub 只弹一次本机批量确认，必须等待真人决定。kind=channel 用 channel；kind=source_model 用 channel/model；kind=client_model 用 id 或 client/entryId，Codex 用 model。不可用通配符。",
            json!({"items":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","properties":{"kind":{"type":"string","enum":["channel","source_model","client_model"]},"channel":{"type":"string"},"model":{"type":"string"},"id":{"type":"string"},"client":{"type":"string"},"entryId":{"type":"string"}},"required":["kind"],"additionalProperties":false}}}),
            &["items"],
        ),
        spec(
            "hub_request_credential",
            "打开 Hub 本机凭据表单并等待用户输入。只得到临时引用和掩码，不得让用户在聊天中发送明文密钥。用户可取消或清空会话。",
            json!({"label":{"type":"string"}}),
            &[],
        ),
        spec(
            "hub_open_page",
            "请求 Hub 界面打开相关页面；queued 表示已提交导航意图，不能断言用户已看到。page=client时必须传client。",
            json!({"page":{"type":"string","enum":["overview","squad","sources","client","settings","help","assistant"]},"client":{"type":"string"}}),
            &["page"],
        ),
        spec(
            "hub_help",
            "读取 Hub 和天才小分队的当前功能帮助、操作入口及边界。",
            json!({}),
            &[],
        ),
    ];
    definitions.extend(super::squad_tools::definitions());
    definitions
}
/// Store an imported key only in the current assistant's ephemeral credential map.
pub(super) async fn remember_import(
    state: &SharedState,
    draft: crate::external_import::ImportDraft,
    generation: u64,
) -> Result<Value> {
    super::ensure_current_account(state).await?;
    let mut tools = state.assistant.tools.lock().await;
    ensure!(
        tools.generation == generation,
        "本次准备已停止，请重新选择密钥"
    );
    let provider = draft.provider;
    let reference = format!("credential:{}", uuid::Uuid::new_v4());
    let mask = mask(&provider.api_key);
    let view = json!({"prepared":true,"keyId":draft.source.key_id.parse::<u64>().ok(),
        "name":draft.source.key_name,"suggestedChannel":provider.name,
        "reference":reference,"credentialReference":reference,"mask":mask,
        "settings":{"providerType":provider.provider_type,"baseUrl":provider.base_url,
            "modelsUrl":provider.models_url,"compatibility":provider.compatibility,
            "models":provider.models,"modelAliases":provider.model_aliases},
        "message":"导入内容已准备，尚未保存渠道；使用 credentialReference 保存后再配置客户端"});
    tools.credentials.insert(
        reference,
        Credential {
            secret: provider.api_key,
            mask,
            label: draft.source.key_name.clone(),
            source: Some(draft.source),
        },
    );
    Ok(view)
}
pub(super) async fn generation(state: &SharedState) -> u64 {
    state.assistant.tools.lock().await.generation
}
async fn configuration(state: &SharedState) -> Result<AppConfig> {
    let mut current = state.config.lock().await;
    let mut latest = AppConfig::load_or_default(&state.config_path)?;
    crate::normalize_config_paths(&mut latest, &state.config_path);
    *current = latest.clone();
    Ok(latest)
}
async fn api(state: &SharedState, operation: &str, body: Value) -> Result<Value> {
    super::ensure_current_account(state).await?;
    crate::web::assistant_configuration_operation(state.clone(), operation, body).await
}
async fn save(state: &SharedState, config: AppConfig) -> Result<Value> {
    api(state, "save", serde_json::to_value(config)?).await
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .with_context(|| format!("缺少 {key}"))
}
fn ordinary(provider: &ProviderConfig) -> Result<()> {
    ensure!(
        provider.desktop_binding.is_none()
            && !provider.is_client_reserved()
            && !provider.name.starts_with("Codex · ")
            && provider.tiangong_builtin.is_none()
            && provider.chatgpt_auth_id.is_none()
            && provider.provider_type != crate::ai_gateway::config::ProviderType::ChatGptResponses,
        "请在对应客户端页管理此专用或登录渠道"
    );
    Ok(())
}
fn channel<'a>(config: &'a AppConfig, name: &str) -> Result<&'a ProviderConfig> {
    let mut found = config
        .ai_gateway
        .providers
        .iter()
        .filter(|p| p.name == name);
    let provider = found.next().context("渠道已不存在，请重新读取配置")?;
    ensure!(
        found.next().is_none(),
        "有重名渠道，请在模型与渠道页调整名称后重试"
    );
    Ok(provider)
}
fn patch_object(value: &mut Value, patch: &Value, allowed: &[&str]) -> Result<()> {
    for (key, entry) in patch.as_object().context("参数应为对象")? {
        ensure!(allowed.contains(&key.as_str()), "当前操作不支持参数 {key}");
        value[key] = entry.clone();
    }
    Ok(())
}
fn valid_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).context("渠道地址无效")?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "地址应为不含凭据、查询参数或片段的 HTTP / HTTPS 服务地址，凭据请用本机表单输入"
    );
    Ok(())
}
/// Strip private fields even from native API results. Never send local tokens,
/// paths to protected auth, or upstream keys back through model tool results.
fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| {
                let k = key.to_ascii_lowercase().replace(['_', '-'], "");
                ![
                    "apikey",
                    "upstreamapikey",
                    "localtoken",
                    "token",
                    "authorization",
                    "auth",
                    "password",
                    "secret",
                    "credentialref",
                    "chatgptauthid",
                    "nativefiles",
                    "sourcefingerprint",
                ]
                .contains(&k.as_str())
            });
            for (key, v) in map {
                if matches!(
                    key.as_str(),
                    "baseUrl" | "modelsUrl" | "upstreamUrl" | "url" | "localUrl"
                ) {
                    if let Some(s) = v.as_str() {
                        if let Ok(mut u) = url::Url::parse(s) {
                            let _ = u.set_username("");
                            let _ = u.set_password(None);
                            u.set_query(None);
                            u.set_fragment(None);
                            *v = u.to_string().into();
                        }
                    }
                } else {
                    redact(v);
                }
            }
        }
        Value::Array(list) => {
            for v in list {
                redact(v);
            }
        }
        _ => (),
    }
}
fn parameter_properties() -> Value {
    json!({
        "displayName":{"type":"string","description":"Codex动态模型显示名，不改模型ID"},
        "capabilityProfile":{"type":["string","null"],"description":"Codex动态模型继承的内置模型ID或协议模板；null恢复自动继承"},
        "contextWindow":{"type":"integer","minimum":1,"description":"输入加输出的上下文窗口，token数"},
        "maxContextWindow":{"type":"integer","minimum":1,"description":"Codex动态模型可支持的最大上下文，不小于contextWindow"},
        "maxTokens":{"type":"integer","minimum":1,"description":"单次最大输出token数，不是上下文窗口"},
        "supportsImages":{"type":"boolean","description":"仅Codex动态模型的图片输入能力"},
        "supportsReasoning":{"type":"boolean","description":"仅Codex动态模型的推理能力描述，不设置会话强度"},
        "supportsImage":{"type":"boolean","description":"其他客户端的图片输入能力；注意与Codex字段拼写不同"},
        "toolCalling":{"type":"boolean","description":"其他客户端是否支持工具调用"},
        "preferStream":{"type":"boolean","description":"上游请求的流式偏好，不表示模型已测试通过"},
        "reasoningEfforts":{"type":"array","items":{"type":"string"},"description":"该客户端模型允许用户选择的强度列表，沿模型实际能力填写"},
        "defaultReasoningEffort":{"type":"string","description":"客户端未指定时的上游默认强度，auto表示不注入；不用于Codex/WorkBuddy/天工"},
        "upstreamDefaultReasoningEffort":{"type":"string","description":"仅WorkBuddy默认强度，取该条目reasoning.supportedEfforts中的值"},
        "temperature":{"type":"number","minimum":0,"maximum":2,"description":"天工采样温度；是否发给上游由temperatureMode决定"},
        "reasoningEffort":{"type":["string","null"],"enum":[null,"none","low","medium","high","xhigh","max"],"description":"天工默认强度，null为自动；原生Claude不支持none"},
        "temperatureMode":{"type":"string","enum":["auto","omit","preserve"],"description":"天工温度策略：自动适配/省略/保留"}
    })
}
fn parameter_fields(client: ClientId, mode: &str) -> Vec<&'static str> {
    match client {
        ClientId::Codex => vec![
            "displayName",
            "capabilityProfile",
            "contextWindow",
            "maxContextWindow",
            "supportsImages",
            "supportsReasoning",
        ],
        ClientId::WorkBuddy => vec!["upstreamDefaultReasoningEffort"],
        ClientId::Tiangong => vec![
            "maxTokens",
            "temperature",
            "reasoningEffort",
            "temperatureMode",
        ],
        _ => {
            let support = crate::desktop::clients::parameter_support(client, mode);
            let mut fields = vec!["preferStream"];
            if support.limits {
                fields.extend(["contextWindow", "maxTokens"]);
            }
            if support.image {
                fields.extend(["supportsImage", "toolCalling"]);
            }
            if support.efforts {
                fields.extend(["reasoningEfforts", "defaultReasoningEffort"]);
            }
            fields
        }
    }
}
fn codex_model_editable(model: &str) -> bool {
    !crate::ai_gateway::catalog::visible_catalog_model_options()
        .iter()
        .any(|known| known.slug == model)
}
fn codex_models_view(config: &AppConfig) -> Value {
    let catalog = crate::ai_gateway::catalog::configured_models_response(&config.ai_gateway);
    let entries = catalog["models"].as_array().into_iter().flatten().map(|model| {
        let id = model["slug"].as_str().unwrap_or_default();
        let fields = if codex_model_editable(id) { parameter_fields(ClientId::Codex, "") } else { vec![] };
        json!({"model":id,"label":model["display_name"],
            "parameters":{"contextWindow":model["context_window"],"maxContextWindow":model["max_context_window"],
                "supportsImages":model["input_modalities"].as_array().is_some_and(|items|items.iter().any(|v|v=="image")),
                "supportsReasoning":model["supports_reasoning_summaries"]},
            "reasoningEfforts":model["supported_reasoning_levels"],
            "supportedParameters":fields,
            "parameterNote":if fields.is_empty(){"内置精确模型沿固定目录；如需独立能力覆盖，请使用动态别名"}else{"动态模型能力参数；不修改会话思考强度或Fast/Ultra开关"}})
    }).collect::<Vec<_>>();
    entries.into()
}
fn annotate_native_entries(view: &mut Value, config: &AppConfig, client: ClientId) {
    if let Some(entries) = view["entries"].as_array_mut() {
        for entry in entries {
            let inherited =
                entry["sourceProvider"].as_str().is_some_and(|name| {
                    config.ai_gateway.providers.iter().any(|provider| {
                        provider.name == name && provider.tiangong_builtin.is_some()
                    })
                });
            let fields = if inherited {
                vec![]
            } else {
                parameter_fields(client, "")
            };
            entry["supportedParameters"] = json!(fields);
            if inherited {
                entry["parameterNote"] = "参数沿天工内置来源，此条目不单独覆盖".into();
            }
        }
    }
}
async fn read_configuration(state: &SharedState, args: &Value) -> Result<Value> {
    let config = configuration(state).await?;
    let library = Library::new(&state.config_path)?;
    let targets = library.targets(&library.load()?)?;
    let providers = config.ai_gateway.providers.iter().map(|p| {
        let fields = p.desktop_binding.as_ref().map(|b| if p.tiangong_builtin.is_some(){vec![]}else{parameter_fields(b.client,&b.mode)});
        json!({"channel":p.name,"enabled":p.enabled,"providerType":p.provider_type,"baseUrl":p.base_url,
            "modelsUrl":p.models_url,"compatibility":p.compatibility,"models":p.models,"modelAliases":p.model_aliases,
            "weight":p.weight,"timeoutSecs":p.timeout_secs,"chatDisableReasoning":p.chat_disable_reasoning,
            "zaiAccessMode":p.zai_access_mode,"promptCacheRetention":p.prompt_cache_retention,
            "credentialConfigured":!p.api_key.is_empty()||p.chatgpt_auth_id.is_some()||p.tiangong_builtin.is_some(),
            "credentialMask":if p.api_key.is_empty(){None}else{Some(mask(&p.api_key))},
            "managedClient":p.desktop_binding.as_ref().map(|b|b.client),"id":p.desktop_binding.as_ref().map(|b|&b.id),
            "model":p.desktop_binding.as_ref().map(|b|&b.model),"label":p.desktop_binding.as_ref().map(|b|&b.label),
            "targetId":p.desktop_binding.as_ref().map(|b|&b.target_id),"capabilities":p.desktop_binding.as_ref().map(|b|&b.capabilities),
            "supportedParameters":fields,
            "ordinary":ordinary(p).is_ok()})
    }).collect::<Vec<_>>();
    let targets = targets
        .iter()
        .map(|t| {
            let fields = parameter_fields(t.client, &t.model_mode);
            json!({"id":t.id,"targetId":t.id,"client":t.client,"label":t.label,
        "present":t.present,"modelMode":t.model_mode,"modelsInherited":t.models_inherited,
        "supportedParameters":fields})
        })
        .collect::<Vec<_>>();
    let mut view = json!({"channels":providers,"targets":targets,"codexVisibleModels":config.ai_gateway.codex_visible_models,
        "codexModelProfiles":config.ai_gateway.codex_model_profiles,"codexModels":codex_models_view(&config),"revision":config.revision,
        "parameterHelp":parameter_properties(),
        "credentialReferences":public_status(state).await["credentialReferences"]});
    if let Some(client) = args["client"].as_str() {
        let client_id = models::client(client)?;
        let mut native = match client {
            "workbuddy" => api(state, "workbuddy_read", Value::Null).await?,
            "tiangong" => api(state, "tiangong_read", Value::Null).await?,
            "codex" => json!({"name":client_id.name(),"models":view["codexModels"]}),
            _ => {
                json!({"name":client_id.name(),"models":view["channels"].as_array().unwrap().iter().filter(|row|row["managedClient"]==client).collect::<Vec<_>>()})
            }
        };
        annotate_native_entries(&mut native, &config, client_id);
        view["client"] = native;
    }
    redact(&mut view);
    Ok(view)
}
async fn save_channel(state: &SharedState, args: &Value) -> Result<Value> {
    let mut config = configuration(state).await?;
    let name = text(args, "name")?.trim();
    ensure!(name.len() <= 256, "渠道名称过长");
    let old = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| p.name == name)
        .cloned();
    if old.is_some() {
        ordinary(channel(&config, name)?)?;
    }
    let mut provider = old.clone().unwrap_or_else(|| ProviderConfig {
        name: name.into(),
        ..Default::default()
    });
    ordinary(&provider)?;
    let patch = args.get("settings").context("缺少 settings")?;
    let mut value = serde_json::to_value(&provider)?;
    patch_object(
        &mut value,
        patch,
        &[
            "providerType",
            "baseUrl",
            "modelsUrl",
            "compatibility",
            "models",
            "modelAliases",
            "weight",
            "timeoutSecs",
            "enabled",
            "chatDisableReasoning",
            "zaiAccessMode",
            "promptCacheRetention",
        ],
    )?;
    provider = serde_json::from_value(value).context("渠道参数格式无效")?;
    if let Some(old) = &old {
        for model in &old.models {
            if !provider.models.contains(model) {
                provider.models.push(model.clone());
            }
        }
        for (alias, model) in &old.model_aliases {
            provider
                .model_aliases
                .entry(alias.clone())
                .or_insert_with(|| model.clone());
        }
    }
    ensure!(
        !provider.models.iter().any(|m| m.trim().is_empty())
            && !provider
                .model_aliases
                .iter()
                .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty()),
        "模型名称不能为空"
    );
    ensure!(
        provider.weight > 0 && provider.timeout_secs > 0 && provider.timeout_secs <= 86400,
        "权重或超时无效"
    );
    valid_url(&provider.base_url)?;
    if let Some(url) = &provider.models_url {
        if !url.is_empty() {
            valid_url(url)?;
        }
    }
    ordinary(&provider)?;
    if let Some(reference) = args["credentialReference"].as_str() {
        let tools = state.assistant.tools.lock().await;
        let credential = tools
            .credentials
            .get(reference)
            .context("凭据引用已失效，请重新准备密钥或在本机表单输入")?;
        provider.api_key = credential.secret.clone();
        provider.import_source = credential.source.clone();
    }
    if let Some(existing) = config
        .ai_gateway
        .providers
        .iter_mut()
        .find(|p| p.name == name)
    {
        *existing = provider;
    } else {
        config.ai_gateway.providers.push(provider);
    }
    save(state, config).await?;
    Ok(json!({"saved":true,"channel":name,"message":"渠道已保存；不会自动发起真实模型测试"}))
}
async fn add_model(state: &SharedState, args: &Value) -> Result<Value> {
    let name = text(args, "channel")?;
    let model = text(args, "model")?;
    let upstream = args["upstreamModel"].as_str().unwrap_or(model);
    ensure!(!upstream.trim().is_empty(), "上游模型名称不能为空");
    save_channel(
        state,
        &json!({"name":name,"settings":{"models":[upstream],"modelAliases":{model:upstream}}}),
    )
    .await
}
async fn configure_client(state: &SharedState, args: &Value) -> Result<Value> {
    let config = configuration(state).await?;
    let provider = channel(&config, text(args, "channel")?)?;
    let model = text(args, "model")?;
    ensure!(
        provider.enabled && provider.matches_model(model),
        "渠道未启用或没有此模型"
    );
    let library = Library::new(&state.config_path)?;
    let target = library
        .targets(&library.load()?)?
        .into_iter()
        .find(|t| t.id == args["targetId"])
        .context("未找到客户端目标，请重新读取配置")?;
    ensure!(
        !target.models_inherited,
        "此目标继承主配置，请选择该客户端的主配置目标"
    );
    let base = json!({"targetId":target.id,"sourceRoute":provider_route_id(provider),"model":model,"revision":config.revision,
        "client":target.client,"using":true,"label":args["label"].as_str().unwrap_or(model)});
    // Legacy writers preserve their own backup/revision rules; ordinary targets
    // use recoverable model_operations::commit through the existing use handler.
    let result = api(
        state,
        if matches!(
            target.client,
            ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
        ) {
            "copy"
        } else {
            "use"
        },
        base,
    )
    .await?;
    let native = &result["status"];
    let configured_model = native["model"].as_str().unwrap_or(model);
    Ok(
        json!({"saved":true,"targetId":target.id,"client":target.client,
        "sourceModel":model,"model":configured_model,"entryId":native["selectedEntryId"],
        "id":result["id"],"undo":result["undo"],
        "message":"已配置到客户端；新会话可读取新配置，尚未调用真实模型"}),
    )
}
fn workbuddy_parameter_patch(row: &Value, patch: &Value) -> Result<Value> {
    let mut model = row["model"].clone();
    patch_object(
        &mut model,
        patch,
        &parameter_fields(ClientId::WorkBuddy, ""),
    )?;
    let effort = model["upstreamDefaultReasoningEffort"]
        .as_str()
        .context("WorkBuddy 默认强度应为字符串")?;
    ensure!(
        row["model"]["reasoning"]["supportedEfforts"]
            .as_array()
            .is_some_and(|efforts| efforts.iter().any(|item| item == effort)),
        "WorkBuddy 当前模型不支持此默认强度，请从读取结果的 reasoning.supportedEfforts 选择"
    );
    Ok(model)
}
async fn set_parameters(state: &SharedState, args: &Value) -> Result<Value> {
    let client = models::client(text(args, "client")?)?;
    let patch = args.get("parameters").context("缺少 parameters")?;
    let mut config = configuration(state).await?;
    if client == ClientId::Codex {
        let model = text(args, "model")?;
        ensure!(
            config
                .ai_gateway
                .codex_visible_models
                .iter()
                .any(|m| m == model),
            "Codex 可见模型中没有此项"
        );
        ensure!(
            codex_model_editable(model),
            "此 Codex 内置模型使用固定能力目录，不能直接覆盖；可按用户要求创建独立动态别名后调整"
        );
        let old = config
            .ai_gateway
            .codex_model_profiles
            .iter()
            .find(|p| p.id == model)
            .cloned()
            .unwrap_or_else(|| crate::ai_gateway::config::CodexModelProfile {
                id: model.into(),
                display_name: model.into(),
                ..Default::default()
            });
        let mut value = serde_json::to_value(old)?;
        patch_object(
            &mut value,
            patch,
            &[
                "displayName",
                "capabilityProfile",
                "contextWindow",
                "maxContextWindow",
                "supportsImages",
                "supportsReasoning",
            ],
        )?;
        let profile = serde_json::from_value(value).context("Codex 参数格式无效")?;
        config
            .ai_gateway
            .codex_model_profiles
            .retain(|p| p.id != model);
        config.ai_gateway.codex_model_profiles.push(profile);
        return save(state, config).await;
    }
    if client == ClientId::WorkBuddy {
        let status = api(state, "workbuddy_read", Value::Null).await?;
        let entry = text(args, "entryId")?;
        let row = status["entries"]
            .as_array()
            .and_then(|rows| rows.iter().find(|v| v["entryId"] == entry))
            .context("WorkBuddy 条目已不存在")?;
        ensure!(
            !row["sourceProvider"].as_str().is_some_and(|name| config
                .ai_gateway
                .providers
                .iter()
                .any(|p| p.name == name && p.tiangong_builtin.is_some())),
            "此模型参数沿用天工内置来源，不能单独覆盖"
        );
        let model = workbuddy_parameter_patch(row, patch)?;
        let result = api(
            state,
            "workbuddy_save",
            json!({"entryId":entry,"revision":status["revision"],"model":model}),
        )
        .await?;
        let actual = result["entries"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["entryId"] == entry));
        return Ok(json!({"saved":true,"client":"workbuddy","entryId":entry,
            "parameters":{"upstreamDefaultReasoningEffort":actual.map(|row|&row["model"]["upstreamDefaultReasoningEffort"])},
            "message":"已按 WorkBuddy 原生接口保存；请回读此条目核对默认强度"}));
    }
    if client == ClientId::Tiangong {
        let status = api(state, "tiangong_read", Value::Null).await?;
        let entry = text(args, "entryId")?;
        let row = status["entries"]
            .as_array()
            .and_then(|rows| rows.iter().find(|v| v["entryId"] == entry))
            .context("天工模型条目已不存在")?;
        let mut request = json!({"entryId":entry,"revision":status["revision"],"sourceProvider":row["sourceProvider"],
            "model":row["model"]["modelName"],"maxTokens":row["model"]["maxTokens"],"temperature":row["model"]["temperature"],"parameters":row["parameters"]});
        for (key, value) in patch.as_object().context("参数应为对象")? {
            match key.as_str() {
                "maxTokens" | "temperature" => request[key] = value.clone(),
                "reasoningEffort" | "temperatureMode" => request["parameters"][key] = value.clone(),
                _ => bail!("天工当前不支持参数 {key}"),
            }
        }
        api(state, "tiangong_save", request).await?;
        return Ok(json!({"saved":true,"client":"tiangong","entryId":entry}));
    }
    let id = text(args, "id")?;
    let provider = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| {
            p.desktop_binding
                .as_ref()
                .is_some_and(|b| b.id == id && b.client == client)
        })
        .context("客户端模型已不存在")?;
    ensure!(
        crate::desktop::tiangong_models::parameters_for_provider(provider).is_none(),
        "此模型参数沿用天工内置来源，请在天工的内置模型设置中修改"
    );
    let binding = provider.desktop_binding.as_ref().unwrap();
    let supported = parameter_fields(client, &binding.mode);
    let mut capabilities = binding.capabilities.clone();
    if !capabilities.is_object() {
        capabilities = json!({});
    }
    for (key, value) in patch.as_object().context("参数应为对象")? {
        ensure!(
            supported.contains(&key.as_str()),
            "此客户端不支持参数 {key}"
        );
        capabilities[key] = value.clone();
    }
    let result=api(state,"use",json!({"id":id,"targetId":binding.target_id,"using":true,"capabilities":capabilities,"revision":config.revision})).await?;
    Ok(json!({"saved":true,"client":client,"id":id,"undo":result["undo"]}))
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteItem {
    kind: String,
    channel: Option<String>,
    model: Option<String>,
    id: Option<String>,
    client: Option<String>,
    entry_id: Option<String>,
}
impl DeleteItem {
    fn label(&self) -> String {
        format!(
            "{}：{}",
            match self.kind.as_str() {
                "channel" => "渠道",
                "source_model" => "渠道模型",
                _ => "客户端模型",
            },
            [
                &self.channel,
                &self.client,
                &self.entry_id,
                &self.id,
                &self.model
            ]
            .into_iter()
            .filter_map(|s| s.as_deref())
            .collect::<Vec<_>>()
            .join(" / ")
        )
    }
}
async fn delete_configurations(state: &SharedState, args: &Value) -> Result<Value> {
    let items: Vec<DeleteItem> =
        serde_json::from_value(args["items"].clone()).context("删除项目格式无效")?;
    ensure!(
        !items.is_empty() && items.len() <= 32,
        "请明确选择 1 至 32 个删除项目"
    );
    let snapshot = configuration(state).await?;
    // Validate identities before presenting a single human confirmation.
    for item in &items {
        match item.kind.as_str() {
            "channel" | "source_model" => {
                let provider =
                    channel(&snapshot, item.channel.as_deref().context("缺少渠道名称")?)?;
                ordinary(provider)?;
                if item.kind == "source_model" {
                    ensure!(
                        provider.matches_model(item.model.as_deref().context("缺少模型名称")?),
                        "渠道中没有所选模型"
                    );
                }
            }
            "client_model" => {
                if let Some(id) = &item.id {
                    ensure!(
                        snapshot
                            .ai_gateway
                            .providers
                            .iter()
                            .any(|p| p.desktop_binding.as_ref().is_some_and(|b| b.id == *id)),
                        "客户端模型已不存在"
                    );
                } else {
                    let client = models::client(item.client.as_deref().context("缺少客户端")?)?;
                    ensure!(
                        matches!(
                            client,
                            ClientId::Codex | ClientId::Tiangong | ClientId::WorkBuddy
                        ),
                        "请用读取结果中的模型 id"
                    );
                    if client == ClientId::Codex {
                        ensure!(
                            snapshot
                                .ai_gateway
                                .codex_visible_models
                                .contains(item.model.as_ref().context("缺少模型名称")?),
                            "可见列表中没有此模型"
                        );
                    } else {
                        ensure!(
                            item.entry_id.as_ref().is_some_and(|s| !s.is_empty()),
                            "缺少 entryId"
                        );
                    }
                }
            }
            _ => bail!("不支持的删除类型"),
        }
    }
    let summary = items
        .iter()
        .map(DeleteItem::label)
        .collect::<Vec<_>>()
        .join("\n");
    let (send, receive) = oneshot::channel();
    let generation = {
        let mut tools = state.assistant.tools.lock().await;
        ensure!(tools.deletion.is_none(), "已有删除确认正在等待用户");
        let nonce = uuid::Uuid::new_v4().to_string();
        tools.deletion = Some(Pending {
            view: json!({"nonce":nonce,"summary":summary,"items":items.iter().map(DeleteItem::label).collect::<Vec<_>>()}),
            nonce,
            response: send,
        });
        tools.generation
    };
    let approved = receive.await.context("删除确认已取消")?;
    if !approved {
        return Ok(json!({"deleted":false,"message":"用户取消删除，配置保留"}));
    }
    super::ensure_current_account(state).await?;
    ensure!(
        state.assistant.tools.lock().await.generation == generation,
        "对话已结束，删除取消"
    );
    ensure!(
        configuration(state).await?.revision == snapshot.revision,
        "等待确认期间配置已变化，请重新读取后提交删除"
    );
    let mut results = Vec::new();
    for item in items {
        // Each native writer is atomic for its target. Report partial failures
        // without retrying or asking for a second confirmation of the same list.
        let result = delete_one(state, &item).await;
        results.push(match result {
            Ok(_) => json!({"item":item.label(),"deleted":true}),
            Err(e) => json!({"item":item.label(),"deleted":false,"error":e.to_string()}),
        });
    }
    Ok(json!({"results":results}))
}
async fn delete_one(state: &SharedState, item: &DeleteItem) -> Result<Value> {
    let mut config = configuration(state).await?;
    match item.kind.as_str() {
        "channel" => {
            let name = item.channel.as_deref().unwrap();
            ordinary(channel(&config, name)?)?;
            config.ai_gateway.providers.retain(|p| p.name != name);
            save(state, config).await
        }
        "source_model" => {
            let name = item.channel.as_deref().unwrap();
            let model = item.model.as_deref().unwrap();
            ordinary(channel(&config, name)?)?;
            let provider = config
                .ai_gateway
                .providers
                .iter_mut()
                .find(|p| p.name == name)
                .unwrap();
            provider.models.retain(|m| m != model);
            provider.model_aliases.remove(model);
            save(state, config).await
        }
        "client_model" => {
            if let Some(id) = &item.id {
                let binding = config
                    .ai_gateway
                    .providers
                    .iter()
                    .filter_map(|p| p.desktop_binding.as_ref())
                    .find(|b| b.id == *id)
                    .context("模型已不存在")?;
                api(state,"use",json!({"revision":config.revision,"id":id,"targetId":binding.target_id,"using":false})).await
            } else {
                match models::client(item.client.as_deref().unwrap())? {
                    ClientId::Codex => {
                        config
                            .ai_gateway
                            .codex_visible_models
                            .retain(|m| Some(m) != item.model.as_ref());
                        save(state, config).await
                    }
                    ClientId::Tiangong | ClientId::WorkBuddy => {
                        let tiangong = item.client.as_deref() == Some("tiangong");
                        let native = api(
                            state,
                            if tiangong {
                                "tiangong_read"
                            } else {
                                "workbuddy_read"
                            },
                            Value::Null,
                        )
                        .await?;
                        api(
                            state,
                            if tiangong {
                                "tiangong_delete"
                            } else {
                                "workbuddy_delete"
                            },
                            json!({"entryId":item.entry_id,"revision":native["revision"]}),
                        )
                        .await
                    }
                    _ => bail!("客户端不支持此删除方式"),
                }
            }
        }
        _ => bail!("删除项目无效"),
    }
}
async fn request_credential(state: &SharedState, args: &Value) -> Result<Value> {
    let (send, receive) = oneshot::channel();
    {
        let mut tools = state.assistant.tools.lock().await;
        ensure!(tools.credential.is_none(), "已有凭据表单等待输入");
        let nonce = uuid::Uuid::new_v4().to_string();
        tools.credential = Some(Pending {
            view: json!({"nonce":nonce,"label":args["label"].as_str().unwrap_or("模型渠道凭据")}),
            nonce,
            response: send,
        });
    }
    let reference = receive.await.context("凭据输入已取消")?;
    let tools = state.assistant.tools.lock().await;
    let value = tools.credentials.get(&reference).context("凭据已清理")?;
    Ok(json!({"reference":reference,"mask":value.mask,"label":value.label}))
}
pub(crate) async fn call(state: &SharedState, name: &str, args: Value) -> Result<Value> {
    super::ensure_current_account(state).await?;
    let mut result = match name {
        "hub_read_configuration" => read_configuration(state, &args).await?,
        "hub_save_channel" => save_channel(state, &args).await?,
        "hub_add_model" => add_model(state, &args).await?,
        "hub_configure_client" => configure_client(state, &args).await?,
        "hub_set_model_parameters" => set_parameters(state, &args).await?,
        "hub_delete_configurations" => delete_configurations(state, &args).await?,
        "hub_request_credential" => request_credential(state, &args).await?,
        "hub_squad_list_api_keys" => super::squad_tools::list_keys(state, args).await?,
        "hub_squad_key_options" => super::squad_tools::key_options(state, args).await?,
        "hub_squad_create_api_key" => super::squad_tools::create_key(state, args).await?,
        "hub_squad_prepare_api_key" => super::squad_tools::prepare_key(state, args).await?,
        "hub_open_page" => {
            let page = text(&args, "page")?;
            ensure!(
                [
                    "overview",
                    "squad",
                    "sources",
                    "client",
                    "settings",
                    "help",
                    "assistant"
                ]
                .contains(&page),
                "不支持打开此页面"
            );
            if page == "client" {
                models::client(text(&args, "client")?)?;
            }
            state.assistant.tools.lock().await.navigation = Some(
                json!({"id":uuid::Uuid::new_v4().to_string(),"page":page,"client":args["client"]}),
            );
            json!({"queued":true,"page":page,"client":args["client"],"message":"已请求 Hub 界面打开页面"})
        }
        "hub_help" => {
            json!({"assistant":"天才搭档可读取配置、添加修改普通渠道/模型、调整支持参数、配置到客户端；删除会集中确认一次。可选当前小分队密钥或按要求新建后准备导入，外部凭据用本机表单输入。",
            "workGuide":WORK_GUIDE,
            "hub":"天才空间 Hub 统一连接模型渠道与多个桌面客户端；客户端模型配置为独立副本，修改来源不会自动同步副本。",
            "navigation":{"models":"左侧模型与渠道；各客户端的模型页管理独立副本","squad":"天才小分队：登录、账户、密钥、使用记录、充值和订阅；按账户权限显示组织功能","tiangongAssistance":"天工 Work → 接入设置 → 本机协助","dumpling":"左侧 Dumpling-MCP 独立入口"},
            "billing":"天才搭档模型由小分队管理员指定并承担费用；天工协助沿用其原模型来源。",
            "temporary":"收起保留对话；停止中断执行；清空删除临时内容；真正退出 Hub 结束后台并清理，已保存配置和主动另存成果保留。",
            "boundary":"助手只能读取当前用户密钥/分组、按要求创建密钥并准备导入；不能修改或撤销已有站点密钥、安装或移除运行端、启动或修复客户端、修改账户资料/组织/支付、管理Skill/MCP、删除任意文件、执行系统命令或发起模型连通性测试；可说明步骤并请求打开对应页面。"})
        }
        _ => bail!("天才搭档不支持工具 {name}"),
    };
    redact(&mut result);
    super::ensure_current_account(state).await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn call(state: &SharedState, name: &str, args: Value) -> Result<Value> {
        super::super::test_tool_scope(state, super::call(state, name, args)).await
    }
    fn fixture() -> (tempfile::TempDir, SharedState) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let mut config = AppConfig {
            state_path: temp.path().join("state.json"),
            ..Default::default()
        };
        config.ai_gateway.providers = vec![
            ProviderConfig {
                name: "fixture-channel".into(),
                base_url: "https://fixture.invalid/v1".into(),
                api_key: "fixture-private-key-not-for-model".into(),
                models: vec!["old-model".into()],
                ..Default::default()
            },
            ProviderConfig {
                name: "keep-channel".into(),
                base_url: "https://other.invalid/v1".into(),
                ..Default::default()
            },
        ];
        config.ai_gateway.codex_visible_models = vec!["old-model".into()];
        config.save(&path).unwrap();
        let state = crate::app_state::AppState::new(path, config, None, None);
        (temp, state)
    }
    fn headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-tiancai-desktop", "1".parse().unwrap());
        h
    }
    async fn pending_deletion(state: &SharedState) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(nonce) = public_status(state).await["pendingDeletion"]["nonce"].as_str()
                {
                    break nonce.to_owned();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn configuration_tools_use_existing_writer_preserve_keys_and_unrelated_data() {
        let (_temp, state) = fixture();
        call(&state,"hub_add_model",json!({"channel":"fixture-channel","model":"visible-new","upstreamModel":"upstream-new"})).await.unwrap();
        let config = AppConfig::load_or_default(&state.config_path).unwrap();
        let provider = channel(&config, "fixture-channel").unwrap();
        assert_eq!(provider.api_key, "fixture-private-key-not-for-model");
        assert!(provider.models.contains(&"old-model".into()));
        assert_eq!(provider.model_aliases["visible-new"], "upstream-new");
        assert!(channel(&config, "keep-channel").is_ok());
        assert_eq!(config.ai_gateway.codex_visible_models, vec!["old-model"]);
        assert!(
            call(
                &state,
                "hub_save_channel",
                json!({"name":"fixture-channel","settings":{"apiKey":"illegal-model-key"}})
            )
            .await
            .is_err()
        );
        assert!(
            call(
                &state,
                "hub_save_channel",
                json!({"name":"workbuddy","settings":{"baseUrl":"https://fixture.invalid"}})
            )
            .await
            .is_err()
        );
        assert!(call(&state,"hub_save_channel",json!({"name":"secret-in-url","settings":{"baseUrl":"https://fixture.invalid/?key=secret"}})).await.is_err());
    }
    #[tokio::test]
    async fn saved_channel_and_codex_parameters_can_be_read_back_with_actual_model_identities() {
        let (_temp, state) = fixture();
        call(&state,"hub_save_channel",json!({"name":"fixture-channel","settings":{
            "chatDisableReasoning":true,"zaiAccessMode":"coding_plan","promptCacheRetention":"1h","weight":3}})).await.unwrap();
        call(&state,"hub_add_model",json!({"channel":"fixture-channel","model":"visible-alias","upstreamModel":"upstream-model"})).await.unwrap();
        call(&state,"hub_set_model_parameters",json!({"client":"codex","model":"old-model","parameters":{
            "displayName":"独立模型","contextWindow":64000,"maxContextWindow":128000,"supportsImages":true}})).await.unwrap();
        let view = call(&state, "hub_read_configuration", json!({"client":"codex"}))
            .await
            .unwrap();
        let source = view["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["channel"] == "fixture-channel")
            .unwrap();
        assert_eq!(source["chatDisableReasoning"], true);
        assert_eq!(source["zaiAccessMode"], "coding_plan");
        assert_eq!(source["promptCacheRetention"], "1h");
        assert_eq!(source["weight"], 3);
        assert_eq!(source["modelAliases"]["visible-alias"], "upstream-model");
        assert_eq!(source["credentialConfigured"], true);
        let model = &view["client"]["models"][0];
        assert_eq!(model["model"], "old-model");
        assert_eq!(model["label"], "独立模型");
        assert_eq!(model["parameters"]["contextWindow"], 64000);
        assert_eq!(model["parameters"]["maxContextWindow"], 128000);
        assert_eq!(model["parameters"]["supportsImages"], true);
        assert!(
            view["parameterHelp"]["contextWindow"]["description"]
                .as_str()
                .unwrap()
                .contains("上下文")
        );
        assert!(
            !view
                .to_string()
                .contains("fixture-private-key-not-for-model")
        );

        let mut config = AppConfig::load_or_default(&state.config_path).unwrap();
        config
            .ai_gateway
            .codex_visible_models
            .push("gpt-6.1-sol".into());
        config.save(&state.config_path).unwrap();
        let before = std::fs::read(&state.config_path).unwrap();
        let error = call(
            &state,
            "hub_set_model_parameters",
            json!({"client":"codex","model":"gpt-6.1-sol", "parameters":{"contextWindow":64000}}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("固定能力目录"));
        assert_eq!(std::fs::read(&state.config_path).unwrap(), before);
        let view = call(&state, "hub_read_configuration", json!({"client":"codex"}))
            .await
            .unwrap();
        assert!(
            view["client"]["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["model"] == "gpt-6.1-sol")
                .unwrap()["supportedParameters"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn ordinary_client_parameters_use_native_writer_and_read_back_the_same_copy() {
        use crate::desktop::targets::{self, McpFormat};
        let (temp, state) = fixture();
        let root = temp.path().join("vscode");
        std::fs::create_dir_all(&root).unwrap();
        let target = targets::target(
            ClientId::VsCode,
            "隔离VS Code",
            root.clone(),
            root.join("skills"),
            root.join("mcp.json"),
            McpFormat::Servers,
            "chat",
        );
        let library = Library::new(&state.config_path).unwrap();
        library
            .save_targets(library.load().unwrap(), vec![target.clone()])
            .unwrap();
        let copied = call(
            &state,
            "hub_configure_client",
            json!({"targetId":target.id,
            "channel":"fixture-channel","model":"old-model","label":"隔离模型"}),
        )
        .await
        .unwrap();
        let id = copied["id"].as_str().unwrap();
        call(&state,"hub_set_model_parameters",json!({"client":"vscode","id":id,"parameters":{
            "contextWindow":64000,"maxTokens":4096,"supportsImage":true,"toolCalling":false,"defaultReasoningEffort":"high","preferStream":true}})).await.unwrap();
        let view = call(&state, "hub_read_configuration", json!({"client":"vscode"}))
            .await
            .unwrap();
        let row = view["client"]["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert_eq!(row["model"], "old-model");
        assert_eq!(row["label"], "隔离模型");
        assert_eq!(row["capabilities"]["contextWindow"], 64000);
        assert_eq!(row["capabilities"]["defaultReasoningEffort"], "high");
        assert_eq!(row["capabilities"]["preferStream"], true);
        let native: Value =
            serde_json::from_slice(&std::fs::read(root.join("chatLanguageModels.json")).unwrap())
                .unwrap();
        assert_eq!(native[0]["models"][0]["maxOutputTokens"], 4096);
        assert_eq!(native[0]["models"][0]["vision"], true);
        assert_eq!(native[0]["models"][0]["toolCalling"], false);
        assert!(
            !view
                .to_string()
                .contains("fixture-private-key-not-for-model")
        );
    }
    #[test]
    fn workbuddy_parameters_only_advertise_and_accept_fields_consumed_by_native_writer() {
        let row = json!({"model":{"upstreamDefaultReasoningEffort":"high", "supportsImages":true,
            "reasoning":{"supportedEfforts":["low","high","max"]}}});
        assert_eq!(
            parameter_fields(ClientId::WorkBuddy, ""),
            vec!["upstreamDefaultReasoningEffort"]
        );
        let updated =
            workbuddy_parameter_patch(&row, &json!({"upstreamDefaultReasoningEffort":"max"}))
                .unwrap();
        assert_eq!(updated["upstreamDefaultReasoningEffort"], "max");
        assert_eq!(updated["supportsImages"], true);
        assert!(workbuddy_parameter_patch(&row, &json!({"supportsImages":false})).is_err());
        assert!(
            workbuddy_parameter_patch(&row, &json!({"upstreamDefaultReasoningEffort":"ultra"}))
                .is_err()
        );
    }
    #[tokio::test]
    async fn help_describes_current_capabilities_and_navigation_only_queues_an_intent() {
        let (_temp, state) = fixture();
        let help = call(&state, "hub_help", json!({})).await.unwrap();
        assert_eq!(help["workGuide"], WORK_GUIDE);
        let result = call(
            &state,
            "hub_open_page",
            json!({"page":"client","client":"tiangong"}),
        )
        .await
        .unwrap();
        assert_eq!(result["queued"], true);
        assert!(result.get("opened").is_none());
        assert_eq!(
            public_status(&state).await["navigation"]["client"],
            "tiangong"
        );
        assert!(
            call(&state, "hub_open_page", json!({"page":"payment"}))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn client_configuration_returns_the_actual_independent_codex_alias_without_secrets() {
        let (_temp, state) = fixture();
        let view = call(&state, "hub_read_configuration", json!({}))
            .await
            .unwrap();
        let target = view["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["client"] == "codex")
            .unwrap();
        let result = call(
            &state,
            "hub_configure_client",
            json!({"targetId":target["id"],
            "channel":"fixture-channel","model":"old-model"}),
        )
        .await
        .unwrap();
        let alias = result["model"].as_str().unwrap();
        assert_ne!(alias, "old-model");
        let config = AppConfig::load_or_default(&state.config_path).unwrap();
        assert!(
            config
                .ai_gateway
                .codex_visible_models
                .contains(&alias.to_string())
        );
        assert!(
            !result
                .to_string()
                .contains("fixture-private-key-not-for-model")
        );
        assert!(
            !view
                .to_string()
                .contains("fixture-private-key-not-for-model")
        );
    }
    #[tokio::test]
    async fn deletion_waits_for_human_and_clear_cancels_before_configuration_changes() {
        let (_temp, state) = fixture();
        let before = std::fs::read(&state.config_path).unwrap();
        let work = state.clone();
        let task = tokio::spawn(async move {
            call(&work,"hub_delete_configurations",json!({"items":[{"kind":"channel","channel":"fixture-channel"},{"kind":"channel","channel":"keep-channel"}]})).await
        });
        let nonce = pending_deletion(&state).await;
        assert_eq!(std::fs::read(&state.config_path).unwrap(), before);
        let reply = confirm(
            State(state.clone()),
            headers(),
            Json(Confirmation {
                nonce,
                approved: false,
            }),
        )
        .await;
        assert_eq!(reply.status(), StatusCode::OK);
        assert_eq!(task.await.unwrap().unwrap()["deleted"], false);
        assert_eq!(std::fs::read(&state.config_path).unwrap(), before);
        let work = state.clone();
        let task = tokio::spawn(async move {
            call(
                &work,
                "hub_delete_configurations",
                json!({"items":[{"kind":"channel","channel":"fixture-channel"}]}),
            )
            .await
        });
        pending_deletion(&state).await;
        clear(&state).await;
        assert!(task.await.unwrap().is_err());
        assert_eq!(std::fs::read(&state.config_path).unwrap(), before);
        assert!(public_status(&state).await["pendingDeletion"].is_null());
    }
    #[tokio::test]
    async fn one_confirmation_deletes_only_selected_configuration() {
        let (_temp, state) = fixture();
        let work = state.clone();
        let task = tokio::spawn(async move {
            call(
                &work,
                "hub_delete_configurations",
                json!({"items":[{"kind":"channel","channel":"fixture-channel"}]}),
            )
            .await
        });
        let nonce = pending_deletion(&state).await;
        confirm(
            State(state.clone()),
            headers(),
            Json(Confirmation {
                nonce,
                approved: true,
            }),
        )
        .await;
        assert_eq!(task.await.unwrap().unwrap()["results"][0]["deleted"], true);
        let config = AppConfig::load_or_default(&state.config_path).unwrap();
        assert!(channel(&config, "fixture-channel").is_err());
        assert!(channel(&config, "keep-channel").is_ok());
    }
    #[tokio::test]
    async fn local_credential_form_exposes_only_a_reference_and_mask_to_tools() {
        let (_temp, state) = fixture();
        let reply = credential(
            State(state.clone()),
            headers(),
            Json(CredentialInput {
                nonce: None,
                label: Some("fixture label".into()),
                secret: "fixture-local-form-secret".into(),
                cancelled: false,
            }),
        )
        .await;
        assert_eq!(reply.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(reply.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let reference: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fixture-local-form-secret"));
        assert!(
            !public_status(&state)
                .await
                .to_string()
                .contains("fixture-local-form-secret")
        );
        call(&state,"hub_save_channel",json!({"name":"fixture-channel","settings":{},"credentialReference":reference["reference"]})).await.unwrap();
        assert_eq!(
            channel(
                &AppConfig::load_or_default(&state.config_path).unwrap(),
                "fixture-channel"
            )
            .unwrap()
            .api_key,
            "fixture-local-form-secret"
        );
        clear(&state).await;
        assert!(
            public_status(&state).await["credentialReferences"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(call(&state,"hub_save_channel",json!({"name":"fixture-channel","settings":{},"credentialReference":reference["reference"]})).await.is_err());
    }
    #[tokio::test]
    async fn cancelling_the_local_credential_form_releases_the_waiting_tool() {
        let (_temp, state) = fixture();
        let work = state.clone();
        let task =
            tokio::spawn(async move { call(&work, "hub_request_credential", json!({})).await });
        let nonce = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(nonce) =
                    public_status(&state).await["pendingCredential"]["nonce"].as_str()
                {
                    break nonce.to_owned();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let reply = credential(
            State(state.clone()),
            headers(),
            Json(CredentialInput {
                nonce: Some(nonce),
                label: None,
                secret: String::new(),
                cancelled: true,
            }),
        )
        .await;
        assert_eq!(reply.status(), StatusCode::OK);
        assert!(task.await.unwrap().is_err());
        assert!(public_status(&state).await["pendingCredential"].is_null());
    }
    #[test]
    fn native_response_redaction_removes_nested_tokens_and_url_credentials() {
        let mut value = json!({"model":{"apiKey":"private","upstream_api_key":"private","extra":{"Authorization":"private"}},"baseUrl":"https://person:pass@fixture.invalid/v1?key=private#secret","nativeFiles":[{"path":"private"} ]});
        redact(&mut value);
        assert!(!value.to_string().contains("private"));
        assert!(!value.to_string().contains("person"));
        assert_eq!(value["baseUrl"], "https://fixture.invalid/v1");
    }
}
