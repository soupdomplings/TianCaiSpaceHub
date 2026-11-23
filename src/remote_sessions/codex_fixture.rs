//! In-process desktop for exercising the native request/response contract.
use super::{
    codex,
    types::{CreateRequest, PageQuery, Scope, TurnRequest},
};
use crate::{
    app_state::{RemoteControlClientState, SharedState},
    remote_control_backend::OutboundWsMessage,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;

pub(crate) struct Desktop {
    pub state: SharedState,
    rx: mpsc::UnboundedReceiver<OutboundWsMessage>,
}
impl Desktop {
    pub async fn connect(state: &SharedState) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut r = state.remote_control.inner.lock().await;
        r.connected = true;
        r.initialized = true;
        r.connection_epoch = 7;
        r.client_id = "app".into();
        r.stream_id = "stream".into();
        r.outbound_tx = Some(tx);
        r.clients.insert(
            "default".into(),
            RemoteControlClientState {
                client_id: "app".into(),
                stream_id: "stream".into(),
                initialized: true,
                next_seq_id: 1,
                pending: Default::default(),
                current_thread_id: None,
                current_turn_id: None,
                last_app_ping_at_ms: None,
                last_app_pong_at_ms: None,
                last_app_pong_status: None,
                last_initialize_sent_at_ms: None,
                recovery_attempt: 0,
                recovery_started_at_ms: None,
            },
        );
        drop(r);
        Self {
            state: state.clone(),
            rx,
        }
    }
    pub async fn reply(&mut self, method: &str, f: impl FnOnce(&Value) -> anyhow::Result<Value>) {
        let OutboundWsMessage::Text(wire) = self.rx.recv().await.unwrap() else {
            panic!("expected native RPC")
        };
        let msg = &wire["message"];
        assert_eq!(msg["method"], method);
        let mut runtime = self.state.remote_control.inner.lock().await;
        let pending = runtime
            .clients
            .get_mut("default")
            .unwrap()
            .pending
            .remove(&msg["id"].to_string())
            .unwrap();
        pending.response_tx.send(f(&msg["params"])).unwrap();
    }
}
fn turn() -> TurnRequest {
    serde_json::from_value(json!({"requestId":"one","text":"hello"})).unwrap()
}

