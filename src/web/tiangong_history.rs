//! Local history ownership actions, shared by the desktop history manager.
use super::desktop::{response, trusted};
use crate::{app_state::SharedState, tiangong_session_history as history};
use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::HeaderMap, response::Response, routing::post};
use serde_json::Value;

pub(super) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/gmclaw/history/read", post(read))
        .route("/api/gmclaw/history/move", post(move_sessions))
        .route("/api/gmclaw/history/restore", post(restore_sessions))
}

async fn read(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    let config_path = state.config_path.clone();
    response(
        tokio::task::spawn_blocking(move || {
            history::list_at(&crate::gmclaw_config::config_path(), &config_path)
        })
        .await
        .context("读取天工会话未完成")
        .and_then(|result| result),
    )
}

async fn move_sessions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<history::MoveRequest>,
) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    let ids = request
        .sessions
        .iter()
        .map(|row| row.session_id.clone())
        .collect();
    let path = state.config_path.clone();
    response(
        edit(state, ids, move || {
            history::move_at(&crate::gmclaw_config::config_path(), &path, &request)
        })
        .await,
    )
}

async fn restore_sessions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<history::RestoreRequest>,
) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    let ids = request
        .sessions
        .iter()
        .map(|row| row.session_id.clone())
        .collect();
    let path = state.config_path.clone();
    response(
        edit(state, ids, move || {
            history::restore_at(&crate::gmclaw_config::config_path(), &path, &request)
        })
        .await,
    )
}

async fn edit<T, F>(state: SharedState, ids: Vec<String>, action: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    // Move this permit into the blocking writer: a cancelled HTTP request must
    // not release the launch lock while a SQLite/file change is still running.
    let permit = crate::gmclaw_runtime::history_edit_permit().await?;
    crate::gmclaw_im::edit_history(state, ids, move || {
        let _permit = permit;
        action()
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn history_routes_require_the_desktop_header_before_reading_or_mutating_data() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hub.toml");
        let config = crate::config::AppConfig {
            state_path: temp.path().join("state.json"),
            ..Default::default()
        };
        let state = crate::app_state::AppState::new(path, config, None, None);
        let routes = router().with_state(state);
        for operation in ["read", "move", "restore"] {
            let body = if operation == "read" {
                "{}"
            } else {
                "{\"sessions\":[]}"
            };
            let result = routes
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/gmclaw/history/{operation}"))
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(result.status(), StatusCode::CONFLICT);
        }
        assert!(!temp.path().join("hub.toml").exists());
    }
}
