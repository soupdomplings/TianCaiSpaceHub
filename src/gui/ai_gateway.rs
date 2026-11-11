use std::cell::RefCell;
use std::rc::Rc;

use wxdragon::prelude::*;
use wxdragon::widgets::dataview::CustomDataViewVirtualListModel;

use crate::ai_gateway::config::{
    AiGatewayConfig, ProviderConfig, ProviderType, is_gmclaw_provider_name,
    is_workbuddy_provider_name, provider_display_base_url,
};

use super::UiHandles;
use super::api::ApiClient;
use super::text::GuiText;
use super::widgets::{LucideIconKind, ProviderLogoKind, lucide_icon_bitmap, provider_logo_bitmap};

pub(super) fn is_minimax_channel(provider_type: &ProviderType, name: &str, base_url: &str) -> bool {
    *provider_type == ProviderType::ChatCompletions
        && (name.trim().to_ascii_lowercase().starts_with("minimax")
            || url::Url::parse(base_url).is_ok_and(|url| {
                matches!(
                    url.host_str(),
                    Some("api.minimax.cn" | "api.minimax.io" | "api.minimaxi.com")
                )
            }))
}

pub(super) type AiGwProviderRows = Rc<RefCell<Vec<AiGwProviderRow>>>;
pub(super) type AiGwProviderModel = Rc<RefCell<CustomDataViewVirtualListModel>>;
pub(super) type PendingAiGwChannelToggle = Rc<RefCell<Option<AiGwChannelToggle>>>;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AiGwProviderRow {
    pub(super) enabled: bool,
    pub(super) name: String,
    pub(super) display_name: String,
    pub(super) provider_type: ProviderType,
    pub(super) compatibility: Option<String>,
    pub(super) base_url: String,
    pub(super) models_url: Option<String>,
    pub(super) weight: u32,
    pub(super) scope_label: String,
}

pub(super) fn provider_scope_display(name: &str, text: GuiText) -> &'static str {
    if let Some(key) = name
        .strip_prefix(crate::desktop::models::PREFIX)
        .and_then(|n| n.split(':').next())
    {
        crate::desktop::clients::ClientId::ALL
            .into_iter()
            .find(|c| c.key() == key)
            .map(|c| c.name())
            .unwrap_or("客户端专用")
    } else if is_workbuddy_provider_name(name) {
        text.ai_gw_scope_workbuddy()
    } else if is_gmclaw_provider_name(name) {
        text.ai_gw_scope_gmclaw()
    } else {
        text.ai_gw_scope_codex()
    }
}

pub(super) fn provider_scope_help(name: &str, text: GuiText) -> &'static str {
    if name.starts_with(crate::desktop::models::PREFIX) {
        "此模型配置只用于所属客户端，可独立调整；同步来源需在对应客户端页面选择。"
    } else if is_workbuddy_provider_name(name) {
        text.ai_gw_scope_workbuddy_help()
    } else if is_gmclaw_provider_name(name) {
        text.ai_gw_scope_gmclaw_help()
    } else {
        text.ai_gw_scope_codex_help()
    }
}

#[derive(Clone)]
pub(super) struct AiGwChannelToggle {
    pub(super) row: usize,
    pub(super) name: String,
    pub(super) enabled: bool,
    pub(super) previous_enabled: bool,
}

pub(super) enum AiGwActionResult {
    Save(Result<(), String>),
    Delete(Result<(), String>),
    ChannelToggle {
        row: usize,
        previous_enabled: bool,
        result: Result<(), String>,
    },
    FilterImageGeneration(Result<bool, String>),
    RequestLogging(Result<bool, String>),
    RequestLogDetails(Result<bool, String>),
}

pub(super) fn save_ai_gw_provider(
    api: &ApiClient,
    mut provider: ProviderConfig,
) -> Result<(), String> {
    provider.base_url = if provider.provider_type == ProviderType::ChatGptResponses {
        crate::ai_gateway::chatgpt_auth::BASE_URL.into()
    } else {
        provider_display_base_url(&provider.base_url)
    };
    let mut config = api.get_app_config()?;
    if let Some(existing) = config
        .ai_gateway
        .providers
        .iter_mut()
        .find(|p| p.name == provider.name)
    {
        *existing = provider;
    } else {
        config.ai_gateway.providers.push(provider);
    }
    api.save_app_config(&config)?;
    Ok(())
}

pub(super) fn delete_ai_gw_provider(api: &ApiClient, name: &str) -> Result<(), String> {
    let mut config = api.get_app_config()?;
    config.ai_gateway.providers.retain(|p| p.name != name);
    api.save_app_config(&config)?;
    Ok(())
}

