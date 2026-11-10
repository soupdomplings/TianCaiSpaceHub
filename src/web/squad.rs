use super::desktop::{response, trusted};
use crate::{
    app_state::SharedState,
    desktop::squad::{Session, Squad},
};
use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
pub(super) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/squad/status", get(status))
        .route("/api/desktop/squad/profile", get(profile))
        .route("/api/desktop/squad/refresh", post(refresh))
        .route(
            "/api/desktop/squad/browser-capability",
            post(browser_capability),
        )
        .route("/api/desktop/squad/browser-start", post(browser_start))
        .route("/api/desktop/squad/browser-poll", post(browser_poll))
        .route("/api/desktop/squad/browser-cancel", post(browser_cancel))
        .route("/api/desktop/squad/website-link", post(website_link))
        .route("/api/desktop/squad/browser-session", post(browser_session))
        .route(
            "/api/desktop/squad/payment-recovery",
            post(payment_recovery),
        )
        .route("/api/desktop/squad/download", post(download))
        .route("/api/desktop/squad/session", post(login))
        .route("/api/desktop/squad/logout", post(logout))
        .route("/api/desktop/squad/request", post(request))
        .route("/api/desktop/squad/confirmed", post(confirmed))
        .route("/api/desktop/squad/import", post(prepare_import))
}
#[derive(Deserialize)]
struct PaymentRecovery {
    id: u64,
}
async fn payment_recovery(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<PaymentRecovery>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.payment_recovery(input.id).await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct Download {
    path: String,
    destination: std::path::PathBuf,
}
async fn download(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<Download>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state)
                .await?
                .download(&input.path, &input.destination)
                .await
        }
        .await,
    )
}
async fn browser_session(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.browser_session().await
        }
        .await,
    )
}
async fn service(state: &SharedState) -> anyhow::Result<std::sync::Arc<Squad>> {
    let config = state.config.lock().await.clone();
    state
        .desktop_squad
        .lock()
        .await
        .get(&state.config_path, &config)
}
async fn refresh(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.invalidate();
            Ok(json!({"refreshed":true}))
        }
        .await,
    )
}
async fn status(State(state): State<SharedState>) -> Response {
    response(async { service(&state).await?.status().await }.await)
}
async fn profile(State(state): State<SharedState>) -> Response {
    response(async { service(&state).await?.profile().await }.await)
}
macro_rules! browser_action {
    ($name:ident) => {
        async fn $name(State(state): State<SharedState>, headers: HeaderMap) -> Response {
            response(
                async {
                    trusted(&headers)?;
                    service(&state).await?.$name().await
                }
                .await,
            )
        }
    };
}
browser_action!(browser_start);
browser_action!(browser_capability);
browser_action!(browser_poll);
browser_action!(browser_cancel);
browser_action!(website_link);
async fn login(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(session): Json<Session>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.login(session).await
        }
        .await,
    )
}
async fn logout(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.logout().await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct Request {
    method: String,
    path: String,
    #[serde(default)]
    query: Vec<(String, String)>,
    body: Option<Value>,
    #[serde(default)]
    expected_account: Option<crate::desktop::squad::ExpectedAccount>,
}
async fn request(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state)
                .await?
                .request_checked(
                    &request.method,
                    &request.path,
                    request.query,
                    request.body,
                    request.expected_account.as_ref(),
                )
                .await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct Confirmed {
    id: String,
}
async fn confirmed(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Confirmed>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.clear_pending(&request.id)?;
            Ok(json!({"confirmed":true}))
        }
        .await,
    )
}
#[derive(Deserialize)]
struct Import {
    id: u64,
}
async fn prepare_import(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Import>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            service(&state).await?.prepare_import(request.id).await
        }
        .await,
    )
}
