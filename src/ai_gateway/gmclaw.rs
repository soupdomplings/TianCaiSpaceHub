//! GMClaw's small Chat request needs vendor-specific policy at the Hub boundary.
//! These rules apply only to its dedicated route, after resolving model aliases.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    config::{ProviderConfig, ProviderType},
    error::GatewayError,
    model::GatewayRequest,
    workbuddy,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemperatureMode {
    #[default]
    Auto,
    Omit,
    Preserve,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GmClawParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub temperature_mode: TemperatureMode,
}

impl GmClawParameters {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(effort) = &self.reasoning_effort
            && !matches!(
                effort.as_str(),
                "none" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        {
            return Err("思考强度无效，请选择自动或列表中的强度 / Invalid reasoning effort".into());
        }
        Ok(())
    }

    pub fn validate_for(&self, provider: &ProviderConfig) -> Result<(), String> {
        self.validate()?;
        if provider.provider_type == ProviderType::AnthropicMessages
            && !is_glm(provider)
            && self.reasoning_effort.as_deref() == Some("none")
        {
            return Err("当前 Claude 原生接入不提供强制关闭思考，请选择自动 / Use automatic reasoning for Claude".into());
        }
        Ok(())
    }
}

fn model_leaf(model: &str) -> String {
    model
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase()
}

fn family(model: &str, prefix: &str) -> bool {
    model == prefix
        || model
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.starts_with('-') || tail.starts_with('.'))
}

fn is_glm(provider: &ProviderConfig) -> bool {
    provider.compatibility.as_deref().is_some_and(|profile| {
        matches!(
            profile.trim().to_ascii_lowercase().as_str(),
            "glm_anthropic" | "zhipu_anthropic"
        )
    })
}

fn is_deepseek(model: &str) -> bool {
    family(model, "deepseek")
}

fn is_openai_reasoning(model: &str) -> bool {
    ["gpt-5", "gpt-6", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| family(model, prefix))
}

fn is_openai_model(model: &str) -> bool {
    model.starts_with("gpt-") || is_openai_reasoning(model)
}

fn is_claude(model: &str) -> bool {
    ["claude", "opus", "sonnet", "haiku"]
        .iter()
        .any(|prefix| family(model, prefix))
}

pub(super) fn uses_chat_cache_controls(provider: &ProviderConfig, model: &str) -> bool {
    !provider.is_gmclaw() || is_openai_model(&model_leaf(model))
}

