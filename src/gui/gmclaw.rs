use super::controls::Button;
use crate::gui::controls::{Choice, TextCtrl};
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
    widgets::connection_field_theme,
};

#[derive(Clone, Debug)]
pub(super) struct GmClawProviderOption {
    name: String,
    display_name: String,
    models: Vec<String>,
    provider_type: ProviderType,
    compatibility: Option<String>,
    model_aliases: BTreeMap<String, String>,
    copy: Option<(String, String)>,
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
struct ModelDraft {
    revision: String,
    provider: Option<String>,
    model: Option<String>,
    max_tokens: String,
    temperature: String,
    parameters: GmClawParameters,
}

#[derive(Clone)]
pub(super) struct GmClawTab {
    pub(super) page: Panel,
    account: super::tiangong_models::AccountBar,
    selected_builtin: Rc<RefCell<Option<String>>>,
    section: Panel,
    builtin_panel: Panel,
    builtin_title: StaticText,
    share_builtin: Button,
    editor_scroll: ScrolledWindow,
    entry: super::model_list::ModelList,
    delete_button: Button,
    provider: Choice,
    model: Choice,
    source_protocol: StaticText,
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
    normal_start_button: Button,
    state: Rc<RefCell<SelectionState>>,
    in_flight: Arc<AtomicBool>,
    draft: Rc<RefCell<Option<ModelDraft>>>,
}

#[derive(Debug)]
pub(super) enum GmClawActionResult {
    Start(Result<String, String>),
    Refresh(
        Result<
            (
                Vec<GmClawProviderOption>,
                GmClawConfigStatus,
                serde_json::Value,
            ),
            String,
        >,
        bool,
    ),
    Save(Result<GmClawConfigStatus, String>),
    Restore(Result<GmClawConfigStatus, String>),
    Delete(Result<GmClawConfigStatus, String>),
}

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

fn parameter_field<W: WxWidget>(parent: &Panel, grid: &FlexGridSizer, name: &str, input: &W) {
    let label = StaticText::builder(parent).with_label(name).build();
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    grid.add(input, 1, SizerFlag::Expand, 0);
}

pub(super) fn create(parent: &dyn WxWidget, text: GuiText) -> GmClawTab {
    create_with_settings(parent, text, None, None)
}
pub(super) fn create_with_settings(
    parent: &dyn WxWidget,
    text: GuiText,
    settings: Option<&Panel>,
    header_actions: Option<(Button, Button)>,
) -> GmClawTab {
    let page = Panel::builder(parent).build();
    page.set_background_color(theme::theme().bg_app);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let account = super::tiangong_models::AccountBar::new(&page);
    root.add(
        &account.panel,
        0,
        SizerFlag::Expand | SizerFlag::Bottom,
        page.from_dip(8),
    );
    let editor_scroll = ScrolledWindow::builder(&page)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    editor_scroll.set_min_size(Size::new(0, 0));
    editor_scroll.set_background_color(theme::theme().bg_app);
    editor_scroll.set_scroll_rate(0, editor_scroll.from_dip(10));
    let section = Panel::builder(&editor_scroll).build();
    section.set_background_color(theme::theme().bg_card);
    super::controls::paint_surface(section, theme::SURFACE_RADIUS);
    let section_sizer = BoxSizer::builder(Orientation::Vertical).build();
    section.set_sizer(section_sizer, false);
    let hint = StaticText::builder(&section)
        .with_label(tr(
            text,
            "选择大模型接入渠道和模型，保存后在天工 Work 中选择使用。",
            "Choose a model channel and model, save, then select the model in TianGong Work.",
        ))
        .build();
    hint.set_foreground_color(theme::theme().ink_muted);
    hint.wrap(920);
    hint.hide();
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(page.from_dip(6))
        .with_hgap(page.from_dip(12))
        .build();
    grid.add_growable_col(1, 1);
    let entry = super::model_list::ModelList::new(&page);
    let provider_label = StaticText::builder(&section)
        .with_label(tr(text, "来源渠道", "Model channel"))
        .build();
    provider_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&provider_label, 0, SizerFlag::AlignCenterVertical, 0);
    let provider = Choice::builder(&section)
        .with_size(Size::new(260, -1))
        .build();
    grid.add(&provider, 1, SizerFlag::Expand, 0);
    let model_label = StaticText::builder(&section)
        .with_label(tr(text, "模型", "Model"))
        .build();
    model_label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&model_label, 0, SizerFlag::AlignCenterVertical, 0);
    let model = Choice::builder(&section)
        .with_size(Size::new(260, -1))
        .build();
    connection_field_theme(&model, true);
    model.set_foreground_color(theme::theme().ink_primary);
    for input in [provider, model] {
        input.set_min_size(super::visual::dip_size(&section, 0, 32));
    }
    grid.add(&model, 1, SizerFlag::Expand, 0);
    section_sizer.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::All,
        page.from_dip(12),
    );
    let source_protocol = StaticText::builder(&section).build();
    source_protocol.set_foreground_color(theme::theme().ink_secondary);
    source_protocol.set_font(&theme::font(theme::TextRole::Caption));
    section_sizer.add(
        &source_protocol,
        0,
        SizerFlag::Left | SizerFlag::Bottom,
        page.from_dip(theme::PANEL_INSET),
    );
    let parameters = FlexGridSizer::builder(0, 4)
        .with_vgap(page.from_dip(6))
        .with_hgap(page.from_dip(8))
        .build();
    parameters.add_growable_col(1, 1);
    parameters.add_growable_col(3, 1);
    let max_tokens = TextCtrl::builder(&section).with_value("8192").build();
    max_tokens.set_tooltip(tr(text, "最大输出 Token 数", "Maximum output tokens"));
    let reasoning_effort = Choice::builder(&section)
        .with_size(Size::new(260, -1))
        .build();
    reasoning_effort.append(tr(text, "自动", "Automatic"));
    reasoning_effort.set_selection(0);
    let temperature_mode = Choice::builder(&section)
        .with_size(Size::new(260, -1))
        .build();
    for label in [
        tr(text, "自动适配", "Automatic adaptation"),
        tr(text, "不发送温度", "Omit temperature"),
        tr(text, "发送所填温度", "Send entered temperature"),
    ] {
        temperature_mode.append(label);
    }
    temperature_mode.set_selection(0);
    let temperature = TextCtrl::builder(&section).with_value("0.7").build();
    temperature.set_tooltip(tr(text, "温度范围 0–2", "Temperature range 0–2"));
    for input in [max_tokens, temperature] {
        input.set_min_size(super::visual::dip_size(&section, 0, 32));
        connection_field_theme(&input, true);
    }
    for input in [reasoning_effort, temperature_mode] {
        input.set_min_size(super::visual::dip_size(&section, 0, 36));
    }
    parameter_field(
        &section,
        &parameters,
        tr(text, "最大输出", "Max output"),
        &max_tokens,
    );
    parameter_field(
        &section,
        &parameters,
        tr(text, "思考强度", "Reasoning"),
        &reasoning_effort,
    );
    parameter_field(
        &section,
        &parameters,
        tr(text, "温度策略", "Temperature"),
        &temperature_mode,
    );
    parameter_field(
        &section,
        &parameters,
        tr(text, "温度", "Value"),
        &temperature,
    );
    section_sizer.add_sizer(
        &parameters,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        page.from_dip(12),
    );
    let selection_hint = StaticText::builder(&section).with_label("").build();
    selection_hint.set_foreground_color(theme::theme().ink_muted);
    selection_hint.hide();
    section_sizer.add(&selection_hint, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let parameter_hint = StaticText::builder(&section).with_label("").build();
    parameter_hint.set_foreground_color(theme::theme().ink_muted);
    parameter_hint.hide();
    section_sizer.add(&parameter_hint, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let editor = BoxSizer::builder(Orientation::Horizontal).build();
    editor.add(
        &entry,
        0,
        SizerFlag::Expand | SizerFlag::Right,
        page.from_dip(12),
    );
    let field_col = BoxSizer::builder(Orientation::Vertical).build();
    field_col.add(&section, 0, SizerFlag::Expand, 0);
    let builtin_panel = Panel::builder(&editor_scroll).build();
    builtin_panel.set_background_color(theme::theme().bg_card);
    super::controls::paint_surface(builtin_panel, theme::SURFACE_RADIUS);
    let builtin_col = BoxSizer::builder(Orientation::Vertical).build();
    let builtin_title = StaticText::builder(&builtin_panel)
        .with_label("天工内置模型")
        .build();
    builtin_title.set_font(&theme::font(theme::TextRole::Title));
    builtin_title.set_foreground_color(theme::theme().ink_primary);
    builtin_col.add(&builtin_title, 0, SizerFlag::All, page.from_dip(20));
    let note = StaticText::builder(&builtin_panel)
        .with_label("模型参数随天工客户端读取。\n分享后继续沿用来源参数，个人登录可选。")
        .build();
    note.set_foreground_color(theme::theme().ink_secondary);
    note.wrap(page.from_dip(390));
    builtin_col.add(
        &note,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        page.from_dip(20),
    );
    let share_builtin = Button::builder(&builtin_panel)
        .with_label("分享给其他客户端")
        .with_icon(super::visual::Icon::Plus)
        .build();
    builtin_col.add(
        &share_builtin,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        page.from_dip(20),
    );
    builtin_panel.set_sizer(builtin_col, false);
    builtin_panel.hide();
    field_col.add(&builtin_panel, 0, SizerFlag::Expand, 0);
    editor_scroll.set_sizer(field_col, true);
    editor.add(&editor_scroll, 1, SizerFlag::Expand, 0);
    root.add_sizer(&editor, 1, SizerFlag::Expand, 0);

    let status = StaticText::builder(&page)
        .with_label(tr(
            text,
            "正在读取天工 Work 配置…",
            "Loading TianGong Work configuration…",
        ))
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
            .with_label(tr(text, "启动天工 Work", "Start TianGong Work"))
            .build()
    });
    start_button.set_label(tr(text, "增强启动", "Enhanced start"));
    start_button.set_tooltip(tr(
        text,
        "使用已保存的外部模型直接进入本地工作区；首次请先退出正在运行的天工。",
        "Enter the local workspace with saved external models. Close a normally running TianGong first.",
    ));
    let normal_start_button = Button::builder(maintenance)
        .with_label(tr(text, "普通启动天工 Work", "Start TianGong normally"))
        .build();
    let reload_button = header_actions.map(|a| a.1).unwrap_or_else(|| {
        Button::builder(maintenance)
            .with_label(tr(text, "重新读取", "Reload"))
            .build()
    });
    let restore_button = Button::builder(maintenance)
        .with_label(tr(text, "撤销上次操作", "Undo last operation"))
        .build();
    let save_button = Button::builder(&page)
        .with_label(tr(text, "保存模型", "Save model"))
        .build();
    let delete_button = Button::builder(&page)
        .with_label(tr(text, "移除模型", "Remove model"))
        .with_kind(super::controls::ButtonKind::Ghost)
        .build();
    let utilities = BoxSizer::builder(Orientation::Vertical).build();
    let maintenance_buttons = if header_actions.is_some() {
        vec![&normal_start_button, &restore_button]
    } else {
        vec![
            &start_button,
            &normal_start_button,
            &reload_button,
            &restore_button,
        ]
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
    actions.add(&delete_button, 0, SizerFlag::Right, 8);
    actions.add(&save_button, 0, SizerFlag::Right, 0);
    root.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom,
        page.from_dip(8),
    );
    let display_status = settings
        .map(StaticText::builder)
        .unwrap_or_else(|| StaticText::builder(&page))
        .with_label(tr(
            text,
            "桌面对话同步：等待本地服务状态",
            "Desktop message sync: waiting for local service status",
        ))
        .build();
    display_status.set_foreground_color(theme::theme().ink_secondary);
    display_status.wrap(920);
    let sync_layout = BoxSizer::builder(Orientation::Vertical).build();
    sync_layout.add(
        &display_status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        20,
    );
    let im_hint = settings.map(StaticText::builder).unwrap_or_else(||StaticText::builder(&page))
        .with_label(tr(
            text,
            "IM 接入：在飞书、微信或企业微信发送 /tg。首次同步桌面对话，请退出天工后从此页启动。",
            "Start TianGong Work here to enable desktop message sync. In connected Feishu, WeChat, or WeCom, send /tg and select a project and model. Send /gpt to return to ChatGPT (Codex).",
        ))
        .build();
    im_hint.set_foreground_color(theme::theme().ink_muted);
    im_hint.wrap(920);
    sync_layout.add(
        &im_hint,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        20,
    );
    if let Some(host) = settings {
        if let Some(s) = host.get_sizer() {
            s.add_sizer(&sync_layout, 0, SizerFlag::Expand, 0);
        }
    } else {
        root.add_sizer(&sync_layout, 0, SizerFlag::Expand, 0);
    }
    page.set_sizer(root, true);
    page.set_min_size(Size::new(0, 0));
    page.layout();
    editor_scroll.fit_inside();
    for choice in [&provider, &reasoning_effort, &temperature_mode] {
        connection_field_theme(choice, true);
    }
    let tab = GmClawTab {
        page,
        account,
        selected_builtin: Default::default(),
        section,
        builtin_panel,
        builtin_title,
        share_builtin,
        editor_scroll,
        entry,
        delete_button,
        provider,
        model,
        source_protocol,
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
        normal_start_button,
        state: Rc::new(RefCell::new(SelectionState {
            reasoning_efforts: vec!["auto"],
            ..SelectionState::default()
        })),
        in_flight: Arc::new(AtomicBool::new(false)),
        draft: Default::default(),
    };
    update_parameter_hint(&tab, text);
    update_controls(&tab);
    bind_editor_events(&tab, text);
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
                    "Not enabled; start TianGong Work or send /tg to prepare sync",
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
        tab.editor_scroll.fit_inside();
    }
}

