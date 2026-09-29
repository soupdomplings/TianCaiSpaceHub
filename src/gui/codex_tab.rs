use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use tokio::sync::mpsc::UnboundedSender;
use wxdragon::prelude::*;

use crate::ai_gateway::catalog::visible_catalog_model_options;
use crate::ai_gateway::config::AiGatewayConfig;
use crate::config::LocalConnectionMode;

use super::api::{ApiClient, ConfigureRequest};
use super::text::GuiText;
use super::theme;
use super::widgets::card_section;
use super::{
    DashboardRefresh, confirm_uninstall_codex_app_config, ensure_service_ready_for_action,
    force_dashboard_refresh, schedule_dashboard_refresh, show_error, show_info,
};

type CodexModelSlugs = Rc<Vec<String>>;
type CodexModelChecks = Rc<Vec<CheckBox>>;
type CodexModelsInitialized = Rc<Cell<bool>>;
type CodexConfigured = Rc<Cell<bool>>;
type CodexHubReady = Rc<Cell<bool>>;
type CodexServiceEnabled = Rc<Cell<bool>>;

#[derive(Clone)]
pub(super) struct CodexTab {
    pub(super) page: ScrolledWindow,
    text: GuiText,
    inject_button: Button,
    clear_button: Button,
    session_history_button: Button,
    enhanced_launch_button: Button,
    websocket_checkbox: CheckBox,
    websocket_enabled: Rc<Cell<bool>>,
    websocket_saving: Rc<Cell<bool>>,
    save_models_button: Button,
    sync_models_button: Button,
    fetch_models_button: Button,
    model_provider: Choice,
    model_provider_names: Rc<RefCell<Vec<String>>>,
    custom_models_input: TextCtrl,
    model_checks: CodexModelChecks,
    model_slugs: CodexModelSlugs,
    models_initialized: CodexModelsInitialized,
    configured: CodexConfigured,
    config_up_to_date: Rc<Cell<bool>>,
    config_saving: Rc<Cell<bool>>,
    remote_ready: CodexHubReady,
    service_enabled: CodexServiceEnabled,
    local_connection_mode: Rc<Cell<LocalConnectionMode>>,
}