/// GMClaw never sends these preferences itself; they remain in Hub configuration.
pub(super) fn prepare_chat_request(
    raw: &mut Value,
    provider: &ProviderConfig,
    upstream_model: &str,
) -> Result<(), GatewayError> {
    let parameters = provider.gmclaw_parameters.clone().unwrap_or_default();
    parameters
        .validate_for(provider)
        .map_err(GatewayError::bad_request)?;
    workbuddy::apply_default_reasoning_effort(raw, parameters.reasoning_effort.as_deref());
    let model = model_leaf(upstream_model);
    let deepseek = is_deepseek(&model);
    let chat = provider.provider_type == ProviderType::ChatCompletions;
    if deepseek && let Some(effort) = raw.get("reasoning_effort").and_then(Value::as_str) {
        let mapped = match effort {
            "medium" | "xhigh" => "high",
            other => other,
        };
        raw["reasoning_effort"] = json!(mapped);
    }
    let disable = chat && provider.chat_disable_reasoning;
    if disable {
        raw["reasoning_effort"] = json!("none");
        if let Some(object) = raw.as_object_mut() {
            object.remove("reasoning");
            object.remove("thinking");
        }
    }
    let effort = raw
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let thinking = effort.as_deref().is_some_and(|effort| effort != "none")
        || (deepseek
            && !disable
            && effort.as_deref() != Some("none")
            && raw.pointer("/thinking/type").and_then(Value::as_str) != Some("disabled"));
    let native_claude =
        provider.provider_type == ProviderType::AnthropicMessages && !is_glm(provider);
    // Unknown models keep their supplied sampling values. Recognized reasoning
    // families use vendor defaults in automatic mode; users can explicitly opt out.
    let omit_sampling = parameters.temperature_mode == TemperatureMode::Auto
        && (is_openai_reasoning(&model)
            || (is_claude(&model) && native_claude)
            || (native_claude && thinking)
            || (deepseek && thinking));
    if let Some(object) = raw.as_object_mut() {
        if omit_sampling || parameters.temperature_mode == TemperatureMode::Omit {
            object.remove("temperature");
        }
        // DeepSeek accepts top_p/logprobs even when temperature is ignored.
        if omit_sampling && !deepseek {
            for name in ["top_p", "logprobs", "top_logprobs"] {
                object.remove(name);
            }
        }
        if chat {
            // Preserve the chosen limit, but never send conflicting token fields.
            let limit = object
                .get("max_completion_tokens")
                .or_else(|| object.get("max_output_tokens"))
                .or_else(|| object.get("max_tokens"))
                .cloned();
            for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
                object.remove(field);
            }
            let modern_chat = !deepseek
                && (is_openai_model(&model)
                    || provider.compatibility.as_deref() == Some("openai_chat"));
            if let Some(limit) = limit {
                object.insert(
                    if modern_chat {
                        "max_completion_tokens"
                    } else {
                        "max_tokens"
                    }
                    .into(),
                    limit,
                );
            }
            if deepseek {
                if let Some(effort) = effort.as_deref() {
                    object.remove("reasoning");
                    if effort == "none" {
                        object.remove("reasoning_effort");
                        object.insert("thinking".into(), json!({"type":"disabled"}));
                    } else {
                        object.insert("thinking".into(), json!({"type":"enabled"}));
                        object.insert("reasoning_effort".into(), json!(effort));
                    }
                }
                // OpenAI cache keys are not part of DeepSeek's native contract.
                object.remove("prompt_cache_key");
                object.remove("prompt_cache_retention");
            }
        }
    }
    Ok(())
}

/// Claude versions before adaptive thinking need a budget, not output_config.
/// Keep half of the output limit available for the answer, and never invent a
/// signature when recovering prior tool rounds.
pub(super) fn prepare_anthropic_request(
    request: &mut GatewayRequest,
    provider: &ProviderConfig,
) -> Result<(), GatewayError> {
    if is_glm(provider) {
        return Ok(());
    }
    let name = model_leaf(&request.model).replace('.', "-");
    let manual = [
        "claude-3-7-sonnet",
        "claude-sonnet-4-0",
        "claude-sonnet-4-5",
        "claude-opus-4-0",
        "claude-opus-4-1",
        "claude-opus-4-5",
        "claude-haiku-4-5",
        "claude-sonnet-4-2025",
        "claude-opus-4-2025",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix));
    let Some(reasoning) = request.reasoning.as_mut() else {
        return Ok(());
    };
    let Some(effort) = reasoning.effort.as_deref() else {
        return Ok(());
    };
    if effort == "none" {
        return Err(GatewayError::bad_request(
            "当前 Claude 原生接入不提供强制关闭思考，请选择自动",
        ));
    }
    if manual && reasoning.budget_tokens.is_none() {
        let ceiling = request.max_output_tokens.unwrap_or(8192) / 2;
        if ceiling < 1024 {
            return Err(GatewayError::bad_request(
                "此 Claude 模型启用思考时，最大输出 Token 至少需 2048 / Thinking requires an output limit of at least 2048",
            ));
        }
        let requested = match effort {
            "low" => 1024,
            "medium" => 2048,
            "high" => 4096,
            "xhigh" => 8192,
            "max" => 16384,
            _ => {
                return Err(GatewayError::bad_request(
                    "Unsupported Claude reasoning effort",
                ));
            }
        };
        reasoning.budget_tokens = Some(requested.min(ceiling));
    }
    Ok(())
}

