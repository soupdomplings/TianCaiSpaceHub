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
    ai_gateway::{
        config::ProviderType,
        gmclaw::{GmClawParameters, TemperatureMode},
    },
    config::AppConfig,
    gmclaw_config::{GmClawConfigStatus, GmClawEntryRequest, GmClawSaveRequest},
};

use super::{
    api::ApiClient,
    show_error, show_info,
    text::{GuiLocale, GuiText},
    theme,
    widgets::{card_section, text_field_row},
};

#[derive(Clone, Debug)]
pub(super) struct GmClawProviderOption {
    name: String,
    models: Vec<String>,
    provider_type: ProviderType,
    compatibility: Option<String>,
    model_aliases: BTreeMap<String, String>,
}

#[derive(Default)]
struct SelectionState {
    providers: Vec<GmClawProviderOption>,
    status: Option<GmClawConfigStatus>,
    preferred_provider: Option<String>,
    preferred_model: Option<String>,
    reasoning_efforts: Vec<&'static str>,
    fresh: bool,
}

#[derive(Clone)]
pub(super) struct GmClawTab {
    pub(super) page: ScrolledWindow,
    path: TextCtrl,
    current_model: TextCtrl,
    default_model: TextCtrl,
    local_url: TextCtrl,
    entry: Choice,
    make_active: CheckBox,
    activate_button: Button,
    delete_button: Button,
    provider: Choice,
    model: ComboBox,
    source_protocol: TextCtrl,
    compatibility: TextCtrl,
    upstream_model: TextCtrl,
    max_tokens: TextCtrl,
    reasoning_effort: Choice,
    temperature_mode: Choice,
    temperature: TextCtrl,
    status: StaticText,
    display_status: StaticText,
    selection_hint: StaticText,
    parameter_hint: StaticText,
    reload_button: Button,
    save_button: Button,
    restore_button: Button,
    start_button: Button,
    state: Rc<RefCell<SelectionState>>,
    in_flight: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(super) enum GmClawActionResult {
    Start(Result<String, String>),
    Refresh(Result<(Vec<GmClawProviderOption>, GmClawConfigStatus), String>),
    Save(Result<GmClawConfigStatus, String>),
    Restore(Result<GmClawConfigStatus, String>),
    Activate(Result<GmClawConfigStatus, String>),
    Delete(Result<GmClawConfigStatus, String>),
}

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

pub(super) fn create(parent: &Notebook, text: GuiText) -> GmClawTab {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_card_alt);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let (section, section_sizer) =
        card_section(&page, tr(text, "天工 Claw 接入", "GMClaw connection"));
    let hint = StaticText::builder(&section)
        .with_label(tr(
            text,
            "选择已在 Hub 启用的渠道和模型。保存后，天工 Claw 通过本机 Hub 使用所选模型，连接地址自动生成。",
            "Choose an enabled Hub channel and model. After saving, GMClaw uses the selected model through the local Hub. The connection URL is generated automatically.",
        ))
        .build();
    hint.set_foreground_color(theme::theme().ink_muted);
    hint.wrap(920);
    section_sizer.add(&hint, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(10)
        .with_hgap(14)
        .build();
    grid.add_growable_col(1, 1);
    let path = text_field_row(
        &section,
        &grid,
        tr(text, "配置路径", "Configuration path"),
        "",
    );
    path.set_editable(false);
    let entry_label = StaticText::builder(&section)
        .with_label(tr(text, "配置条目", "Model entry"))
        .build();
    grid.add(&entry_label, 0, SizerFlag::AlignCenterVertical, 0);
    let entry = Choice::builder(&section)
        .with_size(Size::new(420, -1))
        .build();
    entry.append(tr(text, "新增模型", "Add a model"));
    entry.set_selection(0);
    grid.add(&entry, 1, SizerFlag::Expand, 0);
    let current_model = text_field_row(
        &section,
        &grid,
        tr(text, "Hub 管理模型", "Hub managed model"),
        tr(text, "正在读取…", "Loading…"),
    );
    current_model.set_editable(false);
    let default_model = text_field_row(
        &section,
        &grid,
        tr(text, "当前默认模型 ID", "Current default model ID"),
        tr(text, "正在读取…", "Loading…"),
    );
    default_model.set_editable(false);
    let local_url = text_field_row(
        &section,
        &grid,
        tr(text, "连接地址", "Connection URL"),
        tr(text, "保存时自动生成", "Generated when saved"),
    );
    local_url.set_editable(false);
    let provider_label = StaticText::builder(&section)
        .with_label(tr(text, "Hub 来源渠道", "Hub source channel"))
        .build();
    provider_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&provider_label, 0, SizerFlag::AlignCenterVertical, 0);
    let provider = Choice::builder(&section)
        .with_size(Size::new(420, -1))
        .build();
    grid.add(&provider, 1, SizerFlag::Expand, 0);
    let model_label = StaticText::builder(&section)
        .with_label(tr(text, "模型", "Model"))
        .build();
    model_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&model_label, 0, SizerFlag::AlignCenterVertical, 0);
    let model = ComboBox::builder(&section)
        .with_style(ComboBoxStyle::ReadOnly)
        .with_size(Size::new(420, -1))
        .build();
    model.set_background_color(theme::theme().bg_muted);
    model.set_foreground_color(theme::theme().ink_primary);
    model.set_min_size(Size::new(420, 30));
    grid.add(&model, 1, SizerFlag::Expand, 0);
    let source_protocol =
        text_field_row(&section, &grid, tr(text, "来源协议", "Source protocol"), "");
    source_protocol.set_editable(false);
    let compatibility = text_field_row(
        &section,
        &grid,
        tr(text, "兼容 profile", "Compatibility profile"),
        "",
    );
    compatibility.set_editable(false);
    let upstream_model = text_field_row(
        &section,
        &grid,
        tr(
            text,
            "实际模型（解析别名后）",
            "Actual model (after alias resolution)",
        ),
        "",
    );
    upstream_model.set_editable(false);
    let max_tokens = text_field_row(
        &section,
        &grid,
        tr(text, "最大输出 Token", "Maximum output tokens"),
        "8192",
    );
    let reasoning_label = StaticText::builder(&section)
        .with_label(tr(text, "思考强度", "Reasoning effort"))
        .build();
    reasoning_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&reasoning_label, 0, SizerFlag::AlignCenterVertical, 0);
    let reasoning_effort = Choice::builder(&section)
        .with_size(Size::new(420, -1))
        .build();
    reasoning_effort.append(tr(text, "自动（跟随上游）", "Automatic (upstream default)"));
    reasoning_effort.set_selection(0);
    grid.add(&reasoning_effort, 1, SizerFlag::Expand, 0);
    let temperature_mode_label = StaticText::builder(&section)
        .with_label(tr(text, "温度策略", "Temperature policy"))
        .build();
    temperature_mode_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(
        &temperature_mode_label,
        0,
        SizerFlag::AlignCenterVertical,
        0,
    );
    let temperature_mode = Choice::builder(&section)
        .with_size(Size::new(420, -1))
        .build();
    for label in [
        tr(text, "自动适配", "Automatic adaptation"),
        tr(text, "不发送温度", "Omit temperature"),
        tr(text, "发送所填温度", "Send entered temperature"),
    ] {
        temperature_mode.append(label);
    }
    temperature_mode.set_selection(0);
    grid.add(&temperature_mode, 1, SizerFlag::Expand, 0);
    let temperature = text_field_row(
        &section,
        &grid,
        tr(text, "温度（0–2）", "Temperature (0–2)"),
        "0.7",
    );
    section_sizer.add_sizer(&grid, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let make_active = CheckBox::builder(&section)
        .with_label(tr(
            text,
            "保存后设为天工默认模型",
            "Set as GMClaw default after saving",
        ))
        .build();
    make_active.set_value(true);
    section_sizer.add(&make_active, 0, SizerFlag::All, 10);
    let selection_hint = StaticText::builder(&section).with_label("").build();
    selection_hint.set_foreground_color(theme::theme().ink_muted);
    section_sizer.add(&selection_hint, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let parameter_hint = StaticText::builder(&section).with_label("").build();
    parameter_hint.set_foreground_color(theme::theme().ink_muted);
    section_sizer.add(&parameter_hint, 0, SizerFlag::Expand | SizerFlag::All, 10);
    root.add(&section, 0, SizerFlag::Expand | SizerFlag::All, 10);

    let status = StaticText::builder(&page)
        .with_label(tr(
            text,
            "正在读取天工 Claw 配置…",
            "Loading GMClaw configuration…",
        ))
        .build();
    status.set_foreground_color(theme::theme().ink_muted);
    root.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        20,
    );
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let start_button = Button::builder(&page)
        .with_label(tr(text, "启动天工 Claw", "Start GMClaw"))
        .build();
    let reload_button = Button::builder(&page)
        .with_label(tr(text, "刷新", "Refresh"))
        .build();
    let restore_button = Button::builder(&page)
        .with_label(tr(text, "还原上次备份", "Restore latest backup"))
        .build();
    let save_button = Button::builder(&page)
        .with_label(tr(text, "保存接入配置", "Save connection"))
        .build();
    let activate_button = Button::builder(&page)
        .with_label(tr(text, "设为默认", "Set as default"))
        .build();
    let delete_button = Button::builder(&page)
        .with_label(tr(text, "删除所选模型", "Delete selected model"))
        .build();
    actions.add(&start_button, 0, SizerFlag::Right, 8);
    actions.add(&reload_button, 0, SizerFlag::Right, 8);
    actions.add(&restore_button, 0, SizerFlag::Right, 8);
    actions.add(&save_button, 0, SizerFlag::Right, 8);
    actions.add(&activate_button, 0, SizerFlag::Right, 8);
    actions.add(&delete_button, 0, SizerFlag::Right, 0);
    root.add_sizer(&actions, 0, SizerFlag::All, 20);
    let display_status = StaticText::builder(&page)
        .with_label(tr(
            text,
            "桌面对话同步：等待本地服务状态",
            "Desktop message sync: waiting for local service status",
        ))
        .build();
    display_status.set_foreground_color(theme::theme().ink_secondary);
    display_status.wrap(920);
    root.add(
        &display_status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        20,
    );
    let im_hint = StaticText::builder(&page)
        .with_label(tr(
            text,
            "点击“启动天工 Claw”即可打开桌面并自动准备本地连接。要在天工窗口同步外部对话，首次请正常退出已经手动打开的天工，再通过此按钮打开一次。在已连接的飞书、微信或企业微信中发送 /tg 选择天工执行端，沿用会话卡片或菜单选择项目目录和模型。启用后自动重连，发送 /gpt 返回 ChatGPT（Codex）。",
            "Click Start GMClaw to open the desktop and prepare its local connection. To sync external messages into the desktop window, first close an already manually opened GMClaw and launch it once with this button. Send /tg in connected Feishu, WeChat, or WeCom to select GMClaw, then use the existing session card or menu to choose a project directory and model. Enabled connections recover automatically. Send /gpt to return to ChatGPT (Codex).",
        ))
        .build();
    im_hint.set_foreground_color(theme::theme().ink_muted);
    im_hint.wrap(920);
    root.add(
        &im_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        20,
    );
    page.set_sizer(root, true);
    page.set_scroll_rate(0, 10);
    page.layout();
    page.fit_inside();
    let tab = GmClawTab {
        page,
        path,
        current_model,
        default_model,
        local_url,
        entry,
        make_active,
        activate_button,
        delete_button,
        provider,
        model,
        source_protocol,
        compatibility,
        upstream_model,
        max_tokens,
        reasoning_effort,
        temperature_mode,
        temperature,
        status,
        display_status,
        selection_hint,
        parameter_hint,
        reload_button,
        save_button,
        restore_button,
        start_button,
        state: Rc::new(RefCell::new(SelectionState {
            reasoning_efforts: vec!["auto"],
            ..SelectionState::default()
        })),
        in_flight: Arc::new(AtomicBool::new(false)),
    };
    update_parameter_hint(&tab, text);
    update_controls(&tab);
    tab
}

