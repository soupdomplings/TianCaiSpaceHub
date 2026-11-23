//! Dedicated browser session API. This router is hosted only by the paired
//! remote gateway; it must not be merged into the public/local management API.
mod codex;
mod desktop_text;
#[cfg(test)]
pub(crate) mod codex_fixture;
mod files;
#[path = "../remote_network/mobile/download.rs"]
mod mobile_file;
pub(crate) use mobile_file::read as mobile_download;
mod service;
mod tiangong;
mod transcript;
mod types;

use crate::app_state::SharedState;
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use futures_util::stream;
use serde::Deserialize;
use serde_json::{Value, json};
pub(crate) use service::SessionService;
use std::{collections::VecDeque, convert::Infallible, time::Duration};
use tokio::io::AsyncReadExt;
use types::{
    ApprovalDecision, CreateRequest, PageQuery, RemoteEvent, Scope, StopRequest, TurnRequest,
};

pub(crate) fn router() -> Router<SharedState> {
    Router::new()
        .route("/clients", get(clients))
        .route("/clients/{client}/models", get(client_models))
        .route("/clients/{client}/projects", get(projects))
        .route("/clients/{client}/sessions", get(sessions).post(create))
        .route("/clients/{client}/sessions/{session}/history", get(history))
        .route("/clients/{client}/sessions/{session}/turns", post(send))
        .route("/clients/{client}/sessions/{session}/stop", post(stop))
        .route(
            "/clients/{client}/sessions/{session}/approvals/{approval}",
            post(approve),
        )
        .route("/clients/{client}/sessions/{session}/events", get(events))
        .route(
            "/clients/{client}/sessions/{session}/files",
            post(upload).layer(DefaultBodyLimit::max(files::MAX_UPLOAD)),
        )
        .route(
            "/clients/{client}/sessions/{session}/files/{file}",
            get(download).delete(remove_upload),
        )
}
#[derive(Debug)]
pub(super) struct UnconfirmedWrite;
impl std::fmt::Display for UnconfirmedWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("原生操作结果未确认，请刷新核对；不会自动重发")
    }
}
impl std::error::Error for UnconfirmedWrite {}
#[derive(Debug)]
pub(super) struct NativeRejected(pub String);
impl std::fmt::Display for NativeRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for NativeRejected {}
pub(super) fn unconfirmed(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<NativeRejected>().is_some()
        || error
            .to_string()
            .starts_with("remote-control request failed:")
    {
        error
    } else {
        error.context(UnconfirmedWrite)
    }
}

