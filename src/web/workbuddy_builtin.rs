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
        .route(
            "/api/desktop/workbuddy-builtin/account-refresh",
            post(account_refresh),
        )
        .route(
            "/api/desktop/workbuddy-builtin/visibility",
            post(visibility),
        )
}

#[derive(Deserialize)]
struct Revision {
    revision: String,
}
#[derive(Deserialize)]
struct Visibility {
    visible: bool,
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
    let mut value = state
        .workbuddy_builtin
        .view(&state.config_path, &config)
        .await?;
    value["builtinShareAllowed"] = crate::assistant::builtin_sharing_allowed(state)
        .await
        .into();
    Ok(value)
}
async fn read(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(view(&state).await)
}
async fn account_refresh(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(
        async {
            // This is a query of an existing grant, not a new sharing action.
            // Keep the last balance and its timestamp visible when an update
            // fails; the runtime marks the account query as stale/error.
            let _ = state.workbuddy_builtin.refresh_account().await;
            let current = view(&state).await?;
            ensure!(
                current["enabled"] == true
                    && current["account"]["authorization"]["status"] != "missing",
                "请先启用 WorkBuddy 分享并完成授权"
            );
            Ok(current)
        }
        .await,
    )
}
async fn visibility(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Visibility>,
) -> Response {
    if let Err(error) = trusted(&headers) {
        return response::<Value>(Err(error));
    }
    response(
        async {
            ensure!(
                !request.visible,
                "请在天才搭档对话中明确要求打开 WorkBuddy 内置模型分享"
            );
            state.workbuddy_builtin.hide(&state.config_path).await?;
            view(&state).await
        }
        .await,
    )
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
            crate::assistant::require_builtin_sharing(&state).await?;
            revision(&state, &request.revision).await?;
            state.workbuddy_builtin.enable(&state.config_path).await?;
            // A failed balance query must not undo a successful sharing grant.
            // Its status and any last known balance remain visible in account.
            let _ = state.workbuddy_builtin.refresh_account().await;
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
            crate::assistant::require_builtin_sharing(&state).await?;
            revision(&state, &request.revision).await?;
            state.workbuddy_builtin.disable().await?;
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
            crate::assistant::require_builtin_sharing(&state).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn entry_visibility_only_hides_and_cannot_reveal_or_load_authorization() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hub.toml");
        let mut config = AppConfig {
            state_path: temp.path().join("state.json"),
            ..Default::default()
        };
        config.save(&path).unwrap();
        let state = crate::app_state::AppState::new(
            path.clone(),
            AppConfig::load_or_default(&path).unwrap(),
            None,
            None,
        );
        let before = std::fs::read(&path).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-tiancai-desktop", "1".parse().unwrap());
        assert_eq!(view(&state).await.unwrap()["featureVisible"], false);
        let forbidden = visibility(
            State(state.clone()),
            HeaderMap::new(),
            Json(Visibility { visible: false }),
        )
        .await;
        assert!(!forbidden.status().is_success());
        let reveal = visibility(
            State(state.clone()),
            headers.clone(),
            Json(Visibility { visible: true }),
        )
        .await;
        assert_eq!(reveal.status(), StatusCode::CONFLICT);
        assert!(!state.workbuddy_builtin.feature_visible(&path).unwrap());
        state.workbuddy_builtin.reveal(&path).await.unwrap();
        let hidden = visibility(
            State(state.clone()),
            headers.clone(),
            Json(Visibility { visible: false }),
        )
        .await;
        assert_eq!(hidden.status(), StatusCode::OK);
        let value = view(&state).await.unwrap();
        assert_eq!(value["featureVisible"], false);
        assert_eq!(value["enabled"], false);
        assert_eq!(value["account"]["authorization"]["status"], "missing");
        let refresh = account_refresh(State(state.clone()), headers).await;
        assert_eq!(refresh.status(), StatusCode::CONFLICT);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[tokio::test]
    async fn only_catgirl_can_change_the_host_and_rejected_actions_do_not_prepare_it() {
        use crate::assistant::personality::Personality;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hub.toml");
        let mut config = AppConfig {
            state_path: temp.path().join("state.json"),
            ..Default::default()
        };
        config.save(&path).unwrap();
        let state = crate::app_state::AppState::new(
            path.clone(),
            AppConfig::load_or_default(&path).unwrap(),
            None,
            None,
        );
        let mut headers = HeaderMap::new();
        headers.insert("x-tiancai-desktop", "1".parse().unwrap());
        let before = std::fs::read(&path).unwrap();
        for style in [Personality::Professional, Personality::Girlfriend] {
            crate::assistant::test_personality(&state, style).await;
            for result in [
                enable(
                    State(state.clone()),
                    headers.clone(),
                    Json(Revision {
                        revision: "not-used".into(),
                    }),
                )
                .await,
                disable(
                    State(state.clone()),
                    headers.clone(),
                    Json(Revision {
                        revision: "not-used".into(),
                    }),
                )
                .await,
                share(
                    State(state.clone()),
                    headers.clone(),
                    Json(Share {
                        revision: "not-used".into(),
                        model_id: "not-used".into(),
                        targets: vec![],
                    }),
                )
                .await,
            ] {
                assert_eq!(result.status(), StatusCode::CONFLICT);
                let body = axum::body::to_bytes(result.into_body(), 4096)
                    .await
                    .unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert!(body["error"].as_str().unwrap().contains("仅向软萌猫娘开放"));
            }
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(view(&state).await.unwrap()["enabled"], false);
        }
        crate::assistant::test_personality(&state, Personality::Catgirl).await;
        let revision = state.config.lock().await.revision.clone().unwrap();
        let result = disable(State(state.clone()), headers, Json(Revision { revision })).await;
        assert_eq!(result.status(), StatusCode::OK);
        let body = axum::body::to_bytes(result.into_body(), 128 * 1024)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["builtinShareAllowed"], true);
        assert_eq!(body["enabled"], false);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
