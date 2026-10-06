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
    ai_gateway::config::{
        ProviderConfig, ProviderType, provider_display_base_url, workbuddy_provider_name,
    },
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
    widgets::{card_section, text_field_row},
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
}

type WorkBuddyProviderOptions = Rc<RefCell<Vec<WorkBuddyProviderOption>>>;

#[derive(Clone)]
pub(super) struct WorkBuddyTab {
    pub(super) page: ScrolledWindow,
    entry: Choice,
    dedicated_channel: TextCtrl,
    path_hint: StaticText,
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
    delete_button: Button,
    restore_button: Button,
    reload_button: Button,
    start_button: Button,
    status: StaticText,
    in_flight: Arc<AtomicBool>,
    provider_options: WorkBuddyProviderOptions,
    selection: Rc<RefCell<WorkBuddySelection>>,
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
    Refresh(Result<(Vec<WorkBuddyProviderOption>, WorkBuddyConfigStatus), String>),
}

const PROVIDER_REFRESH_ATTEMPTS: usize = 30;
const PROVIDER_REFRESH_RETRY_DELAY: Duration = Duration::from_millis(250);

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

pub(super) fn create(parent: &Notebook, text: GuiText) -> WorkBuddyTab {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_card_alt);

    let existing = WorkBuddyModelConfig::default();
    let root = BoxSizer::builder(Orientation::Vertical).build();

    let (connection_box, connection_section) = card_section(&page, text.workbuddy_connection());
    let hint = StaticText::builder(&connection_box)
        .with_label(tr(text,
            "每个模型独立选择来源渠道、协议与思考设置，保存后在 WorkBuddy 中选择使用。同一模型可通过不同渠道重复添加。这里的选择只决定编辑对象，不改变 WorkBuddy 默认模型。\n飞书、微信、企业微信中的 /wb 目前仅保留入口，尚不支持 WorkBuddy 桌面任务接入，不会改变当前执行端。",
            "Each model has its own source, protocol, and reasoning settings. Select it in WorkBuddy after saving. You can add the same model through multiple sources. This selector chooses what to edit; it does not change WorkBuddy's default model.\nIn Feishu, WeChat, and WeCom, /wb is reserved. WorkBuddy desktop task integration is unavailable and the selected executor stays unchanged."))
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
    let entry_label = StaticText::builder(&connection_box)
        .with_label(tr(text, "管理模型", "Managed model"))
        .build();
    entry_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&entry_label, 0, SizerFlag::AlignCenterVertical, 0);
    let entry = Choice::builder(&connection_box)
        .with_choices(vec![tr(text, "新增模型", "Add a model").to_string()])
        .with_size(Size::new(420, -1))
        .build();
    entry.set_selection(0);
    grid.add(&entry, 1, SizerFlag::Expand, 0);
    let dedicated_channel = text_field_row(
        &connection_box,
        &grid,
        tr(text, "WorkBuddy 专属渠道", "WorkBuddy dedicated channel"),
        tr(text, "保存后自动生成", "Generated when saved"),
    );
    dedicated_channel.set_editable(false);
    let local_url = text_field_row(
        &connection_box,
        &grid,
        text.workbuddy_local_url(),
        tr(text, "保存后自动生成", "Generated when saved"),
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
        "",
    );
    upstream_api_key.set_editable(false);
    upstream_api_key.set_tooltip(tr(text,
        "沿用来源渠道的凭据；此页不显示或保存真实 Key，账号渠道沿用登录凭据。",
        "Uses the source channel's credentials. This page does not display or submit API keys; account channels keep their signed-in credentials."));
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
    root.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        10,
    );
    let reload_button = Button::builder(&page)
        .with_label(text.workbuddy_reload())
        .build();
    let start_button = Button::builder(&page)
        .with_label(tr(text, "启动 WorkBuddy", "Start WorkBuddy"))
        .build();
    let restore_button = Button::builder(&page)
        .with_label(tr(text, "撤销上次操作", "Undo last operation"))
        .build();
    let save_button = Button::builder(&page)
        .with_label(text.workbuddy_save())
        .build();
    let delete_button = Button::builder(&page)
        .with_label(tr(text, "删除所选模型", "Delete selected model"))
        .build();
    actions.add(&start_button, 0, SizerFlag::Right, 8);
    actions.add(&reload_button, 0, SizerFlag::Right, 8);
    actions.add(&restore_button, 0, SizerFlag::Right, 8);
    actions.add(&save_button, 0, SizerFlag::Right, 8);
    actions.add(&delete_button, 0, SizerFlag::Right, 0);
    root.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top | SizerFlag::Bottom,
        10,
    );

    let path_hint = StaticText::builder(&page)
        .with_label(tr(
            text,
            "正在读取 WorkBuddy 配置…",
            "Loading WorkBuddy configuration…",
        ))
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
        entry,
        dedicated_channel,
        path_hint,
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
        delete_button,
        restore_button,
        reload_button,
        start_button,
        status,
        in_flight: Arc::new(AtomicBool::new(false)),
        provider_options: Rc::new(RefCell::new(Vec::new())),
        selection: Rc::new(RefCell::new(WorkBuddySelection::default())),
    };
    apply_model(&tab, &existing);
    tab.local_url
        .set_value(tr(text, "保存后自动生成", "Generated when saved"));
    update_controls(&tab);
    tab
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
    let entry_tab = tab.clone();
    tab.entry.on_selection_changed(move |_| {
        if entry_tab.in_flight.load(Ordering::SeqCst) {
            return;
        }
        let Some(mut status) = entry_tab.selection.borrow().status.clone() else {
            return;
        };
        let selected = entry_tab
            .entry
            .get_selection()
            .and_then(|index| index.checked_sub(1))
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
            provider_tab.upstream_url.set_value("");
            provider_tab.upstream_api_key.set_value("");
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
        let api = api_for_save.clone();
        let gui_tx = gui_tx_for_save.clone();
        thread::spawn(move || {
            let result = api.save_workbuddy_config(&request);
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
        refresh_configuration(&tab_for_reload, &api_for_reload, &gui_tx_for_reload, false);
    });
    refresh_configuration(tab, api, gui_tx, true);
}

pub(super) fn apply_result(
    tab: &WorkBuddyTab,
    frame: &Frame,
    text: GuiText,
    _api: &ApiClient,
    _gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    result: WorkBuddyActionResult,
) {
    // Only release the busy state after applying the reply on the GUI thread.
    tab.in_flight.store(false, Ordering::SeqCst);
    match result {
        WorkBuddyActionResult::Start(Ok(detail)) => tab.status.set_label(&detail),
        WorkBuddyActionResult::Start(Err(error)) => {
            tab.status
                .set_label(tr(text, "启动未完成。", "Startup did not complete."));
            show_error(frame, &error);
        }
        WorkBuddyActionResult::Save(Ok(status)) => {
            apply_status(tab, status);
            tab.status.set_label(text.workbuddy_saved());
            show_info(frame, text.workbuddy_saved());
        }
        WorkBuddyActionResult::Restore(Ok(status)) => {
            apply_status(tab, status);
            tab.status
                .set_label(tr(text, "上次操作已撤销。", "Last operation undone."));
        }
        WorkBuddyActionResult::Delete(Ok(status)) => {
            apply_status(tab, status);
            tab.status.set_label(tr(
                text,
                "所选模型已删除，可撤销上次操作。",
                "Selected model deleted. You can undo this operation.",
            ));
        }
        WorkBuddyActionResult::Refresh(Ok((options, status))) => {
            *tab.provider_options.borrow_mut() = options;
            apply_status(tab, status);
        }
        WorkBuddyActionResult::Refresh(Err(error)) => {
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
            tab.status.set_label(tr(text,
                "操作未完成，当前输入已保留；请刷新以读取最新配置后重试。",
                "The operation did not complete. Your inputs are retained; refresh to load current configuration before retrying."));
            show_error(frame, &error);
        }
    }
    update_controls(tab);
    tab.status.wrap(920);
    tab.page.layout();
    tab.page.fit_inside();
}

fn refresh_configuration(
    tab: &WorkBuddyTab,
    api: &ApiClient,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    startup: bool,
) {
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
                Ok((provider_options_from_config(&config), status))
            });
            if result.is_ok() || attempt + 1 == attempts {
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
        .filter(|provider| provider.enabled && !provider.is_client_reserved())
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
    Some(WorkBuddyProviderOption {
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
    tab.provider
        .append(tr(tab.text, "请选择来源渠道", "Select a source channel"));
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
        tab.upstream_api_key.set_value("");
    }
    // A manually selected protocol and effort belong to this entry, not its
    // source channel's current defaults or the previous entry in the form.
    apply_saved_model_settings(tab, model);
    if !model.upstream_url.is_empty() {
        // Existing entries use a saved source snapshot until explicitly saved
        // again; show the endpoint returned for this entry by the daemon.
        tab.upstream_url.set_value(&model.upstream_url);
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
    tab.upstream_api_key.set_value(if option.has_api_key {
        tr(
            tab.text,
            "已配置（沿用来源凭据）",
            "Configured (uses source credentials)",
        )
    } else {
        tr(tab.text, "来源未填写 Key", "No key configured on source")
    });
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
    apply_cache_settings(
        tab,
        protocol,
        provider.map(|p| p.name.as_str()).unwrap_or(""),
    );
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
    model.url = tab.local_url.get_value();
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
                "请先选择已启用的来源渠道。",
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
            "请选择来源渠道中的可用模型。",
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

fn current_revision(tab: &WorkBuddyTab) -> Result<String, String> {
    let selection = tab.selection.borrow();
    selection
        .status
        .as_ref()
        .filter(|status| selection.fresh && !status.revision.is_empty())
        .map(|status| status.revision.clone())
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
    tab.start_button.enable(!busy);
    tab.entry.enable(editable);
    tab.provider.enable(editable);
    tab.model.enable(editable && provider.is_some());
    tab.protocol.enable(editable && provider.is_some());
    tab.default_effort.enable(editable && provider.is_some());
    tab.save_button.enable(editable && model_valid);
    tab.delete_button.enable(
        editable
            && selection
                .status
                .as_ref()
                .is_some_and(|status| status.selected_entry_id.is_some()),
    );
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
}

fn apply_status(tab: &WorkBuddyTab, mut status: WorkBuddyConfigStatus) {
    let selected_id = status.selected_entry_id.clone();
    select_status_entry(&mut status, selected_id.as_deref());
    tab.entry.clear();
    tab.entry.append(tr(tab.text, "新增模型", "Add a model"));
    for entry in &status.entries {
        let short_id: String = entry.entry_id.chars().take(8).collect();
        tab.entry.append(&format!(
            "{} · {} · {}",
            entry.model.provider_model,
            entry
                .source_provider
                .as_deref()
                .unwrap_or(tr(tab.text, "来源待选择", "Select source")),
            short_id
        ));
    }
    let selected = status.selected_entry_id.as_ref().and_then(|id| {
        status
            .entries
            .iter()
            .position(|entry| &entry.entry_id == id)
    });
    tab.entry
        .set_selection(selected.map(|index| index as u32 + 1).unwrap_or(0));
    let mut model = status.model.clone();
    if let Some(entry) = selected.and_then(|index| status.entries.get(index)) {
        model.upstream_provider = entry.source_provider.clone().unwrap_or_default();
        model.url = entry.local_url.clone();
    }
    apply_model(tab, &model);
    apply_provider_options(tab, &model);
    tab.dedicated_channel.set_value(
        &status
            .selected_entry_id
            .as_deref()
            .and_then(workbuddy_provider_name)
            .unwrap_or_else(|| tr(tab.text, "保存后自动生成", "Generated when saved").to_string()),
    );
    if status.selected_entry_id.is_none() {
        tab.local_url
            .set_value(tr(tab.text, "保存后自动生成", "Generated when saved"));
    }
    tab.path_hint.set_label(&format!(
        "{}: {}",
        tab.text.workbuddy_config_path(),
        status.path
    ));
    let message = if let Some(error) = &status.error {
        error.as_str()
    } else if status.selected_entry_id.is_none() {
        tr(
            tab.text,
            "选择来源渠道和模型后保存，即可新增独立的 WorkBuddy 模型。",
            "Choose a source channel and model, then save to add a separate WorkBuddy model.",
        )
    } else if selected
        .and_then(|index| status.entries.get(index))
        .is_some_and(|entry| entry.configured)
    {
        tr(
            tab.text,
            "模型已接入 WorkBuddy；本页可编辑或删除当前条目。",
            "This model is connected to WorkBuddy. You can edit or delete this entry here.",
        )
    } else {
        tr(
            tab.text,
            "当前条目接入信息不完整，请核对来源渠道和模型后保存。",
            "This entry has incomplete connection settings. Review its source and model, then save.",
        )
    };
    tab.status.set_label(message);
    tab.status.wrap(920);
    *tab.selection.borrow_mut() = WorkBuddySelection {
        fresh: !status.revision.is_empty(),
        status: Some(status),
    };
    update_controls(tab);
    tab.page.layout();
    tab.page.fit_inside();
}

fn apply_model(tab: &WorkBuddyTab, model: &WorkBuddyModelConfig) {
    tab.local_url.set_value(&model.url);
    tab.local_api_key
        .set_value(workbuddy_config::DEFAULT_WORKBUDDY_API_KEY);
    tab.upstream_url.set_value(&model.upstream_url);
    tab.upstream_api_key.set_value("");
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
    if workbuddy_config::uses_prompt_cache_key(&model.upstream_protocol)
        && let Some(cache_key) = model
            .extra
            .get("cacheKey")
            .or_else(|| model.extra.get("cache_key"))
            .and_then(serde_json::Value::as_str)
            .filter(|key| !key.trim().is_empty())
    {
        tab.cache_key.set_value(cache_key);
    }
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
        assert_eq!(options[0].upstream_url, "https://provider.example/v1");
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