pub(super) fn refresh_display(
    tab: &GmClawTab,
    text: GuiText,
    snapshot: Option<&super::api::DashboardSnapshot>,
) {
    let detail = snapshot
        .filter(|snapshot| snapshot.service_online)
        .and_then(|snapshot| snapshot.client_overview.as_ref())
        .map(|overview| &overview.gmclaw.bridge)
        .map(|bridge| {
            if !bridge.enabled {
                tr(
                    text,
                    "尚未启用；启动天工或在聊天中发送 /tg 后准备同步",
                    "Not enabled; start GMClaw or send /tg to prepare sync",
                )
            } else {
                bridge
                    .detail
                    .split_once("；桌面对话同步：")
                    .map(|(_, detail)| detail)
                    .unwrap_or_else(|| {
                        tr(text, "尚未取得窗口更新结果", "No window update result yet")
                    })
            }
        })
        .unwrap_or_else(|| tr(text, "等待本地服务状态", "Waiting for local service status"));
    let label = format!(
        "{}{}",
        tr(text, "桌面对话同步：", "Desktop message sync: "),
        detail
    );
    if tab.display_status.get_label() != label {
        tab.display_status.set_label(&label);
        tab.display_status.wrap(920);
        tab.page.layout();
        tab.page.fit_inside();
    }
}

