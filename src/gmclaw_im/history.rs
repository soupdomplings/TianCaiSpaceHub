//! Coordinate explicit history edits with IM execution and link-file writes.
use super::*;
use std::collections::HashSet;

const HISTORY_BUSY: &str = "天工 IM 正在处理请求，请完成后发送 /q 释放会话，再重试历史管理。";

pub(crate) async fn edit_history<T, F>(state: SharedState, ids: Vec<String>, action: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let gate = state
        .gmclaw_im
        .history_gate
        .clone()
        .try_write_owned()
        .map_err(|_| anyhow::anyhow!(HISTORY_BUSY))?;

    // Dropping or timing out the API waiter must not release these locks while
    // the blocking edit is still running, or skip invalidating old IM caches.
    tokio::spawn(async move {
        let selected: HashSet<_> = ids.into_iter().collect();
        let bindings = editable_bindings(&state, &selected).await?;
        let io = desktop::LINK_IO.clone().lock_owned().await;
        let (result, _gate, io) = tokio::task::spawn_blocking(move || {
            let result = action();
            (result, gate, io)
        })
        .await
        .context("天工历史编辑未确认完成，请刷新后核对结果")?;
        // A list reader may hold a sender lock while awaiting LINK_IO. The
        // write gate still excludes execution/resume during cache invalidation.
        drop(io);
        let value = result?;
        for binding in bindings {
            binding
                .sessions
                .lock()
                .await
                .history
                .retain(|session| !selected.contains(&session.id));
        }
        Ok(value)
    })
    .await
    .context("天工历史编辑未确认完成，请刷新后核对结果")?
}

