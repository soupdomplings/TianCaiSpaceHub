//! GMClaw execution over the existing, authenticated IM transports.
//! Bindings are deliberately separate from Codex threads and scoped to sender.
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, Semaphore};

use crate::{
    app_state::SharedState,
    gmclaw_executor::{
        GmClawChatRequest, GmClawClient, GmClawConfirmationAction, GmClawConfirmationDecision,
        GmClawPendingConfirmation, GmClawProject,
    },
    im::core::{
        executor_approval::GmClawApproval,
        executor_turn::TurnDisplayGuard,
        outbound::{ImOutboundKind, ImOutboundMessage, ImOutboundPayload, ImOutboundSender},
        routing::route_for_message,
    },
    im::executor_commands::{ExecutorCommand, ExecutorTarget, TargetCommand, parse_command},
    types::{ImPlatformKind, InboundAction, InboundMessage, SessionUiEntry, now_ms},
};

const MAX_SESSIONS: usize = 128;
const MAX_HISTORY: usize = 32;
const APPROVAL_TTL: Duration = Duration::from_secs(15 * 60);
const EXECUTION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

mod desktop;
pub(crate) mod sessions;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GmClawBridgeConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub auth_token: String,
    /// Optional desktop executable/App path; credentials are passed only via the child environment.
    pub desktop_path: Option<String>,
    /// Legacy directory offered as a candidate only; each IM session owns its path.
    pub project_path: String,
    pub model_id: Option<String>,
    pub max_steps: u32,
}

impl std::fmt::Debug for GmClawBridgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GmClawBridgeConfig")
            .field("enabled", &self.enabled)
            .field("endpoint", &self.endpoint)
            .field("auth_token", &"[redacted]")
            .field("desktop_path", &self.desktop_path)
            .field("project_path", &self.project_path)
            .field("model_id", &self.model_id)
            .field("max_steps", &self.max_steps)
            .finish()
    }
}

impl Default for GmClawBridgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: crate::gmclaw_executor::DEFAULT_ENDPOINT.into(),
            auth_token: String::new(),
            desktop_path: None,
            project_path: String::new(),
            model_id: None,
            max_steps: 30,
        }
    }
}

impl GmClawBridgeConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        crate::gmclaw_executor::validate_endpoint(&self.endpoint)?;
        ensure!(
            !self.auth_token.trim().is_empty()
                && self.auth_token.len() <= 8192
                && self.auth_token.is_ascii()
                && !self.auth_token.chars().any(char::is_control),
            "天工外部消息接入需要有效的连接 Token"
        );
        ensure!(
            (1..=200).contains(&self.max_steps),
            "天工最大执行步数必须为 1–200"
        );
        ensure!(
            self.model_id
                .as_ref()
                .is_none_or(|id| id.len() <= 256 && !id.chars().any(char::is_control)),
            "天工模型 ID 无效"
        );
        Ok(())
    }

    fn fingerprint(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(&(
                self.enabled,
                &self.endpoint,
                &self.auth_token,
                &self.model_id,
                self.max_steps,
            ))
            .expect("GMClaw config serialization"),
        ))
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct SenderKey {
    platform: ImPlatformKind,
    account: String,
    chat: String,
    sender: String,
}

impl SenderKey {
    fn for_message(message: &InboundMessage) -> Self {
        Self {
            platform: message.platform,
            account: message.account_id.clone(),
            chat: message.chat_id.clone(),
            sender: message.sender_id.clone(),
        }
    }
}

struct PendingApproval {
    code: String,
    card: GmClawApproval,
    created_at: Instant,
    confirmation: GmClawPendingConfirmation,
    runtime_authorization_identity: String,
}

struct Session {
    id: String,
    title: String,
    project_path: String,
    model_id: Option<String>,
    config_fingerprint: String,
    pending: Option<PendingApproval>,
    // Set before handing execution to Harness. Cancellation or a stream failure
    // leaves this set so another message cannot unknowingly replay a tool turn.
    uncertain: bool,
    desktop: Option<desktop::DesktopContext>,
}

impl Session {
    fn new(config: &GmClawBridgeConfig, project_path: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: "新会话".into(),
            project_path,
            model_id: config.model_id.clone(),
            config_fingerprint: config.fingerprint(),
            pending: None,
            uncertain: false,
            desktop: None,
        }
    }
}

#[derive(Default)]
struct SenderSessions {
    current: Option<Session>,
    history: VecDeque<Session>,
}

impl SenderSessions {
    fn ensure_can_leave(&self) -> Result<()> {
        if let Some(session) = &self.current {
            ensure!(
                session.pending.is_none(),
                "天工正在等待工具审批，请先批准或拒绝当前轮次。"
            );
            ensure!(
                !session.uncertain,
                "天工上次执行状态未知，请先到天工桌面确认结果，再用 /tg new 开始新会话；本次没有退出或停止任务。"
            );
        }
        Ok(())
    }

    fn archive_current(&mut self) {
        if let Some(session) = self.current.take() {
            self.history.push_front(session);
        }
        self.history.truncate(MAX_HISTORY);
    }
}

struct SenderBinding {
    // Selection must remain observable while a long request holds the state lock.
    selected: AtomicBool,
    ui_epoch: AtomicU64,
    nonce: String,
    sessions: Mutex<SenderSessions>,
}

pub struct GmClawImState {
    sessions: Mutex<HashMap<SenderKey, Arc<SenderBinding>>>,
    desktop_claims: Mutex<HashMap<String, String>>,
    seen: Mutex<VecDeque<(SenderKey, String)>>,
    // Transport receipts are independent of the session execution lock. Sending
    // a card must never block the outbound worker on an in-flight Harness turn.
    approval_message_ids: Mutex<VecDeque<(String, String)>>,
    execution: Semaphore,
    waiting: Semaphore,
}

impl Default for GmClawImState {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            desktop_claims: Mutex::new(HashMap::new()),
            seen: Mutex::new(VecDeque::new()),
            approval_message_ids: Mutex::new(VecDeque::new()),
            execution: Semaphore::new(1),
            waiting: Semaphore::new(33),
        }
    }
}

pub(crate) fn reply(
    outbound: &ImOutboundSender,
    message: &InboundMessage,
    text: impl Into<String>,
) -> Result<()> {
    outbound.enqueue(ImOutboundMessage {
        thread_id: "gmclaw-im".into(),
        route: route_for_message(message),
        item_id: None,
        item_type: Some("gmclaw".into()),
        kind: ImOutboundKind::TurnReply,
        payload: ImOutboundPayload::Text(text.into()),
    })
}

pub(crate) async fn remember_approval_message_id(
    state: &SharedState,
    request_key: &str,
    message_id: String,
) {
    if request_key.is_empty() || message_id.is_empty() {
        return;
    }
    let mut receipts = state.gmclaw_im.approval_message_ids.lock().await;
    receipts.retain(|(key, _)| key != request_key);
    receipts.push_back((request_key.to_owned(), message_id));
    while receipts.len() > MAX_SESSIONS {
        receipts.pop_front();
    }
}

fn enqueue_approval(
    outbound: &ImOutboundSender,
    message: &InboundMessage,
    payload: ImOutboundPayload,
) -> Result<()> {
    outbound.enqueue(ImOutboundMessage {
        thread_id: "gmclaw-im".into(),
        route: route_for_message(message),
        item_id: None,
        item_type: Some("gmclaw".into()),
        kind: ImOutboundKind::Approval,
        payload,
    })
}

