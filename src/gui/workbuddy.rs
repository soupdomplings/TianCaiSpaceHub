use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use wxdragon::prelude::*;

use crate::{
    ai_gateway::config::{ProviderConfig, ProviderType, provider_display_base_url},
    config::AppConfig,
    workbuddy_config::{
        self, WorkBuddyConfigStatus, WorkBuddyModelConfig, WorkBuddyReasoningConfig,
    },
};

use super::{
    api::ApiClient,
    show_error, show_info,
    text::GuiText,
    theme,
    widgets::{card_section, text_field_row},
};

#[derive(Clone, Debug)]
pub(super) struct WorkBuddyProviderOption {
    name: String,
    display_name: String,
    upstream_url: String,
    upstream_api_key: String,
    upstream_protocol: String,
    models: Vec<String>,
    model_aliases: BTreeMap<String, String>,
    compatibility: Option<String>,
}

type WorkBuddyProviderOptions = Rc<RefCell<Vec<WorkBuddyProviderOption>>>;

#[derive(Clone)]
pub(super) struct WorkBuddyTab {
    pub(super) page: ScrolledWindow,
    local_url: TextCtrl,
    local_api_key: TextCtrl,
    provider: Choice,
    upstream_url: TextCtrl,
    upstream_api_key: TextCtrl,
    model: ComboBox,
    protocol: Choice,
    default_effort: Choice,
    supported_efforts: TextCtrl,
    cache_key: TextCtrl,
    cache_hint: StaticText,
    text: GuiText,
    save_button: Button,
    restore_button: Button,
    reload_button: Button,
    status: StaticText,
    in_flight: Arc<AtomicBool>,
    provider_options: WorkBuddyProviderOptions,
}

#[derive(Debug)]
pub(super) enum WorkBuddyActionResult {
    Save(Result<WorkBuddyConfigStatus, String>),
    Restore(Result<WorkBuddyConfigStatus, String>),
    Refresh(Result<Vec<WorkBuddyProviderOption>, String>),
}

const PROVIDER_REFRESH_ATTEMPTS: usize = 30;
const PROVIDER_REFRESH_RETRY_DELAY: Duration = Duration::from_millis(250);