async fn editable_bindings(
    state: &SharedState,
    selected: &HashSet<String>,
) -> Result<Vec<Arc<SenderBinding>>> {
    let bindings: Vec<_> = state
        .gmclaw_im
        .sessions
        .lock()
        .await
        .values()
        .cloned()
        .collect();
    for binding in &bindings {
        let sessions = binding
            .sessions
            .try_lock()
            .map_err(|_| anyhow::anyhow!(HISTORY_BUSY))?;
        for session in sessions.current.iter().chain(sessions.history.iter()) {
            if selected.contains(&session.id) {
                ensure!(
                    session.pending.is_none() && !session.uncertain,
                    "所选会话有待审批或未确认的执行，请先在天工完成处理，再在 IM 发送 /q 释放会话。"
                );
            }
        }
        ensure!(
            sessions
                .current
                .as_ref()
                .is_none_or(|session| !selected.contains(&session.id)),
            "所选会话正在 IM 中使用，请先在对应聊天发送 /q 释放会话，再重试。"
        );
    }
    ensure!(
        !state
            .gmclaw_im
            .desktop_claims
            .lock()
            .await
            .keys()
            .any(|id| selected.contains(id)),
        "所选会话正在 IM 中使用，请先在对应聊天发送 /q 释放会话，再重试。"
    );
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gmclaw_im::tests::{
        approval_fixture_binding, approval_fixture_message, approval_fixture_pending,
        approval_fixture_state,
    };
    use futures_util::FutureExt;

    fn session(id: &str) -> Session {
        let mut session = Session::new(&GmClawBridgeConfig::default(), "fixture-project".into());
        session.id = id.into();
        session
    }

    async fn cache(state: &SharedState, sender: &str) -> Arc<SenderBinding> {
        let binding = approval_fixture_binding(false, None);
        state.gmclaw_im.sessions.lock().await.insert(
            SenderKey::for_message(&approval_fixture_message(sender)),
            binding.clone(),
        );
        binding
    }

    #[tokio::test]
    async fn active_im_reader_rejects_edit_without_running_action() {
        let state = approval_fixture_state();
        let _reader = state.gmclaw_im.history_gate.clone().read_owned().await;
        let called = Arc::new(AtomicBool::new(false));
        let action_called = called.clone();
        let error = edit_history(state, vec!["selected".into()], move || {
            action_called.store(true, Ordering::Release);
            Ok(())
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("/q"));
        assert!(!called.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn history_writer_blocks_new_resume_and_inbound_before_side_effects() {
        let state = approval_fixture_state();
        let _writer = state
            .gmclaw_im
            .history_gate
            .clone()
            .try_write_owned()
            .unwrap();
        let mut message = approval_fixture_message("fixture-sender");
        message.text = "/tg status".into();
        let (outbound, _) = crate::im::core::outbound::channel();
        let mut dispatch = None;
        let mut inbound = Box::pin(handle_inbound(
            &state,
            &outbound,
            &mut message,
            &mut dispatch,
        ));
        assert!(inbound.as_mut().now_or_never().is_none());
        drop(inbound);
        let mut create = Box::pin(sessions::create(
            &state,
            &message,
            crate::remote_control_backend::ThreadStartOptions::default(),
            None,
        ));
        assert!(create.as_mut().now_or_never().is_none());
        drop(create);
        let mut resume = Box::pin(sessions::resume(&state, &message, "fixture-task", None));
        assert!(resume.as_mut().now_or_never().is_none());
        assert!(state.gmclaw_im.sessions.lock().await.is_empty());
        assert!(state.gmclaw_im.seen.lock().await.is_empty());
    }

    #[tokio::test]
    async fn current_claimed_pending_and_uncertain_sessions_cannot_be_edited() {
        for status in ["current", "claimed", "pending", "uncertain", "busy"] {
            let state = approval_fixture_state();
            let binding = cache(&state, "fixture-sender").await;
            let mut selected = session("selected");
            if status == "pending" {
                selected.pending = Some(approval_fixture_pending());
            }
            selected.uncertain = status == "uncertain";
            {
                let mut sessions = binding.sessions.lock().await;
                if status == "current" {
                    // A deselected binding still owns its current session until /q.
                    sessions.current = Some(selected);
                } else {
                    sessions.history.push_back(selected);
                }
            }
            if status == "claimed" {
                state
                    .gmclaw_im
                    .desktop_claims
                    .lock()
                    .await
                    .insert("selected".into(), "fixture-owner".into());
            }
            let _busy = if status == "busy" {
                Some(binding.sessions.lock().await)
            } else {
                None
            };
            let called = Arc::new(AtomicBool::new(false));
            let action_called = called.clone();
            let error = edit_history(state.clone(), vec!["selected".into()], move || {
                action_called.store(true, Ordering::Release);
                Ok(())
            })
            .await
            .unwrap_err();
            assert!(error.to_string().contains("/q"), "{status}: {error}");
            assert!(!called.load(Ordering::Acquire), "{status}");
            drop(_busy);
            let sessions = binding.sessions.lock().await;
            assert!(
                sessions
                    .current
                    .iter()
                    .chain(sessions.history.iter())
                    .any(|session| session.id == "selected"),
                "{status}"
            );
        }
    }

    #[tokio::test]
    async fn successful_edit_discards_only_selected_history_across_senders() {
        let state = approval_fixture_state();
        let first = cache(&state, "fixture-first").await;
        let second = cache(&state, "fixture-second").await;
        for binding in [&first, &second] {
            let mut sessions = binding.sessions.lock().await;
            sessions.history.push_back(session("selected"));
            sessions.history.push_back(session("keep-history"));
            sessions.current = Some(session("keep-current"));
        }
        assert_eq!(
            edit_history(state, vec!["selected".into()], || Ok(17))
                .await
                .unwrap(),
            17
        );
        for binding in [&first, &second] {
            let sessions = binding.sessions.lock().await;
            assert_eq!(sessions.history.len(), 1);
            assert_eq!(sessions.history[0].id, "keep-history");
            assert_eq!(sessions.current.as_ref().unwrap().id, "keep-current");
        }
    }

    #[tokio::test]
    async fn failed_edit_keeps_cached_history() {
        let state = approval_fixture_state();
        let binding = cache(&state, "fixture-sender").await;
        binding
            .sessions
            .lock()
            .await
            .history
            .push_back(session("selected"));
        let error = edit_history::<(), _>(state, vec!["selected".into()], || {
            anyhow::bail!("fixture edit failed")
        })
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "fixture edit failed");
        assert_eq!(binding.sessions.lock().await.history[0].id, "selected");
    }

    #[tokio::test]
    async fn cancelled_waiter_keeps_locks_until_edit_and_cache_invalidation_finish() {
        let state = approval_fixture_state();
        let binding = cache(&state, "fixture-sender").await;
        binding
            .sessions
            .lock()
            .await
            .history
            .push_back(session("selected"));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let caller = tokio::spawn(edit_history(
            state.clone(),
            vec!["selected".into()],
            move || {
                let _ = started_tx.send(());
                finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
        ));
        tokio::time::timeout(Duration::from_secs(5), started_rx)
            .await
            .unwrap()
            .unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert!(state.gmclaw_im.history_gate.try_read().is_err());
        assert!(desktop::LINK_IO.try_lock().is_err());
        finish_tx.send(()).unwrap();
        let _reader = tokio::time::timeout(
            Duration::from_secs(5),
            state.gmclaw_im.history_gate.clone().read_owned(),
        )
        .await
        .unwrap();
        assert!(binding.sessions.lock().await.history.is_empty());
    }
}
