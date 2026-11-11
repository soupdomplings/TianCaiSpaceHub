use super::*;
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Notify};

#[derive(Default)]
struct Fixture {
    bodies: Mutex<Vec<Value>>,
    calls: AtomicUsize,
    disconnected: Arc<Notify>,
}
struct Dropped(Arc<Notify>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
async fn native(
    State(state): State<Arc<Fixture>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    assert_eq!(headers["authorization"], "Bearer fixture-ledger");
    assert_eq!(headers["x-workbuddy-mcp-context"], "fixture-context");
    assert!(!headers.contains_key("origin"));
    assert!(!headers.contains_key("x-upstream-secret"));
    state.calls.fetch_add(1, Ordering::Relaxed);
    state.bodies.lock().await.push(body.clone());
    if body["test_error"] == true {
        return (StatusCode::UNAUTHORIZED, "fixture-account-secret").into_response();
    }
    if body["cancel"] == true {
        let stream = futures_util::stream::unfold(
            (false, Dropped(state.disconnected.clone())),
            |(sent, guard)| async move {
                if sent {
                    futures_util::future::pending::<()>().await;
                }
                Some((
                    Ok::<_, std::io::Error>(Bytes::from_static(b"data: {\"fixture\":true}\n\n")),
                    (true, guard),
                ))
            },
        );
        return (
            [("content-type", "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response();
    }
    if body["stream"] == true {
        return ([("content-type","text/event-stream")],"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-fixture\",\"type\":\"function\",\"function\":{\"name\":\"get_value\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n").into_response();
    }
    Json(json!({"id":"fixture-json","choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}]})).into_response()
}
async fn host() -> (
    Arc<Fixture>,
    crate::desktop::workbuddy_builtin::LocalReference,
    tokio::task::JoinHandle<()>,
) {
    let state = Arc::new(Fixture::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/skills/vlm/chat", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/skills/vlm/chat", post(native))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (
        state,
        crate::desktop::workbuddy_builtin::LocalReference {
            url,
            token: "fixture-ledger".into(),
            context: "fixture-context".into(),
        },
        task,
    )
}
fn request(body: &Value) -> reqwest::Request {
    reqwest::Client::new()
        .post("http://127.0.0.1/placeholder")
        .header("origin", "https://client.invalid")
        .header("x-upstream-secret", "do-not-forward")
        .json(body)
        .build()
        .unwrap()
}
fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn workbuddy_json_sse_and_tool_result_roundtrip_preserve_the_client_payload() {
    let (fixture, reference, task) = host().await;
    let tools =
        json!([{"type":"function","function":{"name":"get_value","parameters":{"type":"object"}}}]);
    let first = json!({"model":"fixture-model","stream":true,"tools":tools,"messages":[{"role":"user","content":"read value"}]});
    let response = transport(
        request(&first),
        &ProviderConfig::default(),
        reference.clone(),
        deadline(),
    )
    .await
    .unwrap();
    let sse = response.text().await.unwrap();
    assert!(sse.contains("call-fixture"));
    assert!(sse.ends_with("data: [DONE]\n\n"));
    let next = json!({"model":"fixture-model","stream":false,"tools":tools,"messages":[{"role":"user","content":"read value"},{"role":"assistant","content":null,"tool_calls":[{"id":"call-fixture","type":"function","function":{"name":"get_value","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call-fixture","content":"42"}]});
    let response = transport(
        request(&next),
        &ProviderConfig::default(),
        reference.clone(),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(
        response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
        "done"
    );
    assert_eq!(*fixture.bodies.lock().await, vec![first, next]);
    let error = transport(
        request(&json!({"test_error":true})),
        &ProviderConfig::default(),
        reference,
        deadline(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    assert!(!format!("{error:?}").contains("fixture-account-secret"));
    assert_eq!(fixture.calls.load(Ordering::Relaxed), 3);
    task.abort();
}

#[tokio::test]
async fn workbuddy_cancellation_disconnects_the_host_without_replaying_generation() {
    let (fixture, reference, task) = host().await;
    let mut response = transport(
        request(&json!({"cancel":true})),
        &ProviderConfig::default(),
        reference,
        deadline(),
    )
    .await
    .unwrap();
    response.chunk().await.unwrap().unwrap();
    drop(response);
    tokio::time::timeout(Duration::from_secs(3), fixture.disconnected.notified())
        .await
        .unwrap();
    assert_eq!(fixture.calls.load(Ordering::Relaxed), 1);
    task.abort();
}