pub(super) fn create(parent: &Notebook, text: GuiText) -> WorkBuddyTab {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_card_alt);

    let existing_status = workbuddy_config::load().ok();
    let existing = existing_status
        .as_ref()
        .map(|status| status.model.clone())
        .unwrap_or_default();
    let root = BoxSizer::builder(Orientation::Vertical).build();

    let (connection_box, connection_section) = card_section(&page, text.workbuddy_connection());
    let hint = StaticText::builder(&connection_box)
        .with_label(text.workbuddy_connection_help())
        .build();
    hint.set_foreground_color(theme::theme().ink_muted);
    hint.wrap(920);
    connection_section.add(
        &hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        8,
    );

    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(10)
        .with_hgap(14)
        .build();
    grid.add_growable_col(1, 1);
    let local_url = text_field_row(
        &connection_box,
        &grid,
        text.workbuddy_local_url(),
        workbuddy_config::DEFAULT_WORKBUDDY_URL,
    );
    local_url.set_editable(false);
    local_url.set_tooltip(text.workbuddy_local_url_help());
    let local_api_key = text_field_row(
        &connection_box,
        &grid,
        text.workbuddy_local_api_key(),
        workbuddy_config::DEFAULT_WORKBUDDY_API_KEY,
    );
    local_api_key.set_editable(false);
    local_api_key.set_tooltip(text.workbuddy_local_api_key_help());
    let provider_label = StaticText::builder(&connection_box)
        .with_label(text.workbuddy_provider())
        .build();
    provider_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(
        &provider_label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        0,
    );
    let provider = Choice::builder(&connection_box)
        .with_size(Size::new(420, -1))
        .build();
    provider.set_tooltip(text.workbuddy_provider_help());
    grid.add(&provider, 1, SizerFlag::Expand, 0);
    let upstream_url = text_field_row(
        &connection_box,
        &grid,
        text.workbuddy_upstream_url(),
        &existing.upstream_url,
    );
    upstream_url.set_editable(false);
    upstream_url.set_tooltip(text.workbuddy_upstream_url_help());
    let upstream_api_key = text_field_row(
        &connection_box,
        &grid,
        text.workbuddy_upstream_api_key(),
        &existing.upstream_api_key,
    );
    upstream_api_key.set_editable(false);
    upstream_api_key.set_tooltip(text.workbuddy_upstream_api_key_help());
    let model_label = StaticText::builder(&connection_box)
        .with_label(text.workbuddy_model())
        .build();
    model_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(
        &model_label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        0,
    );
    let model = ComboBox::builder(&connection_box)
        .with_value(&existing.provider_model)
        .with_style(ComboBoxStyle::ReadOnly)
        .with_size(Size::new(420, -1))
        .build();
    model.set_background_color(theme::theme().bg_muted);
    model.set_foreground_color(theme::theme().ink_primary);
    model.set_tooltip(text.workbuddy_model_help());
    model.set_min_size(Size::new(420, 30));
    grid.add(&model, 1, SizerFlag::Expand, 0);

    let protocol_label = StaticText::builder(&connection_box)
        .with_label(text.workbuddy_protocol())
        .build();
    protocol_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(
        &protocol_label,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        0,
    );
    let protocol = Choice::builder(&connection_box)
        .with_choices(vec![
            "openai-responses".to_string(),
            "openai-chat".to_string(),
            "anthropic-messages".to_string(),
        ])
        .with_size(Size::new(420, -1))
        .build();
    protocol.set_selection(protocol_index(&existing.upstream_protocol));
    grid.add(&protocol, 1, SizerFlag::Expand, 0);

    connection_section.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        10,
    );
    root.add(
        &connection_box,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );

    let (reasoning_box, reasoning_section) = card_section(&page, text.workbuddy_reasoning());
    let reasoning_grid = FlexGridSizer::builder(0, 2)
        .with_vgap(10)
        .with_hgap(14)
        .build();
    reasoning_grid.add_growable_col(1, 1);
    let effort_label = StaticText::builder(&reasoning_box)
        .with_label(text.workbuddy_default_effort())
        .build();
    effort_label.set_foreground_color(theme::theme().ink_secondary);
    reasoning_grid.add(&effort_label, 0, SizerFlag::AlignCenterVertical, 0);
    let default_effort = Choice::builder(&reasoning_box)
        .with_size(Size::new(420, -1))
        .build();
    reasoning_grid.add(&default_effort, 1, SizerFlag::Expand, 0);
    let supported_efforts = text_field_row(
        &reasoning_box,
        &reasoning_grid,
        text.workbuddy_supported_efforts(),
        &existing.reasoning.supported_efforts.join(", "),
    );
    supported_efforts.set_editable(false);
    supported_efforts.set_tooltip(text.workbuddy_efforts_help());
    let cache_key = text_field_row(
        &reasoning_box,
        &reasoning_grid,
        text.workbuddy_cache_key(),
        &workbuddy_config::default_cache_key_for_provider(&existing.upstream_provider),
    );
    cache_key.set_editable(false);
    cache_key.set_tooltip(text.workbuddy_cache_key_help());
    reasoning_section.add_sizer(
        &reasoning_grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        10,
    );
    let cache_hint = StaticText::builder(&reasoning_box).with_label("").build();
    cache_hint.set_foreground_color(theme::theme().ink_muted);
    reasoning_section.add(&cache_hint, 0, SizerFlag::Expand | SizerFlag::All, 10);
    root.add(
        &reasoning_box,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );

    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let status = StaticText::builder(&page).with_label("").build();
    status.set_foreground_color(theme::theme().ink_muted);
    actions.add(
        &status,
        1,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        12,
    );
    let reload_button = Button::builder(&page)
        .with_label(text.workbuddy_reload())
        .build();
    let restore_button = Button::builder(&page)
        .with_label(text.workbuddy_restore())
        .build();
    restore_button.enable(
        existing_status
            .as_ref()
            .is_some_and(|status| status.backup_exists)
            || workbuddy_config::backup_exists(),
    );
    let save_button = Button::builder(&page)
        .with_label(text.workbuddy_save())
        .build();
    actions.add(&reload_button, 0, SizerFlag::Right, 8);
    actions.add(&restore_button, 0, SizerFlag::Right, 8);
    actions.add(&save_button, 0, SizerFlag::Right, 0);
    root.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        10,
    );

    let path = workbuddy_config::config_path()
        .to_string_lossy()
        .to_string();
    let path_hint = StaticText::builder(&page)
        .with_label(&format!("{}: {path}", text.workbuddy_config_path()))
        .build();
    path_hint.set_foreground_color(theme::theme().ink_muted);
    root.add(
        &path_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        10,
    );

    page.set_sizer(root, true);
    page.set_scroll_rate(0, 10);
    page.layout();
    page.fit_inside();

    let tab = WorkBuddyTab {
        page,
        local_url,
        local_api_key,
        provider,
        upstream_url,
        upstream_api_key,
        model,
        protocol,
        default_effort,
        supported_efforts,
        cache_key,
        cache_hint,
        text,
        save_button,
        restore_button,
        reload_button,
        status,
        in_flight: Arc::new(AtomicBool::new(false)),
        provider_options: Rc::new(RefCell::new(Vec::new())),
    };
    apply_model(&tab, &existing);
    tab
}

