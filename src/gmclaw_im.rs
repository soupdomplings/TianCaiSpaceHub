//! GMClaw execution over the existing, authenticated IM transports.
//! Bindings are deliberately separate from Codex threads and scoped to sender.
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
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
        outbound::{ImOutboundKind, ImOutboundMessage, ImOutboundPayload, ImOutboundSender},
        routing::{active_turn_for_message, route_for_message},
    },
    types::{ImPlatformKind, InboundMessage, now_ms},
};

const MAX_SESSIONS: usize = 128;
const APPROVAL_TTL: Duration = Duration::from_secs(15 * 60);
const EXECUTION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GmClawBridgeConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub auth_token: String,
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
            Path::new(&self.project_path).is_absolute() && Path::new(&self.project_path).is_dir(),
            "天工工作目录必须是本机已存在的绝对目录"
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
            serde_json::to_vec(self).expect("GMClaw config serialization"),
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
    created_at: Instant,
    confirmation: GmClawPendingConfirmation,
}

struct Session {
    enabled: bool,
    id: String,
    config_fingerprint: String,
    pending: Option<PendingApproval>,
    // Set before handing execution to Harness. Cancellation or a stream failure
    // leaves this set so another message cannot unknowingly replay a tool turn.
    uncertain: bool,
}

impl Session {
    fn new(config: &GmClawBridgeConfig) -> Self {
        Self {
            enabled: true,
            id: uuid::Uuid::new_v4().to_string(),
            config_fingerprint: config.fingerprint(),
            pending: None,
            uncertain: false,
        }
    }
}

pub struct GmClawImState {
    sessions: Mutex<HashMap<SenderKey, Arc<Mutex<Session>>>>,
    seen: Mutex<VecDeque<(SenderKey, String)>>,
    execution: Semaphore,
    waiting: Semaphore,
}

impl Default for GmClawImState {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            seen: Mutex::new(VecDeque::new()),
            execution: Semaphore::new(1),
            waiting: Semaphore::new(33),
        }
    }
}

