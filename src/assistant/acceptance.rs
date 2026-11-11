//! Explicit opt-in acceptance against the installed Server and Squad model.
//! All configuration writers use a temporary Hub and a single temporary client.
use super::*;
use crate::{
    ai_gateway::config::ProviderConfig,
    config::AppConfig,
    desktop::{
        clients::ClientId,
        resources::Library,
        targets::{ClientTarget, McpFormat},
    },
};
use anyhow::bail;
use std::{collections::HashMap, path::PathBuf};

const TARGET: &str = "assistant-acceptance-vscode";
const SOURCE: &str = "验收来源";
const NEW_CHANNEL: &str = "验收新渠道";
const FIXTURE_SECRET: &str = "synthetic-acceptance-secret-never-an-upstream-key";

#[derive(Clone, Copy)]
enum Scenario {
    Configuration,
    KeyPipeline,
    WorkBuddy,
}

struct Acceptance {
    state: SharedState,
    worker: worker::Worker,
    events: mpsc::Receiver<(String, Result<Value>)>,
    controls: mpsc::UnboundedReceiver<Control>,
    http: reqwest::Client,
    base: String,
    model_id: String,
    target_id: String,
    codex_target_id: Option<String>,
    fixture_client: ClientId,
    workbuddy_entry: Option<String>,
    codex_copy: Option<String>,
    key_name: Option<String>,
    created_key: Option<u64>,
    key_create_requested: bool,
    prepared_models: Vec<String>,
    key_revoked: bool,
    calls: Vec<Value>,
    turns: Vec<Value>,
    report: PathBuf,
    stage: String,
    confirmations: usize,
    credential_forms: usize,
    threads: Vec<String>,
    pending_calls: HashMap<String, String>,
}

impl Acceptance {
    fn config(&self) -> Result<AppConfig> {
        AppConfig::load_or_default(&self.state.config_path)
    }

    // The model may discover installed client names. Before executing a tool,
    // constrain every external/native read or write to our fixture target.
    fn allowed(&self, name: &str, args: &Value) -> Result<()> {
        let config = self.config()?;
        let fixture_binding = |id: &str| {
            config.ai_gateway.providers.iter().any(|p| {
                p.desktop_binding
                    .as_ref()
                    .is_some_and(|b| b.id == id && b.target_id == self.target_id)
            })
        };
        match name {
            "hub_help" | "hub_request_credential" | "hub_open_page" => (),
            "hub_resolve_model" => (),
            "hub_read_configuration" => ensure!(
                args["client"]
                    .as_str()
                    .is_none_or(|c| matches!(c, "codex" | "vscode" | "zcode")
                        || (c == "workbuddy" && self.fixture_client == ClientId::WorkBuddy)),
                "验收不读取用户原生客户端配置"
            ),
            "hub_save_channel" => ensure!(
                matches!(args["name"].as_str(), Some(SOURCE | NEW_CHANNEL)),
                "验收只修改临时渠道"
            ),
            "hub_add_model" => ensure!(
                matches!(args["channel"].as_str(), Some(SOURCE | NEW_CHANNEL)),
                "验收只修改临时模型"
            ),
            "hub_configure_client" => {
                ensure!(
                    args["targetId"] == self.target_id
                        || self
                            .codex_target_id
                            .as_deref()
                            .is_some_and(|id| Some(id) == args["targetId"].as_str()),
                    "验收只配置临时客户端"
                )
            }
            "hub_set_model_parameters" => ensure!(
                args["client"] == self.fixture_client.key()
                    && (if self.fixture_client == ClientId::WorkBuddy {
                        args["entryId"]
                            .as_str()
                            .is_some_and(|entry| Some(entry) == self.workbuddy_entry.as_deref())
                    } else {
                        args["id"].as_str().is_some_and(fixture_binding)
                    }),
                "验收只修改临时客户端副本"
            ),
            "hub_squad_key_options" | "hub_squad_list_api_keys" => {
                ensure!(self.key_name.is_some(), "本用例未授权账户操作");
            }
            "hub_squad_create_api_key" => {
                ensure!(
                    self.created_key.is_none()
                        && self.key_name.as_deref() == args["name"].as_str()
                        && args["quota"].as_f64() == Some(0.01)
                        && args["expiresInDays"] == 1,
                    "验收只创建一把具名、限额且限时的临时密钥"
                );
            }
            "hub_squad_prepare_api_key" => ensure!(
                self.created_key.is_some() && args["keyId"].as_u64() == self.created_key,
                "验收只准备本次新建的临时密钥"
            ),
            "hub_delete_configurations" => {
                for item in args["items"].as_array().context("删除清单无效")? {
                    match item["kind"].as_str() {
                        Some("channel" | "source_model") => ensure!(
                            matches!(item["channel"].as_str(), Some(SOURCE | NEW_CHANNEL)),
                            "验收只删除临时渠道/模型"
                        ),
                        Some("client_model") => ensure!(
                            item["id"].as_str().is_some_and(fixture_binding),
                            "验收只删除临时副本"
                        ),
                        _ => bail!("验收不执行未知删除操作"),
                    }
                }
            }
            _ => bail!("验收不执行未知工具"),
        }
        Ok(())
    }

