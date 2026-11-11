//! Explicit local UI actions; read never starts or modifies the WorkBuddy host.
use super::desktop::{response, trusted};
use crate::{
    app_state::SharedState,
    config::AppConfig,
    desktop::{clients::ClientId, resources::Library},
};
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, extract::State, http::HeaderMap, response::Response, routing::post};
use serde::Deserialize;
use serde_json::Value;

pub(super) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/workbuddy-builtin/read", post(read))
        .route("/api/desktop/workbuddy-builtin/enable", post(enable))
        .route("/api/desktop/workbuddy-builtin/disable", post(disable))
        .route("/api/desktop/workbuddy-builtin/share", post(share))
}

#[derive(Deserialize)]
struct Revision {
    revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Share {
    revision: String,
    model_id: String,
    targets: Vec<String>,
}

async fn view(state: &SharedState) -> Result<Value> {
    let config = state.config.lock().await.clone();
    state
        .workbuddy_builtin
        .view(&state.config_path, &config)
        .await
}
async fn read(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(view(&state).await)
}
async fn revision(state: &SharedState, expected: &str) -> Result<()> {
    ensure!(
        state.config.lock().await.revision.as_deref() == Some(expected),
        "Hub 配置已变化，请重新读取分享状态"
    );
    Ok(())
}
async fn enable(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Revision>,
) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(
        async {
            revision(&state, &request.revision).await?;
            state.workbuddy_builtin.enable(&state.config_path).await?;
            view(&state).await
        }
        .await,
    )
}
async fn disable(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Revision>,
) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(
        async {
            revision(&state, &request.revision).await?;
            state.workbuddy_builtin.disable().await;
            view(&state).await
        }
        .await,
    )
}
async fn share(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Share>,
) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(
        async {
            revision(&state, &request.revision).await?;
            ensure!(
                !request.targets.is_empty() && request.targets.len() <= 32,
                "请选择目标客户端"
            );
            let library = Library::new(&state.config_path)?;
            let targets = library.targets(&library.load()?)?;
            ensure!(
                request
                    .targets
                    .iter()
                    .enumerate()
                    .all(|(index, id)| !request.targets[..index].contains(id)
                        && targets.iter().any(|t| t.id == *id
                            && t.client != ClientId::WorkBuddy
                            && !t.models_inherited)),
                "客户端目标已变化，请重新选择"
            );
            let mut config = state.config.lock().await;
            ensure!(
                config.revision.as_deref() == Some(request.revision.as_str()),
                "Hub 配置已变化，请重新读取分享状态"
            );
            let source = state
                .workbuddy_builtin
                .source(&config, &request.model_id)
                .await?;
            let path = state.config_path.clone();
            let mut next = config.clone();
            // Old client copies keep their unique local identity and fail clearly
            // on the old account. Only ordinary source aliases would collide here.
            let account = &source
                .workbuddy_builtin
                .as_ref()
                .context("WorkBuddy 来源绑定不存在")?
                .account_id;
            for provider in &mut next.ai_gateway.providers {
                if crate::desktop::workbuddy_builtin::is_source(provider)
                    && provider
                        .workbuddy_builtin
                        .as_ref()
                        .is_some_and(|b| &b.account_id != account)
                {
                    provider.enabled = false;
                }
            }
            if let Some(old) = next
                .ai_gateway
                .providers
                .iter_mut()
                .find(|p| p.name == source.name)
            {
                *old = source.clone();
            } else {
                next.ai_gateway.providers.push(source.clone());
            }
            next.ai_gateway.enabled = true;
            let next = tokio::task::spawn_blocking(move || -> Result<AppConfig> {
                next.save(&path)?;
                Ok(next)
            })
            .await
            .context("保存 WorkBuddy 分享来源中断，请重新读取状态")??;
            let revision = next.revision.clone().unwrap_or_default();
            *config = next;
            drop(config);
            let result = super::desktop_models::share_builtin_source(
                state.clone(),
                headers,
                revision,
                source,
                request.targets,
                ClientId::WorkBuddy,
            )
            .await;
            let status = result.status();
            let bytes = axum::body::to_bytes(result.into_body(), 2 * 1024 * 1024).await?;
            let outcome: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                status.is_success() && outcome.get("error").is_none(),
                "{}",
                outcome["error"]
                    .as_str()
                    .unwrap_or("分享未完成，请重新读取状态")
            );
            let mut catalog = view(&state).await?;
            catalog["outcomes"] = outcome["outcomes"].clone();
            Ok(catalog)
        }
        .await,
    )
}
