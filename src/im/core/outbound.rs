use std::path::PathBuf;

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;

use crate::{
    app_state::SharedState,
    chain_log,
    im::{
        core::accounts::ImApiRegistry,
        core::executor_approval::{self, GmClawApproval},
        core::executor_turn::{GmClawTurn, GmClawTurnPhase, TurnPresenter},
        core::i18n::im_text_for_state,
        feishu::{FeishuAdapter, FeishuApi},
        telegram::{adapter::TelegramAdapter, api::TelegramApi},
        wechat::{
            adapter::{WECHAT_TEXT_CHUNK_CHARS, WechatAdapter},
            api::WechatApi,
            store as wechat_store,
        },
        wecom::{adapter::WecomAdapter, api::WecomApi},
    },
    im_runtime::{PendingApproval, RouteTarget},
    types::{ImPlatformKind, InboundMessage},
};

#[derive(Clone)]
pub(crate) struct ImOutboundSender {
    sender: mpsc::UnboundedSender<ImOutboundMessage>,
}

pub(crate) struct ImOutboundReceiver {
    receiver: mpsc::UnboundedReceiver<ImOutboundMessage>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImOutboundMessage {
    pub thread_id: String,
    pub route: RouteTarget,
    pub item_id: Option<String>,
    pub item_type: Option<String>,
    pub kind: ImOutboundKind,
    pub payload: ImOutboundPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImOutboundKind {
    TurnReply,
    Item,
    ImageItem,
    Approval,
}

#[derive(Debug, Clone)]
pub(crate) enum ImOutboundPayload {
    Text(String),
    Approval(PendingApproval),
    GmClawApproval(GmClawApproval),
    GmClawApprovalResolved {
        approval: GmClawApproval,
        option_index: usize,
        callback: Option<InboundMessage>,
    },
    GmClawTurnStage {
        turn: GmClawTurn,
        phase: GmClawTurnPhase,
    },
    GmClawTurnFinished {
        turn: GmClawTurn,
        text: String,
    },
    Image {
        path: PathBuf,
        caption: Option<String>,
        fallback_text: Option<String>,
    },
}

pub(crate) fn channel() -> (ImOutboundSender, ImOutboundReceiver) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (ImOutboundSender { sender }, ImOutboundReceiver { receiver })
}

#[cfg(test)]
pub(crate) fn try_recv_for_test(receiver: &mut ImOutboundReceiver) -> Option<ImOutboundMessage> {
    receiver.receiver.try_recv().ok()
}

impl ImOutboundSender {
    pub(crate) fn enqueue(&self, message: ImOutboundMessage) -> Result<()> {
        self.sender
            .send(message)
            .map_err(|_| anyhow!("IM outbound queue is closed"))
    }
}

pub(crate) async fn run_worker(
    state: SharedState,
    api_registry: ImApiRegistry,
    mut receiver: ImOutboundReceiver,
) {
    let mut turns = TurnPresenter::default();
    let mut cleanup = tokio::time::interval(std::time::Duration::from_secs(15));
    cleanup.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let message = tokio::select! {
            message = receiver.receiver.recv() => match message {
                Some(message) => message,
                None => break,
            },
            _ = cleanup.tick() => {
                turns.expire(&state, &api_registry).await;
                continue;
            }
        };
        log_outbound_message("worker_dequeue", &message, None);
        if turns.handle(&state, &api_registry, &message).await {
            continue;
        }
        if !outbound_channel_enabled(&state, &message.route).await {
            state
                .push_event(
                    "warn",
                    "im_outbound_account_disabled",
                    format!(
                        "platform={} account={} thread={} chat={}",
                        message.route.platform.key(),
                        message.route.account_id,
                        message.thread_id,
                        message.route.chat_id
                    ),
                )
                .await;
            continue;
        }
        match message.route.platform {
            ImPlatformKind::Telegram => {
                let Some(api) = api_registry.telegram_for_route(&message.route) else {
                    log_missing_api(&state, &message).await;
                    continue;
                };
                send_telegram_outbound(&state, &api, message).await;
            }
            ImPlatformKind::Wechat => {
                let Some(api) = api_registry.wechat_for_route(&message.route) else {
                    log_missing_api(&state, &message).await;
                    continue;
                };
                if defer_wechat_outbound_if_waiting(&state, &message).await {
                    continue;
                }
                send_wechat_outbound(&state, &api, message).await;
            }
            ImPlatformKind::Feishu => {
                let Some(api) = api_registry.feishu_for_route(&message.route) else {
                    log_missing_api(&state, &message).await;
                    continue;
                };
                send_feishu_outbound(&state, &api, message).await;
            }
            ImPlatformKind::Wecom => {
                let Some(api) = api_registry.wecom_for_route(&message.route) else {
                    log_missing_api(&state, &message).await;
                    continue;
                };
                send_wecom_outbound(&state, &api, message).await;
            }
        }
    }
    turns.close_all(&state, &api_registry).await;
    state
        .push_event(
            "warn",
            "im_outbound_worker_stopped",
            "outbound queue closed",
        )
        .await;
}

