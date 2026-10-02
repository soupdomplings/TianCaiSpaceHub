pub mod anthropic_messages;
pub mod deepseek_chat;
pub mod openai_alpha_search;
pub mod openai_images;
pub mod openai_responses;

#[cfg(test)]
mod retry_tests;

use std::{error::Error as _, time::Duration};

use axum::http::StatusCode;
use tracing::{error, warn};

use crate::ai_gateway::error::GatewayError;
use crate::ai_gateway::{
    chatgpt_auth,
    config::{ProviderConfig, ProviderType},
};

const UPSTREAM_MAX_RETRIES: usize = 2;

pub(super) fn apply_chat_reasoning_override(
    body: &mut serde_json::Value,
    provider: &ProviderConfig,
) {
    if provider.provider_type != crate::ai_gateway::config::ProviderType::ChatCompletions
        || !provider.chat_disable_reasoning
    {
        return;
    }
    if let Some(object) = body.as_object_mut() {
        object.remove("thinking");
        object.remove("reasoning");
        object.insert("reasoning_effort".to_string(), serde_json::json!("none"));
    }
}

pub(crate) async fn execute_openai_request(
    client: &reqwest::Client,
    request: reqwest::Request,
    provider: &ProviderConfig,
    error_log: &'static str,
) -> Result<reqwest::Response, GatewayError> {
    execute_openai_request_with_auth(
        client,
        request,
        provider,
        error_log,
        chatgpt_auth::manager(),
    )
    .await
}

pub(super) async fn execute_openai_request_with_auth(
    client: &reqwest::Client,
    request: reqwest::Request,
    provider: &ProviderConfig,
    error_log: &'static str,
    auth: &chatgpt_auth::AuthManager,
) -> Result<reqwest::Response, GatewayError> {
    let retry = (provider.provider_type == ProviderType::ChatGptResponses)
        .then(|| request.try_clone())
        .flatten();
    let response = execute_provider_request(client, request, provider, error_log).await?;
    if response.status() == StatusCode::UNAUTHORIZED
        && let Some(mut request) = retry
    {
        let rejected = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        drop(response);
        chatgpt_auth::authorize_with_manager(
            auth,
            client,
            &mut request,
            provider,
            rejected.as_deref(),
        )
        .await?;
        return execute_provider_request(client, request, provider, error_log).await;
    }
    Ok(response)
}

pub(super) fn apply_total_request_timeout(
    builder: reqwest::RequestBuilder,
    timeout_secs: u64,
    stream: bool,
) -> reqwest::RequestBuilder {
    if stream {
        builder
    } else {
        builder.timeout(provider_timeout(timeout_secs))
    }
}

pub(super) async fn execute_stream_start(
    client: &reqwest::Client,
    request: reqwest::Request,
    timeout_secs: u64,
    error_log: &'static str,
) -> Result<reqwest::Response, GatewayError> {
    execute_upstream_request(client, request, timeout_secs, error_log).await
}

pub(super) async fn execute_upstream_request(
    client: &reqwest::Client,
    request: reqwest::Request,
    timeout_secs: u64,
    error_log: &'static str,
) -> Result<reqwest::Response, GatewayError> {
    execute_request_with_retries(client, request, timeout_secs, error_log, None).await
}

pub(super) async fn execute_provider_request(
    client: &reqwest::Client,
    request: reqwest::Request,
    provider: &ProviderConfig,
    error_log: &'static str,
) -> Result<reqwest::Response, GatewayError> {
    execute_request_with_retries(
        client,
        request,
        provider.timeout_secs,
        error_log,
        if provider.is_workbuddy() {
            Some("workbuddy")
        } else if provider.is_gmclaw() {
            Some("gmclaw")
        } else {
            None
        },
    )
    .await
}