    async fn event(&mut self, event: (String, Result<Value>)) -> Result<()> {
        let (epoch, message) = event;
        ensure!(epoch == self.worker.epoch, "验收会话已变化");
        if let Ok(value) = &message {
            if matches!(
                value["method"].as_str(),
                Some("thread/started" | "turn/started")
            ) {
                if let Some(id) = value["params"]["threadId"]
                    .as_str()
                    .or_else(|| value["params"]["thread"]["id"].as_str())
                {
                    if !self.threads.iter().any(|t| t == id) {
                        self.threads.push(id.to_owned());
                    }
                }
            }
            if value["method"] == "item/tool/call" {
                let params = &value["params"];
                let name = params["tool"].as_str().context("工具缺少名称")?;
                let args = match params["arguments"].as_str() {
                    Some(text) => serde_json::from_str(text)?,
                    None => params["arguments"].clone(),
                };
                self.allowed(name, &args)?;
                if name == "hub_squad_create_api_key" {
                    self.key_create_requested = true;
                }
                let id = value["id"].to_string();
                self.pending_calls.insert(id, name.to_owned());
                self.calls.push(json!({"stage":self.stage,"name":name}));
                self.write_report(false)?;
            }
        }
        self.worker.event(&self.state, message).await
    }

    async fn forms(&mut self, approve: bool) -> Result<()> {
        let public = tools::public_status(&self.state).await;
        if let Some(nonce) = public["pendingCredential"]["nonce"].as_str() {
            let response = self
                .http
                .post(format!("{}/api/desktop/assistant/credential", self.base))
                .header("x-tiancai-desktop", "1")
                .json(&json!({"nonce":nonce,"label":"隔离验收合成凭据","secret":FIXTURE_SECRET}))
                .send()
                .await?;
            ensure!(response.status().is_success(), "验收本机凭据表单提交失败");
            self.credential_forms += 1;
        }
        if let Some(nonce) = public["pendingDeletion"]["nonce"].as_str() {
            let response = self
                .http
                .post(format!("{}/api/desktop/assistant/confirm", self.base))
                .header("x-tiancai-desktop", "1")
                .json(&json!({"nonce":nonce,"approved":approve}))
                .send()
                .await?;
            ensure!(response.status().is_success(), "验收批量确认提交失败");
            self.confirmations += 1;
        }
        Ok(())
    }

    async fn step(&mut self) -> Result<()> {
        tokio::select! {
            event = self.events.recv() => self.event(event.context("Server 事件已关闭")?).await?,
            control = self.controls.recv() => match control.context("工具结果通道已关闭")? {
                Control::Tool { epoch, turn, id, result } => {
                    ensure!(epoch == self.worker.epoch, "工具会话已变化");
                    let name = self.pending_calls.remove(&id.to_string()).unwrap_or_default();
                    if let Ok(value) = &result {
                        if name == "hub_squad_create_api_key" {
                            self.created_key = value["keyId"].as_u64();
                        } else if name == "hub_squad_prepare_api_key" {
                            self.prepared_models = value["settings"]["models"].as_array()
                                .into_iter().flatten().filter_map(|v|v.as_str().map(str::to_owned)).collect();
                        } else if name == "hub_configure_client" {
                            if value["client"]=="workbuddy" { self.workbuddy_entry=value["entryId"].as_str().map(str::to_owned); }
                            if value["client"]=="codex" { self.codex_copy=value["model"].as_str().map(str::to_owned); }
                        }
                    }
                    // Only record success/shape, never the private raw model or tool transport.
                    self.calls.push(json!({"stage":self.stage,"resultFor":name,"success":result.is_ok()}));
                    self.worker.tool_result(&turn, id, result).await?;
                }
                _ => bail!("验收收到未预期控制消息"),
            },
            _ = tokio::time::sleep(Duration::from_millis(80)) => (),
        }
        self.worker.check_timeout()?;
        Ok(())
    }