pub(super) fn create(parent: &Notebook, text: GuiText) -> CodexTab {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_card_alt);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let (local_config_box, local_config_section) = card_section(&page, text.codex_local_config());
    local_config_box.set_tooltip(text.codex_local_config_help());
    let local_config_hint = StaticText::builder(&local_config_box)
        .with_label(text.codex_local_config_help())
        .build();
    local_config_hint.set_foreground_color(theme::theme().ink_muted);
    local_config_hint.wrap(760);
    let inject_button = Button::builder(&local_config_box)
        .with_label(text.inject_codex_access())
        .build();
    inject_button.set_tooltip(text.inject_codex_access_help());
    inject_button.enable(false);
    let enhanced_launch_button = Button::builder(&local_config_box)
        .with_label(text.codex_enhanced_launch())
        .build();
    enhanced_launch_button.set_tooltip(text.codex_enhanced_launch_help());
    enhanced_launch_button.enable(false);
    let clear_button = Button::builder(&local_config_box)
        .with_label(text.clear_codex_access())
        .build();
    clear_button.set_tooltip(text.clear_codex_access_help());
    clear_button.enable(false);
    local_config_section.add(
        &local_config_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );
    let websocket_checkbox = CheckBox::builder(&local_config_box)
        .with_label(text.provider_websocket())
        .with_value(false)
        .build();
    websocket_checkbox.set_tooltip(text.provider_websocket_help());
    websocket_checkbox.set_foreground_color(theme::theme().ink_primary);
    websocket_checkbox.enable(false);
    local_config_section.add(
        &websocket_checkbox,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );
    let local_config_actions = BoxSizer::builder(Orientation::Horizontal).build();
    local_config_actions.add_stretch_spacer(1);
    local_config_actions.add(
        &enhanced_launch_button,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        12,
    );
    local_config_actions.add(
        &inject_button,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        8,
    );
    local_config_actions.add(&clear_button, 0, SizerFlag::AlignCenterVertical, 0);
    local_config_section.add_sizer(
        &local_config_actions,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );

    let session_hint = StaticText::builder(&local_config_box)
        .with_label(text.codex_session_history_help())
        .build();
    session_hint.set_foreground_color(theme::theme().ink_muted);
    session_hint.wrap(760);
    let session_history_button = Button::builder(&local_config_box)
        .with_label(text.open_codex_session_history())
        .build();
    session_history_button.enable(false);
    local_config_section.add(
        &session_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );
    let session_actions = BoxSizer::builder(Orientation::Horizontal).build();
    session_actions.add_stretch_spacer(1);
    session_actions.add(
        &session_history_button,
        0,
        SizerFlag::AlignCenterVertical,
        0,
    );
    local_config_section.add_sizer(
        &session_actions,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        10,
    );
    sizer.add(
        &local_config_box,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        12,
    );

    let (models_box, models_section) = card_section(&page, text.codex_visible_models());
    models_box.set_tooltip(text.codex_visible_models_help());
    let models_hint = StaticText::builder(&models_box)
        .with_label(text.codex_visible_models_help())
        .build();
    models_hint.set_foreground_color(theme::theme().ink_muted);
    models_hint.wrap(980);
    models_section.add(
        &models_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );
    let models_scope_warning = StaticText::builder(&models_box)
        .with_label(text.codex_visible_models_scope_warning())
        .build();
    models_scope_warning.set_foreground_color(theme::theme().error);
    models_scope_warning.wrap(980);
    models_section.add(
        &models_scope_warning,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );
    let models_warning = StaticText::builder(&models_box)
        .with_label(text.codex_visible_models_warning())
        .build();
    models_warning.set_foreground_color(theme::theme().warn);
    models_warning.wrap(980);
    models_section.add(
        &models_warning,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );

    let custom_models_hint = StaticText::builder(&models_box)
        .with_label(text.codex_custom_models_help())
        .build();
    custom_models_hint.set_foreground_color(theme::theme().ink_muted);
    custom_models_hint.wrap(980);
    models_section.add(
        &custom_models_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );
    let model_provider = Choice::builder(&models_box).build();
    model_provider.append(text.codex_model_no_provider());
    model_provider.set_selection(0);
    models_section.add(
        &model_provider,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );
    let custom_models_input = TextCtrl::builder(&models_box)
        .with_value("")
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::Default)
        .build();
    custom_models_input.set_min_size(Size::new(760, 90));
    custom_models_input.set_foreground_color(theme::theme().ink_primary);
    models_section.add(
        &custom_models_input,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );

    let model_options = visible_catalog_model_options();
    let model_slugs: CodexModelSlugs = Rc::new(
        model_options
            .iter()
            .map(|model| model.slug.clone())
            .collect(),
    );
    let model_grid = FlexGridSizer::builder(0, 3)
        .with_vgap(8)
        .with_hgap(24)
        .build();
    for col in 0..3 {
        model_grid.add_growable_col(col, 1);
    }
    let mut model_checks = Vec::new();
    for model in &model_options {
        let label = if model.display_name == model.slug {
            model.slug.clone()
        } else {
            format!("{} ({})", model.display_name, model.slug)
        };
        let checkbox = CheckBox::builder(&models_box)
            .with_label(&label)
            .with_value(true)
            .build();
        checkbox.enable(false);
        checkbox.set_foreground_color(theme::theme().ink_primary);
        model_grid.add(
            &checkbox,
            0,
            SizerFlag::Expand | SizerFlag::AlignCenterVertical,
            0,
        );
        model_checks.push(checkbox);
    }
    let model_checks: CodexModelChecks = Rc::new(model_checks);
    models_section.add_sizer(
        &model_grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );

    let save_models_button = Button::builder(&models_box)
        .with_label(text.save_codex_models())
        .build();
    let sync_models_button = Button::builder(&models_box)
        .with_label(text.sync_codex_models())
        .build();
    let fetch_models_button = Button::builder(&models_box)
        .with_label(text.ai_gw_fetch_models())
        .build();
    fetch_models_button.enable(false);
    save_models_button.enable(false);
    sync_models_button.enable(false);
    let models_actions = BoxSizer::builder(Orientation::Horizontal).build();
    models_actions.add_stretch_spacer(1);
    models_actions.add(&sync_models_button, 0, SizerFlag::Right, 8);
    models_actions.add(&fetch_models_button, 0, SizerFlag::Right, 8);
    models_actions.add(&save_models_button, 0, SizerFlag::Right, 0);
    models_section.add_sizer(
        &models_actions,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        10,
    );
    sizer.add(
        &models_box,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        12,
    );

    page.set_sizer(sizer, true);
    page.set_scroll_rate(0, 10);
    page.layout();
    page.fit_inside();

    CodexTab {
        page,
        text,
        inject_button,
        clear_button,
        session_history_button,
        enhanced_launch_button,
        websocket_checkbox,
        websocket_enabled: Rc::new(Cell::new(false)),
        websocket_saving: Rc::new(Cell::new(false)),
        save_models_button,
        sync_models_button,
        fetch_models_button,
        model_provider,
        model_provider_names: Rc::new(RefCell::new(Vec::new())),
        custom_models_input,
        model_checks,
        model_slugs,
        models_initialized: Rc::new(Cell::new(false)),
        configured: Rc::new(Cell::new(false)),
        config_up_to_date: Rc::new(Cell::new(false)),
        config_saving: Rc::new(Cell::new(false)),
        remote_ready: Rc::new(Cell::new(false)),
        service_enabled: Rc::new(Cell::new(false)),
        local_connection_mode: Rc::new(Cell::new(LocalConnectionMode::Standard)),
    }
}