pub(super) fn bind_actions(
    tab: &WorkBuddyTab,
    api: &ApiClient,
    frame: &Frame,
    text: GuiText,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
) {
    let provider_tab = tab.clone();
    tab.provider.on_selection_changed(move |_| {
        let Some(index) = provider_tab.provider.get_selection() else {
            return;
        };
        apply_provider_option(&provider_tab, index as usize);
    });
    let model_tab = tab.clone();
    tab.model
        .on_selection_changed(move |_| refresh_model_settings(&model_tab));
    let protocol_tab = tab.clone();
    tab.protocol
        .on_selection_changed(move |_| refresh_model_settings(&protocol_tab));

    let save_button = tab.save_button;
    let tab_for_save = tab.clone();
    let api_for_save = api.clone();
    let gui_tx_for_save = gui_tx.clone();
    let frame_for_save = *frame;
    let frame_for_reload = *frame;
    save_button.on_click(move |_| {
        if tab_for_save.in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        let model = match build_model(&tab_for_save) {
            Ok(model) => model,
            Err(error) => {
                tab_for_save.in_flight.store(false, Ordering::SeqCst);
                show_error(&frame_for_save, &error);
                return;
            }
        };
        tab_for_save.save_button.enable(false);
        tab_for_save.restore_button.enable(false);
        tab_for_save.status.set_label(text.workbuddy_saving());
        let tab = tab_for_save.clone();
        let api = api_for_save.clone();
        let gui_tx = gui_tx_for_save.clone();
        thread::spawn(move || {
            let result = save_model(&api, &model);
            tab.in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::WorkBuddy(WorkBuddyActionResult::Save(
                result,
            )));
            wxdragon::wake_up_idle();
        });
    });

    let tab_for_restore = tab.clone();
    let api_for_restore = api.clone();
    let gui_tx_for_restore = gui_tx.clone();
    tab.restore_button.on_click(move |_| {
        if tab_for_restore.in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        tab_for_restore.save_button.enable(false);
        tab_for_restore.restore_button.enable(false);
        tab_for_restore.status.set_label(text.workbuddy_restoring());
        let tab = tab_for_restore.clone();
        let api = api_for_restore.clone();
        let gui_tx = gui_tx_for_restore.clone();
        thread::spawn(move || {
            let result = api.restore_workbuddy_config();
            tab.in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::WorkBuddy(
                WorkBuddyActionResult::Restore(result),
            ));
            wxdragon::wake_up_idle();
        });
    });

    let tab_for_reload = tab.clone();
    let api_for_reload = api.clone();
    let gui_tx_for_reload = gui_tx.clone();
    tab.reload_button.on_click(move |_| {
        match workbuddy_config::load() {
            Ok(status) => {
                apply_model(&tab_for_reload, &status.model);
                tab_for_reload.restore_button.enable(status.backup_exists);
                tab_for_reload.status.set_label(&status.path);
            }
            Err(error) => {
                tab_for_reload
                    .restore_button
                    .enable(workbuddy_config::backup_exists());
                show_error(&frame_for_reload, &error.to_string());
            }
        }
        refresh_providers(&tab_for_reload, &api_for_reload, &gui_tx_for_reload);
    });

    // Populate the provider and model selectors as soon as the page is bound.
    // The fields are initialized from the saved WorkBuddy file in `create`;
    // re-apply it here as well so a page opened during daemon startup never
    // falls back to empty/default values while the provider list is loading.
    if let Ok(status) = workbuddy_config::load() {
        apply_model(tab, &status.model);
        tab.restore_button.enable(status.backup_exists);
    }
    refresh_providers(tab, api, gui_tx);
}