pub(super) fn bind_actions(
    tab: &GmClawTab,
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
            tr(
                text,
                "正在启动并连接天工 Claw…",
                "Starting and connecting GMClaw…",
            ),
        ) {
            return;
        }
        let api = start_api.clone();
        let tx = start_tx.clone();
        thread::spawn(move || {
            let result = api.start_gmclaw_desktop();
            let _ = tx.send(super::GuiMessage::GmClaw(GmClawActionResult::Start(result)));
            wxdragon::wake_up_idle();
        });
    });
    let entry_tab = tab.clone();
    tab.entry.on_selection_changed(move |_| {
        let mut status = match entry_tab.state.borrow().status.clone() {
            Some(status) => status,
            None => return,
        };
        let entry = entry_tab
            .entry
            .get_selection()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| status.entries.get(index as usize))
            .cloned();
        status.selected_entry_id = entry.as_ref().map(|entry| entry.entry_id.clone());
        status.model = entry.as_ref().map(|entry| entry.model.clone());
        status.source_provider = entry
            .as_ref()
            .and_then(|entry| entry.source_provider.clone());
        status.parameters = entry
            .as_ref()
            .map(|entry| entry.parameters.clone())
            .unwrap_or_default();
        status.active = entry.as_ref().is_some_and(|entry| entry.active);
        status.configured = entry.as_ref().is_some_and(|entry| entry.configured);
        apply_status(&entry_tab, text, status, true);
        update_controls(&entry_tab);
    });
    for (button, deleting) in [(tab.activate_button, false), (tab.delete_button, true)] {
        let action_tab = tab.clone();
        let action_api = api.clone();
        let action_tx = gui_tx.clone();
        let action_frame = *frame;
        button.on_click(move |_| {
            let Some(entry_id) = action_tab
                .state
                .borrow()
                .status
                .as_ref()
                .and_then(|status| status.selected_entry_id.clone())
            else {
                return;
            };
            let revision = match current_revision(&action_tab, text) {
                Ok(revision) => revision,
                Err(error) => {
                    show_error(&action_frame, &error);
                    return;
                }
            };
            if !begin_action(&action_tab, tr(text, "正在更新模型…", "Updating model…")) {
                return;
            }
            let api = action_api.clone();
            let tx = action_tx.clone();
            thread::spawn(move || {
                let request = GmClawEntryRequest { entry_id, revision };
                let result = if deleting {
                    GmClawActionResult::Delete(api.delete_gmclaw_config(&request))
                } else {
                    GmClawActionResult::Activate(api.activate_gmclaw_config(&request))
                };
                let _ = tx.send(super::GuiMessage::GmClaw(result));
                wxdragon::wake_up_idle();
            });
        });
    }
    let provider_tab = tab.clone();
    tab.provider.on_selection_changed(move |_| {
        let selected = selected_provider(&provider_tab);
        provider_tab.state.borrow_mut().preferred_provider = selected.map(|p| p.name);
        apply_model_choices(&provider_tab, text, true);
        update_controls(&provider_tab);
    });
    let model_tab = tab.clone();
    tab.model.on_selection_changed(move |_| {
        model_tab.state.borrow_mut().preferred_model = Some(model_tab.model.get_value());
        update_selection_hint(&model_tab, text);
        update_controls(&model_tab);
    });
    let temperature_tab = tab.clone();
    tab.temperature_mode.on_selection_changed(move |_| {
        update_parameter_hint(&temperature_tab, text);
    });

    let save_tab = tab.clone();
    let save_api = api.clone();
    let save_tx = gui_tx.clone();
    let save_frame = *frame;
    tab.save_button.on_click(move |_| {
        if save_tab.in_flight.load(Ordering::SeqCst) {
            return;
        }
        let request = match build_request(&save_tab, text) {
            Ok(request) => request,
            Err(error) => {
                show_error(&save_frame, &error);
                return;
            }
        };
        if !begin_action(&save_tab, tr(text, "正在保存…", "Saving…")) {
            return;
        }
        let api = save_api.clone();
        let tx = save_tx.clone();
        thread::spawn(move || {
            let result = api.save_gmclaw_config(&request);
            let _ = tx.send(super::GuiMessage::GmClaw(GmClawActionResult::Save(result)));
            wxdragon::wake_up_idle();
        });
    });

    let restore_tab = tab.clone();
    let restore_api = api.clone();
    let restore_tx = gui_tx.clone();
    let restore_frame = *frame;
    tab.restore_button.on_click(move |_| {
        if restore_tab.in_flight.load(Ordering::SeqCst) {
            return;
        }
        let revision = match current_revision(&restore_tab, text) {
            Ok(revision) => revision,
            Err(error) => {
                show_error(&restore_frame, &error);
                return;
            }
        };
        if !begin_action(&restore_tab, tr(text, "正在还原…", "Restoring…")) {
            return;
        }
        let api = restore_api.clone();
        let tx = restore_tx.clone();
        thread::spawn(move || {
            let result = api.restore_gmclaw_config(&revision);
            let _ = tx.send(super::GuiMessage::GmClaw(GmClawActionResult::Restore(
                result,
            )));
            wxdragon::wake_up_idle();
        });
    });

    let reload_tab = tab.clone();
    let reload_api = api.clone();
    let reload_tx = gui_tx.clone();
    tab.reload_button.on_click(move |_| {
        refresh(&reload_tab, &reload_api, text, &reload_tx, false);
    });
    refresh(tab, api, text, gui_tx, true);
}