pub(crate) fn sender_allowed(config: &crate::config::AppConfig, message: &InboundMessage) -> bool {
    fn allowed(list: &[String], value: &str) -> bool {
        list.is_empty() || list.iter().any(|entry| entry.trim() == value)
    }
    if message.sender_id.trim().is_empty() || message.chat_id.trim().is_empty() {
        return false;
    }
    match message.platform {
        ImPlatformKind::Feishu => {
            config
                .feishu_account(&message.account_id)
                .is_some_and(|account| {
                    account.is_active()
                        && allowed(&account.allowed_open_ids, &message.sender_id)
                        && allowed(&account.allowed_chat_ids, &message.chat_id)
                        && (!account.mention_only
                            || message.chat_type == crate::types::ChatType::Direct
                            || message.mentioned)
                })
        }
        ImPlatformKind::Wechat => {
            config
                .wechat_account(&message.account_id)
                .is_some_and(|account| {
                    account.is_active() && allowed(&account.allowed_user_ids, &message.sender_id)
                })
        }
        ImPlatformKind::Wecom => config
            .wecom_account(&message.account_id)
            .is_some_and(|account| {
                account.is_active()
                    && allowed(&account.allowed_user_ids, &message.sender_id)
                    && allowed(
                        &account.allowed_chat_ids,
                        message
                            .chat_id
                            .strip_prefix("group:")
                            .or_else(|| message.chat_id.strip_prefix("single:"))
                            .unwrap_or(&message.chat_id),
                    )
            }),
        ImPlatformKind::Telegram => false,
    }
}

pub(crate) fn turn_delivery_allowed(
    config: &crate::config::AppConfig,
    message: &InboundMessage,
    fingerprint: &str,
) -> bool {
    sender_allowed(config, message) && config.gmclaw_bridge.fingerprint() == fingerprint
}

const HELP: &str = "天工 Claw 指令：\n/tg — 切到天工，保留当前会话\n/gpt — 切到 ChatGPT（Codex）\n/wb — WorkBuddy（暂不支持，不切换）\n/q — 退出当前会话并选择新建或恢复\n/s — 请求停止（当前天工接口不支持，请在天工桌面停止）\n/tg new — 新建天工会话\n/tg status — 查看当前状态\n工具审批：点击当前审批卡片的批准/拒绝；微信回复 /1 批准、/2 拒绝\n/tg approve 确认码、/tg reject 确认码 — 兼容旧审批及备用操作\n兼容 /gmclaw；支持文字和工具审批，附件请在天工桌面使用。";
const STOP_UNSUPPORTED: &str = "当前天工 Harness 没有独立停止接口，本次未停止任务。请到天工桌面停止并确认结果；Hub 不会将断开连接视为已取消。";

async fn binding_for(state: &SharedState, message: &InboundMessage) -> Option<Arc<SenderBinding>> {
    state
        .gmclaw_im
        .sessions
        .lock()
        .await
        .get(&SenderKey::for_message(message))
        .cloned()
}

pub(crate) async fn is_selected(state: &SharedState, message: &InboundMessage) -> bool {
    binding_for(state, message)
        .await
        .is_some_and(|binding| binding.selected.load(Ordering::Acquire))
}

pub(crate) async fn deactivate(state: &SharedState, message: &InboundMessage) -> Result<()> {
    if let Some(binding) = binding_for(state, message).await {
        let sessions = binding
            .sessions
            .try_lock()
            .map_err(|_| anyhow::anyhow!("天工请求正在执行或排队，请等待完成后再切换。"))?;
        sessions.ensure_can_leave()?;
        desktop::release(state, &binding, sessions.current.as_ref()).await;
        binding.selected.store(false, Ordering::Release);
        binding.ui_epoch.fetch_add(1, Ordering::AcqRel);
    }
    Ok(())
}

async fn codex_busy(state: &SharedState, message: &InboundMessage) -> bool {
    let conversation = message.conversation_key();
    let runtime = state.runtime.lock().await;
    runtime.has_pending_approvals(&conversation)
        || runtime.route_by_thread.iter().any(|(thread_id, route)| {
            route.conversation_key == conversation
                && (runtime.starting_turn_by_thread.contains(thread_id)
                    || runtime.current_turn_by_thread.contains_key(thread_id))
        })
}

fn gmclaw_command(text: &str) -> Option<String> {
    let command = match parse_command(text, false) {
        ExecutorCommand::Switch(ExecutorTarget::GmClaw) => String::new(),
        ExecutorCommand::Switch(ExecutorTarget::Codex) => "off".into(),
        ExecutorCommand::Target {
            target: ExecutorTarget::GmClaw,
            command,
        } => match command {
            TargetCommand::New => "new".into(),
            TargetCommand::Status => "status".into(),
            TargetCommand::Help => "help".into(),
            TargetCommand::Off => "off".into(),
            TargetCommand::Approve(code) => format!("approve {code}"),
            TargetCommand::Reject(code) => format!("reject {code}"),
        },
        ExecutorCommand::InvalidArguments { command, .. }
            if ["/tg", "/gmclaw", "/gpt"]
                .iter()
                .any(|known| command.eq_ignore_ascii_case(known)) =>
        {
            "invalid".into()
        }
        _ => return None,
    };
    Some(command)
}

fn is_control_callback(message: &InboundMessage) -> bool {
    message.action.is_some()
        || message.approval_request_key.is_some()
        || message.card_message_id.is_some()
        || matches!(
            message.callback_kind,
            Some(
                crate::types::InboundCallbackKind::CardEvent
                    | crate::types::InboundCallbackKind::Welcome
            )
        )
}

fn approval_reply_index(text: &str) -> Option<usize> {
    match text.trim().to_ascii_lowercase().as_str() {
        "/1" | "1" | "/y" | "/yes" | "y" | "yes" => Some(1),
        "/2" | "2" | "/n" | "/no" | "n" | "no" => Some(2),
        _ => None,
    }
}

fn approval_request_key(
    message: &InboundMessage,
    binding: &SenderBinding,
    session: &Session,
    runtime_identity: &str,
) -> String {
    let identity = serde_json::to_vec(&(
        "gmclaw-approval",
        message.platform.key(),
        &message.account_id,
        &message.chat_id,
        &message.sender_id,
        &binding.nonce,
        binding.ui_epoch.load(Ordering::Acquire),
        &session.id,
        &session.config_fingerprint,
        runtime_identity,
        uuid::Uuid::new_v4().to_string(),
    ))
    .expect("approval identity serialization");
    hex::encode(Sha256::digest(identity))
}

fn current_approval_button_index(
    pending: &PendingApproval,
    request_key: &str,
    option_index: usize,
) -> Option<usize> {
    // No default option: malformed, stale or cross-sender buttons cannot approve.
    (pending.card.request_key == request_key && matches!(option_index, 1 | 2))
        .then_some(option_index)
}

fn session_scope(
    binding: &SenderBinding,
    config: &GmClawBridgeConfig,
    message: &InboundMessage,
) -> String {
    let identity = serde_json::to_vec(&(
        message.platform.key(),
        &message.account_id,
        &message.chat_id,
        &message.sender_id,
        config.fingerprint(),
        &binding.nonce,
        binding.ui_epoch.load(Ordering::Acquire),
    ))
    .expect("session scope serialization");
    format!("gmclaw-{}", hex::encode(Sha256::digest(identity)))
}

