use super::controls::Button;
use crate::gui::controls::Choice;
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
        self, WorkBuddyConfigStatus, WorkBuddyEntryRequest, WorkBuddyModelConfig,
        WorkBuddyReasoningConfig, WorkBuddySaveRequest,
    },
};

use super::{
    api::ApiClient,
    show_error, show_info,
    text::{GuiLocale, GuiText},
    theme,
    widgets::{card_section, connection_choice_row, connection_field_theme},
};

#[derive(Clone, Debug)]
pub(super) struct WorkBuddyProviderOption {
    name: String,
    display_name: String,
    upstream_url: String,
    has_api_key: bool,
    upstream_protocol: String,
    models: Vec<String>,
    model_aliases: BTreeMap<String, String>,
    compatibility: Option<String>,
    copy: Option<(String, String)>,
}

type WorkBuddyProviderOptions = Rc<RefCell<Vec<WorkBuddyProviderOption>>>;

#[derive(Clone)]
pub(super) struct WorkBuddyTab {
    pub(super) page: Panel,
    editor_scroll: ScrolledWindow,
    entry: super::model_list::ModelList,
    provider: Choice,
    model: Choice,
    protocol: Choice,
    default_effort: Choice,
    text: GuiText,
    save_button: Button,
    delete_button: Button,
    restore_button: Button,
    reload_button: Button,
    start_button: Button,
    status: StaticText,
    in_flight: Arc<AtomicBool>,
    refresh_pending: Rc<RefCell<super::configuration_refresh::PendingRefresh>>,
    provider_options: WorkBuddyProviderOptions,
    selection: Rc<RefCell<WorkBuddySelection>>,
    draft: Rc<RefCell<Option<(WorkBuddyModelConfig, String)>>>,
    editor_base: Rc<RefCell<Option<(serde_json::Value, String)>>>,
}

#[derive(Default)]
struct WorkBuddySelection {
    status: Option<WorkBuddyConfigStatus>,
    fresh: bool,
}

#[derive(Debug)]
pub(super) enum WorkBuddyActionResult {
    Start(Result<String, String>),
    Save(Result<WorkBuddyConfigStatus, String>),
    Restore(Result<WorkBuddyConfigStatus, String>),
    Delete(Result<WorkBuddyConfigStatus, String>),
    Refresh(
        Result<(Vec<WorkBuddyProviderOption>, WorkBuddyConfigStatus), String>,
        bool,
    ),
}

const PROVIDER_REFRESH_ATTEMPTS: usize = 30;
const PROVIDER_REFRESH_RETRY_DELAY: Duration = Duration::from_millis(250);

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

pub(super) fn create(parent: &dyn WxWidget, text: GuiText) -> WorkBuddyTab {
    create_with_settings(parent, text, None, None)
}
pub(super) fn create_with_settings(
    parent: &dyn WxWidget,
    text: GuiText,
    settings: Option<&Panel>,
    header_actions: Option<(Button, Button)>,
) -> WorkBuddyTab {
    let page = Panel::builder(parent).build();
    page.set_background_color(theme::theme().bg_app);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let editor_scroll = ScrolledWindow::builder(&page)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    editor_scroll.set_min_size(Size::new(0, 0));
    editor_scroll.set_background_color(theme::theme().bg_app);
    editor_scroll.set_scroll_rate(0, editor_scroll.from_dip(10));
    let (section, section_sizer) =
        card_section(&editor_scroll, tr(text, "模型配置", "Model settings"));
    let hint = StaticText::builder(&section)
        .with_label(tr(
            text,
            "选择大模型接入渠道和模型，保存后在 WorkBuddy 中选择使用。",
            "Choose a model channel and model, save, then select the model in WorkBuddy.",
        ))
        .build();
    hint.set_foreground_color(theme::theme().ink_muted);
    hint.hide();
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(page.from_dip(10))
        .with_hgap(page.from_dip(16))
        .build();
    grid.add_growable_col(1, 1);
    let entry = super::model_list::ModelList::new(&page);
    let provider =
        connection_choice_row(&section, &grid, tr(text, "来源渠道", "Model channel"), &[]);
    let label = StaticText::builder(&section)
        .with_label(tr(text, "模型", "Model"))
        .build();
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    let model = Choice::builder(&section)
        .with_size(Size::new(260, -1))
        .build();
    model.set_min_size(super::visual::dip_size(&section, 0, 36));
    connection_field_theme(&model, true);
    grid.add(&model, 1, SizerFlag::Expand, 0);
    let protocol = connection_choice_row(
        &section,
        &grid,
        tr(text, "接口类型", "API type"),
        &["openai-responses", "openai-chat", "anthropic-messages"],
    );
    protocol.set_selection(0);
    let default_effort = connection_choice_row(
        &section,
        &grid,
        tr(text, "思考强度", "Reasoning effort"),
        &[],
    );
    default_effort.set_tooltip(tr(
        text,
        "未指定思考强度的请求使用此设置。",
        "Used when a request does not specify a reasoning effort.",
    ));
    section_sizer.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::All,
        page.from_dip(12),
    );
    let editor = BoxSizer::builder(Orientation::Horizontal).build();
    editor.add(
        &entry,
        0,
        SizerFlag::Expand | SizerFlag::Right,
        page.from_dip(12),
    );
    let field_col = BoxSizer::builder(Orientation::Vertical).build();
    field_col.add(&section, 0, SizerFlag::Expand, 0);
    editor_scroll.set_sizer(field_col, true);
    editor.add(&editor_scroll, 1, SizerFlag::Expand, 0);
    root.add_sizer(&editor, 1, SizerFlag::Expand, 0);
    let status = StaticText::builder(&page)
        .with_label("")
        .with_style(StaticTextStyle::EllipsizeEnd)
        .build();
    status.set_foreground_color(theme::theme().ink_muted);
    status.set_min_size(Size::new(0, -1));
    root.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        20,
    );
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let maintenance: &dyn WxWidget = settings.map(|p| p as &dyn WxWidget).unwrap_or(&page);
    let start_button = header_actions.map(|a| a.0).unwrap_or_else(|| {
        Button::builder(maintenance)
            .with_label(tr(text, "启动 WorkBuddy", "Start WorkBuddy"))
            .build()
    });
    let reload_button = header_actions.map(|a| a.1).unwrap_or_else(|| {
        Button::builder(maintenance)
            .with_label(tr(text, "重新读取", "Reload"))
            .build()
    });
    let restore_button = Button::builder(maintenance)
        .with_label(tr(text, "恢复配置", "Restore configuration"))
        .with_kind(super::controls::ButtonKind::Ghost)
        .build();
    restore_button.hide();
    let save_button = Button::builder(&page)
        .with_label(tr(text, "保存模型", "Save model"))
        .build();
    let delete_button = Button::builder(&page)
        .with_label(tr(text, "移除模型", "Remove model"))
        .with_kind(super::controls::ButtonKind::Ghost)
        .build();
    let utilities = BoxSizer::builder(Orientation::Vertical).build();
    let maintenance_buttons = if header_actions.is_some() {
        vec![&restore_button]
    } else {
        vec![&start_button, &reload_button, &restore_button]
    };
    for button in maintenance_buttons {
        utilities.add(button, 0, SizerFlag::Left | SizerFlag::Top, 18);
    }
    if let Some(host) = settings {
        if let Some(s) = host.get_sizer() {
            s.add_sizer(&utilities, 0, SizerFlag::Expand, 0);
        }
    } else {
        root.add_sizer(&utilities, 0, SizerFlag::Expand, 0);
    }
    actions.add_stretch_spacer(1);
    actions.add(&delete_button, 0, SizerFlag::Right, page.from_dip(8));
    actions.add(&save_button, 0, SizerFlag::default(), 0);
    root.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom,
        page.from_dip(8),
    );
    page.set_sizer(root, true);
    page.set_min_size(Size::new(0, 0));
    page.layout();
    editor_scroll.fit_inside();
    let tab = WorkBuddyTab {
        page,
        editor_scroll,
        entry,
        provider,
        model,
        protocol,
        default_effort,
        text,
        save_button,
        delete_button,
        restore_button,
        reload_button,
        start_button,
        status,
        in_flight: Arc::new(AtomicBool::new(false)),
        refresh_pending: Default::default(),
        provider_options: Rc::new(RefCell::new(Vec::new())),
        selection: Rc::new(RefCell::new(WorkBuddySelection::default())),
        draft: Default::default(),
        editor_base: Default::default(),
    };
    apply_model(&tab, &WorkBuddyModelConfig::default());
    update_controls(&tab);
    bind_editor_events(&tab);
    tab
}