fn refresh(
    tab: &GmClawTab,
    api: &ApiClient,
    text: GuiText,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    startup: bool,
) {
    if !begin_action(
        tab,
        tr(
            text,
            "正在读取配置和渠道…",
            "Loading configuration and channels…",
        ),
    ) {
        return;
    }
    let api = api.clone();
    let tx = gui_tx.clone();
    let entry_id = tab
        .state
        .borrow()
        .status
        .as_ref()
        .map(|status| status.selected_entry_id.clone().unwrap_or_default());
    thread::spawn(move || {
        let attempts = if startup { 30 } else { 1 };
        let mut result =
            Err(tr(text, "Hub 服务尚未就绪", "The Hub service is not ready").to_string());
        for attempt in 0..attempts {
            result = api.get_app_config().and_then(|config| {
                api.get_gmclaw_config(None).map(|mut status| {
                    if let Some(selected) = &entry_id {
                        let entry = status
                            .entries
                            .iter()
                            .find(|entry| &entry.entry_id == selected)
                            .cloned();
                        status.selected_entry_id =
                            entry.as_ref().map(|entry| entry.entry_id.clone());
                        status.model = entry.as_ref().map(|entry| entry.model.clone());
                        status.source_provider = entry
                            .as_ref()
                            .and_then(|entry| entry.source_provider.clone());
                        status.parameters = entry
                            .as_ref()
                            .map(|entry| entry.parameters.clone())
                            .unwrap_or_default();
                        status.active = entry.as_ref().is_some_and(|entry| entry.active);
                        status.configured = entry.as_ref().is_some_and(|entry| entry.configured);
                        status.local_url = entry
                            .as_ref()
                            .map(|entry| entry.local_url.clone())
                            .unwrap_or_default();
                    }
                    (provider_options(&config), status)
                })
            });
            if result.is_ok() || attempt + 1 == attempts {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        let _ = tx.send(super::GuiMessage::GmClaw(GmClawActionResult::Refresh(
            result,
        )));
        wxdragon::wake_up_idle();
    });
}

pub(super) fn apply_result(
    tab: &GmClawTab,
    frame: &Frame,
    text: GuiText,
    result: GmClawActionResult,
) {
    // Keep the model action busy until its result reaches the GUI thread.
    tab.in_flight.store(false, Ordering::SeqCst);
    match result {
        GmClawActionResult::Start(Ok(detail)) => tab.status.set_label(&detail),
        GmClawActionResult::Start(Err(error)) => {
            tab.status.set_label(tr(
                text,
                "启动或连接未完成。",
                "Startup or connection did not complete.",
            ));
            show_error(frame, &error);
        }
        GmClawActionResult::Refresh(Ok((providers, status))) => {
            tab.state.borrow_mut().providers = providers;
            apply_status(tab, text, status, true);
        }
        GmClawActionResult::Save(Ok(status)) => {
            apply_status(tab, text, status, true);
            show_info(
                frame,
                tr(
                    text,
                    "模型已保存，可直接在天工 Claw 中选择使用。请保持 Hub 运行。",
                    "Model saved. Select it in GMClaw and keep Hub running.",
                ),
            );
        }
        GmClawActionResult::Restore(Ok(status)) => {
            apply_status(tab, text, status, true);
            show_info(
                frame,
                tr(
                    text,
                    "上次操作已还原，可直接在天工 Claw 中选择模型。请保持 Hub 运行。",
                    "Latest operation restored. Select a model in GMClaw and keep Hub running.",
                ),
            );
        }
        GmClawActionResult::Activate(Ok(status)) | GmClawActionResult::Delete(Ok(status)) => {
            apply_status(tab, text, status, true);
        }
        GmClawActionResult::Refresh(Err(error)) => {
            tab.state.borrow_mut().fresh = false;
            tab.status.set_label(&format!(
                "{}\n{error}",
                tr(
                    text,
                    "读取失败，请确认 Hub 服务已启动后刷新。",
                    "Could not load configuration. Make sure Hub is running, then refresh.",
                )
            ));
        }
        GmClawActionResult::Save(Err(error))
        | GmClawActionResult::Restore(Err(error))
        | GmClawActionResult::Activate(Err(error))
        | GmClawActionResult::Delete(Err(error)) => {
            tab.state.borrow_mut().fresh = false;
            tab.status.set_label(tr(
                text,
                "操作未完成，请刷新以读取最新配置后重试。",
                "The operation did not complete. Refresh the latest configuration before retrying.",
            ));
            show_error(frame, &error);
        }
    }
    update_controls(tab);
    tab.status.wrap(920);
    tab.page.layout();
    tab.page.fit_inside();
}

fn provider_options(config: &AppConfig) -> Vec<GmClawProviderOption> {
    config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.enabled && !provider.is_client_reserved())
        .filter(|provider| provider.provider_type != ProviderType::ChatGptResponses)
        .map(|provider| {
            let mut models = Vec::<String>::new();
            for model in provider.models.iter().chain(provider.model_aliases.keys()) {
                let model = model.trim();
                if !model.is_empty()
                    && !models
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(model))
                {
                    models.push(model.to_string());
                }
            }
            GmClawProviderOption {
                name: provider.name.clone(),
                models,
                provider_type: provider.provider_type.clone(),
                compatibility: provider.compatibility.clone(),
                model_aliases: provider.model_aliases.clone(),
            }
        })
        .collect()
}