pub(super) fn bind_actions(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    bind_inject_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_websocket_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_enhanced_launch_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_save_models_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_sync_models_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_fetch_models_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_clear_action(api, frame, tab, refresh, gui_tx, in_flight);
    bind_session_history_action(api, frame, tab, refresh);
}

pub(super) fn set_actions_enabled(tab: &CodexTab, enabled: bool) {
    tab.service_enabled.set(enabled);
    tab.save_models_button.enable(enabled);
    tab.sync_models_button.enable(enabled);
    tab.fetch_models_button.enable(enabled);
    tab.model_provider.enable(enabled);
    tab.custom_models_input.enable(enabled);
    for checkbox in tab.model_checks.iter() {
        checkbox.enable(enabled);
    }
    refresh_config_buttons(tab, enabled);
    tab.session_history_button
        .enable(enabled && tab.remote_ready.get());
}

pub(super) fn refresh_configured(tab: &CodexTab, configured: bool, config_up_to_date: bool) {
    tab.configured.set(configured);
    tab.config_up_to_date.set(configured && config_up_to_date);
    refresh_config_buttons(tab, tab.service_enabled.get());
}

pub(super) fn refresh_websocket(tab: &CodexTab, enabled: bool) {
    if !tab.websocket_saving.get() {
        tab.websocket_enabled.set(enabled);
        tab.websocket_checkbox.set_value(enabled);
    }
}

pub(super) fn refresh_local_connection_mode(tab: &CodexTab, mode: LocalConnectionMode) {
    tab.local_connection_mode.set(mode);
}

pub(super) fn refresh_remote_ready(tab: &CodexTab, remote_ready: bool) {
    tab.remote_ready.set(remote_ready);
    tab.session_history_button
        .enable(tab.service_enabled.get() && remote_ready);
}