pub(super) fn save_confirmed_provider_url(
    api: &ApiClient,
    expected: &ProviderConfig,
    corrected: &str,
) -> Result<(), String> {
    if corrected == expected.base_url {
        return Ok(());
    }
    let mut config = api.get_app_config()?;
    if config
        .ai_gateway
        .providers
        .iter()
        .filter(|p| p.name == expected.name)
        .count()
        != 1
    {
        return Ok(());
    }
    let Some(current) = config
        .ai_gateway
        .providers
        .iter_mut()
        .find(|p| p.name == expected.name)
    else {
        return Ok(());
    };
    if serde_json::to_value(&*current).ok() != serde_json::to_value(expected).ok() {
        return Ok(());
    }
    if !current.confirm_base_url(corrected) {
        return Ok(());
    }
    api.save_app_config(&config).map(|_| ())
}

pub(super) fn set_ai_gw_provider_enabled(
    api: &ApiClient,
    name: &str,
    enabled: bool,
) -> Result<(), String> {
    let mut config = api.get_app_config()?;
    let Some(provider) = config
        .ai_gateway
        .providers
        .iter_mut()
        .find(|provider| provider.name == name)
    else {
        return Err(format!("AI Gateway channel not found: {name}"));
    };
    provider.enabled = enabled;
    api.save_app_config(&config)?;
    Ok(())
}

pub(super) fn set_filter_image_generation_tool(
    api: &ApiClient,
    enabled: bool,
) -> Result<bool, String> {
    let mut config = api.get_app_config()?;
    config.ai_gateway.filter_image_generation_tool = enabled;
    api.save_app_config(&config)?;
    Ok(enabled)
}

pub(super) fn refresh_ai_gw_filter_image_generation(handles: &UiHandles, enabled: bool) {
    if !handles.ai_gw_filter_image_generation.has_focus() {
        handles.ai_gw_filter_image_generation.set_value(enabled);
    }
}

pub(super) fn set_request_logging_enabled(api: &ApiClient, enabled: bool) -> Result<bool, String> {
    let mut config = api.get_app_config()?;
    config.ai_gateway.request_logging_enabled = enabled;
    if !enabled {
        config.ai_gateway.request_log_details_enabled = false;
    }
    api.save_app_config(&config)?;
    Ok(enabled)
}

pub(super) fn set_request_log_details_enabled(
    api: &ApiClient,
    enabled: bool,
) -> Result<bool, String> {
    let mut config = api.get_app_config()?;
    config.ai_gateway.request_log_details_enabled =
        enabled && config.ai_gateway.request_logging_enabled;
    api.save_app_config(&config)?;
    Ok(config.ai_gateway.request_log_details_enabled)
}

pub(super) fn refresh_ai_gw_enable_logging(
    handles: &UiHandles,
    enabled: bool,
    details_enabled: bool,
) {
    if !handles.ai_gw_enable_logging.has_focus() {
        handles.ai_gw_enable_logging.set_value(enabled);
    }
    handles.ai_gw_enable_log_details.enable(enabled);
    if !handles.ai_gw_enable_log_details.has_focus() {
        handles
            .ai_gw_enable_log_details
            .set_value(enabled && details_enabled);
    }
    // Show/hide the disabled hint based on logging state
    if enabled {
        handles.request_log_disabled_hint.show(false);
    } else {
        handles.request_log_disabled_hint.show(true);
    }
}

pub(super) fn refresh_ai_gw_provider_list(handles: &UiHandles, config: Option<&AiGatewayConfig>) {
    // A graph/channel jump reveals the exact dedicated scope before selecting it.
    if let Some(provider) = handles.shell.take_source_focus().and_then(|route| {
        config.and_then(|c| {
            c.providers
                .iter()
                .find(|p| crate::ai_gateway::config::provider_route_id(p) == route)
        })
    }) {
        handles.ai_gw_scope.set_selection(provider_scope(provider));
    }
    let rows = ai_gw_provider_list_rows(config, handles.ai_gw_scope.get_selection().unwrap_or(0));
    let mut current_rows = handles.ai_gw_provider_rows.borrow_mut();
    if *current_rows == rows {
        drop(current_rows);
        focus_pending_source(handles, config);
        return;
    }

    let previous_len = current_rows.len();
    let previous_rows = current_rows.clone();
    let selected_row = handles.ai_gw_provider_list.get_selected_row();
    let new_len = rows.len();
    *current_rows = rows;
    drop(current_rows);

    if previous_len != new_len {
        handles.ai_gw_provider_model.borrow_mut().reset(new_len);
        if let Some(row) = selected_row.filter(|row| *row < new_len) {
            handles.ai_gw_provider_list.select_row(row);
        }
    } else {
        let model = handles.ai_gw_provider_model.borrow();
        let current_rows = handles.ai_gw_provider_rows.borrow();
        for row in 0..new_len {
            if previous_rows.get(row) != current_rows.get(row) {
                model.row_changed(row);
            }
        }
    }
    focus_pending_source(handles, config);
}
pub(super) fn focus_pending_source(handles: &UiHandles, config: Option<&AiGatewayConfig>) {
    if handles.shell.current() != super::shell::PageId::Sources {
        return;
    }
    if let Some(route) = handles.shell.take_source_focus() {
        if let Some(provider) = config.and_then(|c| {
            c.providers
                .iter()
                .find(|p| crate::ai_gateway::config::provider_route_id(p) == route)
        }) {
            if let Some(index) = handles
                .ai_gw_provider_rows
                .borrow()
                .iter()
                .position(|r| r.name == provider.name)
            {
                handles.ai_gw_provider_list.select_row(index);
                handles.shell.clear_source_focus();
            }
        }
    }
}