fn selected_provider(tab: &GmClawTab) -> Option<GmClawProviderOption> {
    let index = tab.provider.get_selection()?.checked_sub(1)? as usize;
    tab.state.borrow().providers.get(index).cloned()
}

fn apply_status(
    tab: &GmClawTab,
    text: GuiText,
    status: GmClawConfigStatus,
    use_saved_selection: bool,
) {
    tab.entry.clear();
    tab.entry.append(tr(text, "新增模型", "Add a model"));
    for entry in &status.entries {
        tab.entry.append(&format!(
            "{} · {}{} · {}",
            entry.model.model_name,
            entry.source_provider.as_deref().unwrap_or("?"),
            if entry.active {
                tr(text, "（默认）", " (default)")
            } else {
                ""
            },
            &entry.entry_id[..entry.entry_id.len().min(8)]
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
    tab.make_active
        .set_value(status.selected_entry_id.is_none() || status.active);
    let saved_effort = status.parameters.reasoning_effort.clone();
    tab.temperature_mode
        .set_selection(match &status.parameters.temperature_mode {
            TemperatureMode::Auto => 0,
            TemperatureMode::Omit => 1,
            TemperatureMode::Preserve => 2,
        });
    tab.path.set_value(&status.path);
    tab.current_model.set_value(
        status
            .model
            .as_ref()
            .map(|model| model.model_name.as_str())
            .unwrap_or(tr(text, "未配置", "Not configured")),
    );
    tab.default_model.set_value(
        status
            .active_model_id
            .as_deref()
            .filter(|model| !model.is_empty())
            .unwrap_or(tr(text, "未配置", "Not configured")),
    );
    tab.local_url.set_value(
        status
            .model
            .as_ref()
            .map(|model| model.local_url.as_str())
            .filter(|url| !url.is_empty())
            .unwrap_or(tr(text, "保存时自动生成", "Generated when saved")),
    );
    if use_saved_selection {
        let mut state = tab.state.borrow_mut();
        state.preferred_provider = status.source_provider.clone();
        state.preferred_model = status.model.as_ref().map(|model| model.model_name.clone());
        if !status.configured && state.preferred_provider.is_none() {
            state.preferred_provider = state
                .providers
                .first()
                .map(|provider| provider.name.clone());
        }
    }
    tab.max_tokens.set_value(
        &status
            .model
            .as_ref()
            .map(|model| model.max_tokens)
            .unwrap_or(8192)
            .to_string(),
    );
    tab.temperature.set_value(
        &status
            .model
            .as_ref()
            .map(|model| model.temperature)
            .unwrap_or(0.7)
            .to_string(),
    );
    let status_text = if !status.exists {
        tr(
            text,
            "未找到天工 Claw 配置。请先安装并打开天工 Claw 完成初始化，再回到这里刷新。",
            "GMClaw configuration was not found. Install and open GMClaw to initialize it, then refresh here.",
        )
    } else if !status.schema_supported {
        tr(
            text,
            "当前天工 Claw 配置格式暂不支持，无法保存或还原。请确认安装版本与配置路径。",
            "This GMClaw configuration format is not supported. Saving and restoring are unavailable. Check the installed version and configuration path.",
        )
    } else if status.configured && !status.active {
        tr(
            text,
            "Hub 接入配置已保存；天工 Claw 当前默认使用其他模型。",
            "The Hub connection is saved; GMClaw currently defaults to another model.",
        )
    } else if status.configured {
        tr(
            text,
            "天工 Claw 已配置为通过 Hub 接入。",
            "GMClaw is configured to connect through Hub.",
        )
    } else {
        tr(
            text,
            "尚未接入 Hub。请选择渠道和模型后保存。",
            "Not connected through Hub yet. Choose a channel and model, then save.",
        )
    };
    if let Some(error) = status
        .error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
    {
        tab.status.set_label(&format!("{status_text}\n{error}"));
    } else {
        tab.status.set_label(status_text);
    }
    {
        let mut state = tab.state.borrow_mut();
        state.status = Some(status);
        state.fresh = true;
    }
    tab.provider.clear();
    tab.provider
        .append(tr(text, "请选择已启用的渠道", "Select an enabled channel"));
    let selected = {
        let state = tab.state.borrow();
        for provider in &state.providers {
            tab.provider.append(&provider.name);
        }
        state.preferred_provider.as_ref().and_then(|name| {
            state
                .providers
                .iter()
                .position(|provider| provider.name.eq_ignore_ascii_case(name))
        })
    };
    tab.provider
        .set_selection(selected.map(|index| index as u32 + 1).unwrap_or(0));
    let allow_default_model = use_saved_selection
        && tab
            .state
            .borrow()
            .status
            .as_ref()
            .is_some_and(|status| !status.configured);
    apply_model_choices(tab, text, allow_default_model);
    apply_reasoning_choices(tab, text, saved_effort.as_deref().unwrap_or("auto"));
    update_parameter_hint(tab, text);
}

fn apply_model_choices(tab: &GmClawTab, text: GuiText, allow_default: bool) {
    let provider = selected_provider(tab);
    let preferred = tab.state.borrow().preferred_model.clone();
    tab.model.clear();
    if let Some(provider) = provider {
        for model in &provider.models {
            tab.model.append(model);
        }
        let matched = preferred.as_ref().and_then(|model| {
            provider
                .models
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(model))
        });
        let selected =
            matched.or_else(|| (allow_default && !provider.models.is_empty()).then_some(0));
        if let Some(index) = selected {
            tab.model.set_selection(index as u32);
            tab.model.set_value(&provider.models[index]);
            tab.state.borrow_mut().preferred_model = Some(provider.models[index].clone());
        } else if let Some(model) = preferred.filter(|model| !model.is_empty()) {
            // Preserve a saved model removed from the channel without silently replacing it.
            tab.model.append(&model);
            tab.model.set_selection(provider.models.len() as u32);
            tab.model.set_value(&model);
        }
    } else if let Some(model) = preferred.filter(|model| !model.is_empty()) {
        tab.model.append(&model);
        tab.model.set_selection(0);
        tab.model.set_value(&model);
    }
    update_selection_hint(tab, text);
}

fn update_selection_hint(tab: &GmClawTab, text: GuiText) {
    let state = tab.state.borrow();
    let hint = if state.providers.is_empty() {
        tr(text, "请先在“大模型接入”中启用普通 API 渠道并配置模型，再刷新本页。账号登录渠道暂不支持。",
            "Enable an API-key channel and configure its models in AI Gateway, then refresh. Account sign-in channels are not supported yet.").to_string()
    } else if let Some(provider) = selected_provider(tab) {
        if provider.models.is_empty() {
            tr(
                text,
                "该渠道尚未配置模型，请在“大模型接入”中添加模型后刷新。",
                "This channel has no models. Add models in AI Gateway, then refresh.",
            )
            .to_string()
        } else if !provider
            .models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(&tab.model.get_value()))
        {
            tr(text, "当前模型不在所选渠道中，请明确选择一个可用模型后保存。",
                "The current model is unavailable in this channel. Select an available model before saving.").to_string()
        } else {
            tr(
                text,
                "修改后保存生效；模型别名沿用所选渠道的映射。",
                "Save to apply changes. Model aliases use the selected channel's mappings.",
            )
            .to_string()
        }
    } else if let Some(name) = state.preferred_provider.as_ref() {
        format!(
            "{}: {name}",
            tr(
                text,
                "原渠道不存在或未启用，请明确选择来源渠道",
                "The previous channel is missing or disabled; select a source channel"
            )
        )
    } else {
        tr(text, "请选择来源渠道。", "Select a source channel.").to_string()
    };
    drop(state);
    tab.selection_hint.set_label(&hint);
    tab.selection_hint.wrap(920);
    update_model_details(tab, text);
    tab.page.layout();
    tab.page.fit_inside();
}

fn resolved_model<'a>(provider: &'a GmClawProviderOption, model: &str) -> Option<&'a str> {
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
        .or_else(|| {
            provider
                .models
                .iter()
                .find(|candidate| candidate.as_str() == model)
        })
        .or_else(|| {
            provider
                .models
                .iter()
                .find(|candidate| candidate.eq_ignore_ascii_case(model))
        })
        .map(String::as_str)
        .filter(|model| !model.trim().is_empty())
}