pub(super) fn initialize_visible_model_checks(tab: &CodexTab, gateway_config: &AiGatewayConfig) {
    let names: Vec<String> = gateway_config
        .providers
        .iter()
        .filter(|provider| provider.enabled && !provider.is_workbuddy())
        .map(|provider| provider.name.clone())
        .collect();
    if *tab.model_provider_names.borrow() != names {
        let previous = selected_model_provider(tab);
        tab.model_provider.clear();
        tab.model_provider
            .append(tab.text.codex_model_no_provider());
        for name in &names {
            tab.model_provider.append(name);
        }
        let index = previous
            .and_then(|name| names.iter().position(|item| item == &name))
            .map(|index| index + 1)
            .unwrap_or(0);
        tab.model_provider.set_selection(index as u32);
        *tab.model_provider_names.borrow_mut() = names;
    }
    if tab.models_initialized.get() {
        return;
    }
    let selected = gateway_config
        .codex_visible_models
        .iter()
        .map(|model| model.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (index, slug) in tab.model_slugs.iter().enumerate() {
        if let Some(checkbox) = tab.model_checks.get(index) {
            checkbox.set_value(selected.contains(slug.as_str()));
        }
    }
    let built_in = visible_catalog_model_options()
        .into_iter()
        .map(|model| model.slug)
        .collect::<std::collections::HashSet<_>>();
    let mut dynamic = std::collections::BTreeSet::new();
    dynamic.extend(
        gateway_config
            .codex_visible_models
            .iter()
            .filter(|model| !built_in.contains(model.as_str()))
            .cloned(),
    );
    tab.custom_models_input
        .change_value(&dynamic.into_iter().collect::<Vec<_>>().join("\n"));
    tab.models_initialized.set(true);
}

pub(super) fn apply_pending_action(
    api: &ApiClient,
    tab: &CodexTab,
    text: GuiText,
    frame: &Frame,
    refresh: &DashboardRefresh,
    result: CodexActionResult,
) {
    if matches!(
        &result,
        CodexActionResult::Inject(_) | CodexActionResult::Clear(_)
    ) {
        tab.config_saving.set(false);
    }
    tab.inject_button.set_label(text.inject_codex_access());
    tab.clear_button.set_label(text.clear_codex_access());
    tab.save_models_button.set_label(text.save_codex_models());
    tab.sync_models_button.set_label(text.sync_codex_models());
    tab.fetch_models_button.set_label(text.ai_gw_fetch_models());
    tab.enhanced_launch_button
        .set_label(text.codex_enhanced_launch());
    let service_enabled = tab.service_enabled.get();
    refresh_config_buttons(tab, service_enabled);
    tab.save_models_button.enable(service_enabled);
    tab.sync_models_button.enable(service_enabled);
    tab.fetch_models_button.enable(service_enabled);
    tab.model_provider.enable(service_enabled);

    match result {
        CodexActionResult::WebSocket(result) => {
            tab.websocket_saving.set(false);
            match result {
                Ok(enabled) => {
                    refresh_websocket(tab, enabled);
                    show_info(frame, text.provider_websocket_saved());
                }
                Err(error) => {
                    tab.websocket_checkbox
                        .set_value(tab.websocket_enabled.get());
                    show_error(frame, &error);
                }
            }
            refresh_config_buttons(tab, service_enabled);
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::Inject(Ok(_)) => {
            refresh_configured(tab, true, true);
            show_info(frame, text.codex_app_config_injected());
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::Inject(Err(err)) => {
            show_error(frame, &err);
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::Clear(Ok(_)) => {
            refresh_configured(tab, false, false);
            show_info(frame, text.codex_app_config_uninstalled());
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::Clear(Err(err)) => {
            show_error(frame, &err);
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::SaveModels(Ok(())) => {
            show_info(frame, text.codex_models_saved());
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::SaveModels(Err(err)) => {
            show_error(frame, &err);
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::SyncModels(Ok(models)) => {
            let mut custom = Vec::new();
            for model in &models {
                if let Some(index) = tab
                    .model_slugs
                    .iter()
                    .position(|slug| slug.eq_ignore_ascii_case(model))
                {
                    tab.model_checks[index].set_value(true);
                } else {
                    custom.push(model.clone());
                }
            }
            let existing = tab.custom_models_input.get_value();
            let merged = merge_model_ids(&existing, &custom);
            tab.custom_models_input.change_value(&merged.join("\n"));
            show_info(frame, &text.codex_models_synced(models.len()));
        }
        CodexActionResult::SyncModels(Err(err)) => {
            show_error(frame, &err);
        }
        CodexActionResult::EnhancedLaunch(Ok(_)) => {
            show_info(frame, text.codex_enhanced_launch_ready());
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::EnhancedLaunch(Err(err)) => {
            show_error(frame, &err);
            force_dashboard_refresh(api, refresh);
        }
    }
}

fn merge_model_ids(existing: &str, incoming: &[String]) -> Vec<String> {
    let mut models = Vec::new();
    for model in existing
        .split([',', '\n', '\r'])
        .chain(incoming.iter().map(String::as_str))
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        if !models
            .iter()
            .any(|known: &String| known.eq_ignore_ascii_case(model))
        {
            models.push(model.to_string());
        }
    }
    models
}

fn refresh_config_buttons(tab: &CodexTab, service_enabled: bool) {
    let configured = tab.configured.get();
    if !tab.config_saving.get() {
        tab.inject_button.set_label(if tab.config_up_to_date.get() {
            tab.text.codex_config_up_to_date()
        } else if configured {
            tab.text.update_codex_access()
        } else {
            tab.text.inject_codex_access()
        });
    }
    tab.inject_button.enable(config_update_enabled(
        service_enabled,
        tab.config_up_to_date.get(),
        tab.config_saving.get(),
    ));
    tab.enhanced_launch_button
        .enable(service_enabled && configured);
    tab.clear_button.enable(service_enabled && configured);
    tab.websocket_checkbox
        .enable(service_enabled && configured && !tab.websocket_saving.get());
}

fn config_update_enabled(service_enabled: bool, up_to_date: bool, saving: bool) -> bool {
    service_enabled && !up_to_date && !saving
}

pub(super) enum CodexActionResult {
    WebSocket(Result<bool, String>),
    Inject(Result<serde_json::Value, String>),
    Clear(Result<serde_json::Value, String>),
    SaveModels(Result<(), String>),
    SyncModels(Result<Vec<String>, String>),
    EnhancedLaunch(Result<serde_json::Value, String>),
}

fn bind_sync_models_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let refresh = refresh.clone();
    let frame = *frame;
    let tab = tab.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    tab.sync_models_button.on_click(move |_| {
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        tab.sync_models_button.enable(false);
        let thread_api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = thread_api.get_app_config().map(|config| {
                let mut models = std::collections::BTreeSet::new();
                for provider in config
                    .ai_gateway
                    .providers
                    .into_iter()
                    .filter(|provider| provider.enabled && !provider.is_workbuddy())
                {
                    models.extend(provider.models);
                    models.extend(provider.model_aliases.into_keys());
                }
                models.into_iter().collect::<Vec<_>>()
            });
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(
                CodexActionResult::SyncModels(outcome),
            ));
            wxdragon::wake_up_idle();
        });
    });
}

fn bind_inject_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let refresh = refresh.clone();
    let frame = *frame;
    let tab = tab.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    let inject_button = tab.inject_button;
    inject_button.on_click(move |_| {
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        tab.config_saving.set(true);
        tab.inject_button
            .set_label(tab.text.injecting_codex_access());
        tab.inject_button.enable(false);
        let request = ConfigureRequest {
            connection_mode: tab.local_connection_mode.get(),
            provider_name: None,
            provider_base_url: None,
            provider_key: None,
            activate: true,
            image_generation_enabled: None,
            supports_websockets: None,
        };
        let thread_api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = thread_api.configure_codex_app(&request);
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(CodexActionResult::Inject(
                outcome,
            )));
            wxdragon::wake_up_idle();
        });
        schedule_dashboard_refresh(&api, &refresh);
    });
}