fn bind_editor_events(tab: &GmClawTab, text: GuiText) {
    let entry_tab = tab.clone();
    tab.entry.on_selection_changed(move |_| {
        if entry_tab.in_flight.load(Ordering::SeqCst) {
            return;
        }
        capture_draft(&entry_tab);
        let count = entry_tab
            .state
            .borrow()
            .status
            .as_ref()
            .map(|s| s.entries.len())
            .unwrap_or(0);
        let selected = entry_tab.entry.get_selection().unwrap_or(u32::MAX) as usize;
        let builtin = selected
            .checked_sub(count)
            .and_then(|i| entry_tab.account.models().get(i).cloned());
        if let Some(builtin) = builtin {
            *entry_tab.selected_builtin.borrow_mut() = builtin["id"].as_str().map(str::to_string);
            entry_tab
                .builtin_title
                .set_label(builtin["name"].as_str().unwrap_or("天工内置模型"));
            update_controls(&entry_tab);
            return;
        }
        entry_tab.selected_builtin.borrow_mut().take();
        let mut status = match entry_tab.state.borrow().status.clone() {
            Some(status) => status,
            None => return,
        };
        let entry = entry_tab
            .entry
            .get_selection()
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
}

pub(super) fn bind_actions(
    tab: &GmClawTab,
    api: &ApiClient,
    frame: &Frame,
    text: GuiText,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
) {
    {
        let (tab, api, frame) = (tab.clone(), api.clone(), *frame);
        tab.account.test.on_click(move |_| {
            if !tab.in_flight.load(Ordering::SeqCst) {
                super::tiangong_models::show_identity_test(&frame, &api);
            }
        });
    }
    {
        let (tab, api, tx, frame) = (tab.clone(), api.clone(), gui_tx.clone(), *frame);
        let button = tab.account.login;
        button.on_click(move |_| {
            if tab.in_flight.load(Ordering::SeqCst) {
                return;
            }
            if super::tiangong_models::show(&frame, &api) {
                refresh(&tab, &api, text, &tx, false, false);
            }
        });
    }
    {
        let (tab, api, tx, frame) = (tab.clone(), api.clone(), gui_tx.clone(), *frame);
        let button = tab.share_builtin;
        button.on_click(move |_| {
            let selected = tab.selected_builtin.borrow().clone();
            let model = tab
                .account
                .models()
                .into_iter()
                .find(|m| m["id"].as_str() == selected.as_deref());
            if let Some(model) = model {
                if super::tiangong_models::share(&frame, &api, &model) {
                    refresh(&tab, &api, text, &tx, false, false);
                }
            }
        });
    }
    for (button, enhanced) in [(tab.start_button, true), (tab.normal_start_button, false)] {
        let start_tab = tab.clone();
        let start_api = api.clone();
        let start_tx = gui_tx.clone();
        button.on_click(move |_| {
            if !begin_action(
                &start_tab,
                if enhanced {
                    tr(
                        text,
                        "正在准备天工外部模型工作区…",
                        "Preparing TianGong external-model workspace…",
                    )
                } else {
                    tr(
                        text,
                        "正在启动并连接天工 Work…",
                        "Starting and connecting TianGong Work…",
                    )
                },
            ) {
                return;
            }
            start_tab
                .start_button
                .set_label(tr(text, "准备中…", "Preparing…"));
            let preferred = start_tab.state.borrow().status.as_ref().and_then(|status| {
                status
                    .entries
                    .iter()
                    .find(|entry| Some(&entry.entry_id) == status.selected_entry_id.as_ref())
                    .map(|entry| entry.model_id.clone())
            });
            let api = start_api.clone();
            let tx = start_tx.clone();
            thread::spawn(move || {
                let result = api.start_gmclaw_desktop(enhanced, preferred);
                let _ = tx.send(super::GuiMessage::GmClaw(GmClawActionResult::Start(result)));
                wxdragon::wake_up_idle();
            });
        });
    }
    {
        let action_tab = tab.clone();
        let action_api = api.clone();
        let action_tx = gui_tx.clone();
        let action_frame = *frame;
        tab.delete_button.on_click(move |_| {
            if is_draft(&action_tab) {
                action_tab.draft.borrow_mut().take();
                let current = action_tab.state.borrow().status.clone();
                if let Some(mut status) = current {
                    select_entry(&mut status, 0);
                    apply_status(&action_tab, text, status, true);
                    update_controls(&action_tab);
                }
                return;
            }
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
                let result = GmClawActionResult::Delete(api.delete_gmclaw_config(&request));
                let _ = tx.send(super::GuiMessage::GmClaw(result));
                wxdragon::wake_up_idle();
            });
        });
    }
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
        let copy = copy_request(&save_tab, &request);
        let api = save_api.clone();
        let tx = save_tx.clone();
        thread::spawn(move || {
            let result = if let Some(body) = copy {
                api.desktop_post("/api/desktop/models/copy", &body)
                    .and_then(|v| {
                        serde_json::from_value(v["status"].clone())
                            .map_err(|_| "已返回保存结果，请重新读取天工 Work 配置核对".into())
                    })
            } else {
                api.save_gmclaw_config(&request)
            };
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
        refresh(&reload_tab, &reload_api, text, &reload_tx, false, true);
    });
    refresh(tab, api, text, gui_tx, true, false);
}