pub(crate) async fn replay_wechat_pending_for_peer(
    state: &SharedState,
    outbound_tx: &ImOutboundSender,
    account_id: &str,
    peer_id: &str,
) -> usize {
    let key = wechat_recovery_key(account_id, peer_id);
    let pending = {
        let mut recovery = state.wechat_recovery.lock().await;
        recovery.awaiting_fresh_context_token.remove(&key);
        recovery
            .pending_outbound_by_peer
            .remove(&key)
            .map(|queue| queue.into_iter().collect::<Vec<_>>())
            .unwrap_or_default()
    };
    if pending.is_empty() {
        return 0;
    }
    let count = pending.len();
    state
        .push_event(
            "info",
            "wechat_context_token_refreshed",
            format!("account={account_id} peer={peer_id} replaying={count}"),
        )
        .await;
    for message in pending {
        if let Err(err) = outbound_tx.enqueue(message.clone()) {
            log_outbound_result("wechat_replay_enqueue_failed", &message, &err.to_string());
            state
                .push_event(
                    "error",
                    "wechat_replay_enqueue_failed",
                    format!(
                        "account={} peer={} thread={} err={}",
                        account_id, peer_id, message.thread_id, err
                    ),
                )
                .await;
        }
    }
    count
}

pub(super) async fn outbound_channel_enabled(state: &SharedState, route: &RouteTarget) -> bool {
    let config = state.config.lock().await;
    match route.platform {
        ImPlatformKind::Feishu => config
            .feishu_account(&route.account_id)
            .is_some_and(|account| account.is_active()),
        ImPlatformKind::Telegram => config
            .telegram_account(&route.account_id)
            .is_some_and(|account| account.is_active()),
        ImPlatformKind::Wechat => config
            .wechat_account(&route.account_id)
            .is_some_and(|account| account.is_active()),
        ImPlatformKind::Wecom => config
            .wecom_account(&route.account_id)
            .is_some_and(|account| account.is_active()),
    }
}

pub(super) async fn send_wecom_outbound(
    state: &SharedState,
    wecom_api: &WecomApi,
    message: ImOutboundMessage,
) {
    let adapter = WecomAdapter::new(wecom_api.clone());
    let result = match &message.payload {
        ImOutboundPayload::GmClawTurnStage { .. }
        | ImOutboundPayload::GmClawTurnFinished { .. } => return,
        ImOutboundPayload::Text(text) => {
            adapter
                .send_text(
                    state,
                    &message.route.account_id,
                    &message.route.chat_id,
                    text,
                )
                .await
        }
        ImOutboundPayload::Approval(approval) => {
            match adapter
                .send_approval_card(&message.route.chat_id, approval)
                .await
            {
                Ok(message_id) => Ok(message_id),
                Err(card_err) => {
                    let text = crate::im::wechat::adapter::approval_text(
                        approval,
                        im_text_for_state(state),
                    );
                    adapter
                        .send_text(
                            state,
                            &message.route.account_id,
                            &message.route.chat_id,
                            &text,
                        )
                        .await
                        .map_err(|text_err| anyhow!("card={card_err}; fallback={text_err}"))
                }
            }
        }
        ImOutboundPayload::GmClawApproval(approval) => {
            send_wecom_gmclaw_approval(state, &adapter, &message, approval).await
        }
        ImOutboundPayload::GmClawApprovalResolved {
            approval,
            option_index,
            callback,
        } => {
            let response = executor_approval::resolved_text(*option_index);
            if let Some(response) = response {
                let updated = if let Some(callback) = callback {
                    adapter
                        .acknowledge_gmclaw_approval(approval, *option_index, callback)
                        .await
                        .unwrap_or(false)
                } else {
                    false
                };
                if updated {
                    Ok(String::new())
                } else {
                    adapter
                        .send_text(
                            state,
                            &message.route.account_id,
                            &message.route.chat_id,
                            &response,
                        )
                        .await
                }
            } else {
                Err(anyhow!("invalid TianGong approval choice"))
            }
        }
        ImOutboundPayload::Image {
            path,
            caption,
            fallback_text,
        } => match adapter.send_media(&message.route.chat_id, path).await {
            Ok(message_id) => Ok(message_id),
            Err(media_err) => {
                let fallback = fallback_text
                    .as_deref()
                    .or(caption.as_deref())
                    .unwrap_or("附件发送失败");
                adapter
                    .send_text(
                        state,
                        &message.route.account_id,
                        &message.route.chat_id,
                        fallback,
                    )
                    .await
                    .map_err(|text_err| anyhow!("media={media_err}; fallback={text_err}"))
            }
        },
    };
    match result {
        Ok(message_id) => {
            log_outbound_result("send_wecom_text_done", &message, &message_id);
            state
                .push_event(
                    "info",
                    match message.kind {
                        ImOutboundKind::TurnReply => "wecom_turn_completed_sent",
                        ImOutboundKind::Approval => "wecom_approval_sent",
                        ImOutboundKind::Item | ImOutboundKind::ImageItem => "wecom_item_sent",
                    },
                    format!(
                        "thread={} chat={} message={message_id}",
                        message.thread_id, message.route.chat_id
                    ),
                )
                .await;
        }
        Err(err) => {
            log_outbound_result("send_wecom_text_failed", &message, &err.to_string());
            state
                .push_event(
                    "error",
                    "wecom_send_failed",
                    format!(
                        "thread={} chat={} err={err}",
                        message.thread_id, message.route.chat_id
                    ),
                )
                .await;
        }
    }
}

async fn log_missing_api(state: &SharedState, message: &ImOutboundMessage) {
    state
        .push_event(
            "error",
            "im_outbound_api_missing",
            format!(
                "platform={} account={} thread={} chat={}",
                message.route.platform.key(),
                message.route.account_id,
                message.thread_id,
                message.route.chat_id
            ),
        )
        .await;
}