fn provider_scope(provider: &ProviderConfig) -> u32 {
    if provider.is_workbuddy() {
        1
    } else if provider.is_gmclaw() {
        2
    } else if provider.is_client_reserved() {
        3
    } else {
        0
    }
}
fn ai_gw_provider_list_rows(config: Option<&AiGatewayConfig>, scope: u32) -> Vec<AiGwProviderRow> {
    let Some(config) = config else {
        return Vec::new();
    };
    config
        .providers
        .iter()
        .filter(|p| scope == 4 || provider_scope(p) == scope)
        .map(|provider| AiGwProviderRow {
            enabled: provider.enabled,
            name: provider.name.clone(),
            display_name: provider_display_name(provider),
            provider_type: provider.provider_type.clone(),
            compatibility: provider.compatibility.clone(),
            base_url: provider.base_url.clone(),
            models_url: provider.models_url.clone(),
            weight: provider.effective_weight(),
            scope_label: provider
                .desktop_binding
                .as_ref()
                .map(|b| format!("{} 专用", b.client.name()))
                .unwrap_or_default(),
        })
        .collect()
}

pub(super) fn provider_display_name(provider: &ProviderConfig) -> String {
    if let Some(binding) = &provider.desktop_binding {
        return if binding.label.trim().is_empty() {
            binding.model.clone()
        } else {
            binding.label.clone()
        };
    }
    if provider.is_workbuddy() || provider.is_gmclaw() {
        // Alias keys can include a native entry UUID or local routing name.
        // The upstream model list is the actual model, never the channel ID.
        return provider
            .models
            .iter()
            .find(|model| !model.trim().is_empty())
            .or_else(|| {
                provider
                    .model_aliases
                    .values()
                    .find(|model| !model.trim().is_empty())
            })
            .cloned()
            .unwrap_or_else(|| "未配置模型".into());
    }
    provider.name.clone()
}

#[cfg(test)]
mod display_tests {
    use super::*;
    #[test]
    fn dedicated_channels_display_real_models_and_keep_internal_identity() {
        let mut provider = ProviderConfig {
            name: crate::ai_gateway::config::workbuddy_provider_name(
                "11111111-2222-3333-4444-555555555555",
            )
            .unwrap(),
            models: vec!["gpt-6-astra".into()],
            model_aliases: std::collections::BTreeMap::from([(
                "0000-native-id".into(),
                "gpt-6-astra".into(),
            )]),
            ..Default::default()
        };
        let name = provider.name.clone();
        let mut config = AiGatewayConfig::default();
        config.providers.push(provider.clone());
        let rows = ai_gw_provider_list_rows(Some(&config), 4);
        assert_eq!(rows[0].display_name, "gpt-6-astra");
        assert_eq!(rows[0].name, name);
        provider.name =
            crate::ai_gateway::config::gmclaw_provider_name("11111111-2222-3333-4444-555555555555")
                .unwrap();
        assert_eq!(provider_display_name(&provider), "gpt-6-astra");
        provider.models.clear();
        assert_eq!(provider_display_name(&provider), "gpt-6-astra");
        provider.model_aliases.clear();
        assert_eq!(provider_display_name(&provider), "未配置模型");
        provider.name = "我的普通渠道".into();
        assert_eq!(provider_display_name(&provider), "我的普通渠道");
    }
}

pub(super) fn provider_logo_variant(row: &AiGwProviderRow) -> Variant {
    let bitmap = if is_minimax_channel(&row.provider_type, &row.name, &row.base_url) {
        lucide_icon_bitmap(LucideIconKind::Router, 18)
    } else {
        provider_logo_bitmap(provider_logo_kind(row), 18)
    };
    (&bitmap).into()
}