fn reasoning_choices(
    provider: Option<&GmClawProviderOption>,
    model: &str,
) -> &'static [&'static str] {
    let Some(provider) = provider else {
        return &["auto"];
    };
    let glm = provider.compatibility.as_deref().is_some_and(|profile| {
        matches!(
            profile.trim().to_ascii_lowercase().as_str(),
            "glm_anthropic" | "zhipu_anthropic"
        )
    });
    let deepseek = provider.provider_type == ProviderType::DeepSeekResponses
        || (provider.provider_type == ProviderType::ChatCompletions
            && resolved_model(provider, model).is_some_and(|model| {
                model
                    .rsplit('/')
                    .next()
                    .unwrap_or(model)
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("deepseek")
            }));
    if glm {
        &["auto", "none", "high", "max"]
    } else if deepseek {
        &["auto", "none", "low", "high", "max"]
    } else if provider.provider_type == ProviderType::AnthropicMessages {
        &["auto", "low", "medium", "high", "xhigh", "max"]
    } else {
        &["auto", "none", "low", "medium", "high", "xhigh", "max"]
    }
}

fn apply_reasoning_choices(tab: &GmClawTab, text: GuiText, preferred: &str) {
    let provider = selected_provider(tab);
    let choices = reasoning_choices(provider.as_ref(), &tab.model.get_value());
    tab.reasoning_effort.clear();
    for effort in choices {
        tab.reasoning_effort.append(if *effort == "auto" {
            tr(text, "自动（跟随上游）", "Automatic (upstream default)")
        } else {
            effort
        });
    }
    let selection = choices
        .iter()
        .position(|effort| effort.eq_ignore_ascii_case(preferred.trim()))
        .unwrap_or(0);
    tab.reasoning_effort.set_selection(selection as u32);
    tab.state.borrow_mut().reasoning_efforts = choices.to_vec();
}