async fn send_wecom_gmclaw_approval(
    state: &SharedState,
    adapter: &WecomAdapter,
    message: &ImOutboundMessage,
    approval: &GmClawApproval,
) -> Result<String> {
    // The native card subtitle is bounded. Send every parameter first; a
    // partially delivered description must not be followed by actionable buttons.
    let details = executor_approval::approval_text(approval);
    for chunk in executor_approval::text_chunks(&details, 3500) {
        adapter
            .send_text(
                state,
                &message.route.account_id,
                &message.route.chat_id,
                chunk,
            )
            .await
            .map_err(|_| anyhow!("天工审批完整参数发送未完成，审批卡片未发送"))?;
    }
    match adapter
        .send_gmclaw_approval_card(&message.route.chat_id, approval)
        .await
    {
        Ok(message_id) => {
            crate::gmclaw_im::remember_approval_message_id(
                state,
                &approval.request_key,
                message_id.clone(),
            )
            .await;
            Ok(message_id)
        }
        Err(_) => {
            state
                .push_event(
                    "warn",
                    "wecom_gmclaw_approval_card_fallback",
                    "天工审批卡片发送失败，改用完整文字审批指令",
                )
                .await;
            let fallback = executor_approval::legacy_approval_text(approval);
            let mut last_id = String::new();
            for chunk in executor_approval::text_chunks(&fallback, 3500) {
                last_id = adapter
                    .send_text(
                        state,
                        &message.route.account_id,
                        &message.route.chat_id,
                        chunk,
                    )
                    .await
                    .map_err(|_| anyhow!("天工审批卡片和文字回退发送未完成"))?;
            }
            Ok(last_id)
        }
    }
}

async fn send_feishu_gmclaw_approval(
    state: &SharedState,
    adapter: &FeishuAdapter,
    message: &ImOutboundMessage,
    approval: &GmClawApproval,
) {
    let mut shown = approval.clone();
    let card =
        crate::im::feishu::renderer::build_gmclaw_approval_card(&shown, im_text_for_state(state));
    if card.to_string().len() > 24 * 1024 {
        let details = format!("天工 Claw 审批请求：完整工具参数\n\n{}", approval.summary);
        if send_feishu_gmclaw_full_text(adapter, &message.route.chat_id, &details)
            .await
            .is_err()
        {
            state
                .push_event(
                    "error",
                    "feishu_gmclaw_approval_failed",
                    "完整工具参数发送未完成，审批按钮未发送",
                )
                .await;
            return;
        }
        shown.summary =
            "完整工具参数已在前面的消息中发送，请核对后选择；仅适用于本次请求，不会永久授权。"
                .into();
    }
    match adapter
        .send_gmclaw_approval(&message.route.chat_id, &shown, im_text_for_state(state))
        .await
    {
        Ok(message_id) => {
            crate::gmclaw_im::remember_approval_message_id(
                state,
                &approval.request_key,
                message_id,
            )
            .await;
            state
                .push_event("info", "feishu_gmclaw_approval_sent", "天工审批卡片已发送")
                .await;
        }
        Err(_) => {
            state
                .push_event(
                    "warn",
                    "feishu_gmclaw_approval_card_fallback",
                    "天工审批卡片发送失败，改用完整文字审批指令",
                )
                .await;
            let fallback = executor_approval::legacy_approval_text(approval);
            if send_feishu_gmclaw_full_text(adapter, &message.route.chat_id, &fallback)
                .await
                .is_err()
            {
                state
                    .push_event(
                        "error",
                        "feishu_gmclaw_approval_failed",
                        "天工审批卡片和文字回退发送未完成",
                    )
                    .await;
            }
        }
    }
}

async fn send_feishu_gmclaw_full_text(
    adapter: &FeishuAdapter,
    target: &str,
    text: &str,
) -> Result<()> {
    let adapter = adapter.for_sensitive_messages();
    for chunk in feishu_gmclaw_text_chunks(text) {
        adapter
            .send_text(target, chunk)
            .await
            .map_err(|_| anyhow!("天工审批文字发送未完成"))?;
    }
    Ok(())
}

async fn send_telegram_outbound(
    state: &SharedState,
    telegram_api: &TelegramApi,
    message: ImOutboundMessage,
) {
    let adapter = TelegramAdapter::new(telegram_api.clone());
    match &message.payload {
        ImOutboundPayload::GmClawTurnStage { .. }
        | ImOutboundPayload::GmClawTurnFinished { .. } => return,
        ImOutboundPayload::Text(text) => {
            send_telegram_text(state, &adapter, &message, text).await;
        }
        ImOutboundPayload::Approval(approval) => {
            send_telegram_approval(state, &adapter, &message, approval).await;
        }
        ImOutboundPayload::GmClawApproval(_) | ImOutboundPayload::GmClawApprovalResolved { .. } => {
            send_telegram_text(
                state,
                &adapter,
                &message,
                "Telegram 暂不支持天工 Claw 审批，请在天工桌面处理。",
            )
            .await;
        }
        ImOutboundPayload::Image {
            path,
            caption,
            fallback_text,
        } => {
            send_telegram_image(
                state,
                &adapter,
                &message,
                path.clone(),
                caption.as_deref(),
                fallback_text.as_deref(),
            )
            .await;
        }
    }
}