pub(super) fn apply_result(
    tab: &WorkBuddyTab,
    frame: &Frame,
    text: GuiText,
    api: &ApiClient,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    result: WorkBuddyActionResult,
) {
    tab.save_button.enable(true);
    match result {
        WorkBuddyActionResult::Save(Ok(status)) => {
            tab.restore_button.enable(status.backup_exists);
            tab.status.set_label(text.workbuddy_saved());
            show_info(frame, text.workbuddy_saved());
        }
        WorkBuddyActionResult::Save(Err(error)) => {
            tab.restore_button.enable(workbuddy_config::backup_exists());
            tab.status.set_label(text.workbuddy_save_failed());
            show_error(frame, &error);
        }
        WorkBuddyActionResult::Restore(Ok(status)) => {
            apply_model(tab, &status.model);
            tab.restore_button.enable(status.backup_exists);
            tab.status.set_label(text.workbuddy_restored());
            show_info(frame, text.workbuddy_restored());
            refresh_providers(tab, api, gui_tx);
        }
        WorkBuddyActionResult::Restore(Err(error)) => {
            tab.restore_button.enable(workbuddy_config::backup_exists());
            tab.status.set_label(text.workbuddy_restore_failed());
            show_error(frame, &error);
        }
        WorkBuddyActionResult::Refresh(Ok(options)) => {
            apply_provider_options(tab, options);
            tab.reload_button.enable(true);
        }
        WorkBuddyActionResult::Refresh(Err(error)) => {
            tab.reload_button.enable(true);
            tab.status.set_label(&error);
        }
    }
}

fn refresh_providers(
    tab: &WorkBuddyTab,
    api: &ApiClient,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
) {
    tab.reload_button.enable(false);
    let api = api.clone();
    let gui_tx = gui_tx.clone();
    thread::spawn(move || {
        let mut result = Err(String::from("provider list is not available yet"));
        for attempt in 0..PROVIDER_REFRESH_ATTEMPTS {
            result = api
                .get_app_config()
                .map(|config| provider_options_from_config(&config));
            if result.is_ok() || attempt + 1 == PROVIDER_REFRESH_ATTEMPTS {
                break;
            }
            thread::sleep(PROVIDER_REFRESH_RETRY_DELAY);
        }
        let _ = gui_tx.send(super::GuiMessage::WorkBuddy(
            WorkBuddyActionResult::Refresh(result),
        ));
        wxdragon::wake_up_idle();
    });
}

fn provider_options_from_config(config: &AppConfig) -> Vec<WorkBuddyProviderOption> {
    config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| !provider.is_workbuddy())
        .filter_map(provider_option)
        .collect()
}

fn provider_option(provider: &ProviderConfig) -> Option<WorkBuddyProviderOption> {
    let mut models = provider.models.clone();
    for alias in provider.model_aliases.keys() {
        if !models.iter().any(|model| model.eq_ignore_ascii_case(alias)) {
            models.push(alias.clone());
        }
    }
    if models.is_empty() || provider.base_url.trim().is_empty() {
        return None;
    }
    let disabled_suffix = if provider.enabled { "" } else { " (disabled)" };
    Some(WorkBuddyProviderOption {
        name: provider.name.clone(),
        display_name: format!("{}{}", provider.name, disabled_suffix),
        upstream_url: provider_display_base_url(&provider.base_url),
        upstream_api_key: provider.api_key.clone(),
        upstream_protocol: match &provider.provider_type {
            ProviderType::OpenAiResponses => "openai-responses",
            ProviderType::AnthropicMessages => "anthropic-messages",
            _ => "openai-chat",
        }
        .to_string(),
        models,
        model_aliases: provider.model_aliases.clone(),
        compatibility: provider.compatibility.clone(),
    })
}