fn bind_websocket_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let frame = *frame;
    let tab = tab.clone();
    let refresh = refresh.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    let checkbox = tab.websocket_checkbox;
    checkbox.on_toggled(move |_| {
        let enabled = checkbox.get_value();
        if !tab.configured.get()
            || tab.websocket_saving.get()
            || in_flight.swap(true, Ordering::SeqCst)
        {
            checkbox.set_value(tab.websocket_enabled.get());
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            checkbox.set_value(tab.websocket_enabled.get());
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        tab.websocket_saving.set(true);
        checkbox.enable(false);
        let api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = api.set_codex_websocket(enabled);
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(
                CodexActionResult::WebSocket(outcome),
            ));
            wxdragon::wake_up_idle();
        });
    });
}

fn bind_enhanced_launch_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let refresh = refresh.clone();
    let frame = *frame;
    let tab = tab.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    let button = tab.enhanced_launch_button;
    button.on_click(move |_| {
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        match api.codex_app_enhanced_preflight() {
            Ok(response) if response.status.running => {
                if !wait_for_codex_app_to_close(&frame, tab.text, &api) {
                    in_flight.store(false, Ordering::SeqCst);
                    return;
                }
            }
            Ok(_) => {}
            Err(err) => {
                show_error(
                    &frame,
                    &tab.text.codex_enhanced_launch_check_failed_detail(&err),
                );
                in_flight.store(false, Ordering::SeqCst);
                return;
            }
        }
        button.set_label(tab.text.codex_enhanced_launching());
        button.enable(false);
        let thread_api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = thread_api.launch_codex_app_enhanced();
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(
                CodexActionResult::EnhancedLaunch(outcome),
            ));
            wxdragon::wake_up_idle();
        });
        schedule_dashboard_refresh(&api, &refresh);
    });
}