/// Returns true only when the message belongs to the GMClaw flow. The bridge
/// serializes dispatch per chat; the binding lock also protects direct callers.
pub(crate) async fn handle_inbound(
    state: &SharedState,
    outbound: &ImOutboundSender,
    message: &mut InboundMessage,
    dispatch: &mut Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<bool> {
    if message.platform == ImPlatformKind::Telegram {
        return Ok(false);
    }
    let text = message.text.trim().to_string();
    let callback = is_control_callback(message);
    let approval_button = match &message.action {
        Some(InboundAction::GmClawApprovalDecision {
            request_key,
            option_index,
        }) => Some((request_key.clone(), *option_index)),
        _ => None,
    };
    if text.eq_ignore_ascii_case("/wb") && !callback {
        return Ok(false);
    }
    let command_text = if callback {
        None
    } else {
        gmclaw_command(&text)
    };
    let command = command_text.as_deref();
    let key = SenderKey::for_message(message);
    let app_config = state.config.lock().await.clone();
    let existing = binding_for(state, message).await;
    let selected = existing
        .as_ref()
        .is_some_and(|b| b.selected.load(Ordering::Acquire));
    if !selected && command.is_none() && approval_button.is_none() {
        return Ok(false);
    }
    if !sender_allowed(&app_config, message) {
        return Ok(true);
    }
    if approval_button.is_some() && !selected {
        reply(
            outbound,
            message,
            "这张天工审批卡片已失效或不属于你当前的会话，未提交批准或拒绝。请由本轮消息发起者处理当前审批。",
        )?;
        return Ok(true);
    }
    // Multiple actions on one card share its message ID. Card replay is guarded
    // by scoped routing requests, not the ordinary text message deduplication.
    if !callback && !message.message_id.is_empty() {
        let mut seen = state.gmclaw_im.seen.lock().await;
        let identity = (key.clone(), message.message_id.clone());
        if seen.contains(&identity) {
            return Ok(true);
        }
        seen.push_back(identity);
        while seen.len() > 512 {
            seen.pop_front();
        }
    }
    if command == Some("off") {
        if let Err(error) = deactivate(state, message).await {
            reply(outbound, message, error.to_string())?;
        } else {
            reply(
                outbound,
                message,
                "当前执行端：ChatGPT（Codex）。沿用原会话、新建/恢复入口，/s 中断、/q 退出；发送 /tg 可返回天工。",
            )?;
        }
        return Ok(true);
    }
    if command == Some("help") {
        reply(outbound, message, HELP)?;
        return Ok(true);
    }
    if command == Some("status") && existing.is_none() {
        reply(outbound, message, "当前未选择天工 Claw；发送 /tg 进入。")?;
        return Ok(true);
    }
    if selected && text.eq_ignore_ascii_case("/s") && !callback {
        reply(outbound, message, STOP_UNSUPPORTED)?;
        return Ok(true);
    }
    if message.received_at_ms > 0
        && now_ms().saturating_sub(message.received_at_ms) > 15 * 60 * 1000
    {
        reply(
            outbound,
            message,
            "这条消息已过期，未执行操作，请重新发送。",
        )?;
        return Ok(true);
    }
    let mut config = app_config.gmclaw_bridge;
    if !selected && !matches!(command, Some("" | "new" | "status")) {
        reply(outbound, message, "先发送 /tg 切到天工。")?;
        return Ok(true);
    }
    if !selected && matches!(command, Some("" | "new")) && codex_busy(state, message).await {
        reply(
            outbound,
            message,
            "当前 Codex 任务正在启动、执行或等待审批，请处理后再切到天工。",
        )?;
        return Ok(true);
    }
    let binding = if let Some(binding) = existing {
        binding
    } else {
        let mut bindings = state.gmclaw_im.sessions.lock().await;
        if bindings.len() >= MAX_SESSIONS {
            reply(
                outbound,
                message,
                "天工发送者绑定已达 128 个上限，本次未创建。",
            )?;
            return Ok(true);
        }
        bindings
            .entry(key)
            .or_insert_with(|| {
                Arc::new(SenderBinding {
                    selected: AtomicBool::new(false),
                    ui_epoch: AtomicU64::new(0),
                    nonce: uuid::Uuid::new_v4().to_string(),
                    sessions: Mutex::new(SenderSessions::default()),
                })
            })
            .clone()
    };
    let Ok(mut sessions) = binding.sessions.try_lock() else {
        reply(
            outbound,
            message,
            "当前天工请求正在执行或排队，请等待完成。切换不会取消天工工具。",
        )?;
        return Ok(true);
    };
    if command == Some("status") {
        let status = if !selected {
            "当前未选择天工，原会话保留。"
        } else if sessions.current.as_ref().is_some_and(|s| s.uncertain) {
            "天工上次执行状态未知，请到桌面确认结果；不会自动重试。"
        } else if sessions
            .current
            .as_ref()
            .is_some_and(|s| s.pending.is_some())
        {
            "当前天工会话等待工具审批，请操作最近一轮审批卡片；微信回复 /1 批准、/2 拒绝。"
        } else if sessions.current.is_some() {
            "当前文字交给天工；/q 退出当前会话，/gpt 返回 Codex。"
        } else {
            "当前执行端：天工 Claw。请使用当前会话选择或新建界面；/q 重新打开。"
        };
        reply(
            outbound,
            message,
            if let Some(s) = &sessions.current {
                format!("{status}\n项目目录：{}", s.project_path)
            } else {
                status.into()
            },
        )?;
        return Ok(true);
    }
    if matches!(command, Some("" | "new")) {
        if !message.attachments.is_empty() {
            reply(outbound, message, "请单独发送 /tg 或 /tg new。")?;
            return Ok(true);
        }
        if command == Some("new") {
            if let Some(session) = sessions.current.as_mut() {
                if let Some(pending) = &session.pending {
                    let runtime_identity = desktop::instance_identity(&config).await?;
                    if session.config_fingerprint == config.fingerprint()
                        && pending.created_at.elapsed() <= APPROVAL_TTL
                        && pending.runtime_authorization_identity == runtime_identity
                    {
                        reply(
                            outbound,
                            message,
                            "天工正在等待工具审批，请先批准或拒绝，不能通过新建会话跳过。",
                        )?;
                        return Ok(true);
                    }
                    session.pending = None;
                    session.uncertain = true;
                }
                if session.uncertain {
                    reply(
                        outbound,
                        message,
                        "旧任务仍标记为状态未知，此操作没有停止旧任务；请先在天工桌面核对结果。",
                    )?;
                }
            }
            desktop::release(state, &binding, sessions.current.as_ref()).await;
            sessions.archive_current();
        } else if sessions.current.as_ref().is_some_and(|s| s.uncertain) {
            if let Err(error) = desktop::claim(
                state,
                &binding,
                &sessions
                    .current
                    .as_ref()
                    .expect("current session checked")
                    .id,
            )
            .await
            {
                reply(
                    outbound,
                    message,
                    format!("{error}；可用 /tg new 选择其他会话。"),
                )?;
                return Ok(true);
            }
            binding.selected.store(true, Ordering::Release);
            reply(
                outbound,
                message,
                "已保留当前天工会话及执行状态。请用 /tg status 查看状态，并先处理原任务或审批。",
            )?;
            return Ok(true);
        }
        reply(outbound, message, "正在自动连接天工…")?;
        let (prepared, status) = match crate::gmclaw_runtime::prepare_for_im(state, message).await {
            Ok(result) => result,
            Err(error) => {
                if sender_allowed(&*state.config.lock().await, message) {
                    reply(
                        outbound,
                        message,
                        format!(
                            "天工自动连接未完成：{error}\n可重新发送 /tg；/gpt 返回 Codex。本次未发送模型任务，也未停止已有任务。"
                        ),
                    )?;
                }
                return Ok(true);
            }
        };
        config = prepared;
        use crate::gmclaw_runtime::GmClawConnectionState;
        if status.state != GmClawConnectionState::Connected
            && !(status.state == GmClawConnectionState::Unverified && status.running)
        {
            reply(
                outbound,
                message,
                format!("{}\n处理后重新发送 /tg；/gpt 返回 Codex。", status.detail),
            )?;
            return Ok(true);
        }
        if let Some(session) = &sessions.current {
            if let Err(error) = desktop::claim(state, &binding, &session.id).await {
                reply(
                    outbound,
                    message,
                    format!("{error}；可用 /tg new 选择其他会话。"),
                )?;
                return Ok(true);
            }
        }
        binding.selected.store(true, Ordering::Release);
        if let Some(session) = &sessions.current {
            reply(
                outbound,
                message,
                format!(
                    "{}\n已切到天工 Claw，保留当前会话。\n项目目录：{}\n/q 退出并选择会话。",
                    status.detail, session.project_path
                ),
            )?;
            return Ok(true);
        }
        binding.ui_epoch.fetch_add(1, Ordering::AcqRel);
        message.session_scope = Some(session_scope(&binding, &config, message));
        message.session_entry = Some(if command == Some("new") {
            SessionUiEntry::Create
        } else {
            SessionUiEntry::Choice
        });
        message.text.clear();
        reply(outbound, message, status.detail)?;
        return Ok(false);
    }
    if text.eq_ignore_ascii_case("/q") && !callback {
        if let Err(error) = sessions.ensure_can_leave() {
            reply(outbound, message, error.to_string())?;
            return Ok(true);
        }
        desktop::release(state, &binding, sessions.current.as_ref()).await;
        sessions.archive_current();
        binding.ui_epoch.fetch_add(1, Ordering::AcqRel);
        message.session_scope = Some(session_scope(&binding, &config, message));
        message.session_entry = Some(SessionUiEntry::Choice);
        message.text.clear();
        return Ok(false);
    }
    if !config.enabled || config.validate().is_err() {
        reply(
            outbound,
            message,
            "天工连接尚未准备好，请发送 /tg 自动连接；/gpt 返回 Codex。",
        )?;
        return Ok(true);
    }
    if callback && approval_button.is_none() {
        message.session_scope = Some(session_scope(&binding, &config, message));
        if message.approval_request_key.is_some()
            || (message.action.is_none()
                && (message.callback_kind == Some(crate::types::InboundCallbackKind::CardEvent)
                    || (message.platform == ImPlatformKind::Feishu
                        && message.card_message_id.is_some())))
            || matches!(message.action, Some(InboundAction::ApprovalDecision { .. }))
            || sessions.current.is_some()
        {
            reply(
                outbound,
                message,
                "这张卡片已不适用于当前执行端或会话，请使用当前会话的卡片。审批请操作最近一轮天工审批卡片。",
            )?;
            return Ok(true);
        }
        if message.action.is_none() {
            message.session_entry = Some(SessionUiEntry::Choice);
        }
        return Ok(false);
    }
    if sessions.current.is_none() {
        if approval_button.is_some() {
            reply(
                outbound,
                message,
                "当前没有等待确认的天工工具调用，旧审批卡片未执行。",
            )?;
            return Ok(true);
        }
        if command.is_some() {
            reply(outbound, message, HELP)?;
            return Ok(true);
        }
        if !message.attachments.is_empty() {
            reply(
                outbound,
                message,
                "请通过会话设置选择项目；天工外部消息暂不支持附件。",
            )?;
            return Ok(true);
        }
        message.session_scope = Some(session_scope(&binding, &config, message));
        return Ok(false);
    }
    let session = sessions.current.as_mut().expect("current session checked");
    if config.fingerprint() != session.config_fingerprint {
        reply(
            outbound,
            message,
            "天工连接配置已改变；旧会话不能在新连接继续。请先处理旧审批并到桌面核对，再发送 /tg new。",
        )?;
        return Ok(true);
    }
    if session.uncertain {
        reply(
            outbound,
            message,
            "上次执行状态未知，请先在天工桌面检查执行结果，再发送 /tg new。Hub 不会重放任务。",
        )?;
        return Ok(true);
    }
    if !message.attachments.is_empty() {
        reply(
            outbound,
            message,
            "天工外部消息当前只支持文字，未转发本条消息及附件。请在天工桌面处理附件。",
        )?;
        return Ok(true);
    }
    if message.received_at_ms > 0
        && now_ms().saturating_sub(message.received_at_ms) > 15 * 60 * 1000
    {
        reply(
            outbound,
            message,
            "这条消息已超过 15 分钟，未交给天工执行，请重新发送。",
        )?;
        return Ok(true);
    }
    let mut confirmation = None;
    let legacy_decision = if let Some(command) = command {
        let parts: Vec<_> = command.split_whitespace().collect();
        if parts.len() != 2 || !matches!(parts[0], "approve" | "reject") {
            reply(outbound, message, HELP)?;
            return Ok(true);
        }
        Some((if parts[0] == "approve" { 1 } else { 2 }, parts[1]))
    } else {
        None
    };
    let text_decision = (!callback && session.pending.is_some())
        .then(|| approval_reply_index(&text))
        .flatten();
    let mut decision_index = None;
    if approval_button.is_some() || legacy_decision.is_some() || text_decision.is_some() {
        let Some(pending) = session.pending.as_ref() else {
            reply(outbound, message, "当前没有等待确认的天工工具调用。")?;
            return Ok(true);
        };
        if pending.created_at.elapsed() > APPROVAL_TTL {
            session.pending = None;
            session.uncertain = true;
            reply(
                outbound,
                message,
                "审批已过期，未执行批准。请发送 /tg new 开始新会话。",
            )?;
            return Ok(true);
        }
        let index = if let Some((request_key, option_index)) = &approval_button {
            let Some(index) = current_approval_button_index(pending, request_key, *option_index)
            else {
                reply(
                    outbound,
                    message,
                    "这张天工审批卡片已失效、选项无效或不属于当前会话，未执行操作。请处理最近一轮审批卡片。",
                )?;
                return Ok(true);
            };
            index
        } else if let Some((index, code)) = legacy_decision {
            if code != pending.code {
                reply(
                    outbound,
                    message,
                    "确认码不匹配，请使用当前会话最近一轮审批消息中的完整确认码。",
                )?;
                return Ok(true);
            }
            index
        } else {
            text_decision.expect("validated text approval")
        };
        decision_index = Some(index);
        confirmation = Some(GmClawConfirmationDecision {
            action: if index == 1 {
                GmClawConfirmationAction::Approve
            } else {
                GmClawConfirmationAction::Deny
            },
            call_ids: pending
                .confirmation
                .pending_calls
                .iter()
                .map(|call| call.call_id.clone())
                .collect(),
        });
    } else if session.pending.is_some() {
        reply(
            outbound,
            message,
            "天工正在等待工具审批。请点击最近一轮卡片的批准/拒绝；微信回复 /1 批准、/2 拒绝。",
        )?;
        return Ok(true);
    } else if text.starts_with('/') {
        reply(outbound, message, HELP)?;
        return Ok(true);
    } else if text.is_empty() || text.len() > 128 * 1024 {
        reply(outbound, message, "请发送非空文字，单条最多 128 KiB。")?;
        return Ok(true);
    }
    let mut turn_display =
        TurnDisplayGuard::begin(outbound, message, &session.id, &session.config_fingerprint)?;
    let Ok(_waiting) = state.gmclaw_im.waiting.try_acquire() else {
        turn_display.finish("天工等待队列已满，本条未执行，请稍后重新发送。")?;
        return Ok(true);
    };
    // Destination and sender state are now locked. Other senders in this chat
    // may dispatch while this request queues or runs; this sender remains busy.
    drop(dispatch.take());
    let Ok(Ok(_permit)) =
        tokio::time::timeout(Duration::from_secs(30), state.gmclaw_im.execution.acquire()).await
    else {
        turn_display.finish("天工仍在处理其他请求，本条未交给天工，请稍后重新发送。")?;
        return Ok(true);
    };
    // Startup may still be initializing after /tg. Never submit a task to an
    // unverified service, and never consume an approval on a preflight failure.
    match tokio::time::timeout(
        crate::gmclaw_runtime::STATUS_TIMEOUT + Duration::from_secs(1),
        crate::gmclaw_runtime::status(&config),
    )
    .await
    {
        Ok(status) if status.state == crate::gmclaw_runtime::GmClawConnectionState::Connected => {}
        Ok(status) => {
            turn_display.finish(format!(
                "{}\n连接会自动恢复，本条未交给天工执行，审批仍保留。请稍后重新发送本条消息。",
                status.detail
            ))?;
            return Ok(true);
        }
        Err(_) => {
            turn_display.finish("天工连接检查超时，本条未执行，审批仍保留。请稍后重试。")?;
            return Ok(true);
        }
    }
    // A removed model can silently fall back to the Harness default. Check the
    // pinned model before consuming any approval or marking execution uncertain.
    if let Err(error) = sessions::validate_session_model(session.model_id.as_deref()).await {
        if sender_allowed(&*state.config.lock().await, message) {
            turn_display.finish(format!("天工任务未提交：{error}。原会话与审批仍保留。"))?;
        }
        return Ok(true);
    }
    // Recheck local configuration after waiting, before consuming an approval.
    let latest = state.config.lock().await.clone();
    if !sender_allowed(&latest, message) {
        return Ok(true);
    }
    if latest.gmclaw_bridge.fingerprint() != session.config_fingerprint {
        turn_display.finish("等待期间连接配置已改变，本条未执行。请发送 /tg new。")?;
        return Ok(true);
    }
    if confirmation.is_some()
        && session
            .pending
            .as_ref()
            .is_none_or(|pending| pending.created_at.elapsed() > APPROVAL_TTL)
    {
        session.pending = None;
        session.uncertain = true;
        turn_display.finish("等待期间审批已过期，本条未执行。请发送 /tg new。")?;
        return Ok(true);
    }
    let authorization = crate::gmclaw_runtime::connection_authorization(&config).await?;
    let runtime_authorization_identity = desktop::instance_identity(&config).await?;
    if confirmation.is_some()
        && session.pending.as_ref().is_some_and(|pending| {
            pending.runtime_authorization_identity != runtime_authorization_identity
        })
    {
        turn_display.finish("天工运行实例的授权已变化，旧工具审批未提交。请到天工核对旧任务，再用 /tg new 开始新会话。",
        )?;
        return Ok(true);
    }
    let desktop_client = crate::gmclaw_desktop::DesktopClient::with_authorization(
        &config.endpoint,
        authorization.clone(),
    )?;
    if let Err(error) =
        desktop::validate_live(&desktop_client, session, confirmation.is_some()).await
    {
        turn_display.finish(format!("天工任务未提交：{error}"))?;
        return Ok(true);
    }
    let client = GmClawClient::with_authorization(&config.endpoint, authorization)?;
    let desktop_context = session.desktop.clone().context("此会话未绑定天工桌面")?;
    let request = GmClawChatRequest {
        user_id: desktop_context.user_id.clone(),
        session_id: session.id.clone(),
        chat_id: uuid::Uuid::new_v4().to_string(),
        query: if confirmation.is_some() {
            String::new()
        } else {
            text.to_string()
        },
        model_id: session.model_id.clone(),
        max_steps: desktop_context.max_steps,
        project: desktop_context.project.clone(),
        confirmation,
    };
    if let Err(error) = request.body() {
        turn_display.finish(format!(
            "天工任务未提交：{error}。原会话与审批仍保留，请先恢复项目目录或检查配置。"
        ))?;
        return Ok(true);
    }
    if request.confirmation.is_none() && session.title == "新会话" {
        session.title = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(60)
            .collect();
    }
    session.uncertain = true;
    desktop::save(state, session).await?;
    let task_id = desktop_context.task_id.clone();
    desktop_client.rename_task(&task_id, &session.title).await?;
    let user_content = if let Some(decision) = &request.confirmation {
        format!("[TianCaiSpaceHub] 工具审批：{:?}", decision.action)
    } else {
        text.to_owned()
    };
    desktop_client
        .append_message(&task_id, "user", &user_content, &[])
        .await?;
    let message_id = desktop_client
        .append_message(
            &task_id,
            "system",
            "正在处理…（由 TianCaiSpaceHub 发起）",
            &[],
        )
        .await?;
    if let Some(context) = session
        .desktop
        .as_mut()
        .filter(|context| context.task_id == task_id)
    {
        context.remember_owned_reply(message_id);
        // Display metadata is best effort. The required pre-execution unknown
        // state was already saved; never suppress an authorized model turn only
        // because this extra display-ownership save could not be confirmed.
        let _ = desktop::save(state, session).await;
    }
    crate::gmclaw_runtime::queue_desktop_refresh(&task_id, &session.id, Some(message_id)).await;
    let recorder = Arc::new(Mutex::new(desktop::TurnRecorder::new(
        desktop_client.clone(),
        task_id.clone(),
        message_id,
    )));
    let approval_message_id = if let Some(pending) = &session.pending {
        let receipts = state.gmclaw_im.approval_message_ids.lock().await;
        receipts
            .iter()
            .find(|(key, _)| key == &pending.card.request_key)
            .map(|(_, id)| id.clone())
    } else {
        None
    };
    // Desktop recording can take time. Recheck approval validity after every
    // preparatory await, immediately before consuming it and submitting once.
    if decision_index.is_some() {
        let final_runtime_identity = desktop::instance_identity(&config).await?;
        let final_config = state.config.lock().await.clone();
        if !sender_allowed(&final_config, message) {
            return Ok(true);
        }
        if final_config.gmclaw_bridge.fingerprint() != session.config_fingerprint {
            turn_display
                .finish("提交前天工连接配置已改变，工具审批未执行。请到天工核对原任务。")?;
            return Ok(true);
        }
        let Some(pending) = session.pending.as_ref() else {
            turn_display.finish("当前工具审批已失效，未提交。")?;
            return Ok(true);
        };
        if pending.created_at.elapsed() > APPROVAL_TTL {
            session.pending = None;
            turn_display.finish("提交前工具审批已过期，未执行批准。请到天工核对原任务。")?;
            return Ok(true);
        }
        if pending.runtime_authorization_identity != final_runtime_identity {
            turn_display
                .finish("提交前天工运行实例已变化，旧工具审批未执行。请到天工核对原任务。")?;
            return Ok(true);
        }
    }
    let submitted_approval = session.pending.take();
    // All local preflight checks and desktop writes succeeded. Disable the
    // current controls before entering Harness execution; an uncertain result
    // must not leave an apparently reusable approval on the screen.
    if let (Some(mut pending), Some(option_index)) = (submitted_approval, decision_index) {
        pending.card.message_id = approval_message_id;
        // UI delivery failures must not interrupt or replay an accepted turn.
        let _ = enqueue_approval(
            outbound,
            message,
            ImOutboundPayload::GmClawApprovalResolved {
                approval: pending.card,
                option_index,
                callback: approval_button.as_ref().map(|_| message.clone()),
            },
        );
    }
    turn_display.running();
    let callback_recorder = recorder.clone();
    let result = tokio::time::timeout(
        EXECUTION_TIMEOUT,
        client.execute(&request, move |event| {
            let recorder = callback_recorder.clone();
            async move {
                recorder.lock().await.event(event).await;
                Ok(())
            }
        }),
    )
    .await;
    let latest = state.config.lock().await.clone();
    if !sender_allowed(&latest, message)
        || latest.gmclaw_bridge.fingerprint() != session.config_fingerprint
    {
        return Ok(true);
    }
    let result = match result {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            let _ = recorder
                .lock()
                .await
                .finish("执行未确认完成，请在天工检查结果；Hub 不会自动重试。")
                .await;
            crate::gmclaw_runtime::queue_desktop_refresh(&task_id, &session.id, Some(message_id))
                .await;
            turn_display.finish(format!(
                "天工请求未确认完成：{error}\n不会自动重试，请检查天工后用 /tg new 重新开始。"
            ))?;
            return Ok(true);
        }
        Err(_) => {
            let _ = recorder
                .lock()
                .await
                .finish("等待超时，执行状态未知；请在天工检查结果，Hub 未停止或重试任务。")
                .await;
            crate::gmclaw_runtime::queue_desktop_refresh(&task_id, &session.id, Some(message_id))
                .await;
            turn_display.finish("天工请求等待超过 15 分钟，执行状态未知。请到天工检查结果；不会自动重试或声称已取消。",
            )?;
            return Ok(true);
        }
    };
    if let Some(error) = result.error {
        let _ = recorder
            .lock()
            .await
            .finish(&format!("天工返回错误：{error}"))
            .await;
        crate::gmclaw_runtime::queue_desktop_refresh(&task_id, &session.id, Some(message_id)).await;
        turn_display.finish(format!(
            "天工返回错误：{error}\n请检查天工后用 /tg new 重新开始。"
        ))?;
        return Ok(true);
    }
    let desktop_content = if let Some(pending) = &result.pending_confirmation {
        format!("{}\n（等待确认：{}）", result.answer, pending.prompt)
    } else if result.answer.trim().is_empty() {
        "天工本轮已结束，没有返回文字。".into()
    } else {
        result.answer.clone()
    };
    if let Err(error) = recorder.lock().await.finish(&desktop_content).await {
        turn_display.finish(format!(
            "{}\n桌面对话保存尚未确认：{error}。请到天工核对；不会重放本轮。",
            bounded_reply(&result.answer)
        ))?;
        return Ok(true);
    }
    crate::gmclaw_runtime::queue_desktop_refresh(&task_id, &session.id, Some(message_id)).await;
    if let Some(pending) = result.pending_confirmation {
        let summary = approval_summary(&pending);
        let summary = match summary {
            Ok(summary) => summary,
            Err(error) => {
                turn_display.finish(format!(
                    "无法在消息中完整展示本次工具审批：{error}。未批准，请到天工检查。"
                ))?;
                return Ok(true);
            }
        };
        let code = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let card = GmClawApproval {
            request_key: approval_request_key(
                message,
                &binding,
                session,
                &runtime_authorization_identity,
            ),
            summary: format!(
                "天工请求执行以下工具（仅本轮请求发起者可审批，15 分钟内有效）：\n{summary}"
            ),
            message_id: None,
            legacy_code: code.clone(),
        };
        session.pending = Some(PendingApproval {
            code,
            card: card.clone(),
            created_at: Instant::now(),
            confirmation: pending,
            runtime_authorization_identity,
        });
        session.uncertain = false;
        desktop::save(state, session).await?;
        turn_display.finish(if result.answer.trim().is_empty() {
            "天工正在等待工具审批，请处理本次审批请求。".into()
        } else {
            format!(
                "{}\n\n天工正在等待工具审批，请处理本次审批请求。",
                bounded_reply(&result.answer)
            )
        })?;
        enqueue_approval(outbound, message, ImOutboundPayload::GmClawApproval(card))?;
        return Ok(true);
    } else {
        let mut final_text = if result.answer.trim().is_empty() {
            "天工本轮已结束，没有返回文字。".into()
        } else {
            bounded_reply(&result.answer)
        };
        if desktop_client.complete_task(&task_id).await.is_err() {
            final_text.push_str("\n\n对话已保存，但桌面完成标记未确认；请在天工查看。");
        }
        turn_display.finish(final_text)?;
    }
    session.uncertain = false;
    desktop::save(state, session).await?;
    Ok(true)
}