fn bind_editor_events(tab: &WorkBuddyTab) {
    let entry_tab = tab.clone();
    tab.entry.on_selection_changed(move |_| {
        if entry_tab.in_flight.load(Ordering::SeqCst) {
            return;
        }
        let Some(mut status) = entry_tab.selection.borrow().status.clone() else {
            return;
        };
        capture_draft(&entry_tab);
        let selected = entry_tab
            .entry
            .get_selection()
            .and_then(|index| status.entries.get(index as usize))
            .map(|entry| entry.entry_id.clone());
        select_status_entry(&mut status, selected.as_deref());
        apply_status(&entry_tab, status);
    });
    let provider_tab = tab.clone();
    tab.provider.on_selection_changed(move |_| {
        let Some(index) = provider_tab.provider.get_selection() else {
            return;
        };
        if let Some(index) = index.checked_sub(1) {
            apply_provider_option(&provider_tab, index as usize);
        } else {
            provider_tab.model.clear();
            refresh_model_settings(&provider_tab);
        }
        update_controls(&provider_tab);
    });
    let model_tab = tab.clone();
    tab.model.on_selection_changed(move |_| {
        refresh_model_settings(&model_tab);
        update_controls(&model_tab);
    });
    let protocol_tab = tab.clone();
    tab.protocol
        .on_selection_changed(move |_| refresh_model_settings(&protocol_tab));
}

pub(super) fn bind_actions(
    tab: &WorkBuddyTab,
    api: &ApiClient,
    frame: &Frame,
    text: GuiText,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
) {
    let start_tab = tab.clone();
    let start_api = api.clone();
    let start_tx = gui_tx.clone();
    tab.start_button.on_click(move |_| {
        if !begin_action(
            &start_tab,
            tr(text, "正在启动 WorkBuddy…", "Starting WorkBuddy…"),
        ) {
            return;
        }
        let api = start_api.clone();
        let tx = start_tx.clone();
        thread::spawn(move || {
            let result = api.start_workbuddy_desktop();
            let _ = tx.send(super::GuiMessage::WorkBuddy(WorkBuddyActionResult::Start(
                result,
            )));
            wxdragon::wake_up_idle();
        });
    });
    let save_button = tab.save_button;
    let tab_for_save = tab.clone();
    let api_for_save = api.clone();
    let gui_tx_for_save = gui_tx.clone();
    let frame_for_save = *frame;
    save_button.on_click(move |_| {
        if tab_for_save.in_flight.load(Ordering::SeqCst) {
            return;
        }
        let request = match build_request(&tab_for_save) {
            Ok(request) => request,
            Err(error) => {
                show_error(&frame_for_save, &error);
                return;
            }
        };
        if !begin_action(&tab_for_save, text.workbuddy_saving()) {
            return;
        }
        let copy = copy_request(&tab_for_save, &request);
        let api = api_for_save.clone();
        let gui_tx = gui_tx_for_save.clone();
        thread::spawn(move || {
            let result = if let Some(body) = copy {
                api.desktop_post("/api/desktop/models/copy", &body)
                    .and_then(|v| {
                        serde_json::from_value(v["status"].clone())
                            .map_err(|_| "已返回保存结果，请重新读取 WorkBuddy 配置核对".into())
                    })
            } else {
                api.save_workbuddy_config(&request)
            };
            let _ = gui_tx.send(super::GuiMessage::WorkBuddy(WorkBuddyActionResult::Save(
                result,
            )));
            wxdragon::wake_up_idle();
        });
    });

    let tab_for_restore = tab.clone();
    let api_for_restore = api.clone();
    let gui_tx_for_restore = gui_tx.clone();
    let frame_for_restore = *frame;
    tab.restore_button.on_click(move |_| {
        let revision = match current_revision(&tab_for_restore) {
            Ok(revision) => revision,
            Err(error) => {
                show_error(&frame_for_restore, &error);
                return;
            }
        };
        if !begin_action(&tab_for_restore, text.workbuddy_restoring()) {
            return;
        }
        let api = api_for_restore.clone();
        let gui_tx = gui_tx_for_restore.clone();
        thread::spawn(move || {
            let result = api.restore_workbuddy_config(&revision);
            let _ = gui_tx.send(super::GuiMessage::WorkBuddy(
                WorkBuddyActionResult::Restore(result),
            ));
            wxdragon::wake_up_idle();
        });
    });

    let delete_tab = tab.clone();
    let delete_api = api.clone();
    let delete_tx = gui_tx.clone();
    let delete_frame = *frame;
    tab.delete_button.on_click(move |_| {
        if is_draft(&delete_tab) {
            delete_tab.draft.borrow_mut().take();
            let current = delete_tab.selection.borrow().status.clone();
            if let Some(mut status) = current {
                let id = status.entries.first().map(|e| e.entry_id.clone());
                select_status_entry(&mut status, id.as_deref());
                apply_status(&delete_tab, status);
            }
            return;
        }
        let Some(entry_id) = selected_entry_id(&delete_tab) else {
            return;
        };
        let revision = match current_revision(&delete_tab) {
            Ok(revision) => revision,
            Err(error) => {
                show_error(&delete_frame, &error);
                return;
            }
        };
        if !begin_action(&delete_tab, tr(text, "正在删除模型…", "Deleting model…")) {
            return;
        }
        let api = delete_api.clone();
        let tx = delete_tx.clone();
        thread::spawn(move || {
            let result = api.delete_workbuddy_config(&WorkBuddyEntryRequest { entry_id, revision });
            let _ = tx.send(super::GuiMessage::WorkBuddy(WorkBuddyActionResult::Delete(
                result,
            )));
            wxdragon::wake_up_idle();
        });
    });

    let tab_for_reload = tab.clone();
    let api_for_reload = api.clone();
    let gui_tx_for_reload = gui_tx.clone();
    tab.reload_button.on_click(move |_| {
        refresh_configuration(
            &tab_for_reload,
            &api_for_reload,
            &gui_tx_for_reload,
            false,
            true,
        );
    });
    refresh_configuration(tab, api, gui_tx, true, false);
}