fn apply_provider_options(tab: &WorkBuddyTab, options: Vec<WorkBuddyProviderOption>) {
    let saved_status = workbuddy_config::load().ok();
    let saved_model = saved_status
        .as_ref()
        .map(|status| status.model.provider_model.clone())
        .unwrap_or_default();
    let configured_provider = saved_status
        .as_ref()
        .map(|status| status.model.upstream_provider.clone())
        .unwrap_or_default();
    let current_model = tab.model.get_value();
    // Prefer the persisted provider model. `ComboBox::clear()` below can
    // discard the pending value in some wxWidgets builds, so reading the
    // control alone is not sufficient to restore the saved selection.
    let preferred_model = if !saved_model.trim().is_empty() {
        saved_model.as_str()
    } else {
        current_model.as_str()
    };
    let current_url = tab.upstream_url.get_value();
    let current_key = tab.upstream_api_key.get_value();
    let selected = options.iter().position(|option| {
        if !configured_provider.trim().is_empty()
            && option.name.eq_ignore_ascii_case(configured_provider.trim())
        {
            return true;
        }
        let model_match = option
            .models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(&current_model));
        let url_match = !current_url.trim().is_empty()
            && provider_display_base_url(&current_url).eq_ignore_ascii_case(&option.upstream_url);
        let key_match =
            current_key.trim().is_empty() || current_key.trim() == option.upstream_api_key.trim();
        (url_match && key_match) || (current_url.trim().is_empty() && model_match)
    });

    *tab.provider_options.borrow_mut() = options;
    tab.provider.clear();
    for option in tab.provider_options.borrow().iter() {
        tab.provider.append(&option.display_name);
    }

    if let Some(index) = selected {
        tab.provider.set_selection(index as u32);
        apply_provider_option_for_model(tab, index, Some(preferred_model));
        if let Some(saved) = saved_status.as_ref().map(|status| &status.model)
            && tab.provider_options.borrow()[index].name == saved.upstream_provider
            && tab
                .model
                .get_value()
                .eq_ignore_ascii_case(&saved.provider_model)
        {
            // A manually selected protocol must survive reopening and refresh.
            apply_saved_model_settings(tab, saved);
        }
    } else {
        tab.model.clear();
        tab.model.set_value(preferred_model);
    }
}

fn apply_provider_option(tab: &WorkBuddyTab, index: usize) {
    apply_provider_option_for_model(tab, index, None);
}

fn apply_provider_option_for_model(
    tab: &WorkBuddyTab,
    index: usize,
    preferred_model: Option<&str>,
) {
    let Some(option) = tab.provider_options.borrow().get(index).cloned() else {
        return;
    };
    let current_model = preferred_model
        .map(str::to_string)
        .unwrap_or_else(|| tab.model.get_value());
    tab.upstream_url.set_value(&option.upstream_url);
    tab.upstream_api_key.set_value(&option.upstream_api_key);
    tab.protocol
        .set_selection(protocol_index(&option.upstream_protocol));
    tab.model.clear();
    for model in &option.models {
        tab.model.append(model);
    }
    if let Some(model_index) = option
        .models
        .iter()
        .position(|model| model.eq_ignore_ascii_case(&current_model))
    {
        tab.model.set_selection(model_index as u32);
        // Explicitly restore the text as well as the selection. This keeps
        // `get_value()` stable across wxWidgets versions after `clear()`.
        tab.model.set_value(&option.models[model_index]);
    } else if !option.models.is_empty() {
        tab.model.set_selection(0);
    } else {
        tab.model.set_value(&current_model);
    }
    refresh_model_settings(tab);
}

fn selected_protocol(tab: &WorkBuddyTab) -> &'static str {
    match tab.protocol.get_selection() {
        Some(1) => "openai-chat",
        Some(2) => "anthropic-messages",
        _ => "openai-responses",
    }
}

fn reasoning_for_selection(
    provider: Option<&WorkBuddyProviderOption>,
    protocol: &str,
    model: &str,
    preferred: &str,
) -> WorkBuddyReasoningConfig {
    let upstream_model = provider
        .and_then(|provider| {
            provider
                .model_aliases
                .get(model)
                .or_else(|| {
                    provider
                        .model_aliases
                        .iter()
                        .find(|(alias, _)| alias.eq_ignore_ascii_case(model))
                        .map(|(_, upstream)| upstream)
                })
                .map(String::as_str)
        })
        .unwrap_or(model);
    WorkBuddyReasoningConfig::for_model(
        protocol,
        upstream_model,
        provider.and_then(|p| p.compatibility.as_deref()),
        preferred,
    )
}

fn apply_reasoning(tab: &WorkBuddyTab, reasoning: &WorkBuddyReasoningConfig) {
    tab.default_effort.clear();
    for effort in &reasoning.supported_efforts {
        tab.default_effort.append(effort);
    }
    let index = reasoning
        .supported_efforts
        .iter()
        .position(|effort| effort == &reasoning.default_effort)
        .unwrap_or(0);
    tab.default_effort.set_selection(index as u32);
    tab.supported_efforts
        .set_value(&reasoning.supported_efforts.join(", "));
}

