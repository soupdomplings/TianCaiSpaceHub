//! Temporary TianGong reply presentation, independent of Codex runtime state.
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::{
    app_state::SharedState,
    im::{
        core::{
            accounts::ImApiRegistry,
            executor_approval::text_chunks,
            outbound::{
                self, ImOutboundKind, ImOutboundMessage, ImOutboundPayload, ImOutboundSender,
            },
            routing::route_for_message,
        },
        feishu::{FeishuAdapter, renderer},
    },
    types::{ImPlatformKind, InboundCallbackKind, InboundMessage},
};

const MAX_TURNS: usize = 128;
const MAX_COMPLETED: usize = 256;
const TURN_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_FEISHU_CARD_BYTES: usize = 24 * 1024;
const MAX_WECOM_STREAM_BYTES: usize = 20 * 1024;
const DISPLAY_SEND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CLOSE_RETRIES: u8 = 3;
const MAX_CLEANUP_UPDATES: usize = 2;
const UNKNOWN: &str = "天工本轮状态未确认，请到天工检查结果；Hub 不会自动重放或声称已停止。";
const CHANGED: &str = "本轮接入权限或连接配置已变化，结果未在聊天中展示。请到天工核对。";
const REPLIED: &str = "天工本轮已回复，完整内容见随后文字消息。";
const UNDELIVERED: &str = "天工回复投递未确认，请在天工查看本轮结果；不会重放任务。";

#[derive(Debug, Clone)]
pub(crate) struct GmClawTurn {
    key: String,
    session_id: String,
    fingerprint: String,
    inbound: InboundMessage,
}

impl GmClawTurn {
    pub(crate) fn delivery_scope(&self) -> (&InboundMessage, &str) {
        (&self.inbound, &self.fingerprint)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GmClawTurnPhase {
    Preparing,
    Running,
}

impl GmClawTurnPhase {
    fn text(self) -> &'static str {
        match self {
            Self::Preparing => "天工 Claw 正在准备回复…",
            Self::Running => "天工 Claw 正在处理…",
        }
    }
}

/// Every local return, error and cancelled future closes its own presentation.
/// Drop reports uncertainty only; it never retries the Harness request.
pub(crate) struct TurnDisplayGuard {
    outbound: ImOutboundSender,
    turn: GmClawTurn,
    finished: bool,
}

impl TurnDisplayGuard {
    pub(crate) fn begin(
        outbound: &ImOutboundSender,
        message: &InboundMessage,
        session_id: &str,
        fingerprint: &str,
    ) -> Result<Self> {
        let mut inbound = message.clone();
        inbound.text.clear();
        inbound.attachments.clear();
        inbound.action = None;
        inbound.approval_request_key = None;
        inbound.card_message_id = None;
        let guard = Self {
            outbound: outbound.clone(),
            turn: GmClawTurn {
                key: uuid::Uuid::new_v4().simple().to_string(),
                session_id: session_id.to_owned(),
                fingerprint: fingerprint.to_owned(),
                inbound,
            },
            finished: false,
        };
        guard.send(ImOutboundPayload::GmClawTurnStage {
            turn: guard.turn.clone(),
            phase: GmClawTurnPhase::Preparing,
        })?;
        Ok(guard)
    }

    pub(crate) fn running(&self) {
        let _ = self.send(ImOutboundPayload::GmClawTurnStage {
            turn: self.turn.clone(),
            phase: GmClawTurnPhase::Running,
        });
    }

    pub(crate) fn finish(&mut self, text: impl Into<String>) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.send(ImOutboundPayload::GmClawTurnFinished {
            turn: self.turn.clone(),
            text: text.into(),
        })
    }

    fn send(&self, payload: ImOutboundPayload) -> Result<()> {
        self.outbound.enqueue(ImOutboundMessage {
            thread_id: "gmclaw-im".into(),
            route: route_for_message(&self.turn.inbound),
            item_id: None,
            item_type: Some("gmclaw-turn".into()),
            kind: ImOutboundKind::TurnReply,
            payload,
        })
    }
}

impl Drop for TurnDisplayGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(UNKNOWN);
        }
    }
}

enum Receipt {
    None,
    Feishu(String),
    Wecom {
        request_id: String,
        stream_id: String,
    },
}

impl Receipt {
    fn known(&self) -> bool {
        !matches!(self, Self::None)
    }
}

struct Closing {
    text: &'static str,
    attempts: u8,
}

struct TurnEntry {
    message: ImOutboundMessage,
    turn: GmClawTurn,
    created: Instant,
    receipt: Receipt,
    closing: Option<Closing>,
}

