//! The target client's tools stay with that client; Hub relays model protocol.
use super::{GatewayError, ProviderConfig, StatusCode, provider_timeout};
use std::time::Duration;

pub(super) async fn execute(
    request: reqwest::Request,
    provider: &ProviderConfig,
    context: Option<&super::super::context::GatewayContext>,
) -> Result<reqwest::Response, GatewayError> {
    crate::desktop::workbuddy_builtin::validate_provider(provider)
        .map_err(|e| GatewayError::bad_request(e.to_string()))?;
    let state = context
        .and_then(|c| c.config_state.as_ref())
        .and_then(std::sync::Weak::upgrade)
        .ok_or_else(|| {
            GatewayError::bad_request("请通过 Hub 客户端入口使用 WorkBuddy 内置模型分享")
        })?;
    let binding = provider
        .workbuddy_builtin
        .as_ref()
        .ok_or_else(|| GatewayError::bad_request("WorkBuddy 模型绑定不存在"))?;
    let deadline = context
        .and_then(|c| c.upstream_deadline)
        .unwrap_or_else(|| tokio::time::Instant::now() + provider_timeout(provider.timeout_secs));
    if let Some(runtime) = state
        .workbuddy_builtin
        .direct_for(binding)
        .await
        .map_err(|e| GatewayError::bad_request(e.to_string()))?
    {
        return runtime.forward(binding, request, deadline).await;
    }
    let reference = state
        .workbuddy_builtin
        .reference(binding)
        .await
        .map_err(|e| GatewayError::bad_request(e.to_string()))?;
    transport(request, provider, reference, deadline).await
}

async fn transport(
    mut request: reqwest::Request,
    provider: &ProviderConfig,
    reference: crate::desktop::workbuddy_builtin::LocalReference,
    deadline: tokio::time::Instant,
) -> Result<reqwest::Response, GatewayError> {
    *request.url_mut() = reference
        .url
        .parse()
        .map_err(|_| GatewayError::bad_request("WorkBuddy 本机会话地址无效"))?;
    // VlmProxy disallows browser origins. Only the in-memory ledger credentials
    // reach this loopback request; neither client nor upstream login headers do.
    let headers = request.headers_mut();
    headers.clear();
    headers.insert(
        "content-type",
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    let mut token = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", reference.token))
        .map_err(|_| GatewayError::bad_request("WorkBuddy 本机会话授权无效"))?;
    token.set_sensitive(true);
    headers.insert("authorization", token);
    let mut context = reqwest::header::HeaderValue::from_str(&reference.context)
        .map_err(|_| GatewayError::bad_request("WorkBuddy 本机会话上下文无效"))?;
    context.set_sensitive(true);
    headers.insert("x-workbuddy-mcp-context", context);
    *request.timeout_mut() = Some(deadline.saturating_duration_since(tokio::time::Instant::now()));
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| GatewayError::bad_request("无法建立 WorkBuddy 本机分享连接"))?;
    // Do not replay a generation on errors or redirects. Dropping its response
    // on cancellation disconnects VlmProxy, which aborts the host's upstream.
    let response = tokio::time::timeout_at(deadline, client.execute(request))
        .await
        .map_err(|_| GatewayError::upstream_timeout())?
        .map_err(|_| {
            GatewayError::upstream(
                StatusCode::BAD_GATEWAY,
                "WorkBuddy 分享连接已中断，请检查客户端及登录状态；本次未自动重发",
            )
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let message = match status.as_u16() {
            401 | 403 => "WorkBuddy 登录或当前模型权限不可用，请重新读取分享状态并检查当前账户",
            429 => "WorkBuddy 当前账户模型调用受限，请稍后重试或检查该账户/组织额度",
            301..=399 => "WorkBuddy 分享接口发生跳转，已停止转发，请重新核对客户端版本",
            404 => "WorkBuddy 当前模型或接口不可用，请重新读取当前账户模型目录",
            _ => "WorkBuddy 模型调用未完成，请根据 HTTP 状态检查客户端；本次未自动重发",
        };
        // Authentication-bearing host errors may echo headers; don't retain raw.
        return Err(GatewayError::upstream_provider(
            status,
            &provider.name,
            message,
            None,
            None,
        ));
    }
    Ok(response)
}

#[cfg(test)]
mod tests;