/// Some compatible servers report a failed generation with HTTP 200.
pub(super) fn validate_chat_response(value: &Value, provider: &str) -> Result<(), GatewayError> {
    let error = value.get("error").filter(|error| !error.is_null());
    if error.is_some()
        || value
            .pointer("/choices/0/message")
            .is_none_or(|message| !message.is_object())
    {
        let message = error
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("上游未返回有效的 Chat Completions 结果 / Invalid upstream chat response");
        return Err(GatewayError::upstream_provider(
            axum::http::StatusCode::BAD_GATEWAY,
            provider,
            message,
            None,
            error
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider(kind: ProviderType, effort: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            name: "gmclaw".into(),
            provider_type: kind,
            gmclaw_parameters: Some(GmClawParameters {
                reasoning_effort: effort.map(str::to_owned),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    #[test]
    fn openai_chat_normalizes_tokens_and_removes_unsupported_sampling() {
        let mut request =
            json!({"model":"alias","temperature":0.7,"top_p":0.9,"max_tokens":8192,"messages":[]});
        prepare_chat_request(
            &mut request,
            &provider(ProviderType::ChatCompletions, Some("high")),
            "gpt-5.2",
        )
        .unwrap();
        assert_eq!(request["max_completion_tokens"], 8192);
        assert!(request.get("max_tokens").is_none());
        assert!(request.get("temperature").is_none());
        assert!(request.get("top_p").is_none());
        assert_eq!(request["reasoning_effort"], "high");
    }
    #[test]
    fn deepseek_preserves_limit_and_uses_native_thinking_with_override() {
        let mut source = provider(ProviderType::ChatCompletions, Some("xhigh"));
        source.compatibility = Some("openai_chat".into());
        let mut request =
            json!({"temperature":0.7,"top_p":0.95,"max_completion_tokens":8192,"messages":[]});
        prepare_chat_request(&mut request, &source, "DeepSeek/deepseek-v4-pro").unwrap();
        assert_eq!(request["max_tokens"], 8192);
        assert_eq!(request["top_p"], 0.95);
        assert_eq!(request["thinking"]["type"], "enabled");
        assert_eq!(request["reasoning_effort"], "high");
        source.chat_disable_reasoning = true;
        prepare_chat_request(&mut request, &source, "deepseek-v4-pro").unwrap();
        assert_eq!(request["thinking"]["type"], "disabled");
        assert!(request.get("reasoning_effort").is_none());
    }
    #[test]
    fn automatic_unknown_models_do_not_gain_reasoning_settings() {
        let original = json!({"temperature":0.7,"max_tokens":8192,"messages":[]});
        let mut request = original.clone();
        prepare_chat_request(
            &mut request,
            &provider(ProviderType::OpenAiResponses, None),
            "custom-model",
        )
        .unwrap();
        assert_eq!(request, original);
    }
    #[test]
    fn explicit_temperature_policy_survives_model_defaults() {
        let mut source = provider(ProviderType::OpenAiResponses, None);
        source.gmclaw_parameters.as_mut().unwrap().temperature_mode = TemperatureMode::Preserve;
        let mut request = json!({"temperature":0.3});
        prepare_chat_request(&mut request, &source, "gpt-5.2").unwrap();
        assert_eq!(request["temperature"], 0.3);
        source.gmclaw_parameters.as_mut().unwrap().temperature_mode = TemperatureMode::Omit;
        prepare_chat_request(&mut request, &source, "custom-model").unwrap();
        assert!(request.get("temperature").is_none());
    }
    #[test]
    fn claude_manual_budget_is_bounded_without_reducing_output_limit() {
        let mut request: GatewayRequest = serde_json::from_value(json!({"model":"claude-sonnet-4-5","input":[],"max_output_tokens":8192,"reasoning":{"effort":"max"}})).unwrap();
        prepare_anthropic_request(
            &mut request,
            &provider(ProviderType::AnthropicMessages, None),
        )
        .unwrap();
        assert_eq!(request.max_output_tokens, Some(8192));
        assert_eq!(request.reasoning.unwrap().budget_tokens, Some(4096));
    }
    #[test]
    fn embedded_errors_are_not_successful_empty_replies() {
        assert!(
            validate_chat_response(
                &json!({"error":{"message":"overloaded","code":"busy"}}),
                "gmclaw"
            )
            .is_err()
        );
        assert!(
            validate_chat_response(
                &json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1"}]}}]}),
                "gmclaw"
            )
            .is_ok()
        );
    }
}
