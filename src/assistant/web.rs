use super::*;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as RoutePath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

pub(crate) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/assistant/status", get(status))
        .route("/api/desktop/assistant/settings", post(settings))
        .route("/api/desktop/assistant/send", post(send))
        .route("/api/desktop/assistant/stop", post(stop))
        .route("/api/desktop/assistant/clear", post(clear))
        .route("/api/assistant/model/{id}/responses", post(model))
        .route("/api/assistant/model/{id}/models", get(catalog))
        .merge(tools::router())
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
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
fn response(result: Result<Value>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}
async fn status(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            super::status(&state).await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct Settings {
    enabled: bool,
}
async fn settings(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<Settings>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            super::configure(&state, input.enabled).await?;
            super::status(&state).await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct Send {
    text: String,
}
async fn send(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<Send>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            super::send(&state, input.text).await?;
            super::status(&state).await
        }
        .await,
    )
}
async fn stop(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            super::stop(&state).await?;
            super::status(&state).await
        }
        .await,
    )
}
async fn clear(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            super::clear(&state).await?;
            super::status(&state).await
        }
        .await,
    )
}
async fn catalog(
    State(state): State<SharedState>,
    RoutePath(id): RoutePath<String>,
    headers: HeaderMap,
) -> Response {
    response(proxy::catalog(&state, &id, &headers).await)
}
async fn model(
    State(state): State<SharedState>,
    RoutePath(id): RoutePath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match proxy::forward(&state, &id, headers, body).await {
        Ok(response) => response,
        Err(error) => proxy::rejection(error),
    }
}