fn selected_reasoning_effort(tab: &GmClawTab) -> Option<String> {
    let selected = tab.reasoning_effort.get_selection()? as usize;
    tab.state
        .borrow()
        .reasoning_efforts
        .get(selected)
        .copied()
        .filter(|effort| *effort != "auto")
        .map(str::to_string)
}

fn update_model_details(tab: &GmClawTab, text: GuiText) {
    if let Some(provider) = selected_provider(tab) {
        let protocol = match &provider.provider_type {
            ProviderType::OpenAiResponses => "OpenAI Responses",
            ProviderType::ChatGptResponses => "ChatGPT Responses",
            ProviderType::DeepSeekResponses => "DeepSeek Responses",
            ProviderType::KimiResponses => "Kimi Responses",
            ProviderType::GrokResponses => "Grok Responses",
            ProviderType::ChatCompletions => "Chat Completions",
            ProviderType::AnthropicMessages => "Anthropic Messages",
        };
        tab.source_protocol.set_value(protocol);
        tab.compatibility.set_value(
            provider
                .compatibility
                .as_deref()
                .map(str::trim)
                .filter(|profile| !profile.is_empty())
                .unwrap_or(tr(text, "默认（未设置）", "Default (not set)")),
        );
        tab.upstream_model.set_value(
            resolved_model(&provider, &tab.model.get_value()).unwrap_or(tr(
                text,
                "请选择可用模型",
                "Select an available model",
            )),
        );
    } else {
        let placeholder = tr(text, "请选择来源渠道", "Select a source channel");
        tab.source_protocol.set_value(placeholder);
        tab.compatibility.set_value(placeholder);
        tab.upstream_model.set_value(placeholder);
    }
    let preferred = selected_reasoning_effort(tab).unwrap_or_else(|| "auto".to_string());
    apply_reasoning_choices(tab, text, &preferred);
    update_parameter_hint(tab, text);
}

fn selected_temperature_mode(tab: &GmClawTab) -> TemperatureMode {
    match tab.temperature_mode.get_selection() {
        Some(1) => TemperatureMode::Omit,
        Some(2) => TemperatureMode::Preserve,
        _ => TemperatureMode::Auto,
    }
}