fn apply_cache_settings(tab: &WorkBuddyTab, protocol: &str, provider: &str) {
    let uses_key = workbuddy_config::uses_prompt_cache_key(protocol);
    tab.cache_key.enable(uses_key);
    let hint = if uses_key {
        tab.cache_key
            .set_value(&workbuddy_config::default_cache_key_for_provider(provider));
        tab.text.workbuddy_cache_key_help()
    } else {
        tab.cache_key
            .set_value(tab.text.workbuddy_cache_key_not_needed());
        tab.text.workbuddy_anthropic_cache_help()
    };
    tab.cache_key.set_tooltip(hint);
    tab.cache_hint.set_label(hint);
    tab.cache_hint.wrap(920);
    tab.page.layout();
    tab.page.fit_inside();
}

fn refresh_model_settings(tab: &WorkBuddyTab) {
    let options = tab.provider_options.borrow();
    let provider = tab
        .provider
        .get_selection()
        .and_then(|i| options.get(i as usize));
    let protocol = selected_protocol(tab);
    let reasoning = reasoning_for_selection(
        provider,
        protocol,
        &tab.model.get_value(),
        &tab.default_effort
            .get_string_selection()
            .unwrap_or_default(),
    );
    apply_reasoning(tab, &reasoning);
    apply_cache_settings(
        tab,
        protocol,
        provider.map(|p| p.name.as_str()).unwrap_or(""),
    );
}

fn build_model(tab: &WorkBuddyTab) -> Result<WorkBuddyModelConfig, String> {
    let mut model = WorkBuddyModelConfig::default();
    model.url = workbuddy_config::DEFAULT_WORKBUDDY_URL.to_string();
    model.api_key = workbuddy_config::DEFAULT_WORKBUDDY_API_KEY.to_string();
    model.upstream_url = clean(&tab.upstream_url.get_value());
    model.upstream_api_key = clean(&tab.upstream_api_key.get_value());
    let provider = tab
        .provider
        .get_selection()
        .and_then(|index| tab.provider_options.borrow().get(index as usize).cloned())
        .ok_or_else(|| "select an upstream provider first".to_string())?;
    model.upstream_provider = provider.name.clone();
    model.provider_model = clean(&tab.model.get_value());
    if !provider
        .models
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&model.provider_model))
    {
        return Err("select a model provided by the selected upstream provider".to_string());
    }
    model.id = model.provider_model.clone();
    model.name = model.provider_model.clone();
    model.upstream_protocol = selected_protocol(tab).to_string();
    model.reasoning = reasoning_for_selection(
        Some(&provider),
        &model.upstream_protocol,
        &model.provider_model,
        &tab.default_effort
            .get_string_selection()
            .unwrap_or_default(),
    );
    model.upstream_default_reasoning_effort = model.reasoning.default_effort.clone();
    if model.provider_model.is_empty() || model.upstream_url.is_empty() {
        return Err("upstream URL and model are required".to_string());
    }
    model.extra.remove("cacheKey");
    if workbuddy_config::uses_prompt_cache_key(&model.upstream_protocol) {
        let cache_key = workbuddy_config::default_cache_key_for_provider(&provider.name);
        model
            .extra
            .insert("cacheKey".to_string(), serde_json::Value::String(cache_key));
    }
    Ok(model)
}

fn save_model(
    api: &ApiClient,
    model: &WorkBuddyModelConfig,
) -> Result<WorkBuddyConfigStatus, String> {
    // Write the WorkBuddy file first so its existing contents are protected
    // before the Hub provider configuration is changed.
    let status = workbuddy_config::save(model).map_err(|error| error.to_string())?;
    let mut config = api.get_app_config()?;
    workbuddy_config::apply_provider(&mut config, model);
    if let Err(error) = api.save_app_config(&config) {
        // The backup still points at the previous WorkBuddy file, so restore
        // it when updating the Hub config fails and keep both sides aligned.
        let _ = workbuddy_config::restore_backup();
        return Err(format!("save TianCaiSpace Hub provider: {error}"));
    }
    Ok(status)
}