fn wait_for_codex_app_to_close(parent: &Frame, text: GuiText, api: &ApiClient) -> bool {
    let dialog = Dialog::builder(parent, text.codex_enhanced_launch_close_title())
        .with_style(DialogStyle::DefaultDialogStyle)
        .with_size(520, 230)
        .build();
    dialog.set_background_color(theme::theme().bg_card);
    let panel = Panel::builder(&dialog).build();
    panel.set_background_color(theme::theme().bg_card);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let title = StaticText::builder(&panel)
        .with_label(text.codex_enhanced_launch_close_title())
        .build();
    title.set_font(&theme::font(theme::TextRole::Title));
    title.set_foreground_color(theme::theme().ink_primary);
    sizer.add(
        &title,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        18,
    );

    let message = StaticText::builder(&panel)
        .with_label(text.codex_enhanced_launch_close_running())
        .build();
    message.set_foreground_color(theme::theme().ink_muted);
    message.wrap(510);
    sizer.add(
        &message,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        18,
    );

    let status = StaticText::builder(&panel)
        .with_label(text.codex_enhanced_launch_waiting_for_close())
        .build();
    status.set_min_size(Size::new(470, 32));
    status.set_foreground_color(theme::theme().warn);
    status.wrap(510);
    sizer.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        16,
    );

    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let cancel_button = Button::builder(&panel)
        .with_id(ID_CANCEL)
        .with_label(text.cancel())
        .build();
    let confirm_button = Button::builder(&panel)
        .with_id(ID_OK)
        .with_label(text.codex_enhanced_launch_confirm())
        .build();
    confirm_button.enable(false);
    confirm_button.set_default();
    actions.add_stretch_spacer(1);
    actions.add(&cancel_button, 0, SizerFlag::Right, 8);
    actions.add(&confirm_button, 0, SizerFlag::Right, 0);
    sizer.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        18,
    );

    panel.set_sizer(sizer, true);
    let dialog_sizer = BoxSizer::builder(Orientation::Vertical).build();
    dialog_sizer.add(&panel, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(dialog_sizer, true);
    dialog.center();

    let timer = Timer::new(&dialog);
    {
        let api = api.clone();
        timer.on_tick(move |_| {
            refresh_enhanced_launch_preflight(&api, text, &status, &confirm_button);
        });
    }
    timer.start(500, false);
    {
        let dialog = dialog;
        cancel_button.on_click(move |_| dialog.end_modal(ID_CANCEL));
    }
    {
        let dialog = dialog;
        confirm_button.on_click(move |_| dialog.end_modal(ID_OK));
    }

    let confirmed = dialog.show_modal() == ID_OK;
    timer.stop();
    dialog.destroy();
    confirmed
}

fn refresh_enhanced_launch_preflight(
    api: &ApiClient,
    text: GuiText,
    status: &StaticText,
    confirm_button: &Button,
) {
    match api.codex_app_enhanced_preflight() {
        Ok(response) if response.status.running => {
            status.set_label(text.codex_enhanced_launch_waiting_for_close());
            status.set_foreground_color(theme::theme().warn);
            confirm_button.enable(false);
        }
        Ok(_) => {
            status.set_label(text.codex_enhanced_launch_ready_to_start());
            status.set_foreground_color(theme::theme().ok);
            confirm_button.enable(true);
        }
        Err(_) => {
            status.set_label(text.codex_enhanced_launch_check_failed());
            status.set_foreground_color(theme::theme().error);
            confirm_button.enable(false);
        }
    }
}

