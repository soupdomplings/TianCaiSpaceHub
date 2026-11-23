//! Display choices belong to the target Hub, shared by its web and phone viewers.
use super::{ApiResult, NetworkState, SharedState, config_file, ensure_loaded};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct DisplayContent {
    pub reasoning: bool,
    pub tool: bool,
    pub file_change: bool,
    pub artifact: bool,
}

impl DisplayContent {
    pub(crate) fn shows(&self, category: &str) -> bool {
        match category {
            "reasoning" => self.reasoning,
            "tool" => self.tool,
            "fileChange" => self.file_change,
            _ => false,
        }
    }
}

impl NetworkState {
    pub(crate) async fn display_content(&self) -> DisplayContent {
        self.inner.lock().await.config.display_content
    }
}

pub(crate) async fn get(state: &SharedState) -> anyhow::Result<DisplayContent> {
    ensure_loaded(state).await?;
    Ok(state.remote_network.display_content().await)
}

// This write route exists only on the trusted local management listener.
// It neither restarts networking nor rewrites protected pairing credentials.
pub(super) async fn update(
    State(state): State<SharedState>,
    Json(choices): Json<DisplayContent>,
) -> ApiResult {
    ensure_loaded(&state).await?;
    let mut inner = state.remote_network.inner.lock().await;
    if choices != inner.config.display_content {
        let mut updated = inner.config.clone();
        updated.display_content = choices;
        crate::desktop::transaction::atomic_write(
            &config_file(&state.config_path),
            &serde_json::to_vec_pretty(&updated).map_err(anyhow::Error::from)?,
        )?;
        inner.config = updated;
    }
    Ok(Json(json!({"displayContent":inner.config.display_content})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn display_content_defaults_persists_and_has_no_remote_write_route() {
        let (_directory, state) = super::super::tests::fixture();
        assert_eq!(get(&state).await.unwrap(), DisplayContent::default());
        // Old network.json files have no displayContent and retain their identity.
        let legacy: super::super::NetworkConfig = serde_json::from_value(json!({
            "version":1,"deviceId":"old-device","workspaceDirs":["kept"]
        }))
        .unwrap();
        assert_eq!(legacy.display_content, DisplayContent::default());
        assert_eq!(legacy.workspace_dirs, ["kept"]);
        let choices = DisplayContent {
            reasoning: true,
            artifact: true,
            ..Default::default()
        };
        let before = state.remote_network.inner.lock().await.config.clone();
        let response = super::super::management_router()
            .with_state(state.clone())
            .oneshot(
                Request::post("/api/remote-network/display-content")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&choices).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let saved: super::super::NetworkConfig =
            serde_json::from_slice(&std::fs::read(config_file(&state.config_path)).unwrap())
                .unwrap();
        assert_eq!(saved.display_content, choices);
        assert_eq!(saved.device_id, before.device_id);
        assert_eq!(saved.enabled, before.enabled);
        assert!(state.remote_network.inner.lock().await.running.is_none());
        assert!(
            !super::super::root(&state.config_path)
                .join("protected")
                .exists()
        );
        let fresh = crate::app_state::AppState::new(
            state.config_path.clone(),
            crate::config::AppConfig::default(),
            None,
            None,
        );
        assert_eq!(get(&fresh).await.unwrap(), choices);
        let remote = crate::remote_sessions::router().with_state(state);
        let denied = remote
            .oneshot(
                Request::post("/api/remote-network/display-content")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), 404);
    }
}