pub(super) fn apply_result(
    tab: &WorkBuddyTab,
    frame: &Frame,
    text: GuiText,
    api: &ApiClient,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    result: WorkBuddyActionResult,
) {
    // Only release the busy state after applying the reply on the GUI thread.
    tab.in_flight.store(false, Ordering::SeqCst);
    let queued = tab.refresh_pending.borrow_mut().take();
    if matches!(&result, WorkBuddyActionResult::Refresh(..)) && queued.is_some() {
        refresh_configuration(tab, api, gui_tx, false, queued.unwrap());
        return;
    }
    match result {
        WorkBuddyActionResult::Start(Ok(detail)) => {
            if let Some(status) = &tab.selection.borrow().status {
                show_configuration_status(tab, status);
            }
            show_info(frame, &detail);
        }
        WorkBuddyActionResult::Start(Err(error)) => {
            if let Some(status) = &tab.selection.borrow().status {
                show_configuration_status(tab, status);
            }
            show_error(frame, &error);
        }
        WorkBuddyActionResult::Save(Ok(status)) => {
            if is_draft(tab) {
                tab.draft.borrow_mut().take();
            }
            apply_status(tab, status);
            show_info(frame, text.workbuddy_saved());
        }
        WorkBuddyActionResult::Restore(Ok(status)) => {
            apply_status(tab, status);
            show_info(
                frame,
                tr(text, "上次操作已撤销。", "Last operation undone."),
            );
        }
        WorkBuddyActionResult::Delete(Ok(status)) => {
            apply_status(tab, status);
            show_info(
                frame,
                tr(
                    text,
                    "所选模型已删除，可撤销上次操作。",
                    "Selected model deleted. You can undo this operation.",
                ),
            );
        }
        WorkBuddyActionResult::Refresh(Ok((options, status)), rebase) => {
            let preserved = preserved_editor(tab, rebase);
            let changed = preserved
                .as_ref()
                .is_some_and(|editor| editor.base.1 != status.revision);
            let removed = preserved.as_ref().is_some_and(|editor| {
                !status
                    .entries
                    .iter()
                    .any(|entry| entry.entry_id == editor.entry_id)
            });
            if rebase || (tab.draft.borrow().is_none() && preserved.is_none()) {
                *tab.provider_options.borrow_mut() = options;
            }
            if rebase {
                if let Some(draft) = tab.draft.borrow_mut().as_mut() {
                    draft.1 = status.revision.clone();
                }
            }
            apply_status(tab, status);
            if let Some(super::configuration_refresh::PreservedEditor {
                entry_id,
                values,
                base,
            }) = preserved
            {
                let mut model = tab
                    .selection
                    .borrow()
                    .status
                    .as_ref()
                    .unwrap()
                    .model
                    .clone();
                model.upstream_provider = values["provider"].as_str().unwrap_or_default().into();
                model.provider_model = values["model"].as_str().unwrap_or_default().into();
                model.upstream_protocol = values["protocol"].as_str().unwrap_or_default().into();
                model.reasoning.default_effort =
                    values["effort"].as_str().unwrap_or_default().into();
                model.upstream_default_reasoning_effort = model.reasoning.default_effort.clone();
                apply_provider_options(tab, &model);
                {
                    let mut selection = tab.selection.borrow_mut();
                    selection.fresh &= !changed && !removed;
                    if let Some(status) = selection.status.as_mut() {
                        status.revision = base.1.clone();
                        status.selected_entry_id = Some(entry_id);
                    }
                }
                *tab.editor_base.borrow_mut() = Some(base);
                if removed {
                    tab.entry.set_selection(0, false);
                    tab.status.set_label(tr(text, "正在编辑的模型已移除；当前输入已保留，请载入更新。", "The model being edited was removed. Your inputs are retained; load updates to continue."));
                } else if changed {
                    tab.status.set_label(tr(text, "配置已更新；当前输入已保留，请载入更新。", "Configuration changed. Your inputs are retained; load updates to continue."));
                }
            }
        }
        WorkBuddyActionResult::Refresh(Err(error), _) => {
            tab.selection.borrow_mut().fresh = false;
            tab.status.set_label(&format!(
                "{}\n{error}",
                tr(
                    text,
                    "读取失败，请确认 Hub 服务已启动后刷新。",
                    "Could not load configuration. Make sure Hub is running, then refresh."
                )
            ));
        }
        WorkBuddyActionResult::Save(Err(error))
        | WorkBuddyActionResult::Delete(Err(error))
        | WorkBuddyActionResult::Restore(Err(error)) => {
            tab.selection.borrow_mut().fresh = false;
            tab.status.set_label(tr(
                text,
                "当前输入已保留；请载入更新。",
                "Your inputs are retained; load updates to continue.",
            ));
            show_error(frame, &error);
        }
    }
    update_controls(tab);
    tab.status.wrap(920);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
    if let Some(rebase) = queued {
        refresh_configuration(tab, api, gui_tx, false, rebase);
    }
}

