use super::controls::Button;
use crate::gui::controls::{Choice, TextCtrl};
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

type CodexModelsInitialized = Rc<Cell<bool>>;
type CodexConfigured = Rc<Cell<bool>>;
type CodexHubReady = Rc<Cell<bool>>;
type CodexServiceEnabled = Rc<Cell<bool>>;

#[derive(Clone)]
struct DiscoveredModels {
    models: Vec<String>,
    source_fingerprint: String,
}

#[derive(Clone)]
pub(super) struct CodexTab {
    pub(super) page: Panel,
    text: GuiText,
    inject_button: Button,
    clear_button: Button,
    session_history_button: Button,
    enhanced_launch_button: Button,
    startup_repair_button: Button,
    websocket_checkbox: CheckBox,
    websocket_enabled: Rc<Cell<bool>>,
    websocket_saving: Rc<Cell<bool>>,
    save_models_button: Button,
    sync_models_button: Button,
    fetch_models_button: Button,
    model_provider: Choice,
    model_provider_names: Rc<RefCell<Vec<String>>>,
    custom_models_input: TextCtrl,
    models_updated: Panel,
    discovered_models: Rc<RefCell<std::collections::BTreeMap<String, DiscoveredModels>>>,
    base_models: Rc<RefCell<Option<Vec<String>>>>,
    latest_models: Rc<RefCell<Option<Vec<String>>>>,
    list_saved: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    model_picker: super::codex_models::ChannelModels,
    models_initialized: CodexModelsInitialized,
    configured: CodexConfigured,
    config_up_to_date: Rc<Cell<bool>>,
    config_saving: Rc<Cell<bool>>,
    remote_ready: CodexHubReady,
    service_enabled: CodexServiceEnabled,
    local_connection_mode: Rc<Cell<LocalConnectionMode>>,
}