#[derive(Debug)]
struct ApiError(anyhow::Error);
impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self(value)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::CONFLICT,Json(json!({"error":{"code":if self.0.downcast_ref::<UnconfirmedWrite>().is_some() {"native_result_unknown"} else {"remote_request_failed"},
            "message":self.0.to_string()}}))).into_response()
    }
}
type ApiResult<T> = std::result::Result<T, ApiError>;
fn valid_client(client: &str) -> Result<()> {
    ensure!(
        client == "tiangong" || client.starts_with("codex:"),
        "此客户端未接入远程会话"
    );
    Ok(())
}
#[derive(Deserialize, Default)]
struct ClientQuery {
    view: Option<String>,
}
async fn clients(State(state): State<SharedState>, Query(view): Query<ClientQuery>) -> Json<Value> {
    let summary = view.view.as_deref() == Some("summary");
    let (mut codex, tiangong) = tokio::join!(
        codex::clients(&state, summary),
        tiangong::client(&state, summary)
    );
    codex.push(tiangong);
    for client in &mut codex {
        client["capabilities"]["liveActivity"] = true.into();
    }
    Json(json!({"clients":codex}))
}
async fn client_models(
    State(state): State<SharedState>,
    Path(client): Path<String>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    let models = if client == "tiangong" {
        tiangong::models_for_client(&state).await?
    } else {
        codex::models(&state, &codex::target(&state, &client).await?).await?
    };
    let switch = client != "tiangong" && !models.is_empty();
    let effort = switch
        && models
            .iter()
            .any(|m| m["efforts"].as_array().is_some_and(|v| !v.is_empty()));
    Ok(Json(
        json!({"models":models,"modelSelection":switch,"effortSelection":effort}),
    ))
}
async fn projects(
    State(state): State<SharedState>,
    Path(client): Path<String>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    Ok(Json(if client == "tiangong" {
        tiangong::projects(&state, &page).await?
    } else {
        codex::projects(&state, &client, &page).await?
    }))
}
async fn sessions(
    State(state): State<SharedState>,
    Path(client): Path<String>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    Ok(Json(if client == "tiangong" {
        tiangong::sessions(&state, &page).await?
    } else {
        codex::sessions(&state, &client, &page).await?
    }))
}
async fn create(
    State(state): State<SharedState>,
    Path(client): Path<String>,
    Json(options): Json<CreateRequest>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    Ok(Json(if client == "tiangong" {
        tiangong::create(&state, &options).await?
    } else {
        codex::create(&state, &client, &options).await?
    }))
}
async fn history(
    State(state): State<SharedState>,
    Path((client, session)): Path<(String, String)>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    let scope = Scope::new(client, session);
    let mut snapshot = if scope.client == "tiangong" {
        tiangong::history(&state, &scope, &page).await?
    } else {
        codex::history(&state, &scope, &page).await?
    };
    transcript::history(&mut snapshot);
    snapshot["displayContent"] = json!(crate::remote_network::display_content(&state).await?);
    Ok(Json(snapshot))
}
async fn send(
    State(state): State<SharedState>,
    Path((client, session)): Path<(String, String)>,
    Json(input): Json<TurnRequest>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    ensure_input(&input)?;
    let scope = Scope::new(client, session);
    if scope.client != "tiangong" {
        // Replace stale connection state before reserving this write. A new
        // native source must not clear an already admitted starting state.
        let target = codex::target(&state, &scope.client).await?;
        state.remote_sessions.watch(&scope, Some(target)).await;
    }
    if let Some(receipt) = state
        .remote_sessions
        .reserve(&scope, &input.request_id)
        .await?
    {
        return Ok(Json(receipt));
    }
    let (reply, response) = tokio::sync::oneshot::channel();
    // Admission, native writes and the receipt have a Hub-owned lifetime.
    // Dropping an HTTP future only drops its receiver, never this worker.
    tokio::spawn(async move {
        let result = if scope.client == "tiangong" {
            tiangong::send(&state, &scope, &input).await
        } else {
            codex::send(&state, &scope, &input).await
        };
        match &result {
            Ok(response) => {
                state
                    .remote_sessions
                    .receipt(&scope, &input.request_id, response.clone())
                    .await
            }
            Err(error) => {
                let uncertain = error.downcast_ref::<UnconfirmedWrite>().is_some();
                if !uncertain {
                    let mut feeds = state.remote_sessions.feeds.lock().await;
                    if let Some(feed) = feeds.get_mut(&scope) {
                        feed.requests.retain(|(id, _)| id != &input.request_id);
                    }
                }
                state
                    .remote_sessions
                    .publish(
                        &scope,
                        "turn.updated",
                        None,
                        json!({"status":if uncertain {"unknown"} else {"idle"}}),
                    )
                    .await;
            }
        }
        let _ = reply.send(result);
    });
    Ok(Json(
        response
            .await
            .context("请求连接已中断，请刷新核对原任务状态")
            .map_err(unconfirmed)??,
    ))
}
fn ensure_input(input: &TurnRequest) -> Result<()> {
    ensure!(
        !input.text.trim().is_empty() || !input.attachment_ids.is_empty(),
        "请输入消息或添加附件"
    );
    ensure!(input.text.len() <= 1024 * 1024, "消息太长，请改用文件附件");
    ensure!(input.attachment_ids.len() <= 20, "每轮最多添加20个附件");
    Ok(())
}
async fn stop(
    State(state): State<SharedState>,
    Path((client, session)): Path<(String, String)>,
    Json(input): Json<StopRequest>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    let scope = Scope::new(client, session);
    Ok(Json(if scope.client == "tiangong" {
        tiangong::stop(&state, &scope, input.turn_id.as_deref()).await?
    } else {
        codex::stop(&state, &scope, input.turn_id.as_deref()).await?
    }))
}
async fn approve(
    State(state): State<SharedState>,
    Path((client, session, id)): Path<(String, String, String)>,
    Json(decision): Json<ApprovalDecision>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    let scope = Scope::new(client, session);
    let pending = state.remote_sessions.claim_approval(&scope, &id).await?;
    let (reply, response) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = match pending.source {
            service::ApprovalSource::Codex {
                target,
                request_id,
                method,
                params,
            } => {
                codex::approve(
                    &state,
                    &scope,
                    &id,
                    &decision,
                    &target,
                    &request_id,
                    &method,
                    &params,
                )
                .await
            }
            service::ApprovalSource::Tiangong {
                run_id,
                call_ids,
                authorization_identity,
            } => {
                tiangong::approve(
                    &state,
                    &scope,
                    &id,
                    &decision,
                    &run_id,
                    &call_ids,
                    &authorization_identity,
                )
                .await
            }
        };
        if let Err(error) = &result {
            // Validation failures may be corrected; an unconfirmed native
            // write stays claimed and cannot execute a tool decision twice.
            if error.downcast_ref::<UnconfirmedWrite>().is_none() {
                if let Some(pending) = state
                    .remote_sessions
                    .feeds
                    .lock()
                    .await
                    .get_mut(&scope)
                    .and_then(|f| f.approvals.get_mut(&id))
                {
                    pending.claimed = false;
                }
            }
        }
        let _ = reply.send(result);
    });
    response
        .await
        .context("确认提交连接已中断，请刷新原任务状态")
        .map_err(unconfirmed)??;
    Ok(Json(json!({"status":"submitted"})))
}
#[derive(Default, Deserialize)]
struct EventQuery {
    after: Option<u64>,
    #[serde(rename = "liveActivity")]
    live_activity: Option<u8>,
}
async fn events(
    State(state): State<SharedState>,
    Path((client, session)): Path<(String, String)>,
    Query(query): Query<EventQuery>,
) -> ApiResult<Response> {
    valid_client(&client)?;
    let scope = Scope::new(client, session);
    crate::remote_network::display_content(&state).await?;
    if scope.client == "tiangong" {
        tiangong::history(
            &state,
            &scope,
            &PageQuery {
                cursor: None,
                limit: Some(1),
            },
        )
        .await?;
    } else {
        let target = codex::target(&state, &scope.client).await?;
        state
            .remote_sessions
            .watch(&scope, Some(target.clone()))
            .await;
        codex::subscribe(&state, &scope, &target).await?;
    }
    let after = query.after.unwrap_or(0);
    let wants_activity = query.live_activity == Some(1);
    let (mut replay, receiver) = {
        let mut feeds = state.remote_sessions.feeds.lock().await;
        let feed = feeds.entry(scope.clone()).or_default();
        let receiver = feed.tx.subscribe();
        let replay = feed
            .events
            .iter()
            .filter(|e| e.id > after)
            .cloned()
            .collect::<VecDeque<_>>();
        let truncated = after > 0
            && (feed
                .events
                .front()
                .is_some_and(|e| after < e.id.saturating_sub(1))
                || feed.events.back().is_none_or(|e| after > e.id));
        let replay = if truncated {
            VecDeque::from([RemoteEvent {
                id: after.saturating_add(1),
                kind: "reset".into(),
                client_id: scope.client.clone(),
                session_id: scope.session.clone(),
                turn_id: feed.turn_id.clone(),
                item_id: None,
                data: json!({"reason":"event_window_expired"}),
            }])
        } else {
            replay
        };
        (replay, receiver)
    };
    // A fresh empty stream needs no synthetic text or task. Keepalive frames
    // maintain the connection while the native runtime is waiting for input.
    let stream = stream::unfold(
        (std::mem::take(&mut replay), receiver, scope, after, state),
        move |(mut replay, mut receiver, scope, last_id, state)| async move {
            let live = replay.is_empty();
            let event = if let Some(event) = replay.pop_front() {
                event
            } else {
                match receiver.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => RemoteEvent {
                        id: last_id.saturating_add(1),
                        kind: "reset".into(),
                        client_id: scope.client.clone(),
                        session_id: scope.session.clone(),
                        turn_id: None,
                        item_id: None,
                        data: json!({"reason":"slow_reader"}),
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            };
            let choices = state.remote_network.display_content().await;
            let event = transcript::event_with_display(event, live && wants_activity, choices);
            let wire = Event::default()
                .event("message")
                .id(event.id.to_string())
                .data(serde_json::to_string(&event).unwrap_or_default());
            Some((
                Ok::<_, Infallible>(wire),
                (replay, receiver, scope, event.id.max(last_id), state),
            ))
        },
    );
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}
async fn ensure_session(state: &SharedState, scope: &Scope) -> Result<()> {
    if scope.client == "tiangong" {
        tiangong::history(
            state,
            scope,
            &PageQuery {
                cursor: None,
                limit: Some(1),
            },
        )
        .await?;
    } else {
        codex::history(
            state,
            scope,
            &PageQuery {
                cursor: None,
                limit: Some(1),
            },
        )
        .await?;
    }
    Ok(())
}
async fn upload(
    State(state): State<SharedState>,
    Path((client, session)): Path<(String, String)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    let scope = Scope::new(client, session);
    ensure_session(&state, &scope).await?;
    Ok(Json(
        json!({"file":files::upload(&state,&scope,headers,bytes).await?}),
    ))
}
async fn remove_upload(
    State(state): State<SharedState>,
    Path((client, session, file)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    valid_client(&client)?;
    files::remove_pending(&state, &Scope::new(client, session), &file).await?;
    Ok(Json(json!({"removed":true})))
}
#[derive(Default, Deserialize)]
struct DownloadQuery {
    download: Option<String>,
}
async fn download(
    State(state): State<SharedState>,
    Path((client, session, file)): Path<(String, String, String)>,
    Query(query): Query<DownloadQuery>,
) -> ApiResult<Response> {
    valid_client(&client)?;
    let (stored, file) = files::open_registered(&state, &Scope::new(client, session), &file).await?;
    let stream = stream::try_unfold(file, |mut file| async move {
        let mut bytes = vec![0; 64 * 1024];
        let count = file.read(&mut bytes).await?;
        if count == 0 {
            Ok::<_, std::io::Error>(None)
        } else {
            bytes.truncate(count);
            Ok(Some((Bytes::from(bytes), file)))
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    let mime = stored.public.mime_type.as_str();
    let passive = (mime.starts_with("image/") && mime != "image/svg+xml")
        || matches!(mime, "application/pdf" | "text/plain");
    let disposition = if passive && query.download.as_deref() != Some("1") {
        "inline"
    } else {
        "attachment"
    };
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{disposition}; filename=\"download\"; filename*=UTF-8''{}",
            types::encode(&stored.public.name)
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

pub(crate) async fn observe_codex(
    state: &SharedState,
    epoch: u64,
    client_id: &str,
    stream_id: &str,
    message: &Value,
) {
    codex::observe(state, epoch, client_id, stream_id, message).await;
}
pub(crate) async fn ensure_im_admission(
    state: &SharedState,
    client_key: &str,
    session: &str,
) -> Result<()> {
    let active = codex::targets(state).await;
    let feeds = state.remote_sessions.feeds.lock().await;
    ensure!(
        !feeds.iter().any(|(scope, feed)| scope.session == session
            && feed
                .source
                .as_ref()
                .is_some_and(|source| source.key == client_key && active.contains(source))
            && types::busy(&feed.status)),
        "此会话正在网页端执行，请先完成或停止当前任务"
    );
    Ok(())
}

#[cfg(test)]
fn test_state() -> (tempfile::TempDir, SharedState) {
    let directory = tempfile::tempdir().unwrap();
    let mut config = crate::config::AppConfig::default();
    config.state_path = directory.path().join("state.json");
    let state =
        crate::app_state::AppState::new(directory.path().join("config.toml"), config, None, None);
    (directory, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn display_content_changes_apply_to_the_existing_event_stream() {
        use futures_util::StreamExt;
        use tower::ServiceExt;
        let (_directory, state) = test_state();
        let _desktop = codex_fixture::Desktop::connect(&state).await;
        let scope = Scope::new("codex:default", "display-fixture");
        let target = codex::target(&state, &scope.client).await.unwrap();
        state.remote_sessions.watch(&scope, Some(target)).await;
        state.remote_sessions.feeds.lock().await.get_mut(&scope).unwrap().subscribed = true;
        let response = events(
            State(state.clone()),
            Path((scope.client.clone(), scope.session.clone())),
            Query(EventQuery { after:Some(0),live_activity:Some(1) }),
        )
        .await.unwrap();
        let mut body = response.into_body().into_data_stream();
        for enabled in [false, true, false] {
            let result = crate::remote_network::management_router()
                .with_state(state.clone())
                .oneshot(axum::http::Request::post("/api/remote-network/display-content")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"tool":enabled}).to_string())).unwrap())
                .await.unwrap();
            assert_eq!(result.status(), StatusCode::OK);
            state.remote_sessions.publish(&scope,"message.delta",Some("tool".into()),json!({"type":"tool","text":"fresh"})).await;
            let wire = tokio::time::timeout(Duration::from_secs(2), body.next()).await.unwrap().unwrap().unwrap();
            let wire = std::str::from_utf8(&wire).unwrap();
            let data = wire.lines().find_map(|line|line.strip_prefix("data: ")).unwrap();
            let event: Value = serde_json::from_str(data).unwrap();
            assert_eq!(event["data"]["displayContent"]["tool"], enabled);
            assert_eq!(event["data"].get("liveActivity").is_some(), enabled);
            assert_eq!(event["sessionId"], scope.session);
        }
    }
    #[tokio::test]
    async fn admission_remembers_ambiguous_write_without_replaying() {
        let service = SessionService::default();
        let scope = Scope::new("codex:fixture", "session");
        assert!(
            service
                .reserve(&scope, "request-a")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            service.reserve(&scope, "request-a").await.unwrap().unwrap()["status"],
            "unknown"
        );
        assert!(service.reserve(&scope, "request-b").await.is_err());
        service
            .publish(&scope, "turn.updated", None, json!({"status":"completed"}))
            .await;
        assert!(
            service
                .reserve(&scope, "request-b")
                .await
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn empty_or_oversized_input_never_reaches_native_admission() {
        let mut input = TurnRequest {
            text: String::new(),
            attachment_ids: Vec::new(),
            model: None,
            effort: None,
            request_id: "fixture".into(),
        };
        assert!(ensure_input(&input).is_err());
        input.text = "hello".into();
        assert!(ensure_input(&input).is_ok());
        input.attachment_ids = vec!["file".into(); 21];
        assert!(ensure_input(&input).is_err());
    }
    #[tokio::test]
    async fn native_source_and_thread_scope_keep_events_separate() {
        let (_directory, state) = test_state();
        let source = super::codex::CodexTarget {
            id: "codex:fixture".into(),
            epoch: 7,
            key: "fixture".into(),
            client_id: "client".into(),
            stream_id: "stream".into(),
            name: "fixture".into(),
        };
        let first = Scope::new("codex:fixture", "first");
        let second = Scope::new("codex:fixture", "second");
        state
            .remote_sessions
            .watch(&first, Some(source.clone()))
            .await;
        state.remote_sessions.watch(&second, Some(source)).await;
        let event = json!({"method":"item/agentMessage/delta","params":{"threadId":"first","itemId":"item","delta":"hello"}});
        observe_codex(&state, 8, "client", "stream", &event).await;
        assert_eq!(state.remote_sessions.snapshot(&first).await.3, 0);
        observe_codex(&state, 7, "client", "stream", &event).await;
        assert!(state.remote_sessions.snapshot(&first).await.3 > 0);
        assert_eq!(state.remote_sessions.snapshot(&second).await.3, 0);
        let replacement = super::codex::CodexTarget {
            epoch: 8,
            ..state.remote_sessions.feeds.lock().await[&first]
                .source
                .clone()
                .unwrap()
        };
        state.remote_sessions.watch(&first, Some(replacement)).await;
        assert_eq!(state.remote_sessions.snapshot(&first).await.3, 0);
        observe_codex(&state, 7, "client", "stream", &event).await;
        assert_eq!(state.remote_sessions.snapshot(&first).await.3, 0);
    }
    #[tokio::test]
    async fn native_approval_before_page_open_is_recoverable_and_claimed_once() {
        let (_directory, state) = test_state();
        {
            let mut remote = state.remote_control.inner.lock().await;
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            remote.connected = true;
            remote.initialized = true;
            remote.connection_epoch = 7;
            remote.client_id = "client".into();
            remote.stream_id = "stream".into();
            remote.outbound_tx = Some(tx);
        }
        let event = json!({"id":42,"method":"item/commandExecution/requestApproval","params":{"threadId":"thread","command":"echo fixture"}});
        observe_codex(&state, 7, "client", "stream", &event).await;
        assert!(state.remote_sessions.feeds.lock().await.is_empty());
        let target = codex::target(&state, "codex:default").await.unwrap();
        let scope = Scope::new("codex:default", "thread");
        state.remote_sessions.watch(&scope, Some(target)).await;
        let (_, _, approvals, _) = state.remote_sessions.snapshot(&scope).await;
        assert_eq!(approvals.len(), 1);
        let id = approvals[0]["id"].as_str().unwrap();
        state
            .remote_sessions
            .claim_approval(&scope, id)
            .await
            .unwrap();
        assert!(
            state
                .remote_sessions
                .claim_approval(&scope, id)
                .await
                .is_err()
        );
        // Native retransmission must not unlock an approval already submitted.
        observe_codex(&state, 7, "client", "stream", &event).await;
        assert!(
            state
                .remote_sessions
                .claim_approval(&scope, id)
                .await
                .is_err()
        );
        observe_codex(
            &state,
            7,
            "client",
            "stream",
            &json!({"method":"serverRequest/resolved","params":{"requestId":42}}),
        )
        .await;
        assert!(state.remote_sessions.snapshot(&scope).await.2.is_empty());
    }
    #[tokio::test]
    async fn attachments_are_scoped_pending_removal_never_deletes_native_artifact() {
        let (directory, state) = test_state();
        let scope = Scope::new("codex:fixture", "one");
        let mut headers = HeaderMap::new();
        headers.insert("x-file-name", HeaderValue::from_static("fixture.txt"));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        let pending = files::upload(
            &state,
            &scope,
            headers.clone(),
            Bytes::from_static(b"fixture"),
        )
        .await
        .unwrap();
        assert!(
            files::lookup(&state, &Scope::new("codex:fixture", "two"), &pending.id)
                .await
                .is_err()
        );
        files::remove_pending(&state, &scope, &pending.id)
            .await
            .unwrap();
        assert!(files::lookup(&state, &scope, &pending.id).await.is_err());
        let committed = files::upload(&state, &scope, headers, Bytes::from_static(b"fixture"))
            .await
            .unwrap();
        files::mark_sent(&state, &scope, std::slice::from_ref(&committed.id))
            .await
            .unwrap();
        assert!(
            files::remove_pending(&state, &scope, &committed.id)
                .await
                .is_err()
        );
        let native_path = directory.path().join("native.txt");
        std::fs::write(&native_path, "native").unwrap();
        let native = files::associate(&state, &scope, &native_path, "artifact")
            .await
            .unwrap()
            .unwrap();
        assert!(
            files::remove_pending(&state, &scope, &native.id)
                .await
                .is_err()
        );
        assert!(native_path.exists());
        let mut headers = HeaderMap::new();
        headers.insert("x-file-name", HeaderValue::from_static("rejected.txt"));
        let rejected = files::upload(&state, &scope, headers, Bytes::from_static(b"rejected"))
            .await
            .unwrap();
        let newly_sent = files::mark_sent(
            &state,
            &scope,
            &[rejected.id.clone(), native.id.clone(), committed.id.clone()],
        )
        .await
        .unwrap();
        assert_eq!(newly_sent, vec![rejected.id.clone()]);
        files::restore_pending(&state, &scope, &newly_sent)
            .await
            .unwrap();
        files::remove_pending(&state, &scope, &rejected.id)
            .await
            .unwrap();
        assert!(
            files::remove_pending(&state, &scope, &native.id)
                .await
                .is_err()
        );
        assert!(
            files::remove_pending(&state, &scope, &committed.id)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn active_content_is_downloaded_with_isolation_headers() {
        let (_directory, state) = test_state();
        let scope = Scope::new("codex:fixture", "one");
        let mut headers = HeaderMap::new();
        headers.insert("x-file-name", HeaderValue::from_static("fixture.html"));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
        let file = files::upload(
            &state,
            &scope,
            headers,
            Bytes::from_static(b"<h1>fixture</h1>"),
        )
        .await
        .unwrap();
        let response = download(
            State(state),
            Path((scope.client, scope.session, file.id)),
            Query(DownloadQuery::default()),
        )
        .await
        .unwrap();
        assert!(
            response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .starts_with("attachment")
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("sandbox")
        );
    }
    #[tokio::test]
    async fn stop_interrupts_exact_native_turn_and_confirms_terminal_status() {
        let (_directory, state) = test_state();
        let (native_tx, mut native_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut remote = state.remote_control.inner.lock().await;
            remote.connected = true;
            remote.initialized = true;
            remote.connection_epoch = 7;
            remote.client_id = "client".into();
            remote.stream_id = "stream".into();
            remote.outbound_tx = Some(native_tx);
            remote.clients.insert(
                "default".into(),
                crate::app_state::RemoteControlClientState {
                    client_id: "client".into(),
                    stream_id: "stream".into(),
                    initialized: true,
                    next_seq_id: 1,
                    pending: Default::default(),
                    current_thread_id: Some("im-thread".into()),
                    current_turn_id: Some("im-turn".into()),
                    last_app_ping_at_ms: None,
                    last_app_pong_at_ms: None,
                    last_app_pong_status: None,
                    last_initialize_sent_at_ms: None,
                    recovery_attempt: 0,
                    recovery_started_at_ms: None,
                },
            );
        }
        let native_state = state.clone();
        let native = tokio::spawn(async move {
            for (index, method) in ["thread/turns/list", "turn/interrupt", "thread/turns/list"]
                .into_iter()
                .enumerate()
            {
                let crate::remote_control_backend::OutboundWsMessage::Text(wire) =
                    native_rx.recv().await.unwrap()
                else {
                    panic!("expected JSON-RPC message")
                };
                assert_eq!(wire["client_id"], "client");
                assert_eq!(wire["stream_id"], "stream");
                assert_eq!(wire["message"]["method"], method);
                assert_eq!(wire["message"]["params"]["threadId"], "browser-thread");
                if method == "turn/interrupt" {
                    assert_eq!(wire["message"]["params"]["turnId"], "native-turn");
                }
                let pending = {
                    let mut remote = native_state.remote_control.inner.lock().await;
                    let client = remote.clients.get_mut("default").unwrap();
                    let key = wire["message"]["id"].to_string();
                    client.pending.remove(&key).unwrap()
                };
                assert!(!pending.track_thread_active);
                let reply = if method == "turn/interrupt" {
                    json!({})
                } else {
                    json!({"data":[{"id":"native-turn","status":if index == 0 {"inProgress"} else {"interrupted"},"items":[]}],"nextCursor":null})
                };
                pending.response_tx.send(Ok(reply)).unwrap();
            }
        });
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            stop(
                State(state.clone()),
                Path(("codex:default".into(), "browser-thread".into())),
                Json(StopRequest {
                    turn_id: Some("native-turn".into()),
                }),
            ),
        )
        .await
        .unwrap()
        .unwrap()
        .0;
        native.await.unwrap();
        assert_eq!(result["status"], "stopped");
        let scope = Scope::new("codex:default", "browser-thread");
        assert_eq!(state.remote_sessions.snapshot(&scope).await.0, "stopped");
        let remote = state.remote_control.inner.lock().await;
        assert_eq!(
            remote.clients["default"].current_thread_id.as_deref(),
            Some("im-thread")
        );
        assert_eq!(
            remote.clients["default"].current_turn_id.as_deref(),
            Some("im-turn")
        );
    }
    #[tokio::test]
    async fn browser_disconnect_keeps_native_turn_receipt_and_im_selection() {
        let (_directory, state) = test_state();
        let (native_tx, mut native_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut remote = state.remote_control.inner.lock().await;
            remote.connected = true;
            remote.initialized = true;
            remote.connection_epoch = 7;
            remote.client_id = "client".into();
            remote.stream_id = "stream".into();
            remote.outbound_tx = Some(native_tx);
            remote.current_thread_id = Some("im-thread".into());
            remote.current_turn_id = Some("im-turn".into());
            remote.clients.insert(
                "default".into(),
                crate::app_state::RemoteControlClientState {
                    client_id: "client".into(),
                    stream_id: "stream".into(),
                    initialized: true,
                    next_seq_id: 1,
                    pending: Default::default(),
                    current_thread_id: Some("im-thread".into()),
                    current_turn_id: Some("im-turn".into()),
                    last_app_ping_at_ms: None,
                    last_app_pong_at_ms: None,
                    last_app_pong_status: None,
                    last_initialize_sent_at_ms: None,
                    recovery_attempt: 0,
                    recovery_started_at_ms: None,
                },
            );
        }
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let (continue_tx, continue_rx) = tokio::sync::oneshot::channel();
        let native_state = state.clone();
        let native = tokio::spawn(async move {
            let mut accepted = Some(accepted_tx);
            let mut proceed = Some(continue_rx);
            let mut starts = 0;
            while let Some(message) = native_rx.recv().await {
                if !matches!(
                    message,
                    crate::remote_control_backend::OutboundWsMessage::Text(_)
                ) {
                    continue;
                }
                let pending = {
                    let mut remote = native_state.remote_control.inner.lock().await;
                    let client = remote.clients.get_mut("default").unwrap();
                    let key = client.pending.keys().next().cloned().unwrap();
                    client.pending.remove(&key).unwrap()
                };
                assert!(!pending.track_thread_active);
                let response = match pending.method.as_str() {
                    "thread/read" | "thread/resume" => {
                        json!({"thread":{"id":"browser-thread","status":{"type":"idle"}}})
                    }
                    "turn/start" => {
                        starts += 1;
                        let _ = accepted.take().unwrap().send(());
                        proceed.take().unwrap().await.unwrap();
                        json!({"turn":{"id":"native-turn","status":"inProgress"}})
                    }
                    other => panic!("unexpected synthetic native request: {other}"),
                };
                let _ = pending.response_tx.send(Ok(response));
                if starts == 1 {
                    break;
                }
            }
            starts
        });
        let request_state = state.clone();
        let request = tokio::spawn(async move {
            send(
                State(request_state),
                Path(("codex:default".into(), "browser-thread".into())),
                Json(TurnRequest {
                    text: "fixture".into(),
                    attachment_ids: Vec::new(),
                    model: None,
                    effort: None,
                    request_id: "browser-request".into(),
                }),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(3), accepted_rx)
            .await
            .unwrap()
            .unwrap();
        request.abort();
        let _ = continue_tx.send(());
        assert_eq!(native.await.unwrap(), 1);
        let scope = Scope::new("codex:default", "browser-thread");
        let receipt = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let receipt = state
                    .remote_sessions
                    .reserve(&scope, "browser-request")
                    .await
                    .unwrap()
                    .unwrap();
                if receipt["turnId"] == "native-turn" {
                    break receipt;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(receipt["turnId"], "native-turn");
        let remote = state.remote_control.inner.lock().await;
        assert_eq!(remote.current_thread_id.as_deref(), Some("im-thread"));
        assert_eq!(
            remote.clients["default"].current_turn_id.as_deref(),
            Some("im-turn")
        );
    }
    #[tokio::test]
    async fn browser_disconnect_after_claim_keeps_native_approval_worker() {
        let (_directory, state) = test_state();
        let (tx, mut native_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut remote = state.remote_control.inner.lock().await;
            remote.connected = true;
            remote.initialized = true;
            remote.connection_epoch = 7;
            remote.client_id = "client".into();
            remote.stream_id = "stream".into();
            remote.outbound_tx = Some(tx);
            remote.clients.insert(
                "default".into(),
                crate::app_state::RemoteControlClientState {
                    client_id: "client".into(),
                    stream_id: "stream".into(),
                    initialized: true,
                    next_seq_id: 1,
                    pending: Default::default(),
                    current_thread_id: Some("im-thread".into()),
                    current_turn_id: None,
                    last_app_ping_at_ms: None,
                    last_app_pong_at_ms: None,
                    last_app_pong_status: None,
                    last_initialize_sent_at_ms: None,
                    recovery_attempt: 0,
                    recovery_started_at_ms: None,
                },
            );
        }
        let target = codex::target(&state, "codex:default").await.unwrap();
        let scope = Scope::new("codex:default", "browser-thread");
        state
            .remote_sessions
            .watch(&scope, Some(target.clone()))
            .await;
        state
            .remote_sessions
            .add_approval(
                &scope,
                "approval".into(),
                json!({"id":"approval","type":"approval"}),
                service::ApprovalSource::Codex {
                    target,
                    request_id: json!(42),
                    method: "item/commandExecution/requestApproval".into(),
                    params: json!({"threadId":"browser-thread","command":"echo fixture"}),
                },
            )
            .await;
        // Block native validation, not the browser worker. Disconnecting at
        // this point previously stranded claimed=true without sending.
        let native_gate = state.remote_control.inner.lock().await;
        let request_state = state.clone();
        let request = tokio::spawn(async move {
            approve(
                State(request_state),
                Path((
                    "codex:default".into(),
                    "browser-thread".into(),
                    "approval".into(),
                )),
                Json(ApprovalDecision {
                    decision: "option:0".into(),
                    answers: Value::Null,
                }),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if state.remote_sessions.feeds.lock().await[&scope].approvals["approval"].claimed {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        request.abort();
        drop(native_gate);
        let crate::remote_control_backend::OutboundWsMessage::Text(response) =
            tokio::time::timeout(Duration::from_secs(3), native_rx.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected approval JSON-RPC response")
        };
        assert_eq!(response["client_id"], "client");
        assert_eq!(response["stream_id"], "stream");
        assert_eq!(response["message"]["id"], 42);
        assert_eq!(response["message"]["result"]["decision"], "accept");
        tokio::time::timeout(Duration::from_secs(3), async {
            while !state.remote_sessions.snapshot(&scope).await.2.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(native_rx.try_recv().is_err());
    }
}