fn apply_model(tab: &WorkBuddyTab, model: &WorkBuddyModelConfig) {
    tab.local_url
        .set_value(workbuddy_config::DEFAULT_WORKBUDDY_URL);
    tab.local_api_key
        .set_value(workbuddy_config::DEFAULT_WORKBUDDY_API_KEY);
    tab.upstream_url.set_value(&model.upstream_url);
    tab.upstream_api_key.set_value(&model.upstream_api_key);
    tab.model.set_value(&model.provider_model);
    apply_saved_model_settings(tab, model);
}

fn apply_saved_model_settings(tab: &WorkBuddyTab, model: &WorkBuddyModelConfig) {
    tab.protocol
        .set_selection(protocol_index(&model.upstream_protocol));
    let options = tab.provider_options.borrow();
    let provider = options
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(&model.upstream_provider));
    let reasoning = reasoning_for_selection(
        provider,
        &model.upstream_protocol,
        &model.provider_model,
        &model.reasoning.default_effort,
    );
    apply_reasoning(tab, &reasoning);
    apply_cache_settings(tab, &model.upstream_protocol, &model.upstream_provider);
}

fn clean(value: &str) -> String {
    value.replace('\0', "").trim().to_string()
}

fn protocol_index(protocol: &str) -> u32 {
    match protocol {
        "openai-chat" => 1,
        "anthropic-messages" => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_gateway::config::AiGatewayConfig;

    fn provider(name: &str, provider_type: ProviderType) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            provider_type,
            base_url: "https://provider.example".to_string(),
            api_key: "secret".to_string(),
            models: vec!["model-a".to_string(), "model-b".to_string()],
            ..ProviderConfig::default()
        }
    }

    #[test]
    fn provider_options_reuse_ai_gateway_connections_and_skip_workbuddy() {
        let mut workbuddy = provider("workbuddy", ProviderType::OpenAiResponses);
        workbuddy.models = vec!["private-model".to_string()];
        let config = AppConfig {
            ai_gateway: AiGatewayConfig {
                providers: vec![
                    provider("openai", ProviderType::OpenAiResponses),
                    provider("claude", ProviderType::AnthropicMessages),
                    provider("compatible", ProviderType::ChatCompletions),
                    workbuddy,
                ],
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };

        let options = provider_options_from_config(&config);

        assert_eq!(options.len(), 3);
        assert_eq!(options[0].upstream_protocol, "openai-responses");
        assert_eq!(options[1].upstream_protocol, "anthropic-messages");
        assert_eq!(options[2].upstream_protocol, "openai-chat");
        assert_eq!(options[0].upstream_url, "https://provider.example/v1");
        assert_eq!(options[0].upstream_api_key, "secret");
        assert_eq!(options[0].models, vec!["model-a", "model-b"]);
    }

    #[test]
    fn provider_options_skip_incomplete_connections() {
        let mut without_models = provider("empty-models", ProviderType::OpenAiResponses);
        without_models.models.clear();
        let mut without_url = provider("empty-url", ProviderType::OpenAiResponses);
        without_url.base_url.clear();
        let config = AppConfig {
            ai_gateway: AiGatewayConfig {
                providers: vec![without_models, without_url],
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };

        assert!(provider_options_from_config(&config).is_empty());
    }

    #[test]
    fn mixed_provider_aliases_select_the_actual_models_efforts() {
        let mut source = provider("mixed", ProviderType::ChatCompletions);
        source
            .model_aliases
            .insert("writing".into(), "claude-opus-new".into());
        source
            .model_aliases
            .insert("coding".into(), "gpt-6-luna".into());
        let option = provider_option(&source).unwrap();
        let claude = reasoning_for_selection(Some(&option), "openai-chat", "WRITING", "max");
        assert_eq!(
            claude.supported_efforts,
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(claude.default_effort, "max");
        let gpt = reasoning_for_selection(
            Some(&option),
            "openai-chat",
            "coding",
            &claude.default_effort,
        );
        assert_eq!(gpt.default_effort, "high");
        assert!(!gpt.supported_efforts.iter().any(|v| v == "max"));
        let native = reasoning_for_selection(Some(&option), "anthropic-messages", "coding", "max");
        assert_eq!(native.default_effort, "max");
        source.compatibility = Some("glm_anthropic".into());
        let option = provider_option(&source).unwrap();
        let glm = reasoning_for_selection(Some(&option), "anthropic-messages", "model-a", "max");
        assert_eq!(glm.supported_efforts, ["high", "max"]);
    }
}
