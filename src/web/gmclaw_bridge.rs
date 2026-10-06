use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{app_state::SharedState, config::AppConfig};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartRequest {
    expected_revision: String,
}

pub(super) async fn start(
    State(state): State<SharedState>,
    Json(request): Json<StartRequest>,
) -> axum::response::Response {
    match prepare_and_start(&state, &request.expected_revision).await {
        Ok(status) => Json(status).into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

/// The desktop button is explicit startup authorization. Prepare only the
/// bridge fields, without changing any IM sender's executor or current session.
async fn prepare_and_start(
    state: &SharedState,
    expected_revision: &str,
) -> anyhow::Result<crate::gmclaw_runtime::GmClawRuntimeStatus> {
    use anyhow::{Context, ensure};

    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let mut config = tokio::task::spawn_blocking(move || AppConfig::load_or_default(&path))
        .await
        .context("无法准备天工连接，请稍后重试")?
        .map_err(|_| anyhow::anyhow!("无法读取天工连接设置，请稍后重试"))?;
    crate::normalize_config_paths(&mut config, &state.config_path);
    *current = config.clone();
    ensure!(
        !expected_revision.is_empty() && config.revision.as_deref() == Some(expected_revision),
        "配置已变化，请刷新后重新启动天工"
    );
    let needs_save =
        !config.gmclaw_bridge.enabled || config.gmclaw_bridge.auth_token.trim().is_empty();
    config.gmclaw_bridge.enabled = true;
    crate::gmclaw_runtime::ensure_auth_token(&mut config.gmclaw_bridge);
    config.gmclaw_bridge.validate()?;
    if needs_save {
        let path = state.config_path.clone();
        let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<AppConfig> {
            config.save(&path).map_err(|_| {
                anyhow::anyhow!("天工自动连接设置保存失败或配置已变化，请刷新后重试")
            })?;
            Ok(config)
        })
        .await
        .context("无法保存天工连接设置，请稍后重试")?;
        config = match saved {
            Ok(saved) => saved,
            Err(error) => {
                // Keep an external writer's latest permissions after a conflict.
                if let Ok(mut latest) = AppConfig::load_or_default(&state.config_path) {
                    crate::normalize_config_paths(&mut latest, &state.config_path);
                    *current = latest;
                }
                return Err(error);
            }
        };
        *current = config.clone();
    }
    drop(current);

    let saved_revision = config.revision.clone();
    let executable = config.gmclaw_bridge.desktop_path.as_ref().map(Into::into);
    let validate_before_spawn = || async {
        let mut guard = state.config.lock().await;
        let mut latest = AppConfig::load_or_default(&state.config_path)
            .map_err(|_| anyhow::anyhow!("无法核对天工连接设置，本次未启动；请稍后重试"))?;
        crate::normalize_config_paths(&mut latest, &state.config_path);
        *guard = latest.clone();
        ensure!(
            latest.revision == saved_revision && latest.gmclaw_bridge.enabled,
            "启动检查期间配置已变化，本次未启动天工；请刷新后重试"
        );
        Ok::<_, anyhow::Error>(guard)
    };
    // Shared with /tg: serialized startup, process/port checks and bounded
    // readiness wait. An already-running desktop is never restarted.
    let status =
        crate::gmclaw_runtime::launch(&config.gmclaw_bridge, executable, validate_before_spawn)
            .await?;

    let mut current = state.config.lock().await;
    let mut latest = AppConfig::load_or_default(&state.config_path)
        .map_err(|_| anyhow::anyhow!("启动检查后无法读取天工设置，请刷新查看当前连接状态"))?;
    crate::normalize_config_paths(&mut latest, &state.config_path);
    *current = latest.clone();
    ensure!(
        latest.revision == config.revision
            && serde_json::to_value(&latest.gmclaw_bridge)?
                == serde_json::to_value(&config.gmclaw_bridge)?,
        "启动检查期间配置已变化，请刷新查看当前连接状态"
    );
    Ok(status)
}