pub(super) fn refresh(
    tab: &GmClawTab,
    api: &ApiClient,
    text: GuiText,
    gui_tx: &tokio::sync::mpsc::UnboundedSender<super::GuiMessage>,
    startup: bool,
    rebase_draft: bool,
) {
    capture_draft(tab);
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
            result = api.get_app_config().and_then(|mut config| {
                let mut builtins = api
                    .desktop_post("/api/desktop/tiangong-models/read", &serde_json::json!({}))
                    .unwrap_or_else(|e| serde_json::json!({"accountError":e}));
                if builtins["parametersCurrent"] != true
                    && builtins["models"].as_array().is_some_and(|m| !m.is_empty())
                {
                    match api.desktop_post(
                        "/api/desktop/tiangong-models/connect",
                        &serde_json::json!({"revision":config.revision,"native":true}),
                    ) {
                        Ok(view) => {
                            builtins = view;
                            config = api.get_app_config()?;
                        }
                        Err(e) => builtins["accountError"] = e.into(),
                    }
                }
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
                    (provider_options_with_copies(&config), status, builtins)
                })
            });
            if result.is_ok() || attempt + 1 == attempts {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        let _ = tx.send(super::GuiMessage::GmClaw(GmClawActionResult::Refresh(
            result,
            rebase_draft,
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
    let start_feedback = match &result {
        GmClawActionResult::Start(Ok(detail) | Err(detail)) => Some(detail.clone()),
        _ => None,
    };
    match result {
        GmClawActionResult::Start(result) => {
            tab.start_button
                .set_label(tr(text, "增强启动", "Enhanced start"));
            if let Err(error) = result {
                // This button is also available on the overview/settings tabs;
                // the model editor's status label can be hidden there.
                show_error(frame, &error);
            }
        }
        GmClawActionResult::Refresh(Ok((providers, status, builtins)), rebase) => {
            tab.account.apply(builtins);
            if rebase || tab.draft.borrow().is_none() {
                tab.state.borrow_mut().providers = providers;
            }
            if rebase {
                if let Some(draft) = tab.draft.borrow_mut().as_mut() {
                    draft.revision = status.revision.clone();
                }
            }
            apply_status(tab, text, status, true);
        }
        GmClawActionResult::Save(Ok(status)) => {
            if is_draft(tab) {
                tab.draft.borrow_mut().take();
            }
            apply_status(tab, text, status, true);
            show_info(
                frame,
                tr(
                    text,
                    "模型已保存，可直接在天工 Work 中选择使用。请保持 Hub 运行。",
                    "Model saved. Select it in TianGong Work and keep Hub running.",
                ),
            );
        }
        GmClawActionResult::Restore(Ok(status)) => {
            apply_status(tab, text, status, true);
            show_info(
                frame,
                tr(
                    text,
                    "上次操作已还原，可直接在天工 Work 中选择模型。请保持 Hub 运行。",
                    "Latest operation restored. Select a model in TianGong Work and keep Hub running.",
                ),
            );
        }
        GmClawActionResult::Delete(Ok(status)) => {
            apply_status(tab, text, status, true);
        }
        GmClawActionResult::Refresh(Err(error), _) => {
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
        | GmClawActionResult::Delete(Err(error)) => {
            tab.state.borrow_mut().fresh = false;
            tab.status.set_label(tr(
                text,
                "操作未完成，当前输入已保留；点击“载入更新”后重试。",
                "The operation did not complete. Your inputs are retained; load updates before retrying.",
            ));
            show_error(frame, &error);
        }
    }
    update_controls(tab);
    if let Some(detail) = start_feedback {
        tab.status.set_label(&detail);
    }
    tab.status.wrap(920);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
}

fn provider_options(config: &AppConfig) -> Vec<GmClawProviderOption> {
    config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.enabled && !provider.is_client_reserved())
        .filter(|provider| provider.provider_type != ProviderType::ChatGptResponses)
        .map(provider_option)
        .collect()
}

fn provider_option(provider: &crate::ai_gateway::config::ProviderConfig) -> GmClawProviderOption {
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
        display_name: provider.name.clone(),
        models,
        provider_type: provider.provider_type.clone(),
        compatibility: provider.compatibility.clone(),
        model_aliases: provider.model_aliases.clone(),
        copy: None,
    }
}

fn provider_options_with_copies(config: &AppConfig) -> Vec<GmClawProviderOption> {
    let mut options = provider_options(config);
    if let Some(revision) = &config.revision {
        options.extend(
            config
                .ai_gateway
                .providers
                .iter()
                .filter(|p| {
                    p.enabled
                        && p.is_client_reserved()
                        && p.provider_type != ProviderType::ChatGptResponses
                })
                .map(|p| {
                    let mut option = provider_option(p);
                    option.display_name = super::workbuddy::copy_source_label(p);
                    option.copy = Some((
                        crate::ai_gateway::config::provider_route_id(p),
                        revision.clone(),
                    ));
                    option
                }),
        );
    }
    options
}

fn copy_request(tab: &GmClawTab, request: &GmClawSaveRequest) -> Option<serde_json::Value> {
    let provider = selected_provider(tab)?;
    if provider.name != request.source_provider {
        return None;
    }
    let (route, revision) = provider.copy?;
    Some(
        serde_json::json!({"client":"tiangong", "sourceRoute":route, "revision":revision, "model":request.model, "tiangong":request}),
    )
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
    let mut labels = status
        .entries
        .iter()
        .map(|entry| {
            format!(
                "{} · {}",
                entry.model.model_name,
                if entry.configured {
                    "使用中"
                } else {
                    "待完善"
                }
            )
        })
        .collect::<Vec<_>>();
    let builtin_models = tab.account.models();
    labels.extend(
        builtin_models
            .iter()
            .map(|m| m["name"].as_str().unwrap_or("天工内置模型").to_string()),
    );
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
    let native_selected = tab
        .selected_builtin
        .borrow()
        .as_ref()
        .and_then(|id| builtin_models.iter().position(|m| m["id"] == id.as_str()));
    if native_selected.is_none() {
        tab.selected_builtin.borrow_mut().take();
    }
    let first_builtin =
        selected.is_none() && tab.draft.borrow().is_none() && !builtin_models.is_empty();
    if first_builtin {
        *tab.selected_builtin.borrow_mut() = builtin_models[0]["id"].as_str().map(str::to_string);
        tab.builtin_title
            .set_label(builtin_models[0]["name"].as_str().unwrap_or("天工内置模型"));
    }
    let index = if let Some(i) = native_selected {
        status.entries.len() + i
    } else if first_builtin {
        status.entries.len()
    } else {
        selected.unwrap_or(status.entries.len() + builtin_models.len())
    };
    tab.entry.set_selection(
        index as u32,
        selected.is_some()
            || tab.draft.borrow().is_some()
            || native_selected.is_some()
            || first_builtin,
    );
    let saved_effort = status.parameters.reasoning_effort.clone();
    tab.temperature_mode
        .set_selection(match &status.parameters.temperature_mode {
            TemperatureMode::Auto => 0,
            TemperatureMode::Omit => 1,
            TemperatureMode::Preserve => 2,
        });
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
    let draft = if status.selected_entry_id.is_none() {
        tab.draft.borrow().clone()
    } else {
        None
    };
    if let Some(draft) = &draft {
        let mut state = tab.state.borrow_mut();
        state.preferred_provider = draft.provider.clone();
        state.preferred_model = draft.model.clone();
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
            "未找到天工 Work 配置。请先安装并打开天工 Work 完成初始化，再回到这里刷新。",
            "TianGong Work configuration was not found. Install and open TianGong Work to initialize it, then refresh here.",
        )
    } else if !status.schema_supported {
        tr(
            text,
            "当前天工 Work 配置格式暂不支持，无法保存或还原。请确认安装版本与配置路径。",
            "This TianGong Work configuration format is not supported. Saving and restoring are unavailable. Check the installed version and configuration path.",
        )
    } else if status.configured {
        tr(
            text,
            "天工 Work 已配置为通过 Hub 接入。",
            "TianGong Work is configured to connect through Hub.",
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
            tab.provider.append(&provider.display_name);
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
    if let Some(draft) = &draft {
        tab.max_tokens.set_value(&draft.max_tokens);
        tab.temperature.set_value(&draft.temperature);
        tab.temperature_mode
            .set_selection(match draft.parameters.temperature_mode {
                TemperatureMode::Auto => 0,
                TemperatureMode::Omit => 1,
                TemperatureMode::Preserve => 2,
            });
        apply_reasoning_choices(
            tab,
            text,
            draft
                .parameters
                .reasoning_effort
                .as_deref()
                .unwrap_or("auto"),
        );
    }
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
                "原渠道不存在或未启用，请明确选择大模型接入渠道",
                "The previous channel is missing or disabled; select a source channel"
            )
        )
    } else {
        tr(text, "请选择大模型接入渠道。", "Select a source channel.").to_string()
    };
    drop(state);
    tab.selection_hint.set_label(&hint);
    tab.selection_hint.hide();
    tab.model.set_tooltip(&hint);
    tab.selection_hint.set_min_size(Size::new(0, -1));
    tab.selection_hint.wrap(920);
    update_model_details(tab, text);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
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
            tr(text, "自动", "Automatic")
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
        tab.source_protocol.set_label(protocol);
    } else {
        let placeholder = tr(text, "请选择大模型接入渠道", "Select a source channel");
        tab.source_protocol.set_label(placeholder);
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
        "思考强度“自动”使用模型默认设置。",
        "Automatic reasoning uses the model default.",
    );
    let temperature = match selected_temperature_mode(tab) {
        TemperatureMode::Auto => tr(
            text,
            "温度“自动适配”按模型支持情况发送。",
            "Automatic temperature follows model support.",
        ),
        TemperatureMode::Omit => tr(
            text,
            "不发送温度参数。",
            "The temperature parameter is omitted.",
        ),
        TemperatureMode::Preserve => tr(
            text,
            "请确认所选模型支持温度参数。",
            "Confirm that the model supports temperature.",
        ),
    };
    tab.parameter_hint
        .set_tooltip(&format!("{reasoning}\n{temperature}"));
    tab.reasoning_effort.set_tooltip(reasoning);
    tab.temperature_mode.set_tooltip(temperature);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
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
    let builtin = tab.selected_builtin.borrow().is_some();
    tab.section.show(!builtin);
    tab.builtin_panel.show(builtin);
    tab.save_button.show(!builtin);
    tab.delete_button.show(!builtin);
    tab.account.login.enable(!busy);
    tab.share_builtin.enable(!busy);
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
    tab.reload_button.set_label("载入更新");
    tab.reload_button.show(!state.fresh && !busy);
    if let Some(parent) = tab.reload_button.get_parent() {
        parent.layout();
    }
    tab.start_button.enable(!busy);
    tab.normal_start_button.enable(!busy);
    tab.entry.enable(ready);
    if builtin {
        tab.entry.enable(!busy);
    }
    if builtin {
        tab.status.set_label("天工内置模型 · 沿用来源参数");
    }
    let selected = state
        .status
        .as_ref()
        .is_some_and(|status| status.selected_entry_id.is_some());
    tab.delete_button
        .enable(ready && (selected || tab.draft.borrow().is_some()));
    tab.delete_button.set_label(if is_draft(tab) {
        "放弃新建"
    } else {
        "移除模型"
    });
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
    for choice in [&tab.provider, &tab.reasoning_effort, &tab.temperature_mode] {
        connection_field_theme(choice, choice.is_enabled());
    }
    connection_field_theme(&tab.model, tab.model.is_enabled());
    for field in [&tab.max_tokens, &tab.temperature] {
        connection_field_theme(field, field.is_editable() && field.is_enabled());
    }
    tab.page.layout();
    tab.editor_scroll.layout();
    tab.editor_scroll.fit_inside();
}