    async fn turn(
        &mut self,
        label: &str,
        prompt: &str,
        approve: bool,
        interrupt: bool,
    ) -> Result<()> {
        self.stage = label.to_owned();
        let before = self.confirmations;
        {
            let mut inner = self.state.assistant.inner.lock().await;
            inner.running = true;
            inner.messages.push(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: "user",
                text: prompt.into(),
                status: "completed",
            });
            inner.messages.push(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: "assistant",
                text: String::new(),
                status: "running",
            });
        }
        self.worker
            .turn(prompt, super::personality::Personality::Professional)
            .await?;
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut interrupted = false;
        while self.state.assistant.inner.lock().await.running {
            ensure!(Instant::now() < deadline, "真实验收回合超时：{label}");
            self.step().await?;
            self.forms(approve).await?;
            let length = self
                .state
                .assistant
                .inner
                .lock()
                .await
                .messages
                .last()
                .map_or(0, |m| m.text.chars().count());
            if interrupt && !interrupted && length >= 40 && self.worker.has_active_turn() {
                tools::cancel_pending(&self.state).await;
                self.worker.interrupt().await?;
                if let Some(bridge) = &self.state.assistant.inner.lock().await.bridge {
                    bridge.cancel();
                }
                interrupted = true;
            }
        }
        let inner = self.state.assistant.inner.lock().await;
        let reply = inner.messages.last().context("验收没有回复")?;
        ensure!(
            reply.status == if interrupt { "stopped" } else { "completed" },
            "回合状态异常 {label}: {}",
            reply.status
        );
        ensure!(!reply.text.contains(FIXTURE_SECRET), "合成凭据意外进入回复");
        self.turns.push(
            json!({"stage":label,"status":reply.status,"reply":if self.key_name.is_some(){"[account response omitted]"}else{&reply.text},
            "confirmations":self.confirmations-before,"interrupted":interrupted}),
        );
        drop(inner);
        self.write_report(false)?;
        eprintln!("acceptance stage passed: {label}");
        Ok(())
    }

    fn write_report(&self, passed: bool) -> Result<()> {
        let value = json!({"passed":passed,"realServer":true,"realSquadModel":true,"model":self.model_id,
            "scope":"isolated Hub config and fixture client only; existing Squad login and shared Server reused",
            "calls":self.calls,"turns":self.turns,"confirmations":self.confirmations,"credentialForms":self.credential_forms,
            "temporaryAccountKeyCreated":self.created_key.is_some(),"temporaryAccountKeyRevoked":self.key_revoked,
            "uniqueThreadNotifications":self.threads.len(),"stage":self.stage});
        std::fs::write(&self.report, serde_json::to_vec_pretty(&value)?)?;
        Ok(())
    }

    async fn scenarios(&mut self) -> Result<()> {
        self.turn("帮助与读取", "这是隔离验收环境。先读取Hub帮助，再查看当前配置，只简要汇报名为‘验收来源’的渠道和名为‘验收 VS Code’的客户端支持哪些操作，不列出其他已安装目标，不作修改。后续只操作这些验收条目。",false,false).await?;
        self.turn("渠道模型与凭据", "新建普通渠道‘验收新渠道’，使用验收来源同一地址https://fixture.invalid/v1、协议OpenAI Responses；本机凭据表单会提供与验收来源相同的测试凭据，保留新渠道停用，不实际请求该地址。沿同连接已存在真实ID source-model添加别名acceptance-alias指向source-model，不凭空增加上游模型。权重7、超时90秒，保存后回读核对。",false,false).await?;
        let config = self.config()?;
        let p = config
            .ai_gateway
            .providers
            .iter()
            .find(|p| p.name == NEW_CHANNEL)
            .context("新渠道没有实际保存")?;
        ensure!(
            !p.enabled && p.weight == 7 && p.timeout_secs == 90 && p.api_key == FIXTURE_SECRET,
            "新渠道实际值不符"
        );
        ensure!(
            p.resolve_upstream_model("acceptance-alias") == Some("source-model"),
            "模型别名没有保存"
        );
        self.turn("客户端配置与参数", "现在启用验收新渠道，然后把它的acceptance-alias配置到名称严格为‘验收 VS Code’的目标，标签‘验收模型’，只选择这个临时目标，不启动客户端，不实际请求上游。对返回的新副本设置上下文64000、输出上限4096、开启流式，可选推理强度low和high、默认high；仅设置该端实际支持的字段。保存后回读确认。",false,false).await?;
        let config = self.config()?;
        let b = config
            .ai_gateway
            .providers
            .iter()
            .filter_map(|p| p.desktop_binding.as_ref())
            .find(|b| b.target_id == TARGET)
            .context("没有实际配置临时客户端")?;
        ensure!(
            b.capabilities["contextWindow"] == 64000
                && b.capabilities["maxTokens"] == 4096
                && b.capabilities["preferStream"] == true
                && b.capabilities["reasoningEfforts"] == json!(["low", "high"])
                && b.capabilities["defaultReasoningEffort"] == "high",
            "临时副本参数不符"
        );
        let path = Library::new(&self.state.config_path)?
            .targets(&Library::new(&self.state.config_path)?.load()?)?
            .into_iter()
            .find(|t| t.id == TARGET)
            .unwrap()
            .root
            .join("chatLanguageModels.json");
        let native: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            native[0]["models"][0]["name"] == "验收模型"
                && native[0]["models"][0]["contextWindow"] == 64000
                && native[0]["models"][0]["maxOutputTokens"] == 4096
                && native[0]["models"][0]["supportsReasoningEffort"] == json!(["low", "high"]),
            "临时客户端原生参数没有实际保存"
        );
        let before = std::fs::read(&self.state.config_path)?;
        let count = self.confirmations;
        self.turn("删除取消与导航", "请打开Hub帮助页。随后集中删除验收新渠道和刚才配置到验收 VS Code的验收模型副本，一次列出这两项等待本机确认，不要先删。",false,false).await?;
        ensure!(
            self.confirmations == count + 1 && std::fs::read(&self.state.config_path)? == before,
            "取消删除不应写入配置或重复确认"
        );
        let count = self.confirmations;
        self.turn("批准批量删除", "重新提交刚才那两项删除：验收新渠道与验收 VS Code中的验收模型副本，一次集中确认，批准后执行并回读验证。保留验收来源。",true,false).await?;
        ensure!(self.confirmations == count + 1, "本轮应只有一次批量确认");
        let config = self.config()?;
        ensure!(
            !config
                .ai_gateway
                .providers
                .iter()
                .any(|p| p.name == NEW_CHANNEL
                    || p.desktop_binding
                        .as_ref()
                        .is_some_and(|b| b.target_id == TARGET)),
            "批准后仍残留待删条目"
        );
        ensure!(
            config
                .ai_gateway
                .providers
                .iter()
                .any(|p| p.name == SOURCE && p.api_key == FIXTURE_SECRET),
            "无关原测试渠道应保留"
        );
        self.turn("停止真实流", "请详细介绍Hub中模型渠道、客户端独立副本、天工协助与天才搭档的关系，写八段完整使用说明，不修改配置。",false,true).await?;
        self.turn(
            "停止后续聊",
            "刚才已停止长回复。请读取配置，只回复验收来源是否仍然保留，不修改。",
            false,
            false,
        )
        .await?;
        // Models/aliases may legitimately be saved together with their channel.
        // Check the saved result above rather than force a redundant add call.
        for name in [
            "hub_help",
            "hub_read_configuration",
            "hub_save_channel",
            "hub_configure_client",
            "hub_set_model_parameters",
            "hub_delete_configurations",
            "hub_request_credential",
            "hub_open_page",
        ] {
            ensure!(
                self.calls.iter().any(|c| c["name"] == name),
                "真实验收未调用工具 {name}"
            );
        }
        ensure!(self.threads.len() == 1, "真实多轮应保持同一个thread");
        Ok(())
    }

    async fn key_pipeline(&mut self) -> Result<()> {
        let key_name = self.key_name.clone().context("未设置临时密钥名")?;
        self.turn("创建当前用户临时密钥", &format!("这是用户授权的隔离配置验收。先读取小分队密钥可用分组，为当前已登录用户创建且仅创建一把名为‘{key_name}’的临时API密钥，额度上限0.01、有效期1天。若有多个可用分组，本次授权按读取顺序选择第一个；无分组时可不指定。不充值、不发起模型连通测试，不操作任何已有密钥，也不把明文密钥放入聊天。创建后仅简要报告本次结果。"),false,false).await?;
        ensure!(self.created_key.is_some(), "真实临时密钥没有创建成功");
        self.turn("读取列表并选择指定密钥", &format!("获取当前API密钥列表，按名称搜索并只选择刚才的‘{key_name}’，其他密钥名称和信息不在回复中复述。用列表中它的keyId准备导入，读取实际返回的协议、地址和可用模型，暂不保存渠道和客户端。密钥使用本机引用，不索要明文。"),false,false).await?;
        let selected = self
            .prepared_models
            .first()
            .cloned()
            .context("真实导入未返回可用模型，不能编造")?;
        self.turn("密钥到渠道和指定模型保存", &format!("沿刚才指定密钥准备结果的真实settings及credentialReference，新建并启用普通渠道‘验收新渠道’。读取Hub目标，只把来源模型‘{selected}’配置到名称严格为‘验收 ZCode’的临时客户端，标签‘验收流水线模型’，不操作其他目标、不启动客户端、不实际请求此用户密钥的模型。保存后回读确认渠道、客户端及指定模型的实际身份。"),false,false).await?;
        let config = self.config()?;
        let channel = config
            .ai_gateway
            .providers
            .iter()
            .find(|p| p.name == NEW_CHANNEL)
            .context("流水线未保存厂商渠道")?;
        ensure!(
            channel.enabled
                && !channel.api_key.is_empty()
                && channel.api_key != FIXTURE_SECRET
                && channel.matches_model(&selected)
                && channel.import_source.is_some(),
            "流水线渠道未保存真实引用凭据、模型或导入来源"
        );
        let binding = config
            .ai_gateway
            .providers
            .iter()
            .filter_map(|p| p.desktop_binding.as_ref())
            .find(|b| b.target_id == self.target_id)
            .context("流水线未保存指定客户端副本")?;
        ensure!(
            binding.client == ClientId::ZCode
                && binding.model == selected
                && binding.label == "验收流水线模型",
            "流水线客户端模型身份不符"
        );
        let library = Library::new(&self.state.config_path)?;
        let root = library
            .targets(&library.load()?)?
            .into_iter()
            .find(|t| t.id == self.target_id)
            .unwrap()
            .root;
        let native: Value = serde_json::from_slice(&std::fs::read(root.join("v2/config.json"))?)?;
        ensure!(
            native["provider"]
                .as_object()
                .context("原生客户端配置无效")?
                .values()
                .any(|p| p["models"].get(&selected).is_some() && p["name"] == "验收流水线模型"),
            "流水线模型没有实际写入客户端文件"
        );
        for required in [
            "hub_squad_key_options",
            "hub_squad_create_api_key",
            "hub_squad_list_api_keys",
            "hub_squad_prepare_api_key",
            "hub_save_channel",
            "hub_configure_client",
            "hub_read_configuration",
        ] {
            ensure!(
                self.calls.iter().any(|c| c["name"] == required),
                "流水线未经过真实工具 {required}"
            );
        }
        ensure!(
            self.credential_forms == 0 && self.threads.len() == 1,
            "已登录用户密钥应直接以引用完成同会话流程"
        );
        Ok(())
    }
    async fn workbuddy_pipeline(&mut self) -> Result<()> {
        let before = std::fs::read(&self.state.config_path)?;
        self.turn("未知GPT模型不造源", "请给ChatGPT配置一个gpt-6.2模型。只使用目前已有模型来源，不新增或修改来源渠道，不运行客户端。",false,false).await?;
        ensure!(
            std::fs::read(&self.state.config_path)? == before,
            "不存在的模型不应写入任何配置"
        );
        self.turn("WorkBuddy前缀歧义", "给workbuddy配置一个gpt-6-a的模型。只使用现有来源，若存在歧义先列出真实候选让我选择，不擅自新增上游ID。",false,false).await?;
        ensure!(
            std::fs::read(&self.state.config_path)? == before,
            "模型前缀有歧义时不应先修改配置"
        );
        self.turn("明确WorkBuddy现有别名", "我选择验收来源中精确存在的别名gpt-6-a-preset，配置到名称严格为‘验收 WorkBuddy’的临时目标，并回读核对。不要新增源模型，不启动客户端，不请求该来源的推理API。",false,false).await?;
        let entry = self
            .workbuddy_entry
            .clone()
            .context("没有执行WorkBuddy配置工具或未取得真实entryId")?;
        let config = self.config()?;
        let native = crate::workbuddy_config::load_selected(&config, Some(&entry))?;
        ensure!(native.error.is_none(), "WorkBuddy原生状态读取失败");
        let row = native
            .entries
            .iter()
            .find(|row| row.entry_id == entry)
            .context("WorkBuddy原生文件没有保存本次模型")?;
        ensure!(
            row.model.provider_model == "gpt-6-a-preset"
                && row.model.id == format!("tiancaispacehub-{entry}"),
            "WorkBuddy别名或条目身份不符"
        );
        let dedicated = config
            .ai_gateway
            .providers
            .iter()
            .find(|p| p.name == format!("workbuddy:{entry}"))
            .context("缺少WorkBuddy独立来源")?;
        ensure!(
            dedicated.resolve_upstream_model(&row.model.id) == Some("gpt-6-astra"),
            "WorkBuddy别名未解析到真实GPT上游"
        );
        self.turn("WorkBuddy参数回读", "把刚才实际WorkBuddy条目的默认思考强度改为max，按该条目支持的字段执行并回读。不改来源和其他条目。",false,false).await?;
        let native = crate::workbuddy_config::load_selected(&self.config()?, Some(&entry))?;
        ensure!(
            native.model.upstream_default_reasoning_effort == "max",
            "WorkBuddy默认强度未保存"
        );
        self.turn("Codex真实独立副本回读", "继续使用验收来源中精确存在的gpt-6-a-preset别名，把它配置到临时ChatGPT/Codex默认目标，保存后用返回的独立model回读，确认using、channel及upstreamModel。不改来源目录或原有条目，不启动客户端。",false,false).await?;
        let alias = self
            .codex_copy
            .as_deref()
            .context("未真正调用Codex配置工具并取得独立别名")?;
        ensure!(alias != "gpt-6-a-preset", "Codex必须返回独立副本身份");
        let view = test_tool_scope(
            &self.state,
            tools::call(
                &self.state,
                "hub_read_configuration",
                json!({"client":"codex"}),
            ),
        )
        .await?;
        let saved = view["client"]["models"]
            .as_array()
            .context("Codex回读缺少模型列表")?
            .iter()
            .find(|m| m["model"] == alias)
            .context("独立别名未出现在Codex回读")?;
        ensure!(
            saved["using"] == true
                && saved["upstreamModel"] == "gpt-6-astra"
                && saved["channel"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("Codex · ")),
            "Codex副本实际来源或使用状态不符"
        );
        let config = self.config()?;
        let source = config
            .ai_gateway
            .providers
            .iter()
            .find(|p| p.name == SOURCE)
            .unwrap();
        ensure!(
            source.models == ["gpt-6-astra", "gpt-6-astra-fast"]
                && source
                    .model_aliases
                    .get("gpt-6-a-preset")
                    .map(String::as_str)
                    == Some("gpt-6-astra"),
            "配置客户端不应扩充来源目录"
        );
        for required in [
            "hub_read_configuration",
            "hub_resolve_model",
            "hub_configure_client",
            "hub_set_model_parameters",
        ] {
            ensure!(
                self.calls.iter().any(|call| call["name"] == required),
                "未实际调用工具 {required}"
            );
        }
        ensure!(
            self.threads.len() == 1,
            "WorkBuddy及Codex配置应保持一个对话"
        );
        Ok(())
    }
}