pub(super) fn create(parent: &dyn WxWidget, text: GuiText) -> CodexTab {
    create_with_settings(parent, text, None, None)
}
pub(super) fn create_with_settings(
    parent: &dyn WxWidget,
    text: GuiText,
    settings: Option<&Panel>,
    header_actions: Option<(Button, Button)>,
) -> CodexTab {
    let page = Panel::builder(parent).build();
    page.set_background_color(theme::theme().bg_card_alt);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();

    let (local_config_box, local_config_section) = if let Some(parent) = settings {
        card_section(parent, text.codex_local_config())
    } else {
        card_section(&page, text.codex_local_config())
    };
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
    let enhanced_launch_button = header_actions.map(|a| a.0).unwrap_or_else(|| {
        Button::builder(&local_config_box)
            .with_label(text.codex_enhanced_launch())
            .build()
    });
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
    if header_actions.is_none() {
        local_config_actions.add(
            &enhanced_launch_button,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            12,
        );
    }
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

    let startup_repair_button = Button::builder(&local_config_box)
        .with_label(text.codex_startup_repair())
        .build();
    startup_repair_button.set_tooltip(text.codex_startup_repair_help());
    startup_repair_button.enable(cfg!(any(windows, target_os = "macos")));
    local_config_section.add(
        &startup_repair_button,
        0,
        SizerFlag::AlignRight | SizerFlag::Right | SizerFlag::Top,
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
    if let Some(settings) = settings {
        if let Some(settings_sizer) = settings.get_sizer() {
            settings_sizer.add(&local_config_box, 0, SizerFlag::Expand | SizerFlag::All, 12);
        }
    } else {
        sizer.add(
            &local_config_box,
            0,
            SizerFlag::Expand
                | SizerFlag::Left
                | SizerFlag::Right
                | SizerFlag::Top
                | SizerFlag::Bottom,
            12,
        );
    }

    local_config_hint.hide();
    session_hint.hide();
    let (models_box, models_section) = card_section(&page, text.codex_visible_models());
    models_box.set_tooltip(text.codex_visible_models_help());
    let models_hint = StaticText::builder(&models_box)
        .with_label(text.codex_visible_models_help())
        .build();
    models_hint.set_foreground_color(theme::theme().ink_muted);
    models_hint.wrap(980);
    models_hint.hide();
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
    models_scope_warning.hide();
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
    models_warning.hide();
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
    custom_models_hint.hide();
    custom_models_hint.set_tooltip(text.codex_custom_models_help());
    models_section.add(
        &custom_models_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        6,
    );
    let model_picker = super::codex_models::ChannelModels::new(&models_box);
    let model_provider = model_picker.channel;
    models_section.add(
        &model_picker.page,
        1,
        SizerFlag::Expand | SizerFlag::All,
        page.from_dip(10),
    );
    let custom_models_input = TextCtrl::builder(&models_box)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::Default)
        .build();
    custom_models_input.set_min_size(super::visual::dip_size(&custom_models_input, 0, 64));
    custom_models_input.set_tooltip("手动模型 ID，每行一个；保存时使用所选渠道，不影响其他客户端");
    custom_models_input.hide();
    models_section.add(
        &custom_models_input,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        page.from_dip(10),
    );
    let manual = Button::builder(&models_box)
        .with_label("手动填写模型 ID")
        .with_kind(super::controls::ButtonKind::Ghost)
        .build();
    models_section.add(&manual, 0, SizerFlag::Left, page.from_dip(10));
    manual.on_click(move |_| {
        custom_models_input.show(!custom_models_input.is_shown());
        models_box.layout();
        page.layout();
    });

    let models_updated = Panel::builder(&models_box).build();
    models_updated.set_background_color(models_box.get_background_color());
    let updated_row = BoxSizer::builder(Orientation::Horizontal).build();
    let updated_hint = StaticText::builder(&models_updated)
        .with_label("模型配置已更新，未保存的选择已保留。")
        .build();
    updated_hint.set_foreground_color(theme::theme().warn);
    let load_updated = Button::builder(&models_updated)
        .with_label("载入更新")
        .with_kind(super::controls::ButtonKind::Ghost)
        .build();
    updated_row.add(&updated_hint, 1, SizerFlag::AlignCenterVertical, 0);
    updated_row.add(
        &load_updated,
        0,
        SizerFlag::Left,
        models_updated.from_dip(8),
    );
    models_updated.set_sizer(updated_row, false);
    models_updated.hide();
    models_section.add(
        &models_updated,
        0,
        SizerFlag::Expand | SizerFlag::All,
        page.from_dip(10),
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
        page.from_dip(12),
    );

    page.set_sizer(sizer, true);
    page.layout();

    let tab = CodexTab {
        page,
        text,
        inject_button,
        clear_button,
        session_history_button,
        enhanced_launch_button,
        startup_repair_button,
        websocket_checkbox,
        websocket_enabled: Rc::new(Cell::new(false)),
        websocket_saving: Rc::new(Cell::new(false)),
        save_models_button,
        sync_models_button,
        fetch_models_button,
        model_provider,
        model_provider_names: Rc::new(RefCell::new(Vec::new())),
        custom_models_input,
        models_updated,
        discovered_models: Default::default(),
        base_models: Default::default(),
        latest_models: Default::default(),
        list_saved: Default::default(),
        model_picker,
        models_initialized: Rc::new(Cell::new(false)),
        configured: Rc::new(Cell::new(false)),
        config_up_to_date: Rc::new(Cell::new(false)),
        config_saving: Rc::new(Cell::new(false)),
        remote_ready: Rc::new(Cell::new(false)),
        service_enabled: Rc::new(Cell::new(false)),
        local_connection_mode: Rc::new(Cell::new(LocalConnectionMode::Standard)),
    };
    let rebase = tab.clone();
    load_updated.on_click(move |_| reload_visible_models(&rebase));
    tab
}