#[derive(Default)]
pub(crate) struct TurnPresenter {
    active: HashMap<String, TurnEntry>,
    completed: VecDeque<String>,
}

fn same_turn(left: &GmClawTurn, right: &GmClawTurn) -> bool {
    left.key == right.key
        && left.session_id == right.session_id
        && left.fingerprint == right.fingerprint
        && left.inbound.platform == right.inbound.platform
        && left.inbound.account_id == right.inbound.account_id
        && left.inbound.chat_id == right.inbound.chat_id
        && left.inbound.sender_id == right.inbound.sender_id
        && left.inbound.message_id == right.inbound.message_id
        && left.inbound.callback_req_id == right.inbound.callback_req_id
}

async fn allowed(state: &SharedState, turn: &GmClawTurn) -> bool {
    let config = state.config.lock().await;
    crate::gmclaw_im::turn_delivery_allowed(&config, &turn.inbound, &turn.fingerprint)
}

async fn display_send<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::time::timeout(DISPLAY_SEND_TIMEOUT, future)
        .await
        .map_err(|_| anyhow::anyhow!("天工回复展示投递超时"))?
}

impl TurnPresenter {
    pub(crate) async fn handle(
        &mut self,
        state: &SharedState,
        apis: &ImApiRegistry,
        message: &ImOutboundMessage,
    ) -> bool {
        let (turn, phase, final_text) = match &message.payload {
            ImOutboundPayload::GmClawTurnStage { turn, phase } => (turn, Some(*phase), None),
            ImOutboundPayload::GmClawTurnFinished { turn, text } => {
                (turn, None, Some(text.as_str()))
            }
            _ => return false,
        };
        if self.completed.contains(&turn.key) {
            return true;
        }
        if let Some(entry) = self.active.get(&turn.key)
            && (!same_turn(&entry.turn, turn)
                || entry.message.route.platform != message.route.platform
                || entry.message.route.account_id != message.route.account_id
                || entry.message.route.chat_id != message.route.chat_id
                || entry.message.route.conversation_key != message.route.conversation_key
                || entry.message.route.remote_client_key != message.route.remote_client_key)
        {
            return true;
        }
        if self
            .active
            .get(&turn.key)
            .is_some_and(|entry| entry.closing.is_some())
        {
            return true;
        }
        if let Some(text) = final_text {
            let entry = self.active.remove(&turn.key);
            // WeChat queues this same payload on context-token expiry. Replays
            // must recheck its sender and config, rather than lose that scope.
            if message.route.platform != ImPlatformKind::Wechat {
                self.remember_completed(turn.key.clone());
            }
            if !outbound::outbound_channel_enabled(state, &message.route).await {
                self.retain_closing(turn.key.clone(), entry, CHANGED);
                return true;
            }
            let permitted = allowed(state, turn).await;
            let text = if permitted { text } else { CHANGED };
            if permitted || entry.is_some() {
                let closing = finish_entry(
                    state,
                    apis,
                    message,
                    entry.as_ref().map(|entry| &entry.receipt),
                    text,
                )
                .await;
                if let Some(text) = closing {
                    self.retain_closing(turn.key.clone(), entry, text);
                }
            }
            return true;
        }
        if !allowed(state, turn).await {
            return true;
        }
        let phase = phase.unwrap();
        if phase == GmClawTurnPhase::Preparing {
            if self.active.contains_key(&turn.key) || self.active.len() >= MAX_TURNS {
                return true;
            }
            let mut entry = TurnEntry {
                message: message.clone(),
                turn: turn.clone(),
                created: Instant::now(),
                receipt: Receipt::None,
                closing: None,
            };
            match turn.inbound.platform {
                ImPlatformKind::Feishu => {
                    if let Some(api) = apis.feishu_for_route(&message.route) {
                        let adapter = FeishuAdapter::new(api).for_sensitive_messages();
                        let card = renderer::build_streaming_reply_card(phase.text(), false);
                        if let Ok(id) =
                            display_send(adapter.send_interactive(&message.route.chat_id, &card))
                                .await
                        {
                            entry.receipt = Receipt::Feishu(id);
                        }
                    }
                }
                ImPlatformKind::Wecom => {
                    if turn.inbound.callback_kind == Some(InboundCallbackKind::Message)
                        && let Some(request_id) = turn
                            .inbound
                            .callback_req_id
                            .as_ref()
                            .filter(|id| !id.is_empty())
                        && let Some(api) = apis.wecom_for_route(&message.route)
                    {
                        let stream_id = format!("tg_{}", turn.key);
                        // Stream identity is locally chosen, so close it even if acknowledgement was lost.
                        entry.receipt = Receipt::Wecom {
                            request_id: request_id.clone(),
                            stream_id: stream_id.clone(),
                        };
                        let _ = display_send(api.reply_stream(
                            request_id,
                            &stream_id,
                            phase.text(),
                            false,
                        ))
                        .await;
                    }
                }
                ImPlatformKind::Wechat | ImPlatformKind::Telegram => {}
            }
            self.active.insert(turn.key.clone(), entry);
        } else if let Some(entry) = self.active.get(&turn.key) {
            update_receipt(apis, &entry.message, &entry.receipt, phase.text(), false).await;
        }
        true
    }