pub(super) fn refresh_configuration(
    tab: &WorkBuddyTab,
    api: &ApiClient,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    startup: bool,
    rebase_draft: bool,
) {
    if tab.in_flight.load(Ordering::SeqCst) {
        tab.refresh_pending.borrow_mut().request(rebase_draft);
        return;
    }
    capture_draft(tab);
    if !begin_action(
        tab,
        tr(
            tab.text,
            "正在读取模型与渠道…",
            "Loading models and channels…",
        ),
    ) {
        return;
    }
    let previous_selection = tab
        .selection
        .borrow()
        .status
        .as_ref()
        .map(|status| status.selected_entry_id.clone());
    let api = api.clone();
    let gui_tx = gui_tx.clone();
    thread::spawn(move || {
        let mut result = Err(String::from("provider list is not available yet"));
        let attempts = if startup {
            PROVIDER_REFRESH_ATTEMPTS
        } else {
            1
        };
        for attempt in 0..attempts {
            result = api.get_app_config().and_then(|config| {
                // Read the collection even if the selected entry was deleted
                // externally, so refresh always provides a way to recover.
                let mut status = api.get_workbuddy_config(None)?;
                if let Some(selected) = &previous_selection {
                    select_status_entry(&mut status, selected.as_deref());
                }
                Ok((provider_options_with_copies(&config), status))
            });
            if result.is_ok() || attempt + 1 == attempts {
                break;
            }
            thread::sleep(PROVIDER_REFRESH_RETRY_DELAY);
        }
        let _ = gui_tx.send(super::GuiMessage::WorkBuddy(
            WorkBuddyActionResult::Refresh(result, rebase_draft),
        ));
        wxdragon::wake_up_idle();
    });
}

fn provider_options_from_config(config: &AppConfig) -> Vec<WorkBuddyProviderOption> {
    config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.enabled && !provider.is_client_reserved())
        .filter_map(provider_option)
        .collect()
}
fn provider_options_with_copies(config: &AppConfig) -> Vec<WorkBuddyProviderOption> {
    let mut options = provider_options_from_config(config);
    if let Some(revision) = &config.revision {
        options.extend(
            config
                .ai_gateway
                .providers
                .iter()
                .filter(|p| p.enabled && p.is_client_reserved())
                .filter_map(|p| {
                    let mut option = provider_option(p)?;
                    option.display_name = copy_source_label(p);
                    option.copy = Some((
                        crate::ai_gateway::config::provider_route_id(p),
                        revision.clone(),
                    ));
                    Some(option)
                }),
        );
    }
    options
}
fn copy_request(tab: &WorkBuddyTab, request: &WorkBuddySaveRequest) -> Option<serde_json::Value> {
    let options = tab.provider_options.borrow();
    let option = options
        .iter()
        .find(|p| p.name == request.model.upstream_provider)?;
    let (route, revision) = option.copy.as_ref()?;
    Some(
        serde_json::json!({"client":"workbuddy","sourceRoute":route,"revision":revision,"model":request.model.provider_model,"workbuddy":request}),
    )
}

pub(super) fn copy_source_label(provider: &ProviderConfig) -> String {
    let client = if provider.is_workbuddy() {
        "WorkBuddy"
    } else if provider.is_gmclaw() {
        "天工 Work"
    } else {
        provider
            .desktop_binding
            .as_ref()
            .map(|b| b.client.name())
            .unwrap_or("客户端")
    };
    let model = super::ai_gateway::provider_display_name(provider);
    format!("{client} · {model}")
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
    Some(WorkBuddyProviderOption {
        copy: None,
        name: provider.name.clone(),
        display_name: provider.name.clone(),
        upstream_url: if provider.provider_type == ProviderType::ChatGptResponses {
            crate::ai_gateway::chatgpt_auth::BASE_URL.to_string()
        } else {
            provider_display_base_url(&provider.base_url)
        },
        has_api_key: !provider.api_key.trim().is_empty()
            || provider.provider_type == ProviderType::ChatGptResponses,
        upstream_protocol: match &provider.provider_type {
            ProviderType::OpenAiResponses | ProviderType::ChatGptResponses => "openai-responses",
            ProviderType::AnthropicMessages => "anthropic-messages",
            _ => "openai-chat",
        }
        .to_string(),
        models,
        model_aliases: provider.model_aliases.clone(),
        compatibility: provider.compatibility.clone(),
    })
}

fn apply_provider_options(tab: &WorkBuddyTab, model: &WorkBuddyModelConfig) {
    let selected = tab.provider_options.borrow().iter().position(|option| {
        !model.upstream_provider.trim().is_empty()
            && option
                .name
                .eq_ignore_ascii_case(model.upstream_provider.trim())
    });
    tab.provider.clear();
    tab.provider.append(tr(
        tab.text,
        "请选择大模型接入渠道",
        "Select a source channel",
    ));
    for option in tab.provider_options.borrow().iter() {
        tab.provider.append(&option.display_name);
    }
    if let Some(index) = selected {
        tab.provider.set_selection(index as u32 + 1);
        apply_provider_option_for_model(tab, index, Some(&model.provider_model));
        if !tab
            .model
            .get_value()
            .eq_ignore_ascii_case(&model.provider_model)
        {
            // Preserve an edited/deleted source model for inspection; saving is
            // disabled until the user explicitly chooses an available model.
            tab.model.append(&model.provider_model);
            tab.model.set_value(&model.provider_model);
        }
    } else {
        tab.provider.set_selection(0);
        tab.model.clear();
        if !model.provider_model.is_empty() {
            tab.model.append(&model.provider_model);
            tab.model.set_selection(0);
            tab.model.set_value(&model.provider_model);
        }
    }
    // A manually selected protocol and effort belong to this entry, not its
    // source channel's current defaults or the previous entry in the form.
    apply_saved_model_settings(tab, model);
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
}