async fn send_feishu_outbound(
    state: &SharedState,
    feishu_api: &FeishuApi,
    message: ImOutboundMessage,
) {
    let adapter = FeishuAdapter::new(feishu_api.clone());
    match &message.payload {
        ImOutboundPayload::GmClawTurnStage { .. }
        | ImOutboundPayload::GmClawTurnFinished { .. } => return,
        ImOutboundPayload::Approval(approval) => {
            send_feishu_approval(state, &adapter, &message, approval).await;
        }
        ImOutboundPayload::GmClawApproval(approval) => {
            send_feishu_gmclaw_approval(state, &adapter, &message, approval).await;
        }
        ImOutboundPayload::GmClawApprovalResolved {
            approval,
            option_index,
            ..
        } => {
            let adapter = adapter.for_sensitive_messages();
            let Some(response) = executor_approval::resolved_text(*option_index) else {
                return;
            };
            if !adapter
                .update_resolved_gmclaw_approval(approval, *option_index, im_text_for_state(state))
                .await
                .unwrap_or(false)
            {
                send_feishu_gmclaw_text(state, &adapter, &message, &response).await;
            }
        }
        ImOutboundPayload::Text(text) if message.item_type.as_deref() == Some("gmclaw") => {
            send_feishu_gmclaw_text(state, &adapter, &message, text).await;
        }
        ImOutboundPayload::Text(_) | ImOutboundPayload::Image { .. } => {
            state
                .push_event(
                    "warn",
                    "im_outbound_unsupported",
                    format!(
                        "platform=feishu thread={} chat={} kind={:?}",
                        message.thread_id, message.route.chat_id, message.kind
                    ),
                )
                .await;
        }
    }
}

async fn send_feishu_gmclaw_text(
    state: &SharedState,
    adapter: &FeishuAdapter,
    message: &ImOutboundMessage,
    text: &str,
) {
    // Keep full approval parameters and Unicode intact. A conservative chunk
    // size also leaves room for JSON escaping in Feishu's nested text payload.
    let chunks = feishu_gmclaw_text_chunks(text);
    for (index, chunk) in chunks.iter().enumerate() {
        if let Err(error) = adapter.send_text(&message.route.chat_id, chunk).await {
            log_outbound_result(
                "send_feishu_gmclaw_text_failed",
                message,
                &error.to_string(),
            );
            state
                .push_event(
                    "error",
                    "feishu_gmclaw_send_failed",
                    format!(
                        "thread={} chat={} sent_chunks={} total_chunks={} err={error}",
                        message.thread_id,
                        message.route.chat_id,
                        index,
                        chunks.len()
                    ),
                )
                .await;
            return;
        }
    }
    state
        .push_event(
            "info",
            "feishu_gmclaw_text_sent",
            format!(
                "thread={} chat={} chunks={}",
                message.thread_id,
                message.route.chat_id,
                chunks.len()
            ),
        )
        .await;
}

fn feishu_gmclaw_text_chunks(mut text: &str) -> Vec<&str> {
    const CHUNK_CHARS: usize = 2000;
    let mut chunks = Vec::new();
    while !text.is_empty() {
        let end = text
            .char_indices()
            .nth(CHUNK_CHARS)
            .map_or(text.len(), |(index, _)| index);
        // Prefer a line boundary so short approval commands stay copyable.
        let end = if end < text.len() {
            text[..end]
                .rfind('\n')
                .filter(|index| *index > 0)
                .map_or(end, |index| index + 1)
        } else {
            end
        };
        chunks.push(&text[..end]);
        text = &text[end..];
    }
    chunks
}

async fn send_feishu_approval(
    state: &SharedState,
    adapter: &FeishuAdapter,
    message: &ImOutboundMessage,
    approval: &PendingApproval,
) {
    state
        .push_event(
            "info",
            "feishu_approval_send_begin",
            format!(
                "thread={} request_id={} chat={}",
                message.thread_id, approval.request_id, message.route.chat_id
            ),
        )
        .await;
    match adapter
        .send_approval(&message.route.chat_id, approval, im_text_for_state(state))
        .await
    {
        Ok(message_id) => {
            state
                .runtime
                .lock()
                .await
                .remember_approval_message_id(&approval.request_id, message_id.clone());
            state
                .push_event(
                    "info",
                    "approval_card_sent",
                    format!(
                        "conversation={} request_id={} message={}",
                        message.route.conversation_key, approval.request_id, message_id
                    ),
                )
                .await;
        }
        Err(err) => {
            state
                .push_event(
                    "error",
                    "feishu_approval_failed",
                    format!(
                        "conversation={} request_id={} chat={} err={}",
                        message.route.conversation_key,
                        approval.request_id,
                        message.route.chat_id,
                        err
                    ),
                )
                .await;
        }
    }
}

#[cfg(test)]
mod gmclaw_outbound_tests {
    use super::feishu_gmclaw_text_chunks;

    #[test]
    fn feishu_chunks_preserve_unicode_and_full_approval_text() {
        let text = format!(
            "  {}\n/gmclaw approve example-code",
            "天工🔧\"\\".repeat(1600)
        );
        let chunks = feishu_gmclaw_text_chunks(&text);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 2000));
        assert_eq!(chunks.concat(), text);
        assert!(feishu_gmclaw_text_chunks("").is_empty());
    }
}