    fn remember_completed(&mut self, key: String) {
        self.completed.push_back(key);
        while self.completed.len() > MAX_COMPLETED {
            self.completed.pop_front();
        }
    }

    fn retain_closing(&mut self, key: String, entry: Option<TurnEntry>, text: &'static str) {
        if let Some(mut entry) = entry.filter(|entry| entry.receipt.known()) {
            // Never retain a reply body for cleanup. Only these fixed statuses
            // can be replayed to an already-owned card or stream identity.
            entry.closing = Some(Closing { text, attempts: 0 });
            self.active.insert(key, entry);
        }
    }

    pub(crate) async fn expire(&mut self, state: &SharedState, apis: &ImApiRegistry) {
        // Expired closings are discarded without any platform request. A
        // platform outage must not turn one cleanup tick into 128 timeouts.
        self.active
            .retain(|_, entry| !(entry.closing.is_some() && entry.created.elapsed() >= TURN_TTL));
        let mut keys: Vec<_> = self.active.keys().cloned().collect();
        keys.sort_by_key(|key| self.active.get(key).map(|entry| entry.created));
        let mut updates = 0;
        for key in keys {
            let Some(mut entry) = self.active.remove(&key) else {
                continue;
            };
            let expired = entry.created.elapsed() >= TURN_TTL;
            if expired {
                if entry.closing.is_none()
                    && entry.receipt.known()
                    && updates < MAX_CLEANUP_UPDATES
                    && outbound::outbound_channel_enabled(state, &entry.message.route).await
                {
                    updates += 1;
                    let text = if allowed(state, &entry.turn).await {
                        UNKNOWN
                    } else {
                        CHANGED
                    };
                    update_receipt(apis, &entry.message, &entry.receipt, text, true).await;
                }
                continue;
            }
            if entry.closing.is_none() {
                self.active.insert(key, entry);
                continue;
            }
            if !outbound::outbound_channel_enabled(state, &entry.message.route).await {
                self.active.insert(key, entry);
                continue;
            }
            if updates >= MAX_CLEANUP_UPDATES {
                self.active.insert(key, entry);
                continue;
            }
            updates += 1;
            let permitted = allowed(state, &entry.turn).await;
            let closing = entry.closing.as_mut().unwrap();
            if !permitted {
                closing.text = CHANGED;
            }
            closing.attempts += 1;
            let text = closing.text;
            let attempts = closing.attempts;
            let confirmed = update_receipt(apis, &entry.message, &entry.receipt, text, true).await;
            if !confirmed && attempts < MAX_CLOSE_RETRIES {
                self.active.insert(key, entry);
            } else if !confirmed {
                state.push_event("warn", "gmclaw_turn_cleanup_unconfirmed", "临时回复状态关闭未获确认，有限展示重试已结束；请在天工查看结果，任务未重放。").await;
            }
        }
    }

    pub(crate) async fn close_all(&mut self, state: &SharedState, apis: &ImApiRegistry) {
        for (_, entry) in self.active.drain() {
            if outbound::outbound_channel_enabled(state, &entry.message.route).await {
                let text = if !allowed(state, &entry.turn).await {
                    CHANGED
                } else {
                    entry
                        .closing
                        .as_ref()
                        .map_or(UNKNOWN, |closing| closing.text)
                };
                update_receipt(apis, &entry.message, &entry.receipt, text, true).await;
            }
        }
    }
}

async fn update_receipt(
    apis: &ImApiRegistry,
    message: &ImOutboundMessage,
    receipt: &Receipt,
    text: &str,
    completed: bool,
) -> bool {
    match receipt {
        Receipt::Feishu(id) => {
            let Some(api) = apis.feishu_for_route(&message.route) else {
                return false;
            };
            let card = renderer::build_streaming_reply_card(text, completed);
            display_send(
                FeishuAdapter::new(api)
                    .for_sensitive_messages()
                    .update_interactive(id, &card),
            )
            .await
            .is_ok()
        }
        Receipt::Wecom {
            request_id,
            stream_id,
        } => {
            let Some(api) = apis.wecom_for_route(&message.route) else {
                return false;
            };
            display_send(api.reply_stream(request_id, stream_id, text, completed))
                .await
                .is_ok()
        }
        Receipt::None => false,
    }
}