fn provider_logo_kind(row: &AiGwProviderRow) -> ProviderLogoKind {
    match row.provider_type {
        ProviderType::OpenAiResponses | ProviderType::ChatGptResponses => ProviderLogoKind::OpenAi,
        ProviderType::DeepSeekResponses => ProviderLogoKind::DeepSeek,
        ProviderType::KimiResponses => ProviderLogoKind::Kimi,
        ProviderType::GrokResponses => ProviderLogoKind::Grok,
        ProviderType::ChatCompletions if row.compatibility.as_deref() == Some("openai_chat") => {
            ProviderLogoKind::OpenAi
        }
        ProviderType::ChatCompletions => ProviderLogoKind::DeepSeek,
        ProviderType::AnthropicMessages => match row.compatibility.as_deref() {
            Some("glm_anthropic" | "zhipu_anthropic") => ProviderLogoKind::Zhipu,
            _ => ProviderLogoKind::Anthropic,
        },
    }
}

pub(super) fn provider_protocol_display(
    provider_type: &ProviderType,
    compatibility: Option<&str>,
) -> String {
    match provider_type {
        ProviderType::OpenAiResponses => "OpenAI Responses".to_string(),
        ProviderType::ChatGptResponses => "ChatGPT OAuth".to_string(),
        ProviderType::DeepSeekResponses => "DeepSeek Responses".to_string(),
        ProviderType::KimiResponses => "Kimi Responses".to_string(),
        ProviderType::GrokResponses => "Grok Responses".to_string(),
        ProviderType::ChatCompletions => "Chat Completions".to_string(),
        ProviderType::AnthropicMessages => match compatibility {
            Some("glm_anthropic" | "zhipu_anthropic") => "Z.AI Anthropic".to_string(),
            _ => "Anthropic Messages".to_string(),
        },
    }
}

pub(super) fn apply_pending_ai_gw_action(
    handles: &UiHandles,
    frame: &Frame,
    result: AiGwActionResult,
) {
    match result {
        AiGwActionResult::Save(Ok(())) => {
            handles
                .ai_gw_delete_button
                .set_label(handles.text.ai_gw_delete_channel());
            set_ai_gw_actions_enabled(handles, true);
            super::show_info(frame, handles.text.ai_gw_saved());
        }
        AiGwActionResult::Save(Err(err)) => {
            handles
                .ai_gw_delete_button
                .set_label(handles.text.ai_gw_delete_channel());
            set_ai_gw_actions_enabled(handles, true);
            super::show_error(frame, &handles.text.ai_gw_save_failed(&err));
        }
        AiGwActionResult::Delete(Ok(())) => {
            handles
                .ai_gw_delete_button
                .set_label(handles.text.ai_gw_delete_channel());
            set_ai_gw_actions_enabled(handles, true);
            super::show_info(frame, handles.text.ai_gw_deleted());
        }
        AiGwActionResult::Delete(Err(err)) => {
            handles
                .ai_gw_delete_button
                .set_label(handles.text.ai_gw_delete_channel());
            set_ai_gw_actions_enabled(handles, true);
            super::show_error(frame, &handles.text.ai_gw_save_failed(&err));
        }
        AiGwActionResult::ChannelToggle {
            row,
            previous_enabled: _,
            result: Ok(()),
        } => {
            handles
                .ai_gw_provider_model
                .borrow()
                .row_value_changed(row, 0);
        }
        AiGwActionResult::ChannelToggle {
            row,
            previous_enabled,
            result: Err(err),
        } => {
            if let Some(row_data) = handles.ai_gw_provider_rows.borrow_mut().get_mut(row) {
                row_data.enabled = previous_enabled;
            }
            handles
                .ai_gw_provider_model
                .borrow()
                .row_value_changed(row, 0);
            super::show_error(frame, &handles.text.ai_gw_save_failed(&err));
        }
        AiGwActionResult::FilterImageGeneration(Ok(_enabled)) => {}
        AiGwActionResult::FilterImageGeneration(Err(err)) => {
            super::show_error(frame, &handles.text.ai_gw_save_failed(&err));
        }
        AiGwActionResult::RequestLogging(Ok(_enabled)) => {}
        AiGwActionResult::RequestLogging(Err(err)) => {
            super::show_error(frame, &handles.text.ai_gw_save_failed(&err));
        }
        AiGwActionResult::RequestLogDetails(Ok(_enabled)) => {}
        AiGwActionResult::RequestLogDetails(Err(err)) => {
            super::show_error(frame, &handles.text.ai_gw_save_failed(&err));
        }
    }
}

pub(super) fn set_ai_gw_actions_enabled(handles: &UiHandles, enabled: bool) {
    handles.ai_gw_delete_button.enable(enabled);
    handles.ai_gw_new_button.enable(enabled);
    handles.ai_gw_edit_button.enable(enabled);
    handles.ai_gw_provider_list.enable(enabled);
    handles.ai_gw_filter_image_generation.enable(enabled);
}