pub(super) async fn send_wechat_outbound(
    state: &SharedState,
    wechat_api: &WechatApi,
    message: ImOutboundMessage,
) {
    let adapter = WechatAdapter::new(wechat_api.clone());
    match &message.payload {
        ImOutboundPayload::GmClawTurnStage { .. } => return,
        ImOutboundPayload::GmClawTurnFinished { text, .. } => {
            send_wechat_text(state, &adapter, &message, text).await;
        }
        ImOutboundPayload::Text(text) => {
            send_wechat_text(state, &adapter, &message, text).await;
        }
        ImOutboundPayload::Approval(approval) => {
            let text =
                crate::im::wechat::adapter::approval_text(approval, im_text_for_state(state));
            send_wechat_text(state, &adapter, &message, &text).await;
        }
        ImOutboundPayload::GmClawApproval(approval) => {
            let text = executor_approval::approval_text(approval);
            send_wechat_text(state, &adapter, &message, &text).await;
        }
        ImOutboundPayload::GmClawApprovalResolved { option_index, .. } => {
            if let Some(response) = executor_approval::resolved_text(*option_index) {
                send_wechat_text(state, &adapter, &message, &response).await;
            }
        }
        ImOutboundPayload::Image {
            path,
            caption,
            fallback_text,
        } => {
            send_wechat_image(
                state,
                &adapter,
                &message,
                path.clone(),
                caption.as_deref(),
                fallback_text.as_deref(),
            )
            .await;
        }
    }
}

async fn send_wechat_text(
    state: &SharedState,
    adapter: &WechatAdapter,
    message: &ImOutboundMessage,
    text: &str,
) -> bool {
    let event_begin = match message.kind {
        ImOutboundKind::TurnReply => "wechat_turn_send_begin",
        ImOutboundKind::Item | ImOutboundKind::ImageItem => "wechat_item_send_begin",
        ImOutboundKind::Approval => "wechat_approval_send_begin",
    };
    let event_done = match message.kind {
        ImOutboundKind::TurnReply => "wechat_turn_completed_sent",
        ImOutboundKind::Item | ImOutboundKind::ImageItem => "wechat_item_sent",
        ImOutboundKind::Approval => "wechat_approval_sent",
    };
    state
        .push_event(
            "info",
            event_begin,
            format!(
                "thread={} item={} type={} peer={} text_len={}",
                message.thread_id,
                message.item_id.as_deref().unwrap_or(""),
                message.item_type.as_deref().unwrap_or(""),
                message.route.chat_id,
                text.chars().count()
            ),
        )
        .await;
    log_outbound_message("send_wechat_text_begin", message, Some(text));
    match send_wechat_text_with_context_mode(state, adapter, message, text, true).await {
        Ok(message_id) => {
            log_outbound_result("send_wechat_text_done", message, &message_id);
            push_wechat_text_sent_event(state, event_done, message, &message_id).await;
            true
        }
        Err(err) => {
            let err_text = err.to_string();
            let mut final_err_text = err_text.clone();
            log_outbound_result("send_wechat_text_failed", message, &err_text);
            if is_wechat_context_token_error(&err_text) && wechat_text_can_retry_without_token(text)
            {
                forget_wechat_context_token(state, message).await;
                state
                    .push_event(
                        "warn",
                        "wechat_text_retry_without_context_token",
                        format!(
                            "thread={} item={} type={} peer={} err={}",
                            message.thread_id,
                            message.item_id.as_deref().unwrap_or(""),
                            message.item_type.as_deref().unwrap_or(""),
                            message.route.chat_id,
                            safe_outbound_error(message, &err_text)
                        ),
                    )
                    .await;
                log_outbound_result(
                    "send_wechat_text_retry_without_context_token",
                    message,
                    &err_text,
                );
                match send_wechat_text_with_context_mode(state, adapter, message, text, false).await
                {
                    Ok(message_id) => {
                        log_outbound_result(
                            "send_wechat_text_retry_without_context_token_done",
                            message,
                            &message_id,
                        );
                        push_wechat_text_sent_event(state, event_done, message, &message_id).await;
                        return true;
                    }
                    Err(retry_err) => {
                        let retry_err_text = retry_err.to_string();
                        final_err_text = retry_err_text.clone();
                        log_outbound_result(
                            "send_wechat_text_retry_without_context_token_failed",
                            message,
                            &retry_err_text,
                        );
                        if defer_wechat_outbound_on_context_error(state, message, &retry_err_text)
                            .await
                        {
                            return false;
                        }
                    }
                }
            } else if defer_wechat_outbound_on_context_error(state, message, &err_text).await {
                return false;
            }
            let event_failed = match message.kind {
                ImOutboundKind::TurnReply => "wechat_turn_completed_failed",
                ImOutboundKind::Item | ImOutboundKind::ImageItem => "wechat_item_failed",
                ImOutboundKind::Approval => "wechat_approval_failed",
            };
            state
                .push_event(
                    "error",
                    event_failed,
                    format!(
                        "thread={} item={} type={} peer={} err={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        message.route.chat_id,
                        safe_outbound_error(message, &final_err_text)
                    ),
                )
                .await;
            false
        }
    }
}