#[tokio::test]
#[ignore = "Explicit user-authorized real Server/model test; requires TIANCAI_ASSISTANT_ACCEPTANCE_CONFIG and TIANCAI_ASSISTANT_ACCEPTANCE_REPORT_DIR"]
async fn real_server_and_squad_model_follow_the_hub_configuration_workflow() -> Result<()> {
    run_acceptance(Scenario::Configuration).await
}

#[tokio::test]
#[ignore = "Explicit user-authorized temporary real account key creation and revocation; isolated Hub/client writes"]
async fn real_squad_key_selection_and_creation_flow_saves_the_specified_client_model() -> Result<()>
{
    run_acceptance(Scenario::KeyPipeline).await
}

#[tokio::test]
#[ignore = "Explicit real administrator-model acceptance; subprocess isolates all native client homes"]
async fn real_workbuddy_gpt_selection_and_codex_copy_use_existing_sources() -> Result<()> {
    const NAME: &str = "real_workbuddy_gpt_selection_and_codex_copy_use_existing_sources";
    if std::env::var("TIANCAI_ASSISTANT_NATIVE_ACCEPTANCE").as_deref() != Ok(NAME) {
        let report = PathBuf::from(
            std::env::var_os("TIANCAI_ASSISTANT_ACCEPTANCE_REPORT_DIR")
                .context("未指定验收报告目录")?,
        );
        ensure!(report.is_absolute() && report.is_dir(), "验收报告目录无效");
        let temp = tempfile::Builder::new()
            .prefix("isolated-native-")
            .tempdir_in(report)?;
        let home = temp.path().join("user");
        std::fs::create_dir_all(&home)?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                &format!("assistant::acceptance::{NAME}"),
                "--ignored",
                "--nocapture",
            ])
            .env("TIANCAI_ASSISTANT_NATIVE_ACCEPTANCE", NAME)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("APPDATA", home.join("AppData/Roaming"))
            .env("WORKBUDDY_CONFIG_PATH", home.join(".workbuddy/models.json"))
            .env("GMCLAW_CONFIG_PATH", home.join("tiangong/electron-data.db"))
            .env("DSH_HOME", home.join("dsh"))
            .env("ZCODE_HOME", home.join("zcode"))
            .output()?;
        ensure!(
            output.status.success(),
            "隔离原生客户端验收失败：{}",
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(
            String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed;"),
            "隔离原生客户端验收未实际执行"
        );
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Ok(());
    }
    run_acceptance(Scenario::WorkBuddy).await
}