async fn execute_request_with_retries(
    client: &reqwest::Client,
    request: reqwest::Request,
    timeout_secs: u64,
    error_log: &'static str,
    http_retry_client: Option<&'static str>,
) -> Result<reqwest::Response, GatewayError> {
    let retry_template = request.try_clone();
    let mut next_request = Some(request);
    let mut retry_count = 0usize;

    loop {
        let Some(request) = next_request.take() else {
            return Err(GatewayError::upstream(
                StatusCode::BAD_GATEWAY,
                "upstream request could not be retried",
            ));
        };

        let response =
            tokio::time::timeout(provider_timeout(timeout_secs), client.execute(request))
                .await
                .map_err(|_| GatewayError::upstream_timeout())?;

        match response {
            Ok(response)
                if http_retry_client.is_some()
                    && matches!(
                        response.status(),
                        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE
                    )
                    && retry_count < UPSTREAM_MAX_RETRIES =>
            {
                let Some(retry_request) = retry_template.as_ref().and_then(|r| r.try_clone())
                else {
                    return Ok(response);
                };
                retry_count += 1;
                let delay = Duration::from_secs(retry_count as u64);
                warn!(
                    provider = http_retry_client.unwrap_or_default(),
                    upstream_status = response.status().as_u16(),
                    retry_count,
                    max_retries = UPSTREAM_MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "{error_log}; retrying upstream HTTP error"
                );
                // Retry only explicit HTTP failures before handing a response body to the caller.
                // Transport and HTTP failures share one budget; successful streams are never replayed.
                drop(response);
                tokio::time::sleep(delay).await;
                next_request = Some(retry_request);
            }
            Ok(response) => return Ok(response),
            Err(err) => {
                if should_retry_transport_error(&err)
                    && retry_count < UPSTREAM_MAX_RETRIES
                    && let Some(template) = retry_template.as_ref()
                    && let Some(retry_request) = template.try_clone()
                {
                    retry_count += 1;
                    warn!(
                        error = %reqwest_error_summary(&err),
                        retry_count,
                        max_retries = UPSTREAM_MAX_RETRIES,
                        "{error_log}; retrying upstream transport error"
                    );
                    tokio::time::sleep(upstream_transport_retry_delay(retry_count)).await;
                    next_request = Some(retry_request);
                    continue;
                }

                return Err(map_reqwest_error(err, error_log));
            }
        }
    }
}

fn map_reqwest_error(err: reqwest::Error, error_log: &'static str) -> GatewayError {
    if err.is_timeout() {
        GatewayError::upstream_timeout()
    } else {
        error!(error = %err, "{error_log}");
        GatewayError::upstream(
            StatusCode::BAD_GATEWAY,
            format!("upstream error: {}", reqwest_error_summary(&err)),
        )
    }
}

fn should_retry_transport_error(err: &reqwest::Error) -> bool {
    err.status().is_none()
        && !err.is_timeout()
        && !err.is_decode()
        && (err.is_request() || err.is_connect() || err.is_body())
}

fn upstream_transport_retry_delay(retry_count: usize) -> Duration {
    match retry_count {
        0 | 1 => Duration::from_millis(200),
        2 => Duration::from_millis(500),
        _ => Duration::from_millis(1000),
    }
}

pub(super) fn reqwest_error_summary(err: &reqwest::Error) -> String {
    let mut parts = vec![err.to_string()];
    if err.is_connect() {
        parts.push("kind=connect".to_string());
    }
    if err.is_timeout() {
        parts.push("kind=timeout".to_string());
    }
    if err.is_body() {
        parts.push("kind=body".to_string());
    }
    if err.is_decode() {
        parts.push("kind=decode".to_string());
    }
    if err.is_request() {
        parts.push("kind=request".to_string());
    }
    if let Some(status) = err.status() {
        parts.push(format!("status={}", status.as_u16()));
    }

    let mut source = err.source();
    while let Some(err) = source {
        parts.push(format!("caused by: {err}"));
        source = err.source();
    }
    parts.join("; ")
}

pub(super) async fn ensure_success_response(
    provider_name: &str,
    response: reqwest::Response,
) -> Result<reqwest::Response, GatewayError> {
    let upstream_status = response.status();
    if upstream_status.is_success() {
        return Ok(response);
    }

    let status = StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body_text = response.text().await.unwrap_or_default();
    Err(GatewayError::from_upstream_body(
        status,
        provider_name,
        &body_text,
    ))
}

fn provider_timeout(timeout_secs: u64) -> Duration {
    Duration::from_secs(timeout_secs.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_request_timeout_is_not_applied_to_streaming_requests() {
        let client = reqwest::Client::new();

        let streaming_request =
            apply_total_request_timeout(client.get("https://example.com/stream"), 7, true)
                .build()
                .unwrap();
        assert!(streaming_request.timeout().is_none());

        let unary_request =
            apply_total_request_timeout(client.get("https://example.com/json"), 7, false)
                .build()
                .unwrap();
        assert_eq!(unary_request.timeout(), Some(&Duration::from_secs(7)));
    }
}