fn select_entry(status: &mut GmClawConfigStatus, index: usize) {
    let entry = status.entries.get(index).cloned();
    status.selected_entry_id = entry.as_ref().map(|e| e.entry_id.clone());
    status.model = entry.as_ref().map(|e| e.model.clone());
    status.source_provider = entry.as_ref().and_then(|e| e.source_provider.clone());
    status.parameters = entry
        .as_ref()
        .map(|e| e.parameters.clone())
        .unwrap_or_default();
    status.active = entry.as_ref().is_some_and(|e| e.active);
    status.configured = entry.as_ref().is_some_and(|e| e.configured);
}
fn is_draft(tab: &GmClawTab) -> bool {
    tab.selected_builtin.borrow().is_none()
        && tab.draft.borrow().is_some()
        && tab
            .state
            .borrow()
            .status
            .as_ref()
            .is_some_and(|s| s.selected_entry_id.is_none())
}
fn capture_draft(tab: &GmClawTab) {
    if !is_draft(tab) {
        return;
    }
    let mut draft = tab.draft.borrow_mut();
    let draft = draft.as_mut().unwrap();
    draft.provider = selected_provider(tab).map(|p| p.name);
    draft.model = Some(tab.model.get_value());
    draft.max_tokens = tab.max_tokens.get_value();
    draft.temperature = tab.temperature.get_value();
    draft.parameters = GmClawParameters {
        reasoning_effort: selected_reasoning_effort(tab),
        temperature_mode: selected_temperature_mode(tab),
    };
}
pub(super) fn begin_model_draft(tab: &GmClawTab, text: GuiText) {
    if tab.in_flight.load(Ordering::SeqCst) {
        return;
    }
    let Some(mut status) = tab.state.borrow().status.clone() else {
        return;
    };
    if !tab.state.borrow().fresh
        || !status.exists
        || !status.schema_supported
        || status.revision.is_empty()
    {
        return;
    }
    if is_draft(tab) {
        return;
    }
    tab.selected_builtin.borrow_mut().take();
    if tab.draft.borrow().is_none() {
        *tab.draft.borrow_mut() = Some(ModelDraft {
            revision: status.revision.clone(),
            provider: None,
            model: None,
            max_tokens: "8192".into(),
            temperature: "0.7".into(),
            parameters: GmClawParameters {
                reasoning_effort: Some("xhigh".into()),
                ..Default::default()
            },
        });
    }
    let index = status.entries.len();
    select_entry(&mut status, index);
    apply_status(tab, text, status, true);
    tab.status.set_label("新建模型尚未保存");
    update_controls(tab);
    tab.page.layout();
    tab.editor_scroll.fit_inside();
}
fn current_revision(tab: &GmClawTab, text: GuiText) -> Result<String, String> {
    let state = tab.state.borrow();
    state
        .status
        .as_ref()
        .filter(|status| state.fresh && status.exists && status.schema_supported)
        .map(|status| {
            if status.selected_entry_id.is_none() {
                tab.draft
                    .borrow()
                    .as_ref()
                    .map(|d| d.revision.clone())
                    .unwrap_or_else(|| status.revision.clone())
            } else {
                status.revision.clone()
            }
        })
        .filter(|revision| !revision.trim().is_empty())
        .ok_or_else(|| {
            tr(
                text,
                "请先刷新，读取可用的天工 Work 配置。",
                "Refresh to load a supported TianGong Work configuration first.",
            )
            .to_string()
        })
}
#[cfg(all(test, windows))]
pub(super) fn review_inline_draft(
    frame: &Frame,
    model: &super::model_fields::ModelPage,
    settings: &Panel,
    header: &super::client_header::ClientHeader,
) {
    let text = GuiText::new(GuiLocale::ZhCn);
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
    tab.state.borrow_mut().providers = vec![GmClawProviderOption {
        name: "synthetic-source".into(),
        display_name: "synthetic-source".into(),
        models: vec!["gpt-6".into()],
        provider_type: ProviderType::OpenAiResponses,
        compatibility: None,
        model_aliases: Default::default(),
        copy: None,
    }];
    let saved = crate::gmclaw_config::GmClawModelConfig {
        local_url: "http://127.0.0.1:3210/synthetic".into(),
        model_name: "gpt-6".into(),
        max_tokens: 8192,
        temperature: 0.7,
        context_window: 128000,
    };
    let status: GmClawConfigStatus = serde_json::from_value(serde_json::json!({"path":"C:/Synthetic/tiangong/config.db","exists":true,"schemaSupported":true,"backupPath":"","backupExists":false,"model":saved,"sourceProvider":"synthetic-source","configured":true,"active":false,"localUrl":"http://127.0.0.1:3210/synthetic","activeModelId":null,"restartRequired":false,"revision":"original-revision","error":null,"entries":[{"entryId":"saved-entry","modelId":"synthetic-model-id","model":saved,"sourceProvider":"synthetic-source","parameters":{},"configured":true,"active":false,"localUrl":"http://127.0.0.1:3210/synthetic"}],"selectedEntryId":"saved-entry"})).unwrap();
    apply_status(&tab, text, status, true);
    update_controls(&tab);
    for _ in 0..4 {
        begin_model_draft(&tab, text);
        begin_model_draft(&tab, text);
        assert_eq!(tab.entry.get_selection(), Some(1));
        assert!(tab.entry.find_window_by_name("client-model-2").is_none());
        tab.provider.set_selection(1);
        tab.state.borrow_mut().preferred_provider = Some("synthetic-source".into());
        apply_model_choices(&tab, text, true);
        tab.max_tokens.set_value("32768");
        tab.temperature.set_value("0.35");
        tab.temperature_mode.set_selection(2);
        apply_reasoning_choices(&tab, text, "max");
        let draft = build_request(&tab, text).unwrap();
        assert!(draft.entry_id.is_none());
        assert_eq!(draft.max_tokens, 32768);
        assert_eq!(draft.temperature, 0.35);
        assert_eq!(draft.parameters.reasoning_effort.as_deref(), Some("max"));
        super::native_model_review::click(
            &tab.entry.find_window_by_name("client-model-0").unwrap(),
        );
        assert_eq!(
            tab.state
                .borrow()
                .status
                .as_ref()
                .unwrap()
                .selected_entry_id
                .as_deref(),
            Some("saved-entry")
        );
        begin_model_draft(&tab, text);
        assert_eq!(tab.max_tokens.get_value(), "32768");
        assert_eq!(tab.temperature.get_value(), "0.35");
        assert_eq!(selected_reasoning_effort(&tab).as_deref(), Some("max"));
    }
    let source = crate::ai_gateway::config::ProviderConfig {
        name: "workbuddy:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        base_url: "https://provider.example.invalid".into(),
        models: vec!["gpt-6".into()],
        ..Default::default()
    };
    let mut copy_config = AppConfig::default();
    copy_config.revision = Some("original-source-revision".into());
    copy_config.ai_gateway.providers.push(source.clone());
    assert!(provider_options(&copy_config).is_empty());
    tab.state
        .borrow_mut()
        .providers
        .extend(provider_options_with_copies(&copy_config));
    tab.provider.append("WorkBuddy · gpt-6");
    tab.provider.set_selection(2);
    tab.state.borrow_mut().preferred_provider = Some(source.name.clone());
    apply_model_choices(&tab, text, true);
    apply_reasoning_choices(&tab, text, "max");
    capture_draft(&tab);
    let request = build_request(&tab, text).unwrap();
    let copy = copy_request(&tab, &request).expect("explicit cross-client source");
    assert_eq!(copy["revision"], "original-source-revision");
    assert_eq!(copy["tiangong"]["revision"], "original-revision");
    assert_eq!(
        copy["sourceRoute"],
        crate::ai_gateway::config::provider_route_id(&source)
    );
    assert_eq!(copy["tiangong"]["maxTokens"], 32768);
    assert_eq!(copy["tiangong"]["temperature"], 0.35);
    assert_eq!(copy["tiangong"]["parameters"]["reasoningEffort"], "max");
    let mut latest = tab.state.borrow().status.clone().unwrap();
    latest.revision = "later-external-revision".into();
    apply_result(
        &tab,
        frame,
        text,
        GmClawActionResult::Refresh(Ok((vec![], latest.clone(), serde_json::Value::Null)), false),
    );
    assert_eq!(
        build_request(&tab, text).unwrap().revision,
        "original-revision"
    );
    assert_eq!(
        copy_request(&tab, &build_request(&tab, text).unwrap()).unwrap()["revision"],
        "original-source-revision"
    );
    tab.state.borrow_mut().fresh = false;
    update_controls(&tab);
    assert!(tab.reload_button.is_shown());
    copy_config.revision = Some("explicitly-loaded-source-revision".into());
    apply_result(
        &tab,
        frame,
        text,
        GmClawActionResult::Refresh(
            Ok((
                provider_options_with_copies(&copy_config),
                latest,
                serde_json::Value::Null,
            )),
            true,
        ),
    );
    let loaded = build_request(&tab, text).unwrap();
    assert_eq!(loaded.revision, "later-external-revision");
    assert_eq!(loaded.max_tokens, 32768);
    assert_eq!(loaded.temperature, 0.35);
    assert_eq!(loaded.parameters.reasoning_effort.as_deref(), Some("max"));
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
        tab.max_tokens.is_shown() && tab.reasoning_effort.is_shown() && tab.temperature.is_shown()
    );
    assert!(tab.save_button.get_position().y + tab.save_button.get_size().height <= bounds.height);
    assert!(
        (bounds.width - tab.save_button.get_position().x - tab.save_button.get_size().width).abs()
            <= tab.page.from_dip(12),
        "TianGong save action must stay at the right edge"
    );
    let viewport = tab.editor_scroll.get_client_size();
    let virtual_size = tab.editor_scroll.get_virtual_size();
    assert!(
        virtual_size.height <= viewport.height + tab.page.from_dip(2),
        "complete TianGong draft must fit: {virtual_size:?} {viewport:?}"
    );
    // Exercise actual model-list command callbacks across native rows and the
    // existing unsaved draft. Neither a login refresh nor sharing may save it.
    tab.account.apply(
        serde_json::json!({"loggedIn":true,"accountName":"合成天工账号","models":[
        {"id":"tiangong-builtin","name":"天工内置-外网","configured":true},
        {"id":"tiangong-builtin-intranet","name":"天工内置-内网","configured":true}]}),
    );
    let current = tab.state.borrow().status.clone().unwrap();
    let native_index = current.entries.len();
    apply_status(&tab, text, current, true);
    update_controls(&tab);
    let command = |index: usize| {
        let button = tab
            .page
            .find_window_by_name(&format!("client-model-{index}"))
            .unwrap();
        super::native_model_review::click(&button);
    };
    command(native_index);
    assert_eq!(
        tab.selected_builtin.borrow().as_deref(),
        Some("tiangong-builtin")
    );
    assert!(tab.builtin_panel.is_shown());
    assert!(!tab.section.is_shown());
    assert!(!tab.save_button.is_shown());
    assert!(!is_draft(&tab));
    assert_eq!(tab.draft.borrow().as_ref().unwrap().max_tokens, "32768");
    super::native_model_review::capture(
        &tab.page,
        if theme::theme().is_dark {
            "tiangong-builtins-dark"
        } else {
            "tiangong-builtins-light"
        },
    );
    command(native_index + 1);
    assert_eq!(
        tab.selected_builtin.borrow().as_deref(),
        Some("tiangong-builtin-intranet")
    );
    command(native_index + 2);
    assert!(is_draft(&tab));
    assert!(tab.section.is_shown());
    assert!(tab.save_button.is_shown());
    assert_eq!(build_request(&tab, text).unwrap().max_tokens, 32768);
}

fn build_request(tab: &GmClawTab, text: GuiText) -> Result<GmClawSaveRequest, String> {
    let revision = current_revision(tab, text)?;
    let provider = selected_provider(tab).ok_or_else(|| {
        tr(
            text,
            "请选择已启用的大模型接入渠道。",
            "Select an enabled Model channel.",
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
        make_active: false,
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