fn bind_save_models_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let refresh = refresh.clone();
    let frame = *frame;
    let tab = tab.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    let save_models_button = tab.save_models_button;
    save_models_button.on_click(move |_| {
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        tab.save_models_button
            .set_label(tab.text.saving_codex_models());
        tab.save_models_button.enable(false);
        let selected_models = selected_visible_models(&tab);
        let provider_name = selected_model_provider(&tab);
        let manual_models = merge_model_ids(&tab.custom_models_input.get_value(), &[]);
        let thread_api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome =
                save_visible_models(&thread_api, selected_models, manual_models, provider_name);
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(
                CodexActionResult::SaveModels(outcome),
            ));
            wxdragon::wake_up_idle();
        });
        schedule_dashboard_refresh(&api, &refresh);
    });
}

fn bind_clear_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let refresh = refresh.clone();
    let frame = *frame;
    let tab = tab.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    let clear_button = tab.clear_button;
    clear_button.on_click(move |_| {
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        if !confirm_uninstall_codex_app_config(&frame, tab.text) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        tab.config_saving.set(true);
        tab.clear_button.set_label(tab.text.clearing_codex_access());
        tab.clear_button.enable(false);
        let thread_api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = thread_api.uninstall_codex_app();
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(CodexActionResult::Clear(
                outcome,
            )));
            wxdragon::wake_up_idle();
        });
        schedule_dashboard_refresh(&api, &refresh);
    });
}

fn bind_session_history_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
) {
    let api = api.clone();
    let frame = *frame;
    let tab = tab.clone();
    let refresh = refresh.clone();
    let button = tab.session_history_button;
    button.on_click(move |_| {
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            return;
        }
        super::show_session_history_window(&frame, tab.text, api.clone());
    });
}

fn selected_visible_models(tab: &CodexTab) -> Vec<String> {
    let mut selected = tab
        .model_slugs
        .iter()
        .enumerate()
        .filter_map(|(index, slug)| {
            tab.model_checks
                .get(index)
                .is_some_and(CheckBox::is_checked)
                .then(|| slug.clone())
        })
        .collect::<Vec<_>>();
    for model in tab
        .custom_models_input
        .get_value()
        .split(|ch: char| ch == ',' || ch == '\n' || ch == '\r')
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        if !selected.iter().any(|known| known == model) {
            selected.push(model.to_string());
        }
    }
    selected
}

fn save_visible_models(
    api: &ApiClient,
    models: Vec<String>,
    manual_models: Vec<String>,
    provider_name: Option<String>,
) -> Result<(), String> {
    let mut config = api.get_app_config()?;
    bind_unrouted_models(
        &mut config.ai_gateway,
        &manual_models,
        provider_name.as_deref(),
    )?;
    config.ai_gateway.codex_visible_models = models;
    api.save_app_config(&config)?;
    let _ = api.refresh_codex_app_models();
    Ok(())
}

fn selected_model_provider(tab: &CodexTab) -> Option<String> {
    let index = tab.model_provider.get_selection()? as usize;
    index
        .checked_sub(1)
        .and_then(|index| tab.model_provider_names.borrow().get(index).cloned())
}

fn bind_unrouted_models(
    config: &mut AiGatewayConfig,
    models: &[String],
    provider_name: Option<&str>,
) -> Result<(), String> {
    let missing: Vec<String> = models
        .iter()
        .filter(|model| {
            !config.providers.iter().any(|provider| {
                provider.enabled && !provider.is_workbuddy() && provider.matches_model(model)
            })
        })
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let Some(provider) = config.providers.iter_mut().find(|provider| {
        provider.enabled
            && !provider.is_workbuddy()
            && Some(provider.name.as_str()) == provider_name
    }) else {
        return Err(format!(
            "请选择一个已启用的渠道，为以下模型建立路由 / Select a provider for: {}",
            missing.join(", ")
        ));
    };
    provider.models.extend(missing);
    Ok(())
}