async fn run_acceptance(scenario: Scenario) -> Result<()> {
    let key_pipeline = matches!(scenario, Scenario::KeyPipeline);
    let workbuddy_pipeline = matches!(scenario, Scenario::WorkBuddy);
    let production = PathBuf::from(
        std::env::var_os("TIANCAI_ASSISTANT_ACCEPTANCE_CONFIG")
            .context("未指定现有登录/运行端配置路径")?,
    );
    let report_dir = PathBuf::from(
        std::env::var_os("TIANCAI_ASSISTANT_ACCEPTANCE_REPORT_DIR")
            .context("未指定验收报告目录")?,
    );
    ensure!(
        report_dir.is_absolute() && report_dir.is_dir(),
        "验收报告目录必须已存在且为绝对路径"
    );
    let fixture = tempfile::Builder::new()
        .prefix("isolated-hub-")
        .tempdir_in(&report_dir)?;
    ensure!(
        fixture.path().parent() == Some(report_dir.as_path()),
        "临时目录必须位于本轮报告目录内"
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let path = fixture.path().join("config.toml");
    let mut config = AppConfig {
        bind: address.to_string(),
        state_path: fixture.path().join("state.json"),
        ..Default::default()
    };
    config.ai_gateway.providers = vec![ProviderConfig {
        name: SOURCE.into(),
        base_url: "https://fixture.invalid/v1".into(),
        api_key: FIXTURE_SECRET.into(),
        models: if workbuddy_pipeline {
            vec!["gpt-6-astra".into(), "gpt-6-astra-fast".into()]
        } else {
            vec!["source-model".into()]
        },
        model_aliases: if workbuddy_pipeline {
            std::collections::BTreeMap::from([("gpt-6-a-preset".into(), "gpt-6-astra".into())])
        } else {
            Default::default()
        },
        enabled: true,
        ..Default::default()
    }];
    config.save(&path)?;
    let library = Library::new(&path)?;
    let fixture_client = if workbuddy_pipeline {
        ClientId::WorkBuddy
    } else if key_pipeline {
        ClientId::ZCode
    } else {
        ClientId::VsCode
    };
    let mut target_id = if key_pipeline {
        "assistant-acceptance-zcode"
    } else {
        TARGET
    }
    .to_string();
    let client_root = if workbuddy_pipeline {
        crate::workbuddy_config::config_path()
            .parent()
            .context("隔离WorkBuddy路径无效")?
            .to_path_buf()
    } else {
        fixture.path().join(fixture_client.key())
    };
    std::fs::create_dir_all(&client_root)?;
    let codex_target_id = if workbuddy_pipeline {
        let defaults = crate::desktop::targets::discover()?;
        target_id = defaults
            .iter()
            .find(|target| target.client == ClientId::WorkBuddy)
            .context("未发现隔离WorkBuddy目标")?
            .id
            .clone();
        Some(
            defaults
                .iter()
                .find(|target| target.client == ClientId::Codex)
                .context("未发现隔离Codex目标")?
                .id
                .clone(),
        )
    } else {
        None
    };
    library.save_targets(
        library.load()?,
        vec![ClientTarget {
            id: target_id.clone(),
            client: fixture_client,
            label: if workbuddy_pipeline {
                "验收 WorkBuddy"
            } else if key_pipeline {
                "验收 ZCode"
            } else {
                "验收 VS Code"
            }
            .into(),
            root: client_root.clone(),
            skills: client_root.join("skills"),
            mcp: client_root.join("mcp.json"),
            mcp_format: McpFormat::McpServers,
            present: true,
            model_mode: if workbuddy_pipeline {
                "workbuddy"
            } else {
                "chat"
            }
            .into(),
            model_root: None,
            models_inherited: false,
        }],
    )?;
    let state = crate::app_state::AppState::new(path, config, None, None);
    let squad = Arc::new(Squad::new(&production, &AppConfig::default())?);
    let account = squad.assistant_identity()?.context("请先在Hub登录小分队")?;
    let model = model::Configuration::parse(squad.assistant_config(&account).await?)?;
    let model_id = model.model.id.clone();
    {
        let mut inner = state.assistant.inner.lock().await;
        inner.enabled = true;
        inner.logged_in = true;
        inner.config = Some(model.clone());
    }
    let ready_runtime = crate::local_assistance::runtime::resolve_ready(&production)?;
    let router = router().with_state(state.clone());
    let http_server = tokio::spawn(async move { axum::serve(listener, router).await });
    let (events_tx, events) = mpsc::channel(128);
    let worker = match worker::Worker::start(
        &state,
        ready_runtime,
        squad.clone(),
        account.clone(),
        model,
        events_tx,
    )
    .await
    {
        Ok(worker) => worker,
        Err(error) => {
            http_server.abort();
            return Err(error);
        }
    };
    let controls = state
        .assistant
        .rx
        .lock()
        .await
        .take()
        .context("验收控制通道不可用")?;
    let mut test = Acceptance {
        state,
        worker,
        events,
        controls,
        http: reqwest::Client::builder().no_proxy().build()?,
        base: format!("http://{address}"),
        model_id: model_id.clone(),
        target_id,
        codex_target_id,
        fixture_client,
        workbuddy_entry: None,
        codex_copy: None,
        key_name: key_pipeline.then(|| format!("Hub搭档验收-{}", uuid::Uuid::new_v4())),
        created_key: None,
        key_create_requested: false,
        prepared_models: vec![],
        key_revoked: false,
        calls: vec![],
        turns: vec![],
        report: report_dir.join(if workbuddy_pipeline {
            "real-workbuddy-gpt-selection.json"
        } else if key_pipeline {
            "real-key-pipeline.json"
        } else {
            "real-acceptance.json"
        }),
        stage: "初始化".into(),
        confirmations: 0,
        credential_forms: 0,
        threads: vec![],
        pending_calls: HashMap::new(),
    };
    let result: Result<()> = async {
        let deadline = Instant::now() + Duration::from_secs(40);
        while !test.worker.ready() {
            ensure!(Instant::now() < deadline, "真实Server初始化超时");
            test.step().await?;
        }
        eprintln!("real Server initialized; administrator model: {model_id}");
        if workbuddy_pipeline {
            test.workbuddy_pipeline().await
        } else if key_pipeline {
            test.key_pipeline().await
        } else {
            test.scenarios().await
        }
    }
    .await;
    tools::cancel_pending(&test.state).await;
    if let Some(bridge) = test.state.assistant.inner.lock().await.bridge.take() {
        bridge.cancel();
    }
    let closed = test.worker.close().await;
    tools::clear(&test.state).await;
    http_server.abort();
    // A lost create reply is not proof of failure. Resolve only this run's
    // unique name before cleanup, without replaying the creation request.
    if test.created_key.is_none() && test.key_create_requested {
        if let Some(name) = &test.key_name {
            let list = squad
                .request_checked(
                    "GET",
                    "/keys",
                    vec![
                        ("search".into(), name.clone()),
                        ("page_size".into(), "100".into()),
                    ],
                    None,
                    Some(&account),
                )
                .await?;
            let matches = list
                .as_array()
                .or_else(|| list["items"].as_array())
                .into_iter()
                .flatten()
                .filter(|row| row["name"].as_str() == Some(name.as_str()))
                .filter_map(|row| row["id"].as_u64())
                .collect::<Vec<_>>();
            ensure!(matches.len() <= 1, "临时密钥名称返回多项，未执行撤销");
            test.created_key = matches.first().copied();
        }
    }
    // Revoke only the unique key created by this test, including failed runs.
    let revoked = if let Some(id) = test.created_key {
        squad
            .request_checked(
                "DELETE",
                &format!("/keys/{id}"),
                vec![],
                None,
                Some(&account),
            )
            .await
            .map(|_| {
                test.key_revoked = true;
            })
    } else {
        Ok(())
    };
    test.write_report(false)?;
    revoked?;
    result?;
    closed?;
    ensure!(
        test.config()?
            .ai_gateway
            .providers
            .iter()
            .any(|p| p.name == SOURCE),
        "关闭会话不应删除已保存配置"
    );
    ensure!(
        crate::local_assistance::runtime::resolve_ready(&production).is_ok(),
        "共享Server应保留"
    );
    test.write_report(true)
}