fn refresh_model_settings(tab: &WorkBuddyTab) {
    let options = tab.provider_options.borrow();
    let provider = tab
        .provider
        .get_selection()
        .and_then(|index| index.checked_sub(1))
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
}

fn build_model(tab: &WorkBuddyTab) -> Result<WorkBuddyModelConfig, String> {
    let mut model = tab
        .selection
        .borrow()
        .status
        .as_ref()
        .filter(|status| status.selected_entry_id.is_some())
        .map(|status| status.model.clone())
        .unwrap_or_default();
    model.api_key = workbuddy_config::DEFAULT_WORKBUDDY_API_KEY.to_string();
    // The daemon resolves the latest source credentials. Do not put a real key
    // in controls, WorkBuddy status, or a save payload.
    model.upstream_url.clear();
    model.upstream_api_key.clear();
    let provider = tab
        .provider
        .get_selection()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| tab.provider_options.borrow().get(index as usize).cloned())
        .ok_or_else(|| {
            tr(
                tab.text,
                "请先选择已启用的大模型接入渠道。",
                "Select an enabled source channel first.",
            )
            .to_string()
        })?;
    model.upstream_provider = provider.name.clone();
    let original_model = model.provider_model.clone();
    model.provider_model = clean(&tab.model.get_value());
    if !provider
        .models
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&model.provider_model))
    {
        return Err(tr(
            tab.text,
            "请选择大模型接入渠道中的可用模型。",
            "Select an available model from the source channel.",
        )
        .to_string());
    }
    if selected_entry_id(tab).is_none() || model.name == original_model {
        model.name = model.provider_model.clone();
    }
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
    model.extra.remove("cacheKey");
    model.extra.remove("cache_key");
    if workbuddy_config::uses_prompt_cache_key(&model.upstream_protocol) {
        let cache_key = workbuddy_config::default_cache_key_for_provider(&provider.name);
        model
            .extra
            .insert("cacheKey".to_string(), serde_json::Value::String(cache_key));
    }
    Ok(model)
}

fn build_request(tab: &WorkBuddyTab) -> Result<WorkBuddySaveRequest, String> {
    Ok(WorkBuddySaveRequest {
        entry_id: selected_entry_id(tab),
        revision: current_revision(tab)?,
        model: build_model(tab)?,
    })
}

fn select_status_entry(status: &mut WorkBuddyConfigStatus, selected: Option<&str>) {
    let entry = selected.and_then(|id| status.entries.iter().find(|entry| entry.entry_id == id));
    status.selected_entry_id = entry.map(|entry| entry.entry_id.clone());
    status.model = entry.map(|entry| entry.model.clone()).unwrap_or_else(|| {
        let mut model = WorkBuddyModelConfig::default();
        model.id.clear();
        model.name.clear();
        model.url.clear();
        model.reasoning.default_effort = "xhigh".into();
        model.upstream_default_reasoning_effort = "xhigh".into();
        model.provider_model.clear();
        model
    });
}

fn selected_entry_id(tab: &WorkBuddyTab) -> Option<String> {
    tab.selection
        .borrow()
        .status
        .as_ref()
        .and_then(|status| status.selected_entry_id.clone())
}
fn is_draft(tab: &WorkBuddyTab) -> bool {
    tab.draft.borrow().is_some() && selected_entry_id(tab).is_none()
}
fn capture_draft(tab: &WorkBuddyTab) {
    if !is_draft(tab) {
        return;
    }
    let mut draft = tab.draft.borrow_mut();
    let (model, _) = draft.as_mut().unwrap();
    model.provider_model = tab.model.get_value();
    model.upstream_provider = tab
        .provider
        .get_selection()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| {
            tab.provider_options
                .borrow()
                .get(i as usize)
                .map(|p| p.name.clone())
        })
        .unwrap_or_default();
    model.upstream_protocol = selected_protocol(tab).into();
    model.reasoning.default_effort = tab
        .default_effort
        .get_string_selection()
        .unwrap_or_default();
}
pub(super) fn begin_model_draft(tab: &WorkBuddyTab) {
    if tab.in_flight.load(Ordering::SeqCst) {
        return;
    }
    let Some(mut status) = tab.selection.borrow().status.clone() else {
        return;
    };
    if !tab.selection.borrow().fresh || status.revision.is_empty() {
        return;
    }
    if is_draft(tab) {
        return;
    }
    if tab.draft.borrow().is_none() {
        let mut model = WorkBuddyModelConfig::default();
        model.id.clear();
        model.name.clear();
        model.provider_model.clear();
        model.url.clear();
        *tab.draft.borrow_mut() = Some((model, status.revision.clone()));
    }
    select_status_entry(&mut status, None);
    apply_status(tab, status);
}

fn current_revision(tab: &WorkBuddyTab) -> Result<String, String> {
    let selection = tab.selection.borrow();
    selection
        .status
        .as_ref()
        .filter(|status| selection.fresh && !status.revision.is_empty())
        .map(|status| {
            if status.selected_entry_id.is_none() {
                tab.draft
                    .borrow()
                    .as_ref()
                    .map(|d| d.1.clone())
                    .unwrap_or_else(|| status.revision.clone())
            } else {
                status.revision.clone()
            }
        })
        .ok_or_else(|| {
            tr(
                tab.text,
                "请先刷新以读取最新 WorkBuddy 配置。",
                "Refresh to load the latest WorkBuddy configuration first.",
            )
            .to_string()
        })
}

fn begin_action(tab: &WorkBuddyTab, message: &str) -> bool {
    if tab.in_flight.swap(true, Ordering::SeqCst) {
        return false;
    }
    tab.status.set_label(message);
    update_controls(tab);
    true
}

