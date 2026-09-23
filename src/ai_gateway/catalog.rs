use std::collections::HashSet;

use once_cell::sync::Lazy;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::config::{AiGatewayConfig, ProviderConfig, ProviderType};

static BASE_MODEL_CATALOG: Lazy<Value> = Lazy::new(|| {
    serde_json::from_str(include_str!("models.json")).expect("embedded AI Gateway model catalog")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModelOption {
    pub slug: String,
    pub display_name: String,
    pub description: String,
}

pub fn visible_catalog_model_options() -> Vec<CatalogModelOption> {
    visible_catalog_model_options_for_config(&AiGatewayConfig::default())
}

pub fn visible_catalog_model_options_for_config(
    config: &AiGatewayConfig,
) -> Vec<CatalogModelOption> {
    let mut options = Vec::new();
    let mut seen = HashSet::new();
    catalog_models()
        .iter()
        .filter(|model| is_catalog_model_visible(model))
        .for_each(|model| {
            let Some(slug) = model_slug(model).map(str::to_string) else {
                return;
            };
            let display_name = model
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(&slug)
                .to_string();
            let description = model
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let option = CatalogModelOption {
                slug,
                display_name,
                description,
            };
            seen.insert(option.slug.clone());
            options.push(option);
        });

    for id in dynamic_model_ids(config) {
        if !seen.insert(id.clone()) {
            continue;
        }
        options.push(CatalogModelOption {
            display_name: config
                .codex_model_profiles
                .iter()
                .find(|profile| profile.id == id)
                .map(|profile| profile.display_name.trim())
                .filter(|name| !name.is_empty())
                .unwrap_or(&id)
                .to_string(),
            slug: id,
            description: "来自已配置模型渠道或手动添加的模型".to_string(),
        });
    }
    options
}

#[cfg(test)]
pub fn configured_models_response(config: &AiGatewayConfig) -> Value {
    build_configured_models_response(config)
}

pub fn configured_models_etag(config: &AiGatewayConfig) -> String {
    let response = build_configured_models_response(config);
    configured_models_etag_from_response(&response)
}

pub fn configured_models_response_with_etag(config: &AiGatewayConfig) -> (Value, String) {
    let response = build_configured_models_response(config);
    let etag = configured_models_etag_from_response(&response);
    (response, etag)
}

fn build_configured_models_response(config: &AiGatewayConfig) -> Value {
    let mut emitted = HashSet::new();
    let mut models = Vec::new();
    let mut priority = 0;

    for model_id in selected_codex_model_ids(config) {
        if !emitted.insert(model_id.to_ascii_lowercase()) {
            continue;
        }

        let model = model_for_id(config, &model_id);
        let Some(mut model) = model else {
            continue;
        };
        if matches!(model_id.as_str(), "deepseek-v4-pro" | "deepseek-v4-flash") {
            normalize_deepseek_model(&mut model);
        }
        if let Some(object) = model.as_object_mut() {
            object.insert("priority".to_string(), json!(priority));
        }
        priority += 1;
        models.push(model);
    }

    json!({ "models": models })
}

fn catalog_models() -> &'static Vec<Value> {
    BASE_MODEL_CATALOG
        .get("models")
        .and_then(Value::as_array)
        .expect("embedded AI Gateway model catalog must contain models array")
}

fn dynamic_model_ids(config: &AiGatewayConfig) -> Vec<String> {
    let mut ids = Vec::new();
    for profile in &config.codex_model_profiles {
        let id = profile.id.trim();
        if !id.is_empty() && !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
    }
    for provider in config
        .providers
        .iter()
        .filter(|p| p.enabled && !p.is_workbuddy())
    {
        for id in provider.models.iter().chain(provider.model_aliases.keys()) {
            let id = id.trim();
            if !id.is_empty() && !ids.iter().any(|known| known == id) {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

fn model_for_id(config: &AiGatewayConfig, id: &str) -> Option<Value> {
    if let Some(model) = catalog_models()
        .iter()
        .find(|model| model_slug(model) == Some(id))
    {
        return is_catalog_model_visible(model).then(|| model.clone());
    }

    let profile = config
        .codex_model_profiles
        .iter()
        .find(|profile| profile.id.eq_ignore_ascii_case(id));
    let provider = config
        .providers
        .iter()
        .filter(|p| p.enabled && !p.is_workbuddy())
        .find(|provider| {
            provider
                .models
                .iter()
                .any(|model| model.eq_ignore_ascii_case(id))
                || provider
                    .model_aliases
                    .keys()
                    .any(|model| model.eq_ignore_ascii_case(id))
        });
    let template = profile
        .and_then(|profile| profile.capability_profile.as_deref())
        .and_then(template_slug)
        .or_else(|| provider.and_then(provider_template_slug))
        .unwrap_or("gpt-5.5");
    let mut model = catalog_models()
        .iter()
        .find(|model| model_slug(model) == Some(template))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(object) = model.as_object_mut() else {
        return None;
    };
    object.insert("slug".to_string(), json!(id));
    object.insert(
        "display_name".to_string(),
        json!(
            profile
                .map(|profile| profile.display_name.trim())
                .filter(|name| !name.is_empty())
                .unwrap_or(id)
        ),
    );
    object.insert(
        "description".to_string(),
        json!("Dynamic model discovered from a configured provider or entered manually."),
    );
    object.insert("supported_in_api".to_string(), Value::Bool(true));
    object.insert("visibility".to_string(), json!("list"));
    object.insert("comp_hash".to_string(), json!("codexhub-dynamic-v1"));
    // A model ID does not prove capabilities. Do not advertise donor-only
    // transports, code mode, speed tiers or multimodal support for a new model.
    object.insert("prefer_websockets".to_string(), json!(false));
    object.insert("use_responses_lite".to_string(), json!(false));
    object.insert("tool_mode".to_string(), Value::Null);
    object.insert("multi_agent_version".to_string(), Value::Null);
    object.insert("service_tiers".to_string(), json!([]));
    object.insert("additional_speed_tiers".to_string(), json!([]));
    object.insert("default_service_tier".to_string(), Value::Null);
    object.insert("context_window".to_string(), json!(32768));
    object.insert("max_context_window".to_string(), json!(32768));
    object.insert("input_modalities".to_string(), json!(["text"]));
    object.insert("supports_image_detail_original".to_string(), json!(false));
    object.insert("supports_search_tool".to_string(), json!(false));
    object.insert("support_verbosity".to_string(), json!(false));
    object.insert("supported_reasoning_levels".to_string(), json!([]));
    object.insert("default_reasoning_level".to_string(), Value::Null);
    object.insert("supports_reasoning_summaries".to_string(), json!(false));
    object.insert(
        "supports_reasoning_summary_parameter".to_string(),
        json!(false),
    );
    if let Some(profile) = profile {
        if let Some(value) = profile.context_window.filter(|v| *v > 0) {
            object.insert("context_window".to_string(), json!(value));
            object.insert("max_context_window".to_string(), json!(value));
        }
        if let Some(value) = profile.max_context_window.filter(|v| *v > 0) {
            let value = value.max(object["context_window"].as_u64().unwrap_or(32768));
            object.insert("max_context_window".to_string(), json!(value));
        }
        if let Some(value) = profile.supports_images {
            object.insert(
                "input_modalities".to_string(),
                if value {
                    json!(["text", "image"])
                } else {
                    json!(["text"])
                },
            );
            object.insert(
                "supports_image_detail_original".to_string(),
                Value::Bool(value),
            );
        }
        if let Some(value) = profile.supports_reasoning {
            object.insert(
                "supports_reasoning_summaries".to_string(),
                Value::Bool(value),
            );
            object.insert(
                "supports_reasoning_summary_parameter".to_string(),
                json!(value),
            );
            if value {
                object.insert(
                    "supported_reasoning_levels".to_string(),
                    json!([
                        {"effort":"low", "description":"Low"},
                        {"effort":"medium", "description":"Medium"},
                        {"effort":"high", "description":"High"}
                    ]),
                );
                object.insert("default_reasoning_level".to_string(), json!("medium"));
            }
        }
    }
    Some(model)
}

fn template_slug(profile: &str) -> Option<&'static str> {
    let profile = profile.to_ascii_lowercase();
    if profile.contains("kimi") {
        Some("kimi-k3")
    } else if profile.contains("deepseek") {
        Some("deepseek-v4-pro")
    } else if profile.contains("anthropic") || profile.contains("claude") {
        Some("Opus-4.8")
    } else if profile.contains("grok") {
        Some("grok-4.6")
    } else if profile.contains("chat") {
        Some("gpt-5.5")
    } else if profile.contains("openai") || profile.contains("responses") {
        Some("gpt-5.6-sol")
    } else {
        None
    }
}

fn provider_template_slug(provider: &ProviderConfig) -> Option<&'static str> {
    match provider.provider_type {
        ProviderType::KimiResponses => Some("kimi-k3"),
        ProviderType::DeepSeekResponses => Some("deepseek-v4-pro"),
        ProviderType::AnthropicMessages => Some("Opus-4.8"),
        ProviderType::GrokResponses => Some("grok-4.6"),
        ProviderType::ChatCompletions => Some("gpt-5.5"),
        ProviderType::OpenAiResponses => Some("gpt-5.6-sol"),
    }
}

fn configured_models_etag_from_response(response: &Value) -> String {
    let serialized = serde_json::to_vec(response)
        .expect("configured models response should always serialize for etag");
    let digest = Sha256::digest(serialized);
    format!("\"sha256:{}\"", hex::encode(digest))
}

fn selected_codex_model_ids(config: &AiGatewayConfig) -> Vec<String> {
    config
        .codex_visible_models
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn model_slug(model: &Value) -> Option<&str> {
    model.get("slug").and_then(Value::as_str)
}

fn is_catalog_model_visible(model: &Value) -> bool {
    model
        .get("supported_in_api")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && model.get("visibility").and_then(Value::as_str) == Some("list")
}

fn normalize_deepseek_model(model: &mut Value) {
    let Some(slug) = model_slug(model) else {
        return;
    };
    if !slug.starts_with("deepseek-") {
        return;
    }

    let is_vision_flash = slug == "deepseek-v4-flash";
    if let Some(object) = model.as_object_mut() {
        object.insert("web_search_tool_type".to_string(), json!("text"));
        object.insert(
            "supports_image_detail_original".to_string(),
            Value::Bool(is_vision_flash),
        );
        object.insert(
            "input_modalities".to_string(),
            if is_vision_flash {
                json!(["text", "image"])
            } else {
                json!(["text"])
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(models: &[&str]) -> AiGatewayConfig {
        AiGatewayConfig {
            codex_visible_models: models.iter().map(|model| model.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn configured_models_response_uses_codex_visible_models() {
        let config = config(&[
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "grok-4.6",
            "gpt-5.5",
            "deepseek-v4-pro",
            "deepseek-v4-flash",
            "GLM-5.3",
            "GLM-5.3-Flash",
            "custom-model",
            "codex-auto-review",
        ]);

        let response = configured_models_response(&config);
        let slugs: Vec<&str> = response["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["slug"].as_str().unwrap())
            .collect();

        assert_eq!(
            slugs,
            vec![
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "grok-4.6",
                "gpt-5.5",
                "deepseek-v4-pro",
                "deepseek-v4-flash",
                "GLM-5.3",
                "GLM-5.3-Flash",
                "custom-model"
            ]
        );
        assert_eq!(response["models"][3]["display_name"], "Grok-4.6");
        assert_eq!(
            response["models"][3]["comp_hash"],
            "codexhub-grok-summary-v1"
        );
        assert_eq!(response["models"][5]["display_name"], "DeepSeek-V4-Pro");
        assert_eq!(response["models"][5]["comp_hash"], "3000");
        assert_eq!(response["models"][5]["apply_patch_tool_type"], "freeform");
        assert_eq!(response["models"][5]["supports_search_tool"], true);
        assert_eq!(
            response["models"][5]["supports_image_detail_original"],
            false
        );
        assert_eq!(response["models"][5]["input_modalities"], json!(["text"]));
        assert_eq!(
            response["models"][6]["supports_image_detail_original"],
            true
        );
        assert_eq!(
            response["models"][6]["input_modalities"],
            json!(["text", "image"])
        );
        assert_eq!(
            response["models"][7]["comp_hash"],
            "codexhub-anthropic-summary-v1"
        );
        assert_eq!(
            response["models"][8]["comp_hash"],
            "codexhub-anthropic-summary-v1"
        );
        assert_eq!(
            response["models"][8]["input_modalities"],
            json!(["text", "image"])
        );
    }

    #[test]
    fn configured_models_etag_is_stable_for_same_response() {
        let config = config(&["deepseek-v4-pro", "deepseek-v4-flash"]);

        let (response, etag) = configured_models_response_with_etag(&config);

        assert_eq!(response, configured_models_response(&config));
        assert_eq!(etag, configured_models_etag(&config));
        assert!(etag.starts_with("\"sha256:"));
        assert!(etag.ends_with('"'));
    }

    #[test]
    fn configured_models_etag_changes_when_visible_models_change() {
        let base_config = config(&["deepseek-v4-pro"]);
        let changed_config = config(&["deepseek-v4-pro", "deepseek-v4-flash"]);

        assert_ne!(
            configured_models_etag(&base_config),
            configured_models_etag(&changed_config)
        );
    }

    #[test]
    fn configured_models_response_exposes_unknown_configured_model() {
        let config = config(&["custom-model"]);

        let response = configured_models_response(&config);
        assert_eq!(response["models"][0]["slug"], "custom-model");
        assert_eq!(response["models"][0]["comp_hash"], "codexhub-dynamic-v1");
    }

    #[test]
    fn configured_models_response_skips_hidden_catalog_model() {
        let config = config(&["codex-auto-review"]);

        let response = configured_models_response(&config);
        assert!(response["models"].as_array().unwrap().is_empty());
    }

    #[test]
    fn configured_models_response_uses_provider_model_as_dynamic_entry() {
        let mut config = config(&["gpt-6-luna"]);
        config.providers.push(ProviderConfig {
            name: "openai".to_string(),
            provider_type: ProviderType::OpenAiResponses,
            models: vec!["gpt-6-luna".to_string()],
            ..ProviderConfig::default()
        });

        let response = configured_models_response(&config);
        let model = &response["models"][0];
        assert_eq!(model["slug"], "gpt-6-luna");
        assert_eq!(model["display_name"], "gpt-6-luna");
        assert_eq!(model["supported_in_api"], true);
        assert_eq!(model["visibility"], "list");
        assert_eq!(model["comp_hash"], "codexhub-dynamic-v1");
        assert_eq!(model["use_responses_lite"], false);
        assert_eq!(model["prefer_websockets"], false);
        assert_eq!(model["context_window"], 32768);
        assert_eq!(model["input_modalities"], json!(["text"]));
        assert_eq!(model["supported_reasoning_levels"], json!([]));
    }

    #[test]
    fn dynamic_profiles_roundtrip_and_refresh_etag() {
        let mut config = config(&["new-model"]);
        let before = configured_models_etag(&config);
        config
            .codex_model_profiles
            .push(super::super::config::CodexModelProfile {
                id: "new-model".into(),
                display_name: "New Model".into(),
                context_window: Some(64000),
                max_context_window: Some(128000),
                supports_images: Some(true),
                supports_reasoning: Some(true),
                ..Default::default()
            });
        let encoded = toml::to_string(&config).unwrap();
        let decoded: AiGatewayConfig = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.codex_model_profiles, config.codex_model_profiles);
        assert_ne!(before, configured_models_etag(&decoded));
        let response = configured_models_response(&decoded);
        assert_eq!(response["models"][0]["display_name"], "New Model");
        assert_eq!(response["models"][0]["context_window"], 64000);
        assert_eq!(response["models"][0]["max_context_window"], 128000);
        assert_eq!(
            response["models"][0]["input_modalities"],
            json!(["text", "image"])
        );
        assert_eq!(response["models"][0]["default_reasoning_level"], "medium");
    }

    #[test]
    fn deepseek_models_preserve_apply_patch_tool_from_catalog() {
        let response = configured_models_response(&config(&["deepseek-v4-pro"]));
        let model = &response["models"][0];
        assert_eq!(model["apply_patch_tool_type"], "freeform");
        assert_eq!(model["supports_image_detail_original"], false);
        assert_eq!(model["input_modalities"], json!(["text"]));
        assert_eq!(model["web_search_tool_type"], "text");
        assert_eq!(model["supports_search_tool"], true);

        let vision_model =
            &configured_models_response(&config(&["deepseek-v4-flash"]))["models"][0];
        assert_eq!(vision_model["supports_image_detail_original"], true);
        assert_eq!(vision_model["input_modalities"], json!(["text", "image"]));
    }

    #[test]
    fn configured_models_response_returns_empty_when_no_models_configured() {
        let config = config(&[]);

        let response = configured_models_response(&config);
        assert!(response["models"].as_array().unwrap().is_empty());
    }

    #[test]
    fn catalog_model_visibility_requires_api_support_and_list_visibility() {
        assert!(is_catalog_model_visible(&json!({
            "supported_in_api": true,
            "visibility": "list"
        })));
        assert!(!is_catalog_model_visible(&json!({
            "supported_in_api": false,
            "visibility": "list"
        })));
        assert!(!is_catalog_model_visible(&json!({
            "supported_in_api": true,
            "visibility": "hide"
        })));
        assert!(!is_catalog_model_visible(&json!({
            "visibility": "list"
        })));
    }

    #[test]
    fn all_visible_catalog_models_declare_comp_hash() {
        let missing = catalog_models()
            .iter()
            .filter(|model| is_catalog_model_visible(model))
            .filter_map(|model| {
                let comp_hash = model
                    .get("comp_hash")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default();
                comp_hash
                    .is_empty()
                    .then(|| model_slug(model).unwrap_or("<missing-slug>").to_string())
            })
            .collect::<Vec<_>>();

        assert!(missing.is_empty(), "models missing comp_hash: {missing:?}");
    }

    #[test]
    fn non_openai_comp_hashes_follow_protocol_families() {
        let comp_hash = |slug: &str| {
            catalog_models()
                .iter()
                .find(|model| model_slug(model) == Some(slug))
                .and_then(|model| model.get("comp_hash"))
                .and_then(Value::as_str)
                .expect("catalog model should declare comp_hash")
        };

        assert_eq!(comp_hash("grok-4.6"), "codexhub-grok-summary-v1");
        assert_eq!(comp_hash("deepseek-v4-pro"), "3000");
        assert_eq!(comp_hash("deepseek-v4-flash"), "3000");
        for slug in ["GLM-5.3", "GLM-5.3-Flash"] {
            assert_eq!(
                comp_hash(slug),
                "codexhub-anthropic-summary-v1",
                "model {slug}"
            );
        }
        assert_eq!(comp_hash("Opus-4.8"), "codexhub-anthropic-summary-v1");
        assert_eq!(comp_hash("Sonnet-4.6"), "codexhub-anthropic-summary-v1");

        assert_ne!(comp_hash("grok-4.6"), comp_hash("gpt-5.6-sol"));
        assert_ne!(comp_hash("deepseek-v4-pro"), comp_hash("gpt-5.5"));
        assert_ne!(comp_hash("Opus-4.8"), comp_hash("deepseek-v4-pro"));
    }

    #[test]
    fn codexhub_third_party_models_use_372k_context_window() {
        for slug in [
            "kimi-k3",
            "grok-4.6",
            "GLM-5.3",
            "GLM-5.3-Flash",
            "Opus-4.8",
            "Sonnet-4.6",
        ] {
            let model = catalog_models()
                .iter()
                .find(|model| model_slug(model) == Some(slug))
                .expect("catalog model should exist");

            assert_eq!(model["context_window"], 372_000, "model {slug}");
            assert_eq!(model["max_context_window"], 372_000, "model {slug}");
        }
    }

    #[test]
    fn kimi_model_uses_official_efforts_and_native_responses_tools() {
        let response = configured_models_response(&config(&["kimi-k3"]));
        let model = &response["models"][0];
        assert_eq!(model["slug"], "kimi-k3");
        assert_eq!(model["context_window"], 372_000);
        assert_eq!(model["max_context_window"], 372_000);
        assert_eq!(model["effective_context_window_percent"], 95);
        assert_eq!(model["default_reasoning_level"], "high");
        let efforts: Vec<_> = model["supported_reasoning_levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|effort| effort["effort"].as_str().unwrap())
            .collect();
        assert_eq!(efforts, ["low", "high", "max"]);
        assert_eq!(model["input_modalities"], json!(["text", "image"]));
        assert_eq!(model["shell_type"], "shell_command");
        assert_eq!(
            model["truncation_policy"],
            json!({"mode":"bytes", "limit":10000})
        );
        assert_eq!(model["support_verbosity"], false);
        assert_eq!(model["supports_reasoning_summaries"], true);
        assert_eq!(model["default_reasoning_summary"], "none");
        assert!(
            !model["base_instructions"]
                .as_str()
                .unwrap()
                .trim()
                .is_empty()
        );
        for slug in ["deepseek-v4-pro", "deepseek-v4-flash"] {
            let deepseek = catalog_models()
                .iter()
                .find(|model| model_slug(model) == Some(slug))
                .expect("DeepSeek catalog model should exist");
            assert_eq!(model["base_instructions"], deepseek["base_instructions"]);
        }
        assert_eq!(model["prefer_websockets"], false);
        assert_eq!(model["use_responses_lite"], false);
        assert_eq!(model["supports_search_tool"], false);
        assert_eq!(model["apply_patch_tool_type"], "freeform");
        assert_eq!(model["web_search_tool_type"], "text");
        assert_eq!(model["comp_hash"], "codexhub-kimi-summary-v1");
        assert!(
            visible_catalog_model_options()
                .iter()
                .any(|option| option.slug == "kimi-k3")
        );
        assert!(
            !visible_catalog_model_options()
                .iter()
                .any(|option| { matches!(option.slug.as_str(), "k3-256k" | "kimi-k3-256k") })
        );
    }

    #[test]
    fn deepseek_models_use_current_official_capabilities() {
        for (slug, display_name, description, priority) in [
            (
                "deepseek-v4-flash",
                "DeepSeek-V4-Flash",
                "Latest frontier agentic coding model.",
                1,
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek-V4-Pro",
                "Most capable frontier agentic coding model.",
                2,
            ),
        ] {
            let model = catalog_models()
                .iter()
                .find(|model| model_slug(model) == Some(slug))
                .expect("DeepSeek catalog model should exist");

            assert_eq!(model["display_name"], display_name, "model {slug}");
            assert_eq!(model["description"], description, "model {slug}");
            assert_eq!(model["prefer_websockets"], false, "model {slug}");
            assert_eq!(model["use_responses_lite"], false, "model {slug}");
            assert_eq!(model["context_window"], 372_000, "model {slug}");
            assert_eq!(model["max_context_window"], 372_000, "model {slug}");
            assert_eq!(
                model["effective_context_window_percent"], 95,
                "model {slug}"
            );
            assert_eq!(model["comp_hash"], "3000", "model {slug}");
            assert_eq!(model["default_reasoning_level"], "high", "model {slug}");
            assert_eq!(model["minimal_client_version"], "0.144.0", "model {slug}");
            assert_eq!(model["priority"], priority, "model {slug}");
            assert_eq!(model["supports_search_tool"], true, "model {slug}");
            if slug == "deepseek-v4-flash" {
                assert_eq!(
                    model["supports_image_detail_original"], true,
                    "model {slug}"
                );
                assert_eq!(
                    model["input_modalities"],
                    json!(["text", "image"]),
                    "model {slug}"
                );
            } else {
                assert_eq!(
                    model["supports_image_detail_original"], false,
                    "model {slug}"
                );
                assert_eq!(model["input_modalities"], json!(["text"]), "model {slug}");
            }
            assert_eq!(
                model["availability_nux"]["message"],
                "不管你是贫穷还是富有, deepseek让所有人都感受到AI的乐趣, 人民的AI",
                "model {slug}"
            );
        }
    }

    #[test]
    fn gpt_lite_models_use_current_official_capabilities() {
        for (slug, priority) in [
            ("gpt-6-astra", 1),
            ("gpt-5.6-sol", 6),
            ("gpt-5.6-terra", 7),
            ("gpt-5.6-luna", 8),
        ] {
            let model = catalog_models()
                .iter()
                .find(|model| model_slug(model) == Some(slug))
                .expect("catalog model should exist");

            assert_eq!(model["context_window"], 272_000, "model {slug}");
            assert_eq!(model["max_context_window"], 872_000, "model {slug}");
            assert_eq!(model["use_responses_lite"], true, "model {slug}");
            assert_eq!(
                model["supports_reasoning_summary_parameter"], true,
                "model {slug}"
            );
            assert_eq!(model["visibility"], "list", "model {slug}");
            assert_eq!(model["priority"], priority, "model {slug}");
        }
    }

    #[test]
    fn gpt_5_5_remains_visible_and_older_models_are_removed() {
        let gpt_5_5 = catalog_models()
            .iter()
            .find(|model| model_slug(model) == Some("gpt-5.5"))
            .expect("gpt-5.5 should exist");
        assert_eq!(gpt_5_5["visibility"], "list");
        assert_eq!(gpt_5_5["supports_reasoning_summary_parameter"], true);
        assert_eq!(gpt_5_5.get("availability_nux"), Some(&Value::Null));

        for slug in ["gpt-5.4", "gpt-5.4-mini"] {
            assert!(
                !catalog_models()
                    .iter()
                    .any(|model| model_slug(model) == Some(slug))
            );
        }
    }

    #[test]
    fn visible_catalog_model_options_returns_listable_api_models() {
        let options = visible_catalog_model_options();
        let slugs = options
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<Vec<_>>();
        for expected in [
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "grok-4.6",
            "gpt-5.5",
            "gpt-6-astra",
            "GLM-5.3",
            "GLM-5.3-Flash",
        ] {
            assert!(
                slugs.contains(&expected),
                "missing visible model {expected}"
            );
        }
        assert!(!slugs.contains(&"gpt-5.2"));
        assert!(
            options
                .iter()
                .all(|model| !model.slug.trim().is_empty() && !model.display_name.trim().is_empty())
        );
    }
}