async fn finish_entry(
    state: &SharedState,
    apis: &ImApiRegistry,
    message: &ImOutboundMessage,
    receipt: Option<&Receipt>,
    text: &str,
) -> Option<&'static str> {
    let receipt = receipt.unwrap_or(&Receipt::None);
    match message.route.platform {
        ImPlatformKind::Feishu => {
            let Some(api) = apis.feishu_for_route(&message.route) else {
                return receipt.known().then_some(UNDELIVERED);
            };
            let adapter = FeishuAdapter::new(api).for_sensitive_messages();
            let card = renderer::build_streaming_reply_card(text, true);
            let fits =
                serde_json::to_vec(&card).is_ok_and(|json| json.len() <= MAX_FEISHU_CARD_BYTES);
            let delivered = if fits {
                if let Receipt::Feishu(id) = receipt {
                    display_send(adapter.update_interactive(id, &card))
                        .await
                        .is_ok()
                } else {
                    display_send(adapter.send_interactive(&message.route.chat_id, &card))
                        .await
                        .is_ok()
                }
            } else {
                false
            };
            if delivered {
                return None;
            }
            // Deliver the full reply first; never remove the waiting card ahead of an unconfirmed reply.
            let mut delivered = true;
            for chunk in text_chunks(text, 3500) {
                if !allowed_for_message(state, message).await
                    || display_send(adapter.send_text(&message.route.chat_id, chunk))
                        .await
                        .is_err()
                {
                    delivered = false;
                    break;
                }
            }
            let terminal = if !allowed_for_message(state, message).await {
                CHANGED
            } else if delivered {
                REPLIED
            } else {
                UNDELIVERED
            };
            let closed = update_receipt(apis, message, receipt, terminal, true).await;
            return (receipt.known() && !closed).then_some(terminal);
        }
        ImPlatformKind::Wecom => {
            if text.len() <= MAX_WECOM_STREAM_BYTES
                && update_receipt(apis, message, receipt, text, true).await
            {
                return None;
            }
            let delivered = if let Some(api) = apis.wecom_for_route(&message.route) {
                let mut delivered = true;
                for chunk in text_chunks(text, 3500) {
                    if !allowed_for_message(state, message).await
                        || display_send(api.send_markdown(&message.route.chat_id, chunk))
                            .await
                            .is_err()
                    {
                        delivered = false;
                        break;
                    }
                }
                delivered
            } else {
                false
            };
            let terminal = if !allowed_for_message(state, message).await {
                CHANGED
            } else if delivered {
                REPLIED
            } else {
                UNDELIVERED
            };
            let closed = update_receipt(apis, message, receipt, terminal, true).await;
            return (receipt.known() && !closed).then_some(terminal);
        }
        ImPlatformKind::Wechat => {
            if !allowed_for_message(state, message).await {
                return None;
            }
            if let Some(api) = apis.wechat_for_route(&message.route) {
                outbound::send_wechat_outbound(state, &api, message.clone()).await;
            }
        }
        ImPlatformKind::Telegram => {}
    }
    None
}

