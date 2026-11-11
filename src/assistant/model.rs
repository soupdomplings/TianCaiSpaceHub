use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Model {
    pub id: String,
    pub label: Option<String>,
    pub context_window: u64,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
    pub supports_reasoning: bool,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Protocol {
    #[default]
    Responses,
    ChatCompletions,
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Api {
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub protocol: Protocol,
    #[serde(default = "stream_default")]
    pub stream: bool,
}
fn stream_default() -> bool {
    true
}
impl Api {
    pub(crate) fn endpoint(&self) -> String {
        crate::ai_gateway::config::provider_endpoint(
            &self.base_url,
            match self.protocol {
                Protocol::Responses => "/v1/responses",
                Protocol::ChatCompletions => "/v1/chat/completions",
            },
        )
    }
}
// Credentials stay in memory, never in desktop status, config files or logs.
#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct Configuration {
    pub enabled: bool,
    pub name: String,
    pub model: Model,
    pub api: Api,
    pub notice: Option<String>,
}
impl Configuration {
    pub(crate) fn parse(value: Value) -> Result<Self> {
        ensure!(value["enabled"] != false, "管理员尚未启用天才搭档");
        let config: Self = serde_json::from_value(value)
            .map_err(|_| anyhow::anyhow!("管理员模型配置不完整，请核对天才搭档接口契约"))?;
        ensure!(config.enabled, "管理员尚未启用天才搭档");
        ensure!(
            !config.model.id.trim().is_empty()
                && !config.name.trim().is_empty()
                && (1024..=i64::MAX as u64).contains(&config.model.context_window),
            "管理员模型或上下文长度无效"
        );
        let url = reqwest::Url::parse(&config.api.base_url)
            .map_err(|_| anyhow::anyhow!("管理员模型 API 地址无效"))?;
        ensure!(
            (url.scheme() == "https"
                || (cfg!(test)
                    && url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))))
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "管理员模型 API 须使用不含凭据和查询参数的 HTTPS 地址"
        );
        ensure!(
            !config.api.api_key.trim().is_empty()
                && reqwest::header::HeaderValue::from_str(&format!(
                    "Bearer {}",
                    config.api.api_key
                ))
                .is_ok(),
            "管理员模型 API 凭据无效"
        );
        let model = &config.model;
        ensure!(
            model.reasoning_efforts.iter().all(|s| matches!(
                s.as_str(),
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
            )),
            "管理员返回了不支持的推理强度"
        );
        ensure!(
            !model.supports_reasoning || !model.reasoning_efforts.is_empty(),
            "推理模型缺少可用推理强度"
        );
        ensure!(
            model
                .default_reasoning_effort
                .as_ref()
                .is_none_or(|s| model.reasoning_efforts.contains(s)),
            "默认推理强度未包含在可用列表中"
        );
        ensure!(
            model.supports_reasoning || model.default_reasoning_effort.is_none(),
            "非推理模型不能指定推理强度"
        );
        Ok(config)
    }
    pub(crate) fn catalog(&self) -> Value {
        json!({"models":[{
            "slug":self.model.id,"display_name":self.model.label.as_deref().unwrap_or(&self.model.id),
            "description":"天才小分队管理员配置的天才搭档模型",
            "default_reasoning_level":self.model.default_reasoning_effort,
            "supported_reasoning_levels":self.model.reasoning_efforts.iter().map(|effort|json!({"effort":effort,"description":effort})).collect::<Vec<_>>(),
            "shell_type":"disabled","visibility":"list","supported_in_api":true,"priority":0,
            "base_instructions":"你是天才搭档，只用 Hub 提供的工具协助用户管理配置。",
            "availability_nux":null,"upgrade":null,
            "supports_reasoning_summaries":self.model.supports_reasoning,
            "supports_reasoning_summary_parameter":self.model.supports_reasoning,"support_verbosity":false,
            "default_verbosity":null,"apply_patch_tool_type":null,"web_search_tool_type":"text",
            "truncation_policy":{"mode":"tokens","limit":10000},
            "supports_parallel_tool_calls":false,"context_window":self.model.context_window,
            "effective_context_window_percent":95,"experimental_supported_tools":[],
            "input_modalities":["text"]
        }]})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_uses_ordinary_api_without_remote_tools_compact_or_billing_fields() {
        let valid = json!({"enabled":true,"name":"天才搭档","model":{"id":"fixture","label":null,
            "contextWindow":128000,"reasoningEfforts":["low","medium"],"defaultReasoningEffort":"medium","supportsReasoning":true},
            "api":{"baseUrl":"https://models.example/v1","apiKey":"fixture-key"}});
        assert!(Configuration::parse(valid.clone()).is_ok());
        assert_eq!(
            Configuration::parse(valid.clone()).unwrap().api.endpoint(),
            "https://models.example/v1/responses"
        );
        assert!(
            Configuration::parse(json!({"enabled":false}))
                .err()
                .unwrap()
                .to_string()
                .contains("尚未启用")
        );
        let mut bad = valid.clone();
        bad["api"]["baseUrl"] = "https://secret@models.example/v1".into();
        assert!(Configuration::parse(bad).is_err());
        let mut bad = valid.clone();
        bad["model"]["defaultReasoningEffort"] = "high".into();
        assert!(Configuration::parse(bad).is_err());
        let mut bad = valid;
        bad["model"]["contextWindow"] = 0.into();
        assert!(Configuration::parse(bad).is_err());
    }
}