fn update_controls(tab: &WorkBuddyTab) {
    let busy = tab.in_flight.load(Ordering::SeqCst);
    let selection = tab.selection.borrow();
    let fresh = selection.fresh
        && selection
            .status
            .as_ref()
            .is_some_and(|status| !status.revision.is_empty());
    let editable = fresh
        && !busy
        && selection
            .status
            .as_ref()
            .is_some_and(|status| status.error.is_none());
    let provider = tab
        .provider
        .get_selection()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| tab.provider_options.borrow().get(index as usize).cloned());
    let model_valid = provider.as_ref().is_some_and(|provider| {
        provider
            .models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(&tab.model.get_value()))
    });
    tab.reload_button.enable(!busy);
    tab.reload_button.set_label("载入更新");
    tab.reload_button.show(!fresh && !busy);
    if let Some(parent) = tab.reload_button.get_parent() {
        parent.layout();
    }
    tab.start_button.enable(!busy);
    tab.entry.enable(editable);
    tab.provider.enable(editable);
    tab.model.enable(editable && provider.is_some());
    tab.protocol.enable(editable && provider.is_some());
    tab.default_effort.enable(editable && provider.is_some());
    tab.save_button.enable(editable && model_valid);
    tab.delete_button.enable(
        editable
            && (selection
                .status
                .as_ref()
                .is_some_and(|status| status.selected_entry_id.is_some())
                || tab.draft.borrow().is_some()),
    );
    tab.delete_button.set_label(if is_draft(tab) {
        "放弃新建"
    } else {
        "移除模型"
    });
    // A legacy backup may still recover a malformed current file; the daemon
    // performs the corresponding backup and revision validation.
    tab.restore_button.enable(
        fresh
            && !busy
            && selection
                .status
                .as_ref()
                .is_some_and(|status| status.backup_exists),
    );
    tab.restore_button
        .show(selection.status.as_ref().is_some_and(|s| s.backup_exists));
    for choice in [&tab.provider, &tab.protocol, &tab.default_effort] {
        connection_field_theme(choice, choice.is_enabled());
    }
    connection_field_theme(&tab.model, tab.model.is_enabled());
}

fn apply_status(tab: &WorkBuddyTab, mut status: WorkBuddyConfigStatus) {
    let selected_id = status.selected_entry_id.clone();
    select_status_entry(&mut status, selected_id.as_deref());
    let mut labels = status
        .entries
        .iter()
        .map(|entry| {
            format!(
                "{} · {}",
                entry.model.provider_model,
                if entry.configured {
                    "使用中"
                } else {
                    "待完善"
                }
            )
        })
        .collect::<Vec<_>>();
    if tab.draft.borrow().is_some() {
        labels.push("新建模型".into());
    }
    tab.entry.replace(&labels);
    let selected = status.selected_entry_id.as_ref().and_then(|id| {
        status
            .entries
            .iter()
            .position(|entry| &entry.entry_id == id)
    });
    tab.entry.set_selection(
        selected.unwrap_or(status.entries.len()) as u32,
        selected.is_some() || tab.draft.borrow().is_some(),
    );
    let mut model = if status.selected_entry_id.is_none() {
        tab.draft
            .borrow()
            .as_ref()
            .map(|d| d.0.clone())
            .unwrap_or_else(|| status.model.clone())
    } else {
        status.model.clone()
    };
    if let Some(entry) = selected.and_then(|index| status.entries.get(index)) {
        model.upstream_provider = entry.source_provider.clone().unwrap_or_default();
        model.url = entry.local_url.clone();
    }
    apply_model(tab, &model);
    apply_provider_options(tab, &model);
    show_configuration_status(tab, &status);
    let revision = status.revision.clone();
    *tab.selection.borrow_mut() = WorkBuddySelection {
        fresh: !revision.is_empty(),
        status: Some(status),
    };
    *tab.editor_base.borrow_mut() = Some((editor_values(tab), revision));
    update_controls(tab);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
}

fn show_configuration_status(tab: &WorkBuddyTab, status: &WorkBuddyConfigStatus) {
    let message = if let Some(error) = &status.error {
        error.as_str()
    } else if status.selected_entry_id.is_none() {
        tr(
            tab.text,
            "选择渠道和模型后保存。",
            "Choose a source channel and model, then save to add a separate WorkBuddy model.",
        )
    } else if status
        .selected_entry_id
        .as_ref()
        .and_then(|id| status.entries.iter().find(|entry| &entry.entry_id == id))
        .is_some_and(|entry| entry.configured)
    {
        tr(
            tab.text,
            "模型已保存，可在 WorkBuddy 中选择使用。",
            "This model is connected to WorkBuddy. You can edit or delete this entry here.",
        )
    } else {
        tr(
            tab.text,
            "当前条目接入信息不完整，请核对大模型接入渠道和模型后保存。",
            "This entry has incomplete connection settings. Review its source and model, then save.",
        )
    };
    tab.status.set_label(message);
    tab.status.wrap(920);
}

fn editor_values(tab: &WorkBuddyTab) -> serde_json::Value {
    let provider = tab
        .provider
        .get_selection()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| {
            tab.provider_options
                .borrow()
                .get(index as usize)
                .map(|provider| provider.name.clone())
        });
    serde_json::json!({"provider":provider,"model":tab.model.get_value(),"protocol":selected_protocol(tab),
        "effort":tab.default_effort.get_string_selection().unwrap_or_default()})
}

fn preserved_editor(
    tab: &WorkBuddyTab,
    rebase: bool,
) -> Option<super::configuration_refresh::PreservedEditor> {
    if rebase {
        return None;
    }
    let entry_id = selected_entry_id(tab)?;
    let base = tab.editor_base.borrow().clone()?;
    let values = editor_values(tab);
    (values != base.0).then_some(super::configuration_refresh::PreservedEditor {
        entry_id,
        values,
        base,
    })
}