async fn send_wechat_text_with_context_mode(
    state: &SharedState,
    adapter: &WechatAdapter,
    message: &ImOutboundMessage,
    text: &str,
    use_context_token: bool,
) -> anyhow::Result<String> {
    if let ImOutboundPayload::GmClawTurnFinished { turn, .. } = &message.payload {
        adapter
            .send_gmclaw_turn_text(
                state,
                &message.route.account_id,
                &message.route.chat_id,
                text,
                use_context_token,
                turn.delivery_scope(),
            )
            .await
    } else if matches!(
        message.payload,
        ImOutboundPayload::GmClawApproval(_) | ImOutboundPayload::GmClawApprovalResolved { .. }
    ) || message.item_type.as_deref() == Some("gmclaw-turn")
    {
        adapter
            .send_gmclaw_approval_text(
                state,
                &message.route.account_id,
                &message.route.chat_id,
                text,
                use_context_token,
            )
            .await
    } else if use_context_token {
        adapter
            .send_text(
                state,
                &message.route.account_id,
                &message.route.chat_id,
                text,
            )
            .await
    } else {
        adapter
            .send_text_without_context_token(
                state,
                &message.route.account_id,
                &message.route.chat_id,
                text,
            )
            .await
    }
}

fn safe_outbound_error<'a>(message: &ImOutboundMessage, error: &'a str) -> &'a str {
    if matches!(
        message.payload,
        ImOutboundPayload::GmClawApproval(_) | ImOutboundPayload::GmClawApprovalResolved { .. }
    ) || message.item_type.as_deref() == Some("gmclaw-turn")
    {
        "天工审批消息发送未完成（详细响应已隐藏）"
    } else {
        error
    }
}

async fn send_wechat_image(
    state: &SharedState,
    adapter: &WechatAdapter,
    message: &ImOutboundMessage,
    path: PathBuf,
    caption: Option<&str>,
    fallback_text: Option<&str>,
) {
    state
        .push_event(
            "info",
            "wechat_image_send_begin",
            format!(
                "thread={} item={} type={} peer={} path={} caption_len={}",
                message.thread_id,
                message.item_id.as_deref().unwrap_or(""),
                message.item_type.as_deref().unwrap_or(""),
                message.route.chat_id,
                path.display(),
                caption.map(|value| value.chars().count()).unwrap_or(0)
            ),
        )
        .await;
    match adapter
        .send_image_path(
            state,
            &message.route.account_id,
            &message.route.chat_id,
            &path,
            caption,
            fallback_text,
        )
        .await
    {
        Ok(message_id) => {
            state
                .push_event(
                    "info",
                    "wechat_image_item_sent",
                    format!(
                        "thread={} item={} type={} peer={} message={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        message.route.chat_id,
                        message_id
                    ),
                )
                .await;
        }
        Err(err) => {
            if defer_wechat_outbound_on_context_error(state, message, &err.to_string()).await {
                return;
            }
            state
                .push_event(
                    "error",
                    "wechat_image_send_failed",
                    format!(
                        "thread={} item={} type={} path={} err={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        path.display(),
                        err
                    ),
                )
                .await;
        }
    }
}

async fn defer_wechat_outbound_if_waiting(
    state: &SharedState,
    message: &ImOutboundMessage,
) -> bool {
    let key = wechat_recovery_key(&message.route.account_id, &message.route.chat_id);
    let waiting = {
        let recovery = state.wechat_recovery.lock().await;
        recovery.awaiting_fresh_context_token.contains(&key)
    };
    if waiting {
        if matches!(
            message.payload,
            ImOutboundPayload::Text(_)
                | ImOutboundPayload::Approval(_)
                | ImOutboundPayload::GmClawApproval(_)
                | ImOutboundPayload::GmClawApprovalResolved { .. }
                | ImOutboundPayload::GmClawTurnFinished { .. }
        ) {
            log_outbound_result(
                "wechat_context_token_waiting_text_allowed",
                message,
                "waiting_for_fresh_context_token",
            );
            return false;
        }
        queue_wechat_pending_outbound(state, message, "waiting_for_fresh_context_token").await;
        return true;
    }
    let context_token = wechat_store::context_token_record(
        state,
        &message.route.account_id,
        &message.route.chat_id,
    )
    .await;
    if context_token.is_none() {
        if matches!(
            message.payload,
            ImOutboundPayload::Text(_)
                | ImOutboundPayload::Approval(_)
                | ImOutboundPayload::GmClawApproval(_)
                | ImOutboundPayload::GmClawApprovalResolved { .. }
                | ImOutboundPayload::GmClawTurnFinished { .. }
        ) {
            log_outbound_result(
                "wechat_context_token_missing_text_allowed",
                message,
                "missing_context_token",
            );
            return false;
        }
        queue_wechat_pending_outbound(state, message, "missing_context_token").await;
        return true;
    }
    if let Some(age_ms) = context_token.as_ref().and_then(|record| record.age_ms()) {
        log_outbound_result(
            "wechat_context_token_available",
            message,
            &format!("token_age_ms={age_ms}"),
        );
    } else {
        log_outbound_result(
            "wechat_context_token_available",
            message,
            "token_age_ms=unknown",
        );
    }
    false
}

async fn defer_wechat_outbound_on_context_error(
    state: &SharedState,
    message: &ImOutboundMessage,
    err: &str,
) -> bool {
    if is_wechat_context_token_error(err) {
        forget_wechat_context_token(state, message).await;
        queue_wechat_pending_outbound(state, message, "ret_minus_2").await;
        return true;
    }
    false
}

async fn forget_wechat_context_token(state: &SharedState, message: &ImOutboundMessage) {
    if let Err(forget_err) =
        wechat_store::forget_context_token(state, &message.route.account_id, &message.route.chat_id)
            .await
    {
        state
            .push_event(
                "warn",
                "wechat_context_token_forget_failed",
                format!(
                    "account={} peer={} err={}",
                    message.route.account_id, message.route.chat_id, forget_err
                ),
            )
            .await;
    }
}