pub(super) fn invalidate_visible_models(tab: &CodexTab) {
    tab.models_initialized.set(false);
}
pub(super) fn reload_visible_models(tab: &CodexTab) {
    tab.models_initialized.set(false);
    tab.discovered_models.borrow_mut().clear();
    tab.custom_models_input.change_value("");
    if let Some(models) = tab.latest_models.borrow().clone() {
        *tab.base_models.borrow_mut() = Some(models.clone());
        tab.model_picker.replace_selected_models(&models);
        tab.models_initialized.set(true);
    }
    tab.models_updated.hide();
    tab.page.layout();
    if let Some(saved) = tab.list_saved.borrow().clone() {
        saved();
    }
}
pub(super) fn focus_model(tab: &CodexTab, model: &str) {
    tab.model_picker.focus_model(model);
}
pub(super) fn bind_model_list(tab: &CodexTab, page: &super::clients::ClientPage) {
    let view = page.clone();
    let current = tab.clone();
    tab.model_picker.on_changed(move |_| {
        view.set_codex_selection(
            selected_visible_models(&current),
            current.base_models.borrow().clone().unwrap_or_default(),
        );
    });
    let view = page.clone();
    let current = tab.clone();
    tab.custom_models_input.on_text_updated(move |_| {
        view.set_codex_selection(
            selected_visible_models(&current),
            current.base_models.borrow().clone().unwrap_or_default(),
        );
    });
    let picker = tab.model_picker.clone();
    let manual = tab.custom_models_input;
    page.on_model_remove(move |model| {
        manual.change_value(
            &merge_model_ids(&manual.get_value(), &[])
                .into_iter()
                .filter(|m| m != model)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        picker.remove_model(model);
    });
    let view = page.clone();
    *tab.list_saved.borrow_mut() = Some(Rc::new(move || view.finish_codex_models()));
}
#[cfg(all(test, windows))]
pub(super) fn review_picker(tab: &CodexTab) -> super::codex_models::ChannelModels {
    tab.model_picker.clone()
}
#[cfg(all(test, windows))]
pub(super) fn review_remote_model_updates(tab: &CodexTab, config: &mut AiGatewayConfig) {
    let original = config.codex_visible_models.clone();
    config.codex_visible_models = vec!["external-saved-model".into()];
    initialize_visible_model_checks(tab, config);
    assert_eq!(tab.model_picker.selected_models(), ["external-saved-model"]);
    assert!(!tab.models_updated.is_shown());

    // A manual selection and a typed entry are both drafts. A second remote
    // save must leave them intact until the user explicitly loads the update.
    tab.model_picker.select_models(&["draft-model".into()]);
    config.codex_visible_models = vec!["second-external-model".into()];
    initialize_visible_model_checks(tab, config);
    assert!(
        tab.model_picker
            .selected_models()
            .contains(&"draft-model".into())
    );
    assert!(tab.models_updated.is_shown());
    tab.custom_models_input.change_value("typed-model");
    initialize_visible_model_checks(tab, config);
    assert_eq!(tab.custom_models_input.get_value(), "typed-model");
    reload_visible_models(tab);
    assert_eq!(
        tab.model_picker.selected_models(),
        ["second-external-model"]
    );
    assert!(!tab.models_updated.is_shown());

    config.codex_visible_models = original;
    initialize_visible_model_checks(tab, config);
}

pub(super) fn bind_actions(
    api: &ApiClient,
    frame: &Frame,
    tab: &CodexTab,
    refresh: &DashboardRefresh,
    gui_tx: &UnboundedSender<super::GuiMessage>,
    in_flight: &Arc<AtomicBool>,
) {
    super::startup_repair::bind_button(&tab.startup_repair_button, frame, tab.text, in_flight);
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
    tab.model_picker.enable(enabled);
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
    *tab.latest_models.borrow_mut() = Some(gateway_config.codex_visible_models.clone());
    let names: Vec<String> = gateway_config
        .providers
        .iter()
        .filter(|provider| provider.enabled && !provider.is_client_reserved())
        .map(|provider| provider.name.clone())
        .collect();
    if *tab.model_provider_names.borrow() != names {
        let previous = selected_model_provider(tab);
        tab.model_provider.clear();
        tab.model_provider.append("内置与其他模型");
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
    let mut groups = super::codex_model_catalog::channel_groups(gateway_config);
    for (index, name) in tab.model_provider_names.borrow().iter().enumerate() {
        if let (Some(group), Some(found)) = (
            groups.get_mut(index + 1),
            tab.discovered_models.borrow().get(name),
        ) {
            for model in &found.models {
                if !group.contains(model) {
                    group.push(model.clone());
                }
            }
        }
    }
    let base = tab.base_models.borrow().clone();
    let has_draft = base.as_ref().is_some_and(|base| {
        !same_model_set(&selected_visible_models(tab), base)
            || !tab.custom_models_input.get_value().trim().is_empty()
    });
    let remote_changed = base
        .as_ref()
        .is_some_and(|base| !same_model_set(base, &gateway_config.codex_visible_models));
    let reload = !tab.models_initialized.get() || (remote_changed && !has_draft);
    if reload || !remote_changed {
        *tab.base_models.borrow_mut() = Some(gateway_config.codex_visible_models.clone());
    }
    let show_update = remote_changed && has_draft && !reload;
    if tab.models_updated.is_shown() != show_update {
        tab.models_updated.show(show_update);
        tab.page.layout();
    }
    tab.model_picker.update(
        groups,
        reload.then_some(&gateway_config.codex_visible_models),
    );
    tab.models_initialized.set(true);
}

pub(super) fn same_model_set(left: &[String], right: &[String]) -> bool {
    use std::collections::BTreeSet;
    left.iter().collect::<BTreeSet<_>>() == right.iter().collect::<BTreeSet<_>>()
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
            tab.models_initialized.set(false);
            tab.discovered_models.borrow_mut().clear();
            tab.custom_models_input.change_value("");
            if let Some(saved) = tab.list_saved.borrow().clone() {
                saved();
            }
            show_info(frame, text.codex_models_saved());
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::SaveModels(Err(err)) => {
            show_error(frame, &err);
            force_dashboard_refresh(api, refresh);
        }
        CodexActionResult::SyncModels(Ok(models)) => {
            tab.model_picker.select_models(&models);
            show_info(frame, &text.codex_models_synced(models.len()));
        }
        CodexActionResult::SyncModels(Err(err)) => {
            show_error(frame, &err);
        }
        CodexActionResult::FetchModels(provider, Ok(found)) => {
            if let Some(index) = tab
                .model_provider_names
                .borrow()
                .iter()
                .position(|name| name == &provider)
            {
                tab.discovered_models
                    .borrow_mut()
                    .insert(provider, found.clone());
                tab.model_picker.show_discovered(index + 1, &found.models);
                show_info(
                    frame,
                    &format!(
                        "已获取 {} 个可选模型。勾选后保存模型列表即可使用。",
                        found.models.len()
                    ),
                );
            } else {
                show_error(frame, "所选渠道已变化，请重新读取模型列表");
            }
        }
        CodexActionResult::FetchModels(_, Err(err)) => {
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
    FetchModels(String, Result<DiscoveredModels, String>),
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
                    .filter(|provider| provider.enabled && !provider.is_client_reserved())
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
    super::motion::install_dialog(dialog);
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
        let expected_models = tab.base_models.borrow().clone();
        let discovered = tab.discovered_models.borrow().clone();
        let thread_api = api.clone();
        let gui_tx = gui_tx.clone();
        let in_flight = in_flight.clone();
        thread::spawn(move || {
            let outcome = save_visible_models(
                &thread_api,
                selected_models,
                manual_models,
                provider_name,
                expected_models,
                discovered,
            );
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
    let mut selected = tab.model_picker.selected_models();
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
    expected_models: Option<Vec<String>>,
    discovered: std::collections::BTreeMap<String, DiscoveredModels>,
) -> Result<(), String> {
    let mut config = api.get_app_config()?;
    prepare_visible_models(
        &mut config.ai_gateway,
        models,
        manual_models,
        provider_name,
        expected_models,
        discovered,
    )?;
    api.save_app_config(&config)?;
    let _ = api.refresh_codex_app_models();
    Ok(())
}
fn prepare_visible_models(
    config: &mut AiGatewayConfig,
    models: Vec<String>,
    manual_models: Vec<String>,
    provider_name: Option<String>,
    expected_models: Option<Vec<String>>,
    discovered: std::collections::BTreeMap<String, DiscoveredModels>,
) -> Result<(), String> {
    if expected_models.as_ref() != Some(&config.codex_visible_models) {
        return Err("模型列表已在其他位置更新，请重新读取后核对；未覆盖原列表".into());
    }
    for (provider, found) in discovered {
        let chosen = found
            .models
            .into_iter()
            .filter(|m| models.contains(m))
            .collect::<Vec<_>>();
        if !chosen.is_empty() {
            let current = config
                .providers
                .iter()
                .find(|p| p.enabled && !p.is_client_reserved() && p.name == provider)
                .ok_or("获取模型的渠道已变化，请重新获取后保存")?;
            if crate::desktop::models::fingerprint(current).map_err(|e| e.to_string())?
                != found.source_fingerprint
            {
                return Err("获取模型后渠道配置已变化，请重新获取后保存；未覆盖原配置".into());
            }
        }
        bind_unrouted_models(config, &chosen, Some(&provider))?;
    }
    bind_unrouted_models(config, &manual_models, provider_name.as_deref())?;
    config.codex_visible_models = models;
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
                provider.enabled && !provider.is_client_reserved() && provider.matches_model(model)
            })
        })
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let Some(provider) = config.providers.iter_mut().find(|provider| {
        provider.enabled
            && !provider.is_client_reserved()
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
            let fetched_provider = provider_name.clone();
            let outcome = (|| {
                let config = api.get_app_config()?;
                let provider = config
                    .ai_gateway
                    .providers
                    .iter()
                    .find(|provider| {
                        provider.enabled
                            && !provider.is_client_reserved()
                            && provider.name == provider_name
                    })
                    .ok_or_else(|| "Selected provider is unavailable".to_string())?;
                if provider.provider_type
                    == crate::ai_gateway::config::ProviderType::ChatGptResponses
                {
                    let id = provider.chatgpt_auth_id.as_deref().ok_or_else(|| {
                        "Sign in to the selected ChatGPT channel first".to_string()
                    })?;
                    let models = api.post_json_with_timeout::<_, Vec<String>>(
                        "/api/chatgpt/account/models",
                        &serde_json::json!({"id": id}),
                        std::time::Duration::from_secs(75),
                    )?;
                    return Ok(DiscoveredModels {
                        models,
                        source_fingerprint: crate::desktop::models::fingerprint(provider)
                            .map_err(|e| e.to_string())?,
                    });
                }
                let (models, corrected_base) = super::fetch_remote_models(
                    &provider.base_url,
                    provider.models_url.as_deref(),
                    &super::known_models_urls_for_provider(provider),
                    &provider.api_key,
                    super::GUI_MODEL_LIST_FETCH_TIMEOUT_SECS,
                )?;
                super::ai_gateway::save_confirmed_provider_url(&api, provider, &corrected_base)?;
                let mut fetched_source = provider.clone();
                if corrected_base != provider.base_url {
                    fetched_source.confirm_base_url(&corrected_base);
                }
                Ok(DiscoveredModels {
                    source_fingerprint: crate::desktop::models::fingerprint(&fetched_source)
                        .map_err(|e| e.to_string())?,
                    models: super::filter_fetched_models_for_provider(
                        &provider.provider_type,
                        models,
                    ),
                })
            })();
            in_flight.store(false, Ordering::SeqCst);
            let _ = gui_tx.send(super::GuiMessage::CodexAction(
                CodexActionResult::FetchModels(fetched_provider, outcome),
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
    fn saving_codex_list_persists_removals_adds_only_selected_discovery_and_preserves_other_routes()
    {
        let mut config = AiGatewayConfig::default();
        config.codex_visible_models = vec!["old".into(), "keep".into()];
        config.providers = vec![
            ProviderConfig {
                name: "source".into(),
                enabled: true,
                models: vec!["keep".into()],
                ..Default::default()
            },
            ProviderConfig {
                name: crate::ai_gateway::config::workbuddy_provider_name("test").unwrap(),
                models: vec!["old".into()],
                ..Default::default()
            },
        ];
        let reserved = serde_json::to_value(&config.providers[1]).unwrap();
        let base = config.codex_visible_models.clone();
        let found = DiscoveredModels {
            models: vec!["new".into(), "not-selected".into()],
            source_fingerprint: crate::desktop::models::fingerprint(&config.providers[0]).unwrap(),
        };
        prepare_visible_models(
            &mut config,
            vec!["keep".into(), "new".into()],
            vec![],
            Some("source".into()),
            Some(base.clone()),
            std::collections::BTreeMap::from([("source".into(), found)]),
        )
        .unwrap();
        assert_eq!(config.codex_visible_models, vec!["keep", "new"]);
        assert_eq!(config.providers[0].models, vec!["keep", "new"]);
        assert_eq!(
            serde_json::to_value(&config.providers[1]).unwrap(),
            reserved
        );
        let before = serde_json::to_value(&config).unwrap();
        assert!(
            prepare_visible_models(
                &mut config,
                vec![],
                vec![],
                None,
                Some(base),
                Default::default()
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
        let expected = config.codex_visible_models.clone();
        prepare_visible_models(
            &mut config,
            vec![],
            vec![],
            None,
            Some(expected),
            Default::default(),
        )
        .unwrap();
        assert!(config.codex_visible_models.is_empty());
    }

    #[test]
    fn discovery_cannot_bind_to_a_replaced_source_and_an_empty_list_stays_empty() {
        let mut config = AiGatewayConfig::default();
        config.providers = vec![ProviderConfig {
            name: "channel".into(),
            models: vec!["old".into()],
            ..Default::default()
        }];
        let discovered = DiscoveredModels {
            models: vec!["new".into()],
            source_fingerprint: crate::desktop::models::fingerprint(&config.providers[0]).unwrap(),
        };
        config.providers[0].base_url = "https://replacement.invalid/v1".into();
        let before = serde_json::to_value(&config).unwrap();
        let base = config.codex_visible_models.clone();
        assert!(
            prepare_visible_models(
                &mut config,
                vec!["new".into()],
                vec![],
                Some("channel".into()),
                Some(base),
                std::collections::BTreeMap::from([("channel".into(), discovered)])
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
        let base = config.codex_visible_models.clone();
        prepare_visible_models(
            &mut config,
            vec![],
            vec![],
            None,
            Some(base),
            Default::default(),
        )
        .unwrap();
        assert!(
            crate::ai_gateway::catalog::configured_models_response(&config)["models"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

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
