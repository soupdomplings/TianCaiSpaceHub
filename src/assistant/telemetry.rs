//! Summary only: never persist prompts, tool payloads, JWT, or local bridge keys.
use super::*;
use crate::ai_gateway::request_log::{self, RequestLogContext, RequestLogRecord, RequestLogUpdate};

pub(super) fn begin(
    state: &SharedState,
    model: &str,
    stream: bool,
    enabled: bool,
) -> Option<RequestLogContext> {
    if !enabled {
        return None;
    }
    let store = state.ai_gateway_request_logs.clone();
    let record = RequestLogRecord {
        request_id: uuid::Uuid::new_v4().to_string(),
        model_id: model.into(),
        stream,
        channel: "天才搭档管理员服务".into(),
        provider_type: "openai_responses".into(),
        status: "running".into(),
        usage: Default::default(),
        cost_usd: Some(0.0),
        latency_ms: None,
        ttft_ms: None,
        created_at_ms: request_log::now_ms(),
        error_message: None,
        request_headers_json: None,
        request_json: None,
        upstream_request_body_bytes: None,
        upstream_request_headers_json: None,
        upstream_request_json: None,
        upstream_response_sse: None,
        response_json: None,
    };
    let log_id = store.insert_record(&record).ok()?;
    store.bind_identity(
        log_id,
        Some(crate::desktop::request_source::GENIUS_BUDDY.into()),
        None,
        Some("squad-assistant".into()),
    );
    Some(RequestLogContext {
        store,
        log_id,
        started_at: Instant::now(),
        details_enabled: false,
    })
}
pub(super) fn finish(
    context: Option<&RequestLogContext>,
    status: &str,
    value: Option<&Value>,
    message: Option<String>,
) {
    if let Some(context) = context {
        let _ = context.store.update_record(
            context.log_id,
            &RequestLogUpdate {
                status: Some(status.into()),
                usage: value.map(request_log::usage_from_response_value),
                latency_ms: Some(request_log::elapsed_ms(context.started_at)),
                error_message: message,
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    #[tokio::test]
    async fn assistant_usage_is_attributed_without_persisting_conversation_or_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::config::AppConfig {
            state_path: temp.path().join("state.json"),
            ..Default::default()
        };
        let state =
            crate::app_state::AppState::new(temp.path().join("config.toml"), config, None, None);
        let context = begin(&state, "fixture-admin-model", true, true).unwrap();
        let id = context.log_id;
        let fixture=axum::body::Bytes::from_static(b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":11,\"output_tokens\":3,\"total_tokens\":14}}}\n\n");
        let stream = futures_util::stream::iter(vec![Ok::<_, std::io::Error>(fixture)]);
        let _ = request_log::ResponsesSseLogStream::new(stream, context)
            .collect::<Vec<_>>()
            .await;
        let detail = state
            .ai_gateway_request_logs
            .get_detail(id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.client_id.as_deref(), Some("genius-buddy"));
        assert_eq!(detail.summary.channel, "天才搭档管理员服务");
        assert_eq!(detail.summary.total_tokens, Some(14));
        assert_eq!(detail.summary.cost_usd, Some(0.0));
        assert!(detail.request_json.is_none());
        assert!(detail.response_json.is_none());
        assert!(detail.request_headers_json.is_none());
    }
}