fn apply_model(tab: &WorkBuddyTab, model: &WorkBuddyModelConfig) {
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
#[cfg(all(test, windows))]
pub(super) fn review_inline_draft(
    frame: &Frame,
    model: &super::model_fields::ModelPage,
    settings: &Panel,
    header: &super::client_header::ClientHeader,
) {
    let text = GuiText::new(GuiLocale::ZhCn);
    let review_api = ApiClient::new("http://127.0.0.1:1/unused-native-review".into(), text);
    let (review_tx, _review_rx) = tokio::sync::mpsc::unbounded_channel();
    let tab = create_with_settings(
        &model.legacy_host,
        text,
        Some(settings),
        Some((header.launch, header.reload)),
    );
    model
        .legacy_host
        .get_sizer()
        .unwrap()
        .add(&tab.page, 1, SizerFlag::Expand, 0);
    let config = AppConfig {
        ai_gateway: crate::ai_gateway::config::AiGatewayConfig {
            providers: vec![ProviderConfig {
                name: "synthetic-source".into(),
                base_url: "https://provider.example.invalid".into(),
                models: vec!["gpt-6".into(), "gpt-6-astra".into()],
                ..Default::default()
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    *tab.provider_options.borrow_mut() = provider_options_from_config(&config);
    let mut saved = WorkBuddyModelConfig::default();
    saved.provider_model = "gpt-6".into();
    saved.upstream_provider = "synthetic-source".into();
    let status = WorkBuddyConfigStatus {
        path: "C:/Synthetic/workbuddy/models.json".into(),
        exists: true,
        backup_path: String::new(),
        backup_exists: false,
        model: saved.clone(),
        entries: vec![crate::workbuddy_config::WorkBuddyEntryStatus {
            entry_id: "saved-entry".into(),
            model: saved,
            configured: true,
            local_url: "http://127.0.0.1:3210/synthetic".into(),
            source_provider: Some("synthetic-source".into()),
        }],
        selected_entry_id: Some("saved-entry".into()),
        revision: "original-revision".into(),
        error: None,
    };
    apply_status(&tab, status.clone());
    let original_options = tab.provider_options.borrow().clone();
    tab.protocol.set_selection(1);
    refresh_model_settings(&tab);
    tab.default_effort.set_value("max");
    let edited = editor_values(&tab);
    let mut latest = status.clone();
    latest.revision = "assistant-saved-revision".into();
    let mut new_entry = latest.entries[0].clone();
    new_entry.entry_id = "assistant-added-entry".into();
    new_entry.model.provider_model = "gpt-6-astra".into();
    latest.entries.push(new_entry);
    apply_result(
        &tab,
        frame,
        text,
        &review_api,
        &review_tx,
        WorkBuddyActionResult::Refresh(Ok((original_options.clone(), latest.clone())), false),
    );
    assert_eq!(
        tab.selection
            .borrow()
            .status
            .as_ref()
            .unwrap()
            .entries
            .len(),
        2
    );
    assert_eq!(editor_values(&tab), edited);
    assert_eq!(selected_entry_id(&tab).as_deref(), Some("saved-entry"));
    assert_eq!(
        tab.selection.borrow().status.as_ref().unwrap().revision,
        "original-revision"
    );
    assert!(build_request(&tab).is_err());
    assert!(tab.reload_button.is_shown());
    apply_result(
        &tab,
        frame,
        text,
        &review_api,
        &review_tx,
        WorkBuddyActionResult::Refresh(Ok((original_options.clone(), latest.clone())), true),
    );
    assert_eq!(
        build_request(&tab).unwrap().revision,
        "assistant-saved-revision"
    );
    assert_ne!(editor_values(&tab), edited);
    assert!(!tab.reload_button.is_shown());
    tab.protocol.set_selection(1);
    refresh_model_settings(&tab);
    tab.default_effort.set_value("max");
    let edited = editor_values(&tab);
    latest.entries.remove(0);
    latest.revision = "external-delete-revision".into();
    select_status_entry(&mut latest, None);
    apply_result(
        &tab,
        frame,
        text,
        &review_api,
        &review_tx,
        WorkBuddyActionResult::Refresh(Ok((original_options.clone(), latest)), false),
    );
    assert_eq!(editor_values(&tab), edited);
    assert_eq!(selected_entry_id(&tab).as_deref(), Some("saved-entry"));
    assert!(tab.entry.get_selection().is_none());
    assert!(build_request(&tab).is_err());
    assert!(tab.reload_button.is_shown());
    *tab.provider_options.borrow_mut() = original_options;
    apply_status(&tab, status);
    for _ in 0..4 {
        begin_model_draft(&tab);
        begin_model_draft(&tab);
        assert_eq!(tab.entry.get_selection(), Some(1));
        assert!(tab.entry.find_window_by_name("client-model-0").is_some());
        assert!(tab.entry.find_window_by_name("client-model-2").is_none());
        tab.provider.set_selection(1);
        apply_provider_option(&tab, 0);
        tab.protocol.set_selection(1);
        refresh_model_settings(&tab);
        tab.default_effort.set_value("max");
        let draft = build_request(&tab).unwrap();
        assert!(draft.entry_id.is_none());
        assert_eq!(draft.model.upstream_protocol, "openai-chat");
        assert_eq!(draft.model.reasoning.default_effort, "max");
        super::native_model_review::click(
            &tab.entry.find_window_by_name("client-model-0").unwrap(),
        );
        assert_eq!(selected_entry_id(&tab).as_deref(), Some("saved-entry"));
        begin_model_draft(&tab);
        assert_eq!(selected_protocol(&tab), "openai-chat");
        assert_eq!(
            tab.default_effort.get_string_selection().as_deref(),
            Some("max")
        );
    }
    let mut copy_config = config.clone();
    copy_config.revision = Some("original-source-revision".into());
    let mut source = copy_config.ai_gateway.providers[0].clone();
    source.name = "gmclaw:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
    copy_config.ai_gateway.providers.push(source.clone());
    assert_eq!(provider_options_from_config(&copy_config).len(), 1);
    let current = build_model(&tab).unwrap();
    *tab.provider_options.borrow_mut() = provider_options_with_copies(&copy_config);
    apply_provider_options(&tab, &current);
    tab.provider.set_selection(2);
    apply_provider_option(&tab, 1);
    tab.protocol.set_selection(1);
    refresh_model_settings(&tab);
    tab.default_effort.set_value("max");
    capture_draft(&tab);
    let request = build_request(&tab).unwrap();
    let copy = copy_request(&tab, &request).expect("explicit cross-client source");
    assert_eq!(copy["revision"], "original-source-revision");
    assert_eq!(copy["workbuddy"]["revision"], "original-revision");
    assert_eq!(
        copy["sourceRoute"],
        crate::ai_gateway::config::provider_route_id(&source)
    );
    assert_eq!(
        copy["workbuddy"]["model"]["upstreamProtocol"],
        "openai-chat"
    );
    assert_eq!(
        copy["workbuddy"]["model"]["upstreamDefaultReasoningEffort"],
        "max"
    );
    let mut latest = tab.selection.borrow().status.clone().unwrap();
    latest.revision = "later-external-revision".into();
    apply_result(
        &tab,
        frame,
        text,
        &review_api,
        &review_tx,
        WorkBuddyActionResult::Refresh(Ok((vec![], latest.clone())), false),
    );
    assert_eq!(build_request(&tab).unwrap().revision, "original-revision");
    assert_eq!(
        copy_request(&tab, &build_request(&tab).unwrap()).unwrap()["revision"],
        "original-source-revision"
    );
    tab.selection.borrow_mut().fresh = false;
    update_controls(&tab);
    assert!(tab.reload_button.is_shown());
    copy_config.revision = Some("explicitly-loaded-source-revision".into());
    apply_result(
        &tab,
        frame,
        text,
        &review_api,
        &review_tx,
        WorkBuddyActionResult::Refresh(
            Ok((provider_options_with_copies(&copy_config), latest)),
            true,
        ),
    );
    let loaded = build_request(&tab).unwrap();
    assert_eq!(loaded.revision, "later-external-revision");
    assert_eq!(loaded.model.upstream_protocol, "openai-chat");
    assert_eq!(loaded.model.reasoning.default_effort, "max");
    assert_eq!(
        copy_request(&tab, &loaded).unwrap()["revision"],
        "explicitly-loaded-source-revision"
    );
    assert!(!tab.reload_button.is_shown());
    frame.layout();
    model.show_draft(false);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
    let bounds = tab.page.get_client_size();
    assert!(bounds.width > 500 && bounds.height > 280, "{bounds:?}");
    assert!(
        tab.provider.is_shown()
            && tab.model.is_shown()
            && tab.protocol.is_shown()
            && tab.default_effort.is_shown()
    );
    assert!(tab.save_button.get_position().y + tab.save_button.get_size().height <= bounds.height);
    assert!(
        (bounds.width - tab.save_button.get_position().x - tab.save_button.get_size().width).abs()
            <= tab.page.from_dip(12),
        "WorkBuddy save action must stay at the right edge"
    );
    let viewport = tab.editor_scroll.get_client_size();
    let virtual_size = tab.editor_scroll.get_virtual_size();
    assert!(
        virtual_size.height <= viewport.height + tab.page.from_dip(2),
        "complete WorkBuddy draft must fit: {virtual_size:?} {viewport:?}"
    );
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
    fn provider_options_reuse_ai_gateway_connections_and_skip_reserved_channels() {
        let mut workbuddy = provider("workbuddy", ProviderType::OpenAiResponses);
        workbuddy.models = vec!["private-model".to_string()];
        let config = AppConfig {
            ai_gateway: AiGatewayConfig {
                providers: vec![
                    provider("openai", ProviderType::OpenAiResponses),
                    provider("claude", ProviderType::AnthropicMessages),
                    provider("compatible", ProviderType::ChatCompletions),
                    provider("account", ProviderType::ChatGptResponses),
                    workbuddy,
                    provider("workbuddy:entry-a", ProviderType::OpenAiResponses),
                    provider("WORKBUDDY:entry-b", ProviderType::OpenAiResponses),
                    provider("gmclaw", ProviderType::OpenAiResponses),
                    provider("gmclaw:entry-a", ProviderType::OpenAiResponses),
                ],
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };

        let options = provider_options_from_config(&config);

        assert_eq!(options.len(), 4);
        assert_eq!(options[0].upstream_protocol, "openai-responses");
        assert_eq!(options[1].upstream_protocol, "anthropic-messages");
        assert_eq!(options[2].upstream_protocol, "openai-chat");
        assert_eq!(options[3].upstream_protocol, "openai-responses");
        assert_eq!(
            options[3].upstream_url,
            crate::ai_gateway::chatgpt_auth::BASE_URL
        );
        assert_eq!(options[0].upstream_url, "https://provider.example");
        assert!(options[0].has_api_key);
        assert!(!format!("{:?}", options[0]).contains("secret"));
        assert_eq!(options[0].models, vec!["model-a", "model-b"]);
    }

    #[test]
    fn provider_options_skip_incomplete_connections() {
        let mut without_models = provider("empty-models", ProviderType::OpenAiResponses);
        without_models.models.clear();
        let mut without_url = provider("empty-url", ProviderType::OpenAiResponses);
        without_url.base_url.clear();
        let mut disabled = provider("disabled", ProviderType::OpenAiResponses);
        disabled.enabled = false;
        let config = AppConfig {
            ai_gateway: AiGatewayConfig {
                providers: vec![without_models, without_url, disabled],
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };

        assert!(provider_options_from_config(&config).is_empty());
    }

    #[test]
    fn selecting_one_model_keeps_its_native_settings_and_missing_entries_use_add_view() {
        let mut status: WorkBuddyConfigStatus = serde_json::from_value(serde_json::json!({
            "path":"fixture/models.json", "exists":true, "backupPath":"fixture/models.json.bak",
            "backupExists":true, "revision":"fixture-revision", "model":{},
            "selectedEntryId":"entry-a", "error":null,
            "entries":[
                {"entryId":"entry-a", "model":{"providerModel":"same-model", "upstreamProvider":"source-a", "supportsImages":false, "extraOption":"keep-a"}, "configured":true, "localUrl":"http://127.0.0.1:3847/ai-gateway/workbuddy/entry-a/v1", "sourceProvider":"source-a"},
                {"entryId":"entry-b", "model":{"providerModel":"same-model", "upstreamProvider":"source-b", "useCustomProtocol":true, "extraOption":"keep-b"}, "configured":true, "localUrl":"http://127.0.0.1:3847/ai-gateway/workbuddy/entry-b/v1", "sourceProvider":"source-b"}
            ]
        })).unwrap();
        select_status_entry(&mut status, Some("entry-a"));
        assert_eq!(status.model.upstream_provider, "source-a");
        assert!(!status.model.supports_images);
        assert_eq!(status.model.extra["extraOption"], "keep-a");
        select_status_entry(&mut status, Some("entry-b"));
        assert_eq!(status.model.upstream_provider, "source-b");
        assert!(status.model.use_custom_protocol);
        assert_eq!(status.model.extra["extraOption"], "keep-b");
        select_status_entry(&mut status, Some("deleted-externally"));
        assert!(status.selected_entry_id.is_none());
        assert!(status.model.url.is_empty() && status.model.provider_model.is_empty());
        assert_eq!(status.revision, "fixture-revision");
        assert_eq!(status.entries.len(), 2);
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
        assert_eq!(gpt.default_effort, "max");
        assert!(gpt.supported_efforts.iter().any(|v| v == "max"));
        let native = reasoning_for_selection(Some(&option), "anthropic-messages", "coding", "max");
        assert_eq!(native.default_effort, "max");
        source.compatibility = Some("glm_anthropic".into());
        let option = provider_option(&source).unwrap();
        let glm = reasoning_for_selection(Some(&option), "anthropic-messages", "model-a", "max");
        assert_eq!(glm.supported_efforts, ["high", "max"]);
    }
}