fn reply(
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

fn sender_allowed(config: &crate::config::AppConfig, message: &InboundMessage) -> bool {
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

const HELP: &str = "天工 Claw 指令：\n/gmclaw — 切到天工，随后直接发文字\n/gmclaw new — 新建天工会话\n/gmclaw status — 查看当前状态\n/gmclaw off — 返回 Codex\n/gmclaw approve 确认码 — 批准本轮列出的工具\n/gmclaw reject 确认码 — 拒绝本轮工具\n首版支持文本和工具审批；附件请在天工桌面使用。";

/// Returns true only when the message belongs to the GMClaw flow. All sends go
/// through the same per-account outbound worker used by Codex.
pub(crate) async fn handle_inbound(
    state: &SharedState,
    outbound: &ImOutboundSender,
    message: &InboundMessage,
) -> Result<bool> {
    if message.platform == ImPlatformKind::Telegram
        || message.action.is_some()
        || matches!(
            message.callback_kind,
            Some(
                crate::types::InboundCallbackKind::CardEvent
                    | crate::types::InboundCallbackKind::Welcome
            )
        )
    {
        return Ok(false);
    }
    let text = message.text.trim();
    let command = text
        .strip_prefix("/gmclaw")
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        .map(str::trim);
    let key = SenderKey::for_message(message);
    let app_config = state.config.lock().await.clone();
    let mut sessions = state.gmclaw_im.sessions.lock().await;
    let existing = sessions.get(&key).cloned();
    if existing.is_none() && command.is_none() {
        return Ok(false);
    }
    if !sender_allowed(&app_config, message) {
        return Ok(true);
    }
    let config = app_config.gmclaw_bridge;
    if !message.message_id.is_empty() {
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
    if existing.is_none() && matches!(command, Some("help" | "status" | "off")) {
        drop(sessions);
        reply(
            outbound,
            message,
            if command == Some("off") {
                "当前未使用天工。"
            } else {
                HELP
            },
        )?;
        return Ok(true);
    }
    let session = if let Some(session) = existing {
        session
    } else {
        if !config.enabled {
            drop(sessions);
            reply(
                outbound,
                message,
                "请先在 Hub「天工 Claw 接入」启用外部消息接入，并配置连接 Token 与工作目录。",
            )?;
            return Ok(true);
        }
        if !matches!(command, Some("" | "new")) {
            drop(sessions);
            reply(outbound, message, HELP)?;
            return Ok(true);
        }
        if active_turn_for_message(state, message).await.is_some()
            || state
                .runtime
                .lock()
                .await
                .has_pending_approvals(&message.conversation_key())
        {
            drop(sessions);
            reply(
                outbound,
                message,
                "当前 Codex 任务或审批尚未结束，请处理后再切到天工。",
            )?;
            return Ok(true);
        }
        // Inactive entries are removed only when no in-flight handler owns them.
        sessions.retain(|_, session| {
            Arc::strong_count(session) > 1
                || session.try_lock().map_or(true, |session| session.enabled)
        });
        if sessions.len() >= MAX_SESSIONS {
            drop(sessions);
            reply(
                outbound,
                message,
                "天工会话数量已达上限，请先在不用的会话发送 /gmclaw off。",
            )?;
            return Ok(true);
        }
        let session = Arc::new(Mutex::new(Session::new(&config)));
        sessions.insert(key.clone(), session.clone());
        session
    };
    drop(sessions);
    let Ok(mut session) = session.try_lock() else {
        reply(
            outbound,
            message,
            "当前天工请求正在执行或排队，请等待完成。切换会话不会取消天工中的工具执行。",
        )?;
        return Ok(true);
    };
    if !session.enabled && command.is_none() {
        return Ok(false);
    }
    if command == Some("off") {
        session.enabled = false;
        session.pending = None;
        reply(
            outbound,
            message,
            "已返回 Codex 消息流程。天工桌面中的会话记录仍保留；此操作不取消已交给天工的任务。",
        )?;
        return Ok(true);
    }
    if command == Some("help") {
        reply(outbound, message, HELP)?;
        return Ok(true);
    }
    if !config.enabled {
        reply(
            outbound,
            message,
            "Hub 的天工外部消息接入已停用。发送 /gmclaw off 返回 Codex，或在 Hub 重新启用。",
        )?;
        return Ok(true);
    }
    if let Err(error) = config.validate() {
        reply(outbound, message, format!("天工连接配置不可用：{error}"))?;
        return Ok(true);
    }
    if matches!(command, Some("" | "new")) {
        if !session.enabled
            && (active_turn_for_message(state, message).await.is_some()
                || state
                    .runtime
                    .lock()
                    .await
                    .has_pending_approvals(&message.conversation_key()))
        {
            reply(
                outbound,
                message,
                "当前 Codex 任务或审批尚未结束，请处理后再切到天工。",
            )?;
            return Ok(true);
        }
        if command == Some("new") || !session.enabled {
            *session = Session::new(&config);
        }
        reply(
            outbound,
            message,
            "已切到天工 Claw，直接发送文字开始对话。此处仅建立会话，连接状态会在发送第一条消息时确认。\n新建会话：/gmclaw new\n返回 Codex：/gmclaw off",
        )?;
        return Ok(true);
    }
    if command == Some("status") {
        let status = if !session.enabled {
            "当前未使用天工。"
        } else if session.uncertain {
            "上次执行未确认完成。请先在天工检查执行情况，再用 /gmclaw new 开始新会话；不会自动重试。"
        } else if session.pending.is_some() {
            "当前天工会话等待工具审批。请使用该轮消息中的确认码。"
        } else {
            "当前文字消息交给天工。发送 /gmclaw off 返回 Codex。"
        };
        reply(outbound, message, status)?;
        return Ok(true);
    }
    if !session.enabled {
        reply(outbound, message, "先发送 /gmclaw 切到天工。")?;
        return Ok(true);
    }
    if config.fingerprint() != session.config_fingerprint {
        reply(
            outbound,
            message,
            "天工连接配置已改变，请发送 /gmclaw new 建立新会话。旧审批不会用于新连接。",
        )?;
        return Ok(true);
    }
    if session.uncertain {
        reply(
            outbound,
            message,
            "上次执行状态未知，请先在天工检查执行结果，再发送 /gmclaw new。Hub 不会重放上次任务。",
        )?;
        return Ok(true);
    }
    if !message.attachments.is_empty() {
        reply(
            outbound,
            message,
            "天工外部消息首版只支持文字，未转发本条消息及附件。请在天工桌面处理附件。",
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
    if let Some(command) = command {
        let parts: Vec<_> = command.split_whitespace().collect();
        if parts.len() != 2 || !matches!(parts[0], "approve" | "reject") {
            reply(outbound, message, HELP)?;
            return Ok(true);
        }
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
                "审批已过期，未执行批准。请发送 /gmclaw new 开始新会话。",
            )?;
            return Ok(true);
        }
        if parts[1] != pending.code {
            reply(
                outbound,
                message,
                "确认码不匹配，请使用当前会话最近一轮审批消息中的完整确认码。",
            )?;
            return Ok(true);
        }
        confirmation = Some(GmClawConfirmationDecision {
            action: if parts[0] == "approve" {
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
            "天工正在等待工具审批。请先批准或拒绝当前轮次，或用 /gmclaw new 开始新会话。",
        )?;
        return Ok(true);
    } else if text.starts_with('/') {
        reply(outbound, message, HELP)?;
        return Ok(true);
    } else if text.is_empty() || text.len() > 128 * 1024 {
        reply(outbound, message, "请发送非空文字，单条最多 128 KiB。")?;
        return Ok(true);
    }
    let Ok(_waiting) = state.gmclaw_im.waiting.try_acquire() else {
        reply(
            outbound,
            message,
            "天工等待队列已满，本条未执行，请稍后重新发送。",
        )?;
        return Ok(true);
    };
    reply(outbound, message, "正在等待天工 Claw 处理…")?;
    let Ok(Ok(_permit)) =
        tokio::time::timeout(Duration::from_secs(30), state.gmclaw_im.execution.acquire()).await
    else {
        reply(
            outbound,
            message,
            "天工仍在处理其他请求，本条未交给天工，请稍后重新发送。",
        )?;
        return Ok(true);
    };
    // Recheck local configuration after waiting, before consuming an approval.
    let latest = state.config.lock().await.clone();
    if !sender_allowed(&latest, message) {
        return Ok(true);
    }
    if latest.gmclaw_bridge.fingerprint() != session.config_fingerprint {
        reply(
            outbound,
            message,
            "等待期间连接配置已改变，本条未执行。请发送 /gmclaw new。",
        )?;
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
        reply(
            outbound,
            message,
            "等待期间审批已过期，本条未执行。请发送 /gmclaw new。",
        )?;
        return Ok(true);
    }
    let client = GmClawClient::new(&config.endpoint, &config.auth_token)?;
    let memory_scope = format!(
        "hub-im-{}",
        &hex::encode(Sha256::digest(serde_json::to_vec(&(
            message.platform.key(),
            &message.account_id,
            &message.chat_id,
            &message.sender_id
        ))?))[..24]
    );
    let request = GmClawChatRequest {
        user_id: memory_scope.clone(),
        session_id: session.id.clone(),
        chat_id: uuid::Uuid::new_v4().to_string(),
        query: if confirmation.is_some() {
            String::new()
        } else {
            text.to_string()
        },
        model_id: config.model_id.clone(),
        max_steps: config.max_steps,
        project: GmClawProject {
            id: memory_scope,
            name: "Hub IM".into(),
            path: config.project_path.clone(),
        },
        confirmation,
    };
    session.pending = None;
    session.uncertain = true;
    let result = tokio::time::timeout(
        EXECUTION_TIMEOUT,
        client.execute(&request, |_| async { Ok(()) }),
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
            reply(
                outbound,
                message,
                format!(
                    "天工请求未确认完成：{error}\n不会自动重试，请检查天工后用 /gmclaw new 重新开始。"
                ),
            )?;
            return Ok(true);
        }
        Err(_) => {
            reply(
                outbound,
                message,
                "天工请求等待超过 15 分钟，执行状态未知。请到天工检查结果；不会自动重试或声称已取消。",
            )?;
            return Ok(true);
        }
    };
    if let Some(error) = result.error {
        reply(
            outbound,
            message,
            format!("天工返回错误：{error}\n请检查天工后用 /gmclaw new 重新开始。"),
        )?;
        return Ok(true);
    }
    if let Some(pending) = result.pending_confirmation {
        let summary = approval_summary(&pending);
        let summary = match summary {
            Ok(summary) => summary,
            Err(error) => {
                reply(
                    outbound,
                    message,
                    format!("无法在消息中完整展示本次工具审批：{error}。未批准，请到天工检查。"),
                )?;
                return Ok(true);
            }
        };
        let code = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        reply(
            outbound,
            message,
            format!(
                "天工请求执行以下工具（仅本条消息发起者可审批，15 分钟内有效）：\n{summary}\n批准全部：/gmclaw approve {code}\n拒绝全部：/gmclaw reject {code}"
            ),
        )?;
        session.pending = Some(PendingApproval {
            code,
            created_at: Instant::now(),
            confirmation: pending,
        });
    } else {
        reply(
            outbound,
            message,
            if result.answer.trim().is_empty() {
                "天工本轮已结束，没有返回文字。".into()
            } else {
                bounded_reply(&result.answer)
            },
        )?;
    }
    session.uncertain = false;
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
