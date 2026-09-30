use super::*;
use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub fn build_client(
    proxy: &crate::config::OutboundProxyConfig,
    local_port: Option<u16>,
) -> Result<Client, String> {
    crate::outbound_http::apply_async_proxy(
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(5)),
        proxy,
        local_port,
    )
    .and_then(|builder| builder.build().map_err(Into::into))
    .map_err(|_| "无法初始化导入网络连接 / Cannot initialize import connection".into())
}

async fn body(mut response: Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE as u64)
    {
        return Err("响应过大 / Import response too large".into());
    }
    let mut result = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "响应读取失败 / Response read failed")?
    {
        if result.len() + chunk.len() > MAX_RESPONSE {
            return Err("响应过大 / Import response too large".into());
        }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}

pub async fn resolve(
    client: &Client,
    link: &ImportLink,
    local: bool,
) -> Result<ImportDraft, String> {
    // No automatic retries: the server consumes the ticket exactly once.
    let response = client.post(format!("{}/api/v1/external-import/resolve", link.origin))
        .json(&serde_json::json!({"target":"tiancaispace-hub", "schema_version":1, "ticket":link.ticket}))
        .send().await.map_err(|_| "兑换失败或超时，请从站点重新发起导入 / Resolve failed; request a new import link")?;
    if response.status() != StatusCode::OK {
        return Err(match response.status().as_u16() {
            400 => "导入格式或版本不支持 / Unsupported import format",
            403 => "此 Key 已不可导入 / Key is no longer available",
            404 => "导入码无效、已过期或已使用，请重新发起 / Ticket expired or used",
            429 => "请求过于频繁，请稍后重新发起 / Rate limited",
            503 => "站点暂不可用，请稍后重新发起 / Site unavailable",
            _ => "站点兑换失败（不跟随重定向），请重新发起 / Resolve failed",
        }
        .into());
    }
    let envelope: ResolveEnvelope = serde_json::from_slice(&body(response).await?)
        .map_err(|_| "站点返回的导入数据无效 / Invalid import response")?;
    if envelope.code != 0 {
        return Err("站点拒绝导入，请重新发起 / Import rejected".into());
    }
    envelope
        .data
        .ok_or("站点未返回导入数据 / Missing import data")?
        .into_draft(link, local)
}

pub fn models_endpoint(draft: &ImportDraft, local: bool) -> Result<Url, String> {
    validate_endpoint(
        draft.provider.models_url.as_deref().unwrap_or(&format!(
            "{}/models",
            draft.provider.base_url.trim_end_matches('/')
        )),
        local,
    )
}

/// Check destination consent before calling: the model endpoint may differ from
/// the issuing site. Never send the credential to fallback/redirect destinations.
pub async fn fetch_models(
    client: &Client,
    draft: &mut ImportDraft,
    local: bool,
) -> Result<(), String> {
    let destination = models_endpoint(draft, local)?;
    let response = client
        .get(destination)
        .bearer_auth(&draft.provider.api_key)
        .send()
        .await
        .map_err(|_| "模型列表获取失败或超时 / Model discovery failed")?;
    if response.status() != StatusCode::OK {
        return Err("模型列表接口返回错误 / Model discovery rejected".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&body(response).await?)
        .map_err(|_| "模型列表格式无效 / Invalid model response")?;
    let list = value
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or("模型列表格式无效 / Invalid model response")?;
    let fetched = list
        .iter()
        .map(|v| {
            v.get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or("模型 ID 无效 / Invalid model ID".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut models = normalized_models(&fetched)?;
    // The issuing site decides which models match the chosen protocol. Discovery
    // cannot expand a nonempty protocol-scoped list into unrelated model families.
    if !draft.provider.models.is_empty() {
        models.retain(|m| draft.provider.models.contains(m));
    }
    draft.provider.models = models;
    if draft.provider.models.is_empty() {
        return Err(
            "没有获取到适用于该渠道的模型，可先禁用保存后补全 / No compatible models returned"
                .into(),
        );
    }
    Ok(())
}