fn update_parameter_hint(tab: &GmClawTab, text: GuiText) {
    let reasoning = tr(
        text,
        "思考强度“自动”跟随上游默认，不强制 high；其他档位需上游模型支持。切换渠道或模型时保留有效选择，不适用的档位回到自动。",
        "Automatic reasoning follows the upstream default without forcing high. Other levels require upstream model support. Switching channels or models keeps valid choices and resets incompatible levels to Automatic.",
    );
    let temperature = match selected_temperature_mode(tab) {
        TemperatureMode::Auto => tr(
            text,
            "“自动适配”按来源协议、兼容 profile 及实际模型自动保留或省略所填温度；具体参数支持以上游为准。",
            "Automatic adaptation keeps or omits the entered temperature based on the source protocol, compatibility profile, and actual model. Parameter support depends on the upstream service.",
        ),
        TemperatureMode::Omit => tr(
            text,
            "“不发送温度”不向上游发送温度参数；输入值仍保留，供切换策略后使用。",
            "Omit temperature does not send the temperature parameter upstream. The entered value is retained for other policies.",
        ),
        TemperatureMode::Preserve => tr(
            text,
            "“发送所填温度”始终发送输入值，适用于明确接受此参数的上游；不支持的模型可能拒绝请求。",
            "Send entered temperature always sends the input value. Use it for upstream models that accept this parameter; unsupported models may reject the request.",
        ),
    };
    tab.parameter_hint
        .set_label(&format!("{reasoning}\n{temperature}"));
    tab.parameter_hint.wrap(920);
    tab.page.layout();
    tab.page.fit_inside();
}

fn begin_action(tab: &GmClawTab, message: &str) -> bool {
    if tab.in_flight.swap(true, Ordering::SeqCst) {
        return false;
    }
    tab.status.set_label(message);
    update_controls(tab);
    true
}

fn update_controls(tab: &GmClawTab) {
    let busy = tab.in_flight.load(Ordering::SeqCst);
    let state = tab.state.borrow();
    let ready = !busy
        && state.fresh
        && state.status.as_ref().is_some_and(|status| {
            status.exists && status.schema_supported && !status.revision.trim().is_empty()
        });
    let has_model = selected_provider(tab).is_some_and(|provider| {
        provider
            .models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(&tab.model.get_value()))
    });
    tab.reload_button.enable(!busy);
    tab.start_button.enable(!busy);
    tab.entry.enable(ready);
    tab.make_active.enable(ready);
    let selected = state
        .status
        .as_ref()
        .is_some_and(|status| status.selected_entry_id.is_some());
    tab.activate_button.enable(
        ready
            && selected
            && state
                .status
                .as_ref()
                .is_some_and(|status| status.configured && !status.active),
    );
    tab.delete_button.enable(ready && selected);
    tab.save_button.enable(ready && has_model);
    tab.restore_button.enable(
        ready
            && state
                .status
                .as_ref()
                .is_some_and(|status| status.backup_exists),
    );
    tab.provider.enable(ready);
    tab.model.enable(ready && selected_provider(tab).is_some());
    tab.max_tokens.enable(ready);
    tab.reasoning_effort
        .enable(ready && selected_provider(tab).is_some());
    tab.temperature_mode.enable(ready);
    tab.temperature.enable(ready);
}

fn current_revision(tab: &GmClawTab, text: GuiText) -> Result<String, String> {
    let state = tab.state.borrow();
    state
        .status
        .as_ref()
        .filter(|status| state.fresh && status.exists && status.schema_supported)
        .map(|status| status.revision.clone())
        .filter(|revision| !revision.trim().is_empty())
        .ok_or_else(|| {
            tr(
                text,
                "请先刷新，读取可用的天工 Claw 配置。",
                "Refresh to load a supported GMClaw configuration first.",
            )
            .to_string()
        })
}

fn build_request(tab: &GmClawTab, text: GuiText) -> Result<GmClawSaveRequest, String> {
    let revision = current_revision(tab, text)?;
    let provider = selected_provider(tab).ok_or_else(|| {
        tr(
            text,
            "请选择已启用的 Hub 来源渠道。",
            "Select an enabled Hub source channel.",
        )
        .to_string()
    })?;
    let model = tab.model.get_value();
    if !provider
        .models
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&model))
    {
        return Err(tr(
            text,
            "请选择渠道中的可用模型。",
            "Select an available model from the channel.",
        )
        .to_string());
    }
    let max_tokens = tab
        .max_tokens
        .get_value()
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            tr(
                text,
                "最大输出 Token 必须是大于 0 的整数。",
                "Maximum output tokens must be a positive integer.",
            )
            .to_string()
        })?;
    let temperature = tab
        .temperature
        .get_value()
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (0.0..=2.0).contains(value))
        .ok_or_else(|| {
            tr(
                text,
                "温度必须是 0 到 2 之间的数字。",
                "Temperature must be a number between 0 and 2.",
            )
            .to_string()
        })?;
    Ok(GmClawSaveRequest {
        entry_id: tab
            .state
            .borrow()
            .status
            .as_ref()
            .and_then(|status| status.selected_entry_id.clone()),
        make_active: tab.make_active.is_checked(),
        source_provider: provider.name,
        model,
        max_tokens,
        temperature,
        parameters: GmClawParameters {
            reasoning_effort: selected_reasoning_effort(tab),
            temperature_mode: selected_temperature_mode(tab),
        },
        revision,
    })
}