fn approval_summary(pending: &GmClawPendingConfirmation) -> Result<String> {
    ensure!(
        !pending.pending_calls.is_empty() && pending.pending_calls.len() <= 16,
        "待审批工具数量不支持"
    );
    let mut ids = std::collections::HashSet::new();
    let mut summary = String::new();
    for (index, call) in pending.pending_calls.iter().enumerate() {
        ensure!(
            !call.call_id.trim().is_empty() && ids.insert(&call.call_id),
            "工具调用 ID 无效"
        );
        summary.push_str(&format!(
            "{}. {}\n{}\n",
            index + 1,
            call.tool_name,
            serde_json::to_string_pretty(&call.arguments)?
        ));
    }
    ensure!(summary.len() <= 24 * 1024, "工具参数超过消息展示上限");
    Ok(summary)
}

fn bounded_reply(answer: &str) -> String {
    // Do not monopolize the shared outbound worker with hundreds of messages.
    // Approval parameters use their separate all-or-nothing size check.
    let mut chars = answer.chars();
    let mut text: String = chars.by_ref().take(16_000).collect();
    if chars.next().is_some() {
        text.push_str("\n\n[回复过长，已截取前 16000 字；完整内容请在天工会话记录中查看。]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval_fixture_state() -> SharedState {
        use crate::app_state::{
            AppState, FeishuWsState, RemoteControlState, TelegramState, WechatRecoveryState,
            WechatState,
        };
        use std::path::PathBuf;

        let mut config = crate::config::AppConfig::default();
        config.state_path = PathBuf::from(":memory:");
        config.feishu.account_id = "fixture-account".into();
        config.feishu.app_id = "fixture-app".into();
        config.feishu.app_secret = "fixture-secret".into();
        config.gmclaw_bridge.enabled = true;
        config.gmclaw_bridge.endpoint = "http://127.0.0.1:1".into();
        config.gmclaw_bridge.auth_token = "fixture-authorization".into();
        // AppState::new reads persisted state and initializes disk logs. These
        // rejection fixtures use only in-memory state, including SQLite logs.
        let state = Arc::new(AppState {
            config_path: PathBuf::from(":memory:"),
            daemon_identity: crate::daemon_process::DaemonIdentity {
                service: "fixture".into(),
                pid: 1,
                instance_id: "fixture-instance".into(),
                started_at_ms: 0,
            },
            config: Mutex::new(config),
            ai_gateway_request_logs: crate::ai_gateway::request_log::RequestLogStore::new(
                PathBuf::from(":memory:"),
            ),
            ai_gateway_routing: Mutex::new(Default::default()),
            persisted: Mutex::new(Default::default()),
            runtime: Mutex::new(Default::default()),
            gmclaw_im: GmClawImState::default(),
            im_dispatch: Mutex::new(Default::default()),
            remote_control: RemoteControlState::new(),
            events: Mutex::new(Vec::new()),
            bridge_task: Mutex::new(None),
            feishu_ws: Mutex::new(FeishuWsState::default()),
            telegram: Mutex::new(TelegramState::default()),
            wechat: Mutex::new(WechatState::default()),
            wechat_recovery: Mutex::new(WechatRecoveryState::default()),
            im_accounts: Mutex::new(Default::default()),
            wechat_onboard: Mutex::new(None),
            wecom_onboard: Mutex::new(None),
            shutdown_tx: Mutex::new(None),
        });
        // A regression that misses an early rejection still cannot reach any
        // runtime discovery, DataServer, Harness, or model request. Assertions
        // below distinguish the intended refusal from this queue tripwire.
        state.gmclaw_im.waiting.close();
        state
    }

    fn approval_fixture_message(sender: &str) -> InboundMessage {
        serde_json::from_value(serde_json::json!({
            "platform":"feishu", "accountId":"fixture-account", "senderId":sender,
            "chatId":"fixture-group", "chatType":"group", "messageId":"fixture-card",
            "text":"", "mentioned":true
        }))
        .unwrap()
    }

    fn approval_fixture_pending() -> PendingApproval {
        PendingApproval {
            code: "fixture-code".into(),
            card: GmClawApproval {
                request_key: "fixture-current-request".into(),
                summary: "fixture tool parameters".into(),
                message_id: None,
                legacy_code: "fixture-code".into(),
            },
            created_at: Instant::now(),
            confirmation: GmClawPendingConfirmation {
                prompt: "fixture confirmation".into(),
                pending_calls: vec![crate::gmclaw_executor::GmClawPendingCall {
                    call_id: "fixture-call".into(),
                    tool_name: "fixture-tool".into(),
                    arguments: serde_json::json!({"argument":"fixture-value"}),
                }],
            },
            runtime_authorization_identity: "fixture-runtime".into(),
        }
    }

    fn approval_fixture_binding(selected: bool, session: Option<Session>) -> Arc<SenderBinding> {
        Arc::new(SenderBinding {
            selected: AtomicBool::new(selected),
            ui_epoch: AtomicU64::new(0),
            nonce: "fixture-binding".into(),
            sessions: Mutex::new(SenderSessions {
                current: session,
                history: VecDeque::new(),
            }),
        })
    }

    fn assert_approval_fixture_refusal(
        receiver: &mut crate::im::core::outbound::ImOutboundReceiver,
        expected: &str,
    ) {
        let response = crate::im::core::outbound::try_recv_for_test(receiver)
            .expect("approval refusal must be enqueued without a worker");
        let ImOutboundPayload::Text(text) = response.payload else {
            panic!("approval refusal must not enqueue an approval decision");
        };
        assert!(text.contains(expected), "unexpected refusal: {text}");
        assert!(crate::im::core::outbound::try_recv_for_test(receiver).is_none());
    }

    #[tokio::test]
    async fn stale_approval_card_is_consumed_without_creating_or_selecting_a_binding() {
        for existing_binding in [false, true] {
            let state = approval_fixture_state();
            let mut message = approval_fixture_message("fixture-sender");
            message.action = Some(InboundAction::GmClawApprovalDecision {
                request_key: "fixture-old-request".into(),
                option_index: 1,
            });
            let inactive = approval_fixture_binding(false, None);
            if existing_binding {
                state
                    .gmclaw_im
                    .sessions
                    .lock()
                    .await
                    .insert(SenderKey::for_message(&message), inactive.clone());
            }
            let original_count = usize::from(existing_binding);
            let (outbound, mut receiver) = crate::im::core::outbound::channel();
            for _ in 0..2 {
                assert!(
                    handle_inbound(&state, &outbound, &mut message, &mut None)
                        .await
                        .unwrap()
                );
                assert_approval_fixture_refusal(&mut receiver, "已失效");
                assert_eq!(state.gmclaw_im.sessions.lock().await.len(), original_count);
                assert!(!inactive.selected.load(Ordering::Acquire));
                assert!(message.session_scope.is_none());
                assert!(message.session_entry.is_none());
            }
            assert!(state.runtime.lock().await.route_by_thread.is_empty());
            assert!(state.gmclaw_im.seen.lock().await.is_empty());
        }
    }

    #[tokio::test]
    async fn codex_and_malformed_cards_and_foreign_sender_cannot_consume_gmclaw_pending() {
        for case in [
            "codex-card",
            "malformed-card",
            "stale-tg-card",
            "foreign-sender",
        ] {
            let state = approval_fixture_state();
            let original_message = approval_fixture_message("fixture-sender");
            let config = state.config.lock().await.gmclaw_bridge.clone();
            let mut session = Session::new(&config, "fixture-project".into());
            session.pending = Some(approval_fixture_pending());
            let binding = approval_fixture_binding(true, Some(session));
            state
                .gmclaw_im
                .sessions
                .lock()
                .await
                .insert(SenderKey::for_message(&original_message), binding.clone());
            let mut message = original_message;
            let expected = match case {
                "codex-card" => {
                    message.text = "/1".into();
                    message.approval_request_key = Some("string:fixture-codex-request".into());
                    "已不适用于"
                }
                "malformed-card" => {
                    message.text = "/1".into();
                    message.card_message_id = Some("fixture-old-codex-card".into());
                    "已不适用于"
                }
                "stale-tg-card" => {
                    message.action = Some(InboundAction::GmClawApprovalDecision {
                        request_key: "fixture-old-request".into(),
                        option_index: 1,
                    });
                    "已失效"
                }
                "foreign-sender" => {
                    message.sender_id = "fixture-other-sender".into();
                    message.action = Some(InboundAction::GmClawApprovalDecision {
                        request_key: "fixture-current-request".into(),
                        option_index: 1,
                    });
                    "已失效"
                }
                _ => unreachable!(),
            };
            let (outbound, mut receiver) = crate::im::core::outbound::channel();
            assert!(
                handle_inbound(&state, &outbound, &mut message, &mut None)
                    .await
                    .unwrap()
            );
            assert_approval_fixture_refusal(&mut receiver, expected);
            let sessions = binding.sessions.lock().await;
            let current = sessions.current.as_ref().unwrap();
            assert!(!current.uncertain);
            let pending = current
                .pending
                .as_ref()
                .expect("current approval must remain");
            assert_eq!(pending.card.request_key, "fixture-current-request");
            assert_eq!(pending.code, "fixture-code");
            assert_eq!(
                pending.confirmation.pending_calls[0].call_id,
                "fixture-call"
            );
            assert_eq!(
                pending.confirmation.pending_calls[0].arguments,
                serde_json::json!({"argument":"fixture-value"})
            );
            assert_eq!(state.gmclaw_im.sessions.lock().await.len(), 1);
            assert!(state.runtime.lock().await.route_by_thread.is_empty());
            assert!(state.gmclaw_im.seen.lock().await.is_empty());
            assert!(message.session_entry.is_none());
        }
    }

    #[tokio::test]
    async fn codex_approval_card_without_current_gmclaw_session_is_consumed() {
        let state = approval_fixture_state();
        let mut message = approval_fixture_message("fixture-sender");
        message.text = "/1".into();
        message.approval_request_key = Some("string:fixture-codex-request".into());
        let binding = approval_fixture_binding(true, None);
        state
            .gmclaw_im
            .sessions
            .lock()
            .await
            .insert(SenderKey::for_message(&message), binding.clone());
        let (outbound, mut receiver) = crate::im::core::outbound::channel();
        assert!(
            handle_inbound(&state, &outbound, &mut message, &mut None)
                .await
                .unwrap()
        );
        assert_approval_fixture_refusal(&mut receiver, "已不适用于");
        assert!(binding.sessions.lock().await.current.is_none());
        assert!(message.session_entry.is_none());
        assert_eq!(state.gmclaw_im.sessions.lock().await.len(), 1);
    }

    #[test]
    fn history_restores_original_identity_and_filters_changed_connections() {
        let config = GmClawBridgeConfig::default();
        let mut sessions = SenderSessions::default();
        let mut original = Session::new(&config, "fixture-project".into());
        let original_id = original.id.clone();
        original.title = "First conversation".into();
        sessions.current = Some(original);
        sessions.archive_current();
        assert!(sessions.current.is_none());
        assert_eq!(sessions.history[0].config_fingerprint, config.fingerprint());

        let changed = GmClawBridgeConfig {
            model_id: Some("other-model".into()),
            ..config.clone()
        };
        assert_ne!(
            sessions.history[0].config_fingerprint,
            changed.fingerprint()
        );
        sessions.current = sessions.history.remove(0);
        assert_eq!(sessions.current.as_ref().unwrap().id, original_id);
        assert_eq!(
            sessions.current.as_ref().unwrap().title,
            "First conversation"
        );
    }

    #[test]
    fn pending_and_unknown_sessions_cannot_be_abandoned_by_switching() {
        let mut sessions = SenderSessions::default();
        let mut current = Session::new(&GmClawBridgeConfig::default(), "fixture-project".into());
        current.uncertain = true;
        sessions.current = Some(current);
        assert!(sessions.ensure_can_leave().is_err());
        let current = sessions.current.as_mut().unwrap();
        current.uncertain = false;
        current.pending = Some(PendingApproval {
            code: "fixture-code".into(),
            card: GmClawApproval {
                request_key: "fixture-current-request".into(),
                summary: "fixture tool parameters".into(),
                message_id: None,
                legacy_code: "fixture-code".into(),
            },
            created_at: Instant::now(),
            confirmation: GmClawPendingConfirmation {
                prompt: String::new(),
                pending_calls: vec![],
            },
            runtime_authorization_identity: "fixture-runtime".into(),
        });
        assert!(sessions.ensure_can_leave().is_err());
        sessions.current.as_mut().unwrap().pending = None;
        assert!(sessions.ensure_can_leave().is_ok());
    }

    #[test]
    fn history_is_bounded_and_unknown_entries_remain_explicit() {
        let config = GmClawBridgeConfig::default();
        let mut sessions = SenderSessions::default();
        for _ in 0..MAX_HISTORY + 2 {
            sessions.current = Some(Session::new(&config, "fixture-project".into()));
            sessions.archive_current();
        }
        assert_eq!(sessions.history.len(), MAX_HISTORY);
        sessions.history[0].uncertain = true;
        assert!(sessions.history[0].uncertain);
    }

    #[test]
    fn wecom_message_callback_is_not_a_codex_card_action() {
        let mut message: InboundMessage = serde_json::from_value(serde_json::json!({
            "platform":"wecom", "accountId":"corp", "senderId":"sender",
            "chatId":"single:sender", "chatType":"direct", "messageId":"fixture",
            "text":"/tg", "mentioned":false, "callbackKind":"message"
        }))
        .unwrap();
        assert!(!is_control_callback(&message));
        message.callback_kind = Some(crate::types::InboundCallbackKind::CardEvent);
        assert!(is_control_callback(&message));
    }

    #[test]
    fn codex_card_choice_cannot_be_treated_as_plain_gmclaw_approval() {
        let message: InboundMessage = serde_json::from_value(serde_json::json!({
            "platform":"feishu", "accountId":"fixture", "senderId":"sender",
            "chatId":"chat", "chatType":"direct", "messageId":"old-card",
            "text":"/1", "mentioned":true,
            "approvalRequestKey":"string:fixture-codex-request"
        }))
        .unwrap();
        assert!(is_control_callback(&message));
        assert!(gmclaw_command(&message.text).is_none());
    }

    #[test]
    fn stale_or_invalid_buttons_never_default_to_approve() {
        let pending = PendingApproval {
            code: "fixture-code".into(),
            card: GmClawApproval {
                request_key: "fixture-current-request".into(),
                summary: String::new(),
                message_id: None,
                legacy_code: "fixture-code".into(),
            },
            created_at: Instant::now(),
            confirmation: GmClawPendingConfirmation {
                prompt: String::new(),
                pending_calls: vec![],
            },
            runtime_authorization_identity: "fixture-runtime".into(),
        };
        for index in [0, 3, usize::MAX] {
            assert!(
                current_approval_button_index(&pending, "fixture-current-request", index).is_none()
            );
        }
        for key in ["", "fixture-old-request", "fixture-other-sender-request"] {
            assert!(current_approval_button_index(&pending, key, 1).is_none());
        }
        assert_eq!(
            current_approval_button_index(&pending, "fixture-current-request", 2),
            Some(2)
        );
        for text in ["/0", "/3", "/1 anything", "approve", "anything"] {
            assert!(approval_reply_index(text).is_none());
        }
        assert_eq!(approval_reply_index("/n"), Some(2));
    }

    #[test]
    fn approval_keeps_arguments_and_rejects_duplicate_ids() {
        let call = crate::gmclaw_executor::GmClawPendingCall {
            call_id: "call-1".into(),
            tool_name: "shell".into(),
            arguments: serde_json::json!({"command":"example"}),
        };
        let mut pending = GmClawPendingConfirmation {
            prompt: String::new(),
            pending_calls: vec![call.clone()],
        };
        assert!(approval_summary(&pending).unwrap().contains("example"));
        pending.pending_calls.push(call);
        assert!(approval_summary(&pending).is_err());
    }

    #[test]
    fn sender_identity_is_part_of_session_key() {
        let a = SenderKey {
            platform: ImPlatformKind::Feishu,
            account: "a".into(),
            chat: "c".into(),
            sender: "one".into(),
        };
        let mut b = a.clone();
        b.sender = "two".into();
        assert!(a != b);
    }

    #[test]
    fn bridge_defaults_disabled_and_debug_hides_token() {
        let config = GmClawBridgeConfig {
            auth_token: "test-secret".into(),
            ..Default::default()
        };
        assert!(!config.enabled);
        assert!(!format!("{config:?}").contains("test-secret"));
    }

    #[test]
    fn wecom_permissions_use_raw_chat_id_and_current_account() {
        let mut config = crate::config::AppConfig::default();
        config.wecom_accounts.push(crate::config::WecomConfig {
            account_id: "corp".into(),
            enabled: true,
            bot_id: "fixture-bot".into(),
            secret: "fixture-secret".into(),
            allowed_user_ids: vec!["sender".into()],
            allowed_chat_ids: vec!["sender".into()],
            ..Default::default()
        });
        let mut message: InboundMessage = serde_json::from_value(serde_json::json!({
            "platform":"wecom", "accountId":"corp", "senderId":"sender",
            "chatId":"single:sender", "chatType":"direct", "messageId":"fixture",
            "text":"/gmclaw", "mentioned":false
        }))
        .unwrap();
        assert!(sender_allowed(&config, &message));
        message.sender_id = "another".into();
        assert!(!sender_allowed(&config, &message));
        message.sender_id = "sender".into();
        config.wecom_accounts[0].enabled = false;
        assert!(!sender_allowed(&config, &message));
    }

    #[test]
    fn oversized_approval_is_rejected_without_truncating_arguments() {
        let pending = GmClawPendingConfirmation {
            prompt: String::new(),
            pending_calls: vec![crate::gmclaw_executor::GmClawPendingCall {
                call_id: "large".into(),
                tool_name: "tool".into(),
                arguments: serde_json::json!({"text":"x".repeat(24 * 1024)}),
            }],
        };
        assert!(approval_summary(&pending).is_err());
        let long_answer = "中".repeat(16_001);
        let clipped = bounded_reply(&long_answer);
        assert!(clipped.starts_with(&"中".repeat(16_000)));
        assert!(clipped.contains("已截取"));
    }
}