async fn push_wechat_text_sent_event(
    state: &SharedState,
    event_done: &str,
    message: &ImOutboundMessage,
    message_id: &str,
) {
    if let ImOutboundPayload::GmClawApproval(approval) = &message.payload {
        crate::gmclaw_im::remember_approval_message_id(
            state,
            &approval.request_key,
            message_id.to_owned(),
        )
        .await;
    }
    state
        .push_event(
            "info",
            event_done,
            format!(
                "thread={} item={} type={} peer={} message={}",
                message.thread_id,
                message.item_id.as_deref().unwrap_or(""),
                message.item_type.as_deref().unwrap_or(""),
                message.route.chat_id,
                message_id
            ),
        )
        .await;
}

fn wechat_text_can_retry_without_token(text: &str) -> bool {
    text.trim().chars().count() <= WECHAT_TEXT_CHUNK_CHARS
}

async fn queue_wechat_pending_outbound(
    state: &SharedState,
    message: &ImOutboundMessage,
    reason: &str,
) {
    let key = wechat_recovery_key(&message.route.account_id, &message.route.chat_id);
    let pending_len = {
        let mut recovery = state.wechat_recovery.lock().await;
        recovery.awaiting_fresh_context_token.insert(key.clone());
        let queue = recovery.pending_outbound_by_peer.entry(key).or_default();
        queue.push_back(message.clone());
        queue.len()
    };
    log_outbound_result("wechat_pending_until_fresh_context_token", message, reason);
    state
        .push_event(
            "warn",
            "wechat_waiting_for_fresh_context_token",
            format!(
                "account={} peer={} thread={} reason={} pending={}",
                message.route.account_id,
                message.route.chat_id,
                message.thread_id,
                reason,
                pending_len
            ),
        )
        .await;
}

fn is_wechat_context_token_error(err: &str) -> bool {
    err.contains("ret_minus_2")
        || err.contains("ret=-2")
        || err.contains("ret\":-2")
        || err.contains("errcode=-2")
        || err.contains("code=-2")
        || err.contains("wechat image message context_token is missing")
}

fn wechat_recovery_key(account_id: &str, peer_id: &str) -> String {
    format!("{account_id}:{peer_id}")
}

async fn send_telegram_approval(
    state: &SharedState,
    adapter: &TelegramAdapter,
    message: &ImOutboundMessage,
    approval: &PendingApproval,
) {
    state
        .push_event(
            "info",
            "telegram_approval_send_begin",
            format!(
                "thread={} request_id={} chat={}",
                message.thread_id, approval.request_id, message.route.chat_id
            ),
        )
        .await;
    match adapter
        .send_approval(&message.route.chat_id, approval, im_text_for_state(state))
        .await
    {
        Ok(message_id) => {
            state
                .runtime
                .lock()
                .await
                .remember_approval_message_id(&approval.request_id, message_id.clone());
            state
                .push_event(
                    "info",
                    "telegram_approval_sent",
                    format!(
                        "conversation={} request_id={} message={}",
                        message.route.conversation_key, approval.request_id, message_id
                    ),
                )
                .await;
        }
        Err(err) => {
            state
                .push_event(
                    "error",
                    "telegram_approval_failed",
                    format!(
                        "conversation={} request_id={} chat={} err={}",
                        message.route.conversation_key,
                        approval.request_id,
                        message.route.chat_id,
                        err
                    ),
                )
                .await;
        }
    }
}

async fn send_telegram_text(
    state: &SharedState,
    adapter: &TelegramAdapter,
    message: &ImOutboundMessage,
    text: &str,
) {
    let event_begin = match message.kind {
        ImOutboundKind::TurnReply => "telegram_turn_send_begin",
        ImOutboundKind::Item | ImOutboundKind::ImageItem => "telegram_item_send_begin",
        ImOutboundKind::Approval => "telegram_approval_send_begin",
    };
    let event_done = match message.kind {
        ImOutboundKind::TurnReply => "telegram_turn_completed_sent",
        ImOutboundKind::Item | ImOutboundKind::ImageItem => "telegram_item_sent",
        ImOutboundKind::Approval => "telegram_approval_sent",
    };
    state
        .push_event(
            "info",
            event_begin,
            format!(
                "thread={} item={} type={} chat={} text_len={}",
                message.thread_id,
                message.item_id.as_deref().unwrap_or(""),
                message.item_type.as_deref().unwrap_or(""),
                message.route.chat_id,
                text.chars().count()
            ),
        )
        .await;
    log_outbound_message("send_telegram_text_begin", message, Some(text));
    match adapter.send_text(&message.route.chat_id, text).await {
        Ok(message_id) => {
            log_outbound_result("send_telegram_text_done", message, &message_id);
            state
                .push_event(
                    "info",
                    event_done,
                    format!(
                        "thread={} item={} type={} chat={} message={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        message.route.chat_id,
                        message_id
                    ),
                )
                .await;
        }
        Err(err) => {
            log_outbound_result("send_telegram_text_failed", message, &err.to_string());
            let event_failed = match message.kind {
                ImOutboundKind::TurnReply => "telegram_turn_completed_failed",
                ImOutboundKind::Item | ImOutboundKind::ImageItem => "telegram_item_failed",
                ImOutboundKind::Approval => "telegram_approval_failed",
            };
            state
                .push_event(
                    "error",
                    event_failed,
                    format!(
                        "thread={} item={} type={} chat={} err={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        message.route.chat_id,
                        err
                    ),
                )
                .await;
        }
    }
}