fn bind_fetch_models_action(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    let api = api.clone();
    let frame = *frame;
    let tab = tab.clone();
    let refresh = refresh.clone();
    let gui_tx = gui_tx.clone();
    let in_flight = in_flight.clone();
    let button = tab.fetch_models_button;
    button.on_click(move |_| {
        let Some(provider_name) = selected_model_provider(&tab) else {
            show_error(&frame, tab.text.codex_model_choose_provider());
            return;
        };
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !ensure_service_ready_for_action(&api, &frame, &refresh) {
            in_flight.store(false, Ordering::SeqCst);
            return;
        }
        tab.fetch_models_button.enable(false);
        tab.model_provider.enable(false);
        tab.fetch_models_button
            .set_label(tab.text.ai_gw_fetching_models());
        let api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = (|| {
                let config = api.get_app_config()?;
                let provider = config
                    .ai_gateway
                    .providers
                    .iter()
                    .find(|provider| {
                        provider.enabled
                            && !provider.is_workbuddy()
                            && provider.name == provider_name
                    })
                    .ok_or_else(|| "Selected provider is unavailable".to_string())?;
                if provider.provider_type
                    == crate::ai_gateway::config::ProviderType::ChatGptResponses
                {
                    let id = provider.chatgpt_auth_id.as_deref().ok_or_else(|| {
                        "Sign in to the selected ChatGPT channel first".to_string()
                    })?;
                    return api.post_json_with_timeout::<_, Vec<String>>(
                        "/api/chatgpt/account/models",
                        &serde_json::json!({"id": id}),
                        std::time::Duration::from_secs(75),
                    );
                }
                let (models, _) = super::fetch_remote_models(
                    &provider.base_url,
                    provider.models_url.as_deref(),
                    &super::known_models_urls_for_provider(provider),
                    &provider.api_key,
                    super::GUI_MODEL_LIST_FETCH_TIMEOUT_SECS,
                )?;
                Ok(super::filter_fetched_models_for_provider(
                    &provider.provider_type,
                    models,
                ))
            })();
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(
                CodexActionResult::SyncModels(outcome),
            ));
            wxdragon::wake_up_idle();
        });
    });
}

#[cfg(test)]
mod dynamic_model_tests {
    use super::*;
    use crate::ai_gateway::config::ProviderConfig;

    #[test]
    fn sync_preserves_manual_entries_and_deduplicates() {
        assert_eq!(
            merge_model_ids(
                " custom-one\nGPT-new,custom-two ",
                &["gpt-new".into(), "vendor/new".into()]
            ),
            ["custom-one", "GPT-new", "custom-two", "vendor/new"]
        );
    }

    #[test]
    fn manual_models_gain_routes_without_changing_existing_aliases() {
        let mut config = AiGatewayConfig::default();
        config.providers.push(ProviderConfig {
            name: "selected".into(),
            ..Default::default()
        });
        config.providers.push(ProviderConfig {
            name: "other".into(),
            model_aliases: [("existing".into(), "actual-upstream".into())].into(),
            ..Default::default()
        });
        bind_unrouted_models(
            &mut config,
            &["existing".into(), "new-model".into()],
            Some("selected"),
        )
        .unwrap();
        assert_eq!(config.providers[0].models, ["new-model"]);
        assert_eq!(
            config.providers[1].resolve_upstream_model("existing"),
            Some("actual-upstream")
        );
        assert_eq!(
            config.select_provider("new-model").unwrap().name,
            "selected"
        );
        bind_unrouted_models(&mut config, &["NEW-MODEL".into()], Some("selected")).unwrap();
        assert_eq!(config.providers[0].models, ["new-model"]);
    }

    #[test]
    fn unrouted_model_requires_enabled_non_workbuddy_provider() {
        let mut config = AiGatewayConfig::default();
        config.providers.push(ProviderConfig {
            name: "workbuddy".into(),
            ..Default::default()
        });
        config.providers.push(ProviderConfig {
            name: "disabled".into(),
            enabled: false,
            ..Default::default()
        });
        for name in [None, Some("workbuddy"), Some("disabled")] {
            assert!(bind_unrouted_models(&mut config, &["new-model".into()], name).is_err());
        }
        assert!(config.providers.iter().all(|p| p.models.is_empty()));
    }
}

#[cfg(test)]
mod tests {
    use super::config_update_enabled;

    #[test]
    fn config_update_button_only_enables_for_pending_changes() {
        assert!(config_update_enabled(true, false, false));
        assert!(!config_update_enabled(true, true, false));
        assert!(!config_update_enabled(true, false, true));
        assert!(!config_update_enabled(true, true, true));
        assert!(!config_update_enabled(false, false, false));
        assert!(!config_update_enabled(false, true, false));
    }
}