async fn allowed_for_message(state: &SharedState, message: &ImOutboundMessage) -> bool {
    match &message.payload {
        ImOutboundPayload::GmClawTurnStage { turn, .. }
        | ImOutboundPayload::GmClawTurnFinished { turn, .. } => allowed(state, turn).await,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sender: &str) -> InboundMessage {
        serde_json::from_value(serde_json::json!({
            "platform":"feishu", "accountId":"fixture-account", "senderId":sender,
            "chatId":"fixture-group", "chatType":"group", "messageId":"fixture-inbound",
            "text":"private user request", "mentioned":true
        }))
        .unwrap()
    }

    #[test]
    fn dropped_turn_closes_after_start_in_fifo_order_and_keeps_sender_identity() {
        let (tx, mut rx) = outbound::channel();
        {
            let guard =
                TurnDisplayGuard::begin(&tx, &message("sender-one"), "session-one", "config-one")
                    .unwrap();
            guard.running();
        }
        let first = outbound::try_recv_for_test(&mut rx).unwrap();
        let ImOutboundPayload::GmClawTurnStage {
            turn: first,
            phase: GmClawTurnPhase::Preparing,
        } = first.payload
        else {
            panic!("start first");
        };
        assert!(first.inbound.text.is_empty());
        let second = outbound::try_recv_for_test(&mut rx).unwrap();
        assert!(matches!(
            second.payload,
            ImOutboundPayload::GmClawTurnStage {
                phase: GmClawTurnPhase::Running,
                ..
            }
        ));
        let last = outbound::try_recv_for_test(&mut rx).unwrap();
        let ImOutboundPayload::GmClawTurnFinished { turn: last, text } = last.payload else {
            panic!("drop must close");
        };
        assert!(same_turn(&first, &last));
        assert_eq!(text, UNKNOWN);
        assert!(outbound::try_recv_for_test(&mut rx).is_none());
        let mut other = last.clone();
        other.inbound.sender_id = "sender-two".into();
        assert!(!same_turn(&first, &other));
        other = last.clone();
        other.session_id = "session-two".into();
        assert!(!same_turn(&first, &other));
        other = last;
        other.fingerprint = "config-two".into();
        assert!(!same_turn(&first, &other));
    }

    #[test]
    fn completed_turn_does_not_enqueue_drop_reply_or_reuse_identity() {
        let (tx, mut rx) = outbound::channel();
        let mut guard =
            TurnDisplayGuard::begin(&tx, &message("sender-one"), "session-one", "config-one")
                .unwrap();
        let first_key = guard.turn.key.clone();
        guard.finish("真实答复").unwrap();
        guard.finish("重复答复").unwrap();
        drop(guard);
        assert!(outbound::try_recv_for_test(&mut rx).is_some());
        let final_message = outbound::try_recv_for_test(&mut rx).unwrap();
        assert!(
            matches!(final_message.payload, ImOutboundPayload::GmClawTurnFinished { text, .. } if text == "真实答复")
        );
        assert!(outbound::try_recv_for_test(&mut rx).is_none());
        let other =
            TurnDisplayGuard::begin(&tx, &message("sender-one"), "session-one", "config-one")
                .unwrap();
        assert_ne!(first_key, other.turn.key);
    }

    #[test]
    fn failed_close_keeps_owned_receipt_after_dedup_but_never_keeps_reply_body() {
        let (tx, mut rx) = outbound::channel();
        let guard =
            TurnDisplayGuard::begin(&tx, &message("sender-one"), "session-one", "config-one")
                .unwrap();
        let queued = outbound::try_recv_for_test(&mut rx).unwrap();
        let mut presenter = TurnPresenter::default();
        let key = guard.turn.key.clone();
        presenter.remember_completed(key.clone());
        presenter.retain_closing(
            key.clone(),
            Some(TurnEntry {
                message: queued.clone(),
                turn: guard.turn.clone(),
                created: Instant::now(),
                receipt: Receipt::Feishu("owned-card".into()),
                closing: None,
            }),
            UNDELIVERED,
        );
        assert!(presenter.completed.contains(&key));
        let closing = presenter.active.get(&key).unwrap();
        assert!(matches!(&closing.receipt, Receipt::Feishu(id) if id == "owned-card"));
        assert_eq!(closing.closing.as_ref().unwrap().text, UNDELIVERED);
        assert_eq!(closing.closing.as_ref().unwrap().attempts, 0);
        assert!(closing.turn.inbound.text.is_empty());
        assert!(matches!(
            closing.message.payload,
            ImOutboundPayload::GmClawTurnStage { .. }
        ));
        let unowned_key = "unowned-turn".to_owned();
        presenter.retain_closing(
            unowned_key.clone(),
            Some(TurnEntry {
                message: queued,
                turn: guard.turn.clone(),
                created: Instant::now(),
                receipt: Receipt::None,
                closing: None,
            }),
            REPLIED,
        );
        assert!(!presenter.active.contains_key(&unowned_key));
    }
    #[test]
    fn completed_reply_card_has_no_generating_marker_and_long_reply_is_lossless() {
        let preparing =
            renderer::build_streaming_reply_card(GmClawTurnPhase::Preparing.text(), false)
                .to_string();
        assert!(preparing.contains("生成中"));
        let done = renderer::build_streaming_reply_card("真实答复", true).to_string();
        assert!(done.contains("真实答复"));
        assert!(!done.contains("生成中"));
        let long = format!("  {} \n", "天工🔧\\\"".repeat(6000));
        assert!(
            serde_json::to_vec(&renderer::build_streaming_reply_card(&long, true))
                .unwrap()
                .len()
                > MAX_FEISHU_CARD_BYTES
        );
        assert_eq!(text_chunks(&long, 3500).concat(), long);
    }
}