#[tokio::test]
async fn submitted_form_goes_back_to_original_stream_and_request_with_typed_answer() {
    use super::service::ApprovalSource;
    let (_dir, state) = super::test_state();
    let mut desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "local-thread");
    let params = json!({"threadId":"local-thread","turnId":null,"serverName":"Fixture","mode":"form","message":"Continue?",
        "requestedSchema":{"type":"object","required":["allow"],"properties":{"allow":{"type":"boolean"}}}});
    let event = json!({"id":"42","method":"mcpServer/elicitation/request","params":params});
    codex::observe(&state, 7, "app", "stream", &event).await;
    let target = codex::target(&state, &scope.client).await.unwrap();
    state.remote_sessions.watch(&scope, Some(target)).await;
    let (_, _, cards, _) = state.remote_sessions.snapshot(&scope).await;
    let id = cards[0]["id"].as_str().unwrap();
    assert_eq!(cards[0]["questions"][0]["id"], "allow");
    // A resolved request with the number 42 must not clear the string "42".
    codex::observe(&state,7,"app","stream",&json!({"method":"serverRequest/resolved","params":{"threadId":"local-thread","requestId":42}})).await;
    let pending = state
        .remote_sessions
        .claim_approval(&scope, id)
        .await
        .unwrap();
    let ApprovalSource::Codex {
        target,
        request_id,
        method,
        params,
    } = pending.source
    else {
        panic!("wrong adapter")
    };
    let decision =
        serde_json::from_value(json!({"decision":"accept","answers":{"allow":"false"}})).unwrap();
    codex::approve(
        &state,
        &scope,
        id,
        &decision,
        &target,
        &request_id,
        &method,
        &params,
    )
    .await
    .unwrap();
    let OutboundWsMessage::Text(wire) = desktop.rx.try_recv().unwrap() else {
        panic!("missing native reply")
    };
    assert_eq!(wire["message"]["id"], "42");
    assert_eq!(
        wire["message"]["result"],
        json!({"action":"accept","content":{"allow":false}})
    );
    assert!(
        state
            .remote_sessions
            .claim_approval(&scope, id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_wait_without_actionable_id_stays_visible_and_commentary_is_not_final_text() {
    let (_dir, state) = super::test_state();
    let _desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "local-thread");
    state
        .remote_sessions
        .watch(
            &scope,
            Some(codex::target(&state, &scope.client).await.unwrap()),
        )
        .await;
    let event = |method: &str, p: Value| {
        let mut p = p;
        p["threadId"] = "local-thread".into();
        json!({"method":method,"params":p})
    };
    for (method, p) in [
        (
            "thread/status/changed",
            json!({"status":{"type":"active","activeFlags":["waitingOnUserInput"]}}),
        ),
        (
            "item/started",
            json!({"item":{"id":"progress","type":"agentMessage","phase":"commentary","text":"progress"}}),
        ),
        (
            "item/agentMessage/delta",
            json!({"itemId":"progress","delta":"more"}),
        ),
        (
            "item/completed",
            json!({"item":{"id":"progress","type":"agentMessage","phase":"commentary","text":"progress more"}}),
        ),
        (
            "item/completed",
            json!({"item":{"id":"answer","type":"agentMessage","phase":"partial_answer","text":"real answer"}}),
        ),
        (
            "item/completed",
            json!({"item":{"id":"hidden","type":"hookPrompt","text":"not visible"}}),
        ),
        (
            "item/completed",
            json!({"item":{"id":"user","type":"userMessage","content":[{"type":"text","text":"normal commentary text"}]}}),
        ),
    ] {
        codex::observe(&state, 7, "app", "stream", &event(method, p)).await;
    }
    let (status, _, cards, _) = state.remote_sessions.snapshot(&scope).await;
    assert_eq!(status, "waiting_approval");
    assert!(cards.is_empty());
    let feeds = state.remote_sessions.feeds.lock().await;
    let events = &feeds[&scope].events;
    assert!(
        events
            .iter()
            .filter(|e| e.item_id.as_deref() == Some("progress"))
            .all(|e| e.data.pointer("/item/type") == Some(&json!("progress"))
                || e.data["type"] == "progress")
    );
    assert!(
        events
            .iter()
            .any(|e| e.data.pointer("/item/text") == Some(&json!("real answer")))
    );
    assert!(
        events
            .iter()
            .any(|e| e.data.pointer("/item/text") == Some(&json!("normal commentary text")))
    );
    assert!(
        events
            .iter()
            .any(|e| e.data.pointer("/item/type") == Some(&json!("internal"))
                && e.data.pointer("/item/text") == Some(&json!("")))
    );
}

#[tokio::test]
async fn client_summary_does_not_query_models_until_a_specific_client_requests_them() {
    use axum::{
        Json,
        extract::{Path, Query, State},
    };
    let (_dir, state) = super::test_state();
    state.config.lock().await.gmclaw_bridge.enabled = false;
    let mut desktop = Desktop::connect(&state).await;
    let Json(summary) = super::clients(
        State(state.clone()),
        Query(super::ClientQuery {
            view: Some("summary".into()),
        }),
    )
    .await;
    assert_eq!(summary["clients"][0]["id"], "codex:default");
    assert_eq!(summary["clients"][0]["ready"], true);
    assert_eq!(summary["clients"][0]["modelsDeferred"], true);
    assert!(
        desktop.rx.try_recv().is_err(),
        "summary must not call the host model catalog"
    );
    let native = desktop.reply("model/list", |_|Ok(json!({"data":[{"id":"model-a","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]})));
    let read = super::client_models(State(state), Path("codex:default".to_string()));
    let (_, response) = run(native, read).await;
    let models = response.unwrap().0;
    assert_eq!(models["models"][0]["id"], "model-a");
    assert_eq!(models["modelSelection"], true);
    assert_eq!(models["effortSelection"], true);
}
async fn run<A: std::future::Future, B: std::future::Future>(a: A, b: B) -> (A::Output, B::Output) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(a, b)
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn fresh_session_refresh_and_first_send_use_the_confirmed_thread_without_rollout() {
    let (dir, state) = super::test_state();
    let mut desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "fresh");
    let cwd = dir.path().to_str().unwrap();
    let target = codex::target(&state, &scope.client).await.unwrap();
    state
        .remote_sessions
        .listed_workspaces
        .lock()
        .await
        .remember(&target, std::iter::once(cwd));
    let native = async {
        desktop
            .reply("thread/start", |p| {
                assert_eq!(p["cwd"], cwd);
                Ok(json!({"thread":{"id":"fresh","cwd":cwd}}))
            })
            .await;
        desktop
            .reply("turn/start", |p| {
                assert_eq!(p["threadId"], "fresh");
                Ok(json!({"turn":{"id":"first"}}))
            })
            .await;
        desktop
            .reply("thread/turns/list", |_| {
                Ok(json!({"data":[{"id":"first","status":"completed","items":[
            {"id":"msg","type":"agentMessage","text":"received"}]}]}))
            })
            .await;
    };
    let browser = async {
        let options = CreateRequest {
            cwd: Some(cwd.to_string()),
            ..Default::default()
        };
        assert_eq!(
            codex::create(&state, &scope.client, &options)
                .await
                .unwrap()["session"]["id"],
            "fresh"
        );
        for _ in 0..2 {
            let page = codex::history(&state, &scope, &PageQuery::default())
                .await
                .unwrap();
            assert_eq!(page["items"], json!([]));
            codex::subscribe(&state, &scope, &target).await.unwrap();
        }
        assert_eq!(
            codex::send(&state, &scope, &turn()).await.unwrap()["turnId"],
            "first"
        );
        assert_eq!(
            codex::history(&state, &scope, &PageQuery::default())
                .await
                .unwrap()["items"][0]["text"],
            "received"
        );
    };
    run(native, browser).await;
}

#[tokio::test]
async fn provider_mismatch_blocks_continuation_but_stored_history_and_legacy_pages_are_readable() {
    let (_dir, state) = super::test_state();
    let mut desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "stored");
    let native = async {
        for index in 0..2 {
            desktop
                .reply("thread/turns/list", |p| {
                    assert_eq!(p["limit"], 1);
                    assert_eq!(p["sortDirection"], "desc");
                    anyhow::bail!("method not found (-32601)")
                })
                .await;
            desktop
                .reply("thread/read", |p| {
                    assert_eq!(p["includeTurns"], true);
                    let mut turns = vec![
                      json!({"id":"t1","items":[{"id":"m1","type":"agentMessage","text":"older reply"}]}),
                      json!({"id":"t2","items":[{"id":"m2","type":"agentMessage","text":"last reply"}]})];
                    if index == 1 {
                        turns.push(json!({"id":"t3","items":[{"id":"m3","type":"agentMessage","text":"arrived while scrolling"}]}));
                    }
                    Ok(json!({"thread":{"id":"stored","turns":turns}}))
                })
                .await;
            if index == 0 {
                desktop
                    .reply("thread/read", |_| {
                        Ok(json!({"thread":{"id":"stored","status":"idle"}}))
                    })
                    .await;
                desktop
                    .reply("thread/resume", |_| {
                        anyhow::bail!("remote-control request failed: modelProviderMismatch")
                    })
                    .await;
            }
        }
        desktop
            .reply("thread/turns/list", |_| anyhow::bail!("method not found"))
            .await;
        desktop
            .reply("thread/read", |_| {
                Ok(json!({"thread":{"id":"another-thread","turns":[]}}))
            })
            .await;
    };
    let browser = async {
        let mut page = PageQuery {
            limit: Some(1),
            cursor: None,
        };
        let first = codex::history(&state, &scope, &page).await.unwrap();
        assert_eq!(first["items"][0]["text"], "last reply");
        assert!(
            codex::send(&state, &scope, &turn())
                .await
                .unwrap_err()
                .to_string()
                .contains("modelProviderMismatch")
        );
        page.cursor = Some(first["nextCursor"].as_str().unwrap().to_owned());
        assert_eq!(
            codex::history(&state, &scope, &page).await.unwrap()["items"][0]["text"],
            "older reply"
        );
        assert!(
            codex::history(&state, &scope, &PageQuery::default())
                .await
                .unwrap_err()
                .to_string()
                .contains("其他会话")
        );
    };
    run(native, browser).await;
}

#[tokio::test]
async fn desktop_text_reads_annotations_as_dialogue_in_history_and_live_events() {
    let (_dir, state) = super::test_state();
    let mut desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "annotated-thread");
    let target = codex::target(&state, &scope.client).await.unwrap();
    state.remote_sessions.watch(&scope, Some(target)).await;
    let item = json!({"type":"userMessage","id":"annotated","content":[
        {"type":"text","text":"# Response annotations:\nEach item contains text selected from an earlier Codex response.\n<response-annotations>\n[{\"text\":\"开关\",\"source\":{\"messageId\":\"fixture\",\"startOffset\":16,\"endOffset\":18}}]\n</response-annotations>"},
        {"type":"text","text":"## My request:\n加一项产物。"}]});
    let native = async {
        desktop
            .reply("thread/turns/list", |_| {
                Ok(json!({"data":[{"id":"turn","threadId":scope.session,
           "status":"completed","items":[item]}]}))
            })
            .await;
    };
    let browser = async {
        let history = codex::history(&state, &scope, &PageQuery::default())
            .await
            .unwrap();
        let text = &history["items"][0]["text"];
        assert_eq!(text, "引用「开关」\n\n加一项产物。");
        codex::observe(
            &state,
            7,
            "app",
            "stream",
            &json!({"method":"item/completed",
            "params":{"threadId":scope.session,"item":item}}),
        )
        .await;
        let feeds = state.remote_sessions.feeds.lock().await;
        assert_eq!(
            feeds[&scope].events.back().unwrap().data["item"]["text"],
            *text
        );
    };
    run(native, browser).await;
}

#[tokio::test]
async fn modern_pages_use_one_rpc_and_reject_another_threads_turns() {
    let (_dir, state) = super::test_state();
    let mut desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "desktop-original");
    let native = async {
        for (index, thread_id) in ["desktop-original", "desktop-original", "different"]
            .into_iter()
            .enumerate()
        {
            desktop.reply("thread/turns/list", |p| {
                assert_eq!(p["threadId"],"desktop-original");
                assert_eq!(p["cursor"].as_str(), (index == 1).then_some("older"));
                Ok(json!({"nextCursor":"older","data":[{"id":"t1","threadId":thread_id,"status":"completed",
                   "items":[{"id":"m1","type":"agentMessage","text":"Visible in desktop"}]}]}))
            }).await;
        }
    };
    let browser = async {
        let history = codex::history(&state, &scope, &PageQuery::default())
            .await
            .unwrap();
        assert_eq!(history["items"][0]["text"], "Visible in desktop");
        assert_eq!(history["nextCursor"], "older");
        let older = PageQuery {
            cursor: Some("older".into()),
            ..Default::default()
        };
        assert_eq!(
            codex::history(&state, &scope, &older).await.unwrap()["items"][0]["text"],
            "Visible in desktop"
        );
        assert!(
            codex::history(&state, &scope, &PageQuery::default())
                .await
                .unwrap_err()
                .to_string()
                .contains("其他会话")
        );
    };
    run(native, browser).await;
}

#[tokio::test]
async fn missing_rollout_after_disconnect_external_turn_or_unknown_receipt_is_not_empty_history() {
    let (_dir, state) = super::test_state();
    let mut desktop = Desktop::connect(&state).await;
    let scope = Scope::new("codex:default", "missing");
    let target = codex::target(&state, &scope.client).await.unwrap();
    let native = async {
        for attempt in 0..4 {
            if attempt == 3 {
                desktop
                    .reply("turn/start", |_| anyhow::bail!("native reply lost"))
                    .await;
            }
            desktop
                .reply("thread/turns/list", |_| {
                    if attempt == 2 {
                        anyhow::bail!("no rollout found for thread");
                    }
                    Ok(json!({"data":[],"nextCursor":null}))
                })
                .await;
            if attempt != 2 {
                desktop
                    .reply("thread/read", |_| {
                        anyhow::bail!("no rollout found for thread")
                    })
                    .await;
            }
        }
    };
    let browser = async {
        let read_missing = || async {
            let error = codex::history(&state, &scope, &PageQuery::default())
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("no rollout") && error.contains("打开同一条会话"));
        };
        read_missing().await; // An unknown session never becomes a fresh thread.
        {
            let mut feeds = state.remote_sessions.feeds.lock().await;
            let feed = feeds.get_mut(&scope).unwrap();
            feed.awaiting_first_turn = true;
            feed.source.as_mut().unwrap().epoch -= 1;
        }
        read_missing().await; // Previous connection's empty marker is stale.
        {
            state
                .remote_sessions
                .feeds
                .lock()
                .await
                .get_mut(&scope)
                .unwrap()
                .awaiting_first_turn = true;
        }
        codex::observe(&state,target.epoch,&target.client_id,&target.stream_id,
            &json!({"method":"turn/started","params":{"threadId":"missing","turn":{"id":"desktop-turn"}}})).await;
        read_missing().await;
        {
            let mut feeds = state.remote_sessions.feeds.lock().await;
            let feed = feeds.get_mut(&scope).unwrap();
            feed.awaiting_first_turn = true;
            feed.subscribed = true;
        }
        assert!(
            codex::send(&state, &scope, &turn())
                .await
                .unwrap_err()
                .downcast_ref::<super::UnconfirmedWrite>()
                .is_some()
        );
        read_missing().await;
    };
    run(native, browser).await;
}