fn log_outbound_message(event: &str, message: &ImOutboundMessage, text: Option<&str>) {
    if !chain_log::diagnostic_enabled() {
        return;
    }
    let (payload_kind, text_len, preview) = match (&message.payload, text) {
        (ImOutboundPayload::GmClawTurnStage { .. }, _) => {
            ("gmclaw_turn_stage", 0, "[redacted]".to_owned())
        }
        (ImOutboundPayload::GmClawTurnFinished { text, .. }, _) => {
            ("gmclaw_turn_finished", text.len(), "[redacted]".to_owned())
        }
        (_, _) if message.item_type.as_deref() == Some("gmclaw-turn") => {
            ("gmclaw_turn_fallback", 0, "[redacted]".to_owned())
        }
        (ImOutboundPayload::GmClawApproval(approval), _) => (
            "gmclaw_approval",
            approval.summary.chars().count(),
            "[redacted]".to_owned(),
        ),
        (ImOutboundPayload::GmClawApprovalResolved { .. }, _) => {
            ("gmclaw_approval_resolved", 0, "[redacted]".to_owned())
        }
        (_, Some(text)) => ("text", text.chars().count(), trace_preview(text, 500)),
        (ImOutboundPayload::Text(text), None) => {
            ("text", text.chars().count(), trace_preview(text, 500))
        }
        (ImOutboundPayload::Approval(approval), None) => (
            "approval",
            approval.summary.chars().count(),
            trace_preview(&approval.summary, 500),
        ),
        (
            ImOutboundPayload::Image {
                path,
                caption,
                fallback_text,
            },
            None,
        ) => {
            let image_text = format!(
                "path={} caption={} fallback={}",
                path.display(),
                caption.as_deref().unwrap_or(""),
                fallback_text.as_deref().unwrap_or("")
            );
            (
                "image",
                image_text.chars().count(),
                trace_preview(&image_text, 500),
            )
        }
    };
    chain_log::write_diagnostic_lazy(|| {
        format!(
            "[im_trace] event=remote_to_im_outbound_{} platform={} account={} chat={} thread={} item={} type={} kind={:?} payload={} text_len={} preview={}",
            event,
            message.route.platform.key(),
            message.route.account_id,
            message.route.chat_id,
            message.thread_id,
            message.item_id.as_deref().unwrap_or(""),
            message.item_type.as_deref().unwrap_or(""),
            message.kind,
            payload_kind,
            text_len,
            preview
        )
    });
}

fn log_outbound_result(event: &str, message: &ImOutboundMessage, result: &str) {
    let result = if matches!(
        message.payload,
        ImOutboundPayload::GmClawApproval(_)
            | ImOutboundPayload::GmClawApprovalResolved { .. }
            | ImOutboundPayload::GmClawTurnStage { .. }
            | ImOutboundPayload::GmClawTurnFinished { .. }
    ) || message.item_type.as_deref() == Some("gmclaw-turn")
    {
        "[redacted]"
    } else {
        result
    };
    chain_log::write_diagnostic_lazy(|| {
        format!(
            "[im_trace] event=remote_to_im_outbound_{} platform={} account={} chat={} thread={} item={} type={} kind={:?} result={}",
            event,
            message.route.platform.key(),
            message.route.account_id,
            message.route.chat_id,
            message.thread_id,
            message.item_id.as_deref().unwrap_or(""),
            message.item_type.as_deref().unwrap_or(""),
            message.kind,
            trace_preview(result, 300)
        )
    });
}

fn trace_preview(text: &str, limit: usize) -> String {
    let compact = text.replace("\r\n", "\n").replace('\n', "\\n");
    let mut out = String::new();
    for ch in compact.chars().take(limit) {
        out.push(ch);
    }
    if compact.chars().count() > limit {
        out.push_str("...");
    }
    out
}

async fn send_telegram_image(
    state: &SharedState,
    adapter: &TelegramAdapter,
    message: &ImOutboundMessage,
    path: PathBuf,
    caption: Option<&str>,
    fallback_text: Option<&str>,
) {
    state
        .push_event(
            "info",
            "telegram_image_send_begin",
            format!(
                "thread={} item={} type={} chat={} path={} caption_len={}",
                message.thread_id,
                message.item_id.as_deref().unwrap_or(""),
                message.item_type.as_deref().unwrap_or(""),
                message.route.chat_id,
                path.display(),
                caption.map(|value| value.chars().count()).unwrap_or(0)
            ),
        )
        .await;
    match adapter
        .send_image_path(&message.route.chat_id, &path, caption)
        .await
    {
        Ok(message_id) => {
            state
                .push_event(
                    "info",
                    "telegram_image_item_sent",
                    format!(
                        "thread={} item={} type={} chat={} message={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        message.route.chat_id,
                        message_id
                    ),
                )
                .await;
        }
        Err(err) => {
            state
                .push_event(
                    "warn",
                    "telegram_image_send_failed",
                    format!(
                        "thread={} item={} type={} path={} err={}",
                        message.thread_id,
                        message.item_id.as_deref().unwrap_or(""),
                        message.item_type.as_deref().unwrap_or(""),
                        path.display(),
                        err
                    ),
                )
                .await;
            if let Some(fallback_text) = fallback_text {
                send_telegram_text(state, adapter, message, fallback_text).await;
            }
        }
    }
}
