mod layout;
#[cfg(windows)]
mod startup;

use super::controls::{Button, ButtonKind};
use super::{
    api::ApiClient,
    dialog_job,
    shell::{PageId, Shell},
    text::{GuiLocale, GuiText},
    theme::{self, ThemeMode},
};
use crate::gui::controls::{Choice, TextCtrl};
use layout::{SettingsCard, caption};
use serde_json::json;
use wxdragon::dc::GraphicsDC;
use wxdragon::prelude::*;

fn save_motion(path: &std::path::Path, reduce: bool) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(&json!({ "reduceMotion": reduce }))?;
    crate::desktop::transaction::atomic_write(path, &bytes)?;
    // The current session changes only when subsequent launches can use the same value.
    theme::set_motion_enabled(!reduce);
    Ok(())
}

#[cfg(test)]
#[test]
fn failed_motion_save_keeps_last_applied_preference() {
    let before = theme::motion_enabled();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("appearance.json");
    save_motion(&file, true).unwrap();
    let broken = file.join("inside-a-file.json");
    assert!(save_motion(&broken, false).is_err());
    assert!(!theme::motion_enabled());
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(persisted["reduceMotion"], true);
    theme::set_motion_enabled(before);
}

/// Keep the OS toggle's pressed state, Tab focus and Space action. Only its drawing is custom.
fn motion_switch(page: &Panel, frame: Frame) -> ToggleButton {
    let switch = ToggleButton::builder(page)
        .with_label("减少界面动效")
        .with_style(ToggleButtonStyle::BorderNone | ToggleButtonStyle::ExactFit)
        .build();
    switch.set_name("减少界面动效");
    switch.set_tooltip("立即应用；聚焦后按空格切换");
    switch.set_value(!theme::motion_enabled());
    let size = super::visual::dip_size(&switch, 112, 34);
    switch.set_min_size(size);
    switch.set_max_size(size);
    switch.set_background_style(BackgroundStyle::Paint);
    switch.on_paint(move |_| {
        let base = AutoBufferedPaintDC::new(&switch);
        let dc = GraphicsDC::new(&base);
        let scale = super::visual::scale(&switch);
        let c = super::visual::Canvas::new(&dc, scale);
        let t = theme::theme();
        let on = switch.get_value();
        let size = switch.get_client_size();
        let height = (size.height as f64 / scale).round() as i32;
        let width = (size.width as f64 / scale).round() as i32;
        let mid = height / 2;
        c.clear(t.bg_card);
        if switch.has_focus() {
            c.rounded(
                Rect::new(1, 1, (width - 2).max(1), (height - 2).max(1)),
                8,
                t.bg_card,
                Some(t.accent),
            );
        }
        c.rounded(
            Rect::new(5, mid - 11, 40, 22),
            11,
            if on { t.accent } else { t.ink_muted },
            None,
        );
        c.dot(if on { 34 } else { 16 }, mid, 8, Colour::rgb(255, 255, 255));
        c.label(
            if on { "已开启" } else { "已关闭" },
            Rect::new(52, 0, (width - 56).max(1), height),
            13,
            false,
            t.ink_primary,
            false,
        );
    });
    switch.on_set_focus(move |e| {
        switch.refresh(false, None);
        e.skip(true);
    });
    switch.on_kill_focus(move |e| {
        switch.refresh(false, None);
        e.skip(true);
    });
    switch.on_toggle(move |_| {
        let result = super::daemon_config_path()
            .ok_or_else(|| anyhow::anyhow!("未找到 Hub 配置目录"))
            .and_then(|cfg| {
                save_motion(
                    &crate::desktop::state_root(&cfg).join("appearance.json"),
                    switch.get_value(),
                )
            });
        if let Err(e) = result {
            switch.set_value(!theme::motion_enabled());
            super::show_error(&frame, &format!("无法保存动效设置：{e:#}"));
        }
        switch.refresh(false, None);
    });
    switch
}

pub(super) fn create(
    shell: &Shell,
    frame: Frame,
    api: ApiClient,
    text: GuiText,
    resources: &super::resources::ResourceUi,
    export_diagnostics: std::rc::Rc<dyn Fn()>,
) -> CheckBox {
    let (page, dumpling) = settings_page(
        &shell.book,
        frame,
        api,
        text,
        SettingsValues {
            theme: super::load_gui_theme(),
            proxy: super::load_outbound_proxy_config().mode,
            local: super::load_local_connection_mode(),
            dumpling: super::load_dumpling_mcp_visibility(),
        },
        |panel, body| resources.add_recovery_control(panel, body),
    );
    shell.register(PageId::Settings, &page);
    let help = ScrolledWindow::builder(&shell.book)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    help.set_scroll_rate(0, 12);
    help.set_background_color(theme::theme().bg_card);
    let s = BoxSizer::builder(Orientation::Vertical).build();
    heading(&help, &s, "开始使用");
    for (title, description, target) in [
        (
            "接入模型",
            "在“模型与渠道”添加渠道和模型。ChatGPT / Codex 只需选择模型，Hub 会匹配提供该模型的渠道。",
            PageId::Sources,
        ),
        (
            "天才搭档",
            "启用后可协助配置模型与渠道，并解答 Hub 和小分队使用问题。",
            PageId::Assistant,
        ),
        (
            "ChatGPT 接入与修复",
            "初始化、增强启动、历史恢复及检查修复集中在 ChatGPT / Codex 的接入设置。",
            PageId::Client(crate::desktop::clients::ClientId::Codex),
        ),
        (
            "运行状态与用量",
            "运行图显示真实活动；本机统计与站点账单分别查看。原请求详情保留协议报告的原始字段。",
            PageId::Overview,
        ),
        (
            "天才小分队账户",
            "登录后管理密钥、用量和订单，组织功能按账户实际权限展示。",
            PageId::Squad,
        ),
    ] {
        let b = Button::builder(&help).with_label(title).build();
        s.add(&b, 0, SizerFlag::Left | SizerFlag::Top, 20);
        let label = StaticText::builder(&help).with_label(description).build();
        label.wrap(680);
        s.add(&label, 0, SizerFlag::Expand | SizerFlag::All, 20);
        let nav = shell.clone();
        b.on_click(move |_| nav.navigate(target));
    }
    let diagnostics = Button::builder(&help).with_label("导出连接诊断…").build();
    s.add(&diagnostics, 0, SizerFlag::Left | SizerFlag::Top, 20);
    diagnostics.on_click(move |_| export_diagnostics());
    let about = Button::builder(&help)
        .with_label("关于 TianCaiSpaceHub")
        .build();
    s.add(&about, 0, SizerFlag::All, 20);
    about.on_click(move |_| super::show_about_dialog(&frame));
    super::shell::inset_page_content(&help, s);
    shell.register(PageId::Help, &help);
    dumpling
}
struct SettingsValues {
    theme: ThemeMode,
    proxy: crate::config::OutboundProxyMode,
    local: crate::config::LocalConnectionMode,
    dumpling: bool,
}

fn settings_page(
    parent: &impl WxWidget,
    frame: Frame,
    api: ApiClient,
    text: GuiText,
    values: SettingsValues,
    add_recovery: impl FnOnce(&Panel, &BoxSizer),
) -> (ScrolledWindow, CheckBox) {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_scroll_rate(0, page.from_dip(12));
    page.set_background_color(theme::theme().bg_app);
    let column = BoxSizer::builder(Orientation::Vertical).build();
    column.add_spacer(page.from_dip(theme::SPACE_LG));

    let top = layout::card_pair(&page, &column);
    let appearance_card = SettingsCard::new(&page, &top, "外观与语言");
    let surface = appearance_card.panel;
    let appearance = Choice::builder(&surface)
        .with_choices(vec!["跟随系统".into(), "浅色".into(), "深色".into()])
        .build();
    appearance.set_selection(match values.theme {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    });
    appearance.set_min_size(super::visual::dip_size(
        &appearance,
        152,
        theme::CONTROL_HEIGHT,
    ));
    appearance_card.row("主题", &appearance);
    let language = Choice::builder(&surface)
        .with_choices(vec!["简体中文".into(), "English".into()])
        .build();
    language.set_selection(if text.locale == GuiLocale::ZhCn { 0 } else { 1 });
    language.set_min_size(super::visual::dip_size(
        &language,
        152,
        theme::CONTROL_HEIGHT,
    ));
    appearance_card.row("界面语言", &language);
    caption(
        &surface,
        &appearance_card.body,
        "保存后，主题和语言在下次打开 Hub 时生效。",
    );
    let save = appearance_card.action("保存外观设置", ButtonKind::Primary);
    let motion = motion_switch(&surface, frame);
    appearance_card.row("减少界面动效", &motion);
    save.on_click(move |_| {
        let mode = match appearance.get_selection() {
            Some(1) => ThemeMode::Light,
            Some(2) => ThemeMode::Dark,
            _ => ThemeMode::System,
        };
        let locale = if language.get_selection() == Some(1) {
            GuiLocale::EnUs
        } else {
            GuiLocale::ZhCn
        };
        let result = super::save_gui_theme(mode).and_then(|_| super::save_gui_locale(locale));
        match result {
            Ok(()) => super::show_info(
                &frame,
                "外观设置已保存。主题及语言将在重新打开 Hub 后应用。",
            ),
            Err(e) => super::show_error(&frame, &e),
        }
    });
    let integration = SettingsCard::new(&page, &top, "启动与集成");
    #[cfg(windows)]
    startup::add(&integration.panel, &integration.body, frame);
    let actions = WrapSizer::builder(Orientation::Horizontal)
        .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
        .build();
    for (register, title) in [(true, "启用网页导入"), (false, "解除网页导入")] {
        let button = Button::builder(&integration.panel)
            .with_label(title)
            .with_kind(ButtonKind::Secondary)
            .build();
        actions.add(
            &button,
            0,
            SizerFlag::Right | SizerFlag::Bottom,
            integration.panel.from_dip(theme::SPACE_SM),
        );
        button.on_click(move |_| {
            let outcome = if register {
                crate::external_import::registration::register()
            } else {
                crate::external_import::registration::unregister()
            };
            match outcome {
                Ok(()) => super::show_info(&frame, "网页导入关联已更新"),
                Err(e) => super::show_error(&frame, &e),
            }
        });
    }
    caption(
        &integration.panel,
        &integration.body,
        "允许网站通过已注册的链接把配置导入 Hub。",
    );
    integration
        .body
        .add_sizer(&actions, 0, SizerFlag::Expand, 0);

    let bottom = layout::card_pair(&page, &column);
    let network = SettingsCard::new(&page, &bottom, "连接与网络");
    let proxy = Choice::builder(&network.panel)
        .with_choices(vec![
            "系统代理".into(),
            "直接连接".into(),
            "自定义代理".into(),
        ])
        .build();
    proxy.set_selection(match values.proxy {
        crate::config::OutboundProxyMode::System => 0,
        crate::config::OutboundProxyMode::Direct => 1,
        crate::config::OutboundProxyMode::Custom => 2,
    });
    proxy.set_min_size(super::visual::dip_size(&proxy, 152, theme::CONTROL_HEIGHT));
    network.row("出站代理", &proxy);
    let apply = network.action("保存网络设置", ButtonKind::Primary);
    let worker = api.clone();
    apply.on_click(move |_| {
        super::handle_outbound_proxy_selected(
            &frame,
            text,
            &worker,
            match proxy.get_selection() {
                Some(1) => crate::config::OutboundProxyMode::Direct,
                Some(2) => crate::config::OutboundProxyMode::Custom,
                _ => crate::config::OutboundProxyMode::System,
            },
        )
    });
    let current_mode = values.local;
    let local_title = |mode| format!("当前为{}", text.local_connection_label(mode));
    let local_state = caption(&network.panel, &network.body, &local_title(current_mode));
    let local = network.action("切换本机连接模式", ButtonKind::Secondary);
    local.on_click(move |_| {
        let mode = match super::load_local_connection_mode() {
            crate::config::LocalConnectionMode::Standard => {
                crate::config::LocalConnectionMode::VpnCompatible
            }
            _ => crate::config::LocalConnectionMode::Standard,
        };
        super::handle_local_connection_selected(&frame, text, mode);
        local_state.set_label(&format!(
            "当前为{}",
            text.local_connection_label(super::load_local_connection_mode())
        ));
        network.panel.layout();
    });
    let clients = SettingsCard::new(&page, &bottom, "客户端与扩展");
    caption(
        &clients.panel,
        &clients.body,
        "使用非默认配置目录时，可为对应客户端添加位置。",
    );
    let custom = clients.action("添加其他配置位置", ButtonKind::Secondary);
    let a = api.clone();
    custom.on_click(move |_| target_editor(&frame, a.clone()));
    add_recovery(&clients.panel, &clients.body);
    let dumpling = CheckBox::builder(&clients.panel)
        .with_label("显示 Dumpling-MCP 环境管理")
        .with_value(values.dumpling)
        .build();
    dumpling.set_foreground_color(theme::theme().ink_primary);
    dumpling.set_background_color(theme::theme().bg_card);
    clients.body.add(
        &dumpling,
        0,
        SizerFlag::Top,
        clients.panel.from_dip(theme::SPACE_SM),
    );
    super::shell::inset_page_content(&page, column);
    page.on_size(move |event| {
        layout::resize_pairs(page, [top, bottom]);
        event.skip(true);
    });
    (page, dumpling)
}

fn heading<W: WxWidget>(parent: &W, sizer: &BoxSizer, label: &str) {
    let title = StaticText::builder(parent).with_label(label).build();
    title.set_font(&theme::font(theme::TextRole::Title));
    title.set_foreground_color(theme::theme().ink_primary);
    sizer.add(&title, 0, SizerFlag::Left | SizerFlag::Top, 20);
}

#[cfg(all(test, windows))]
#[test]
fn settings_page_reflows_and_scrolls_without_recreating_controls() {
    wxdragon::app::render_offscreen(|| {
        theme::init(ThemeMode::Light);
        let frame = Frame::builder()
            .with_title("Windows preferences layout")
            .build();
        frame.set_font(&theme::font(theme::TextRole::Body));
        let text = GuiText::new(GuiLocale::ZhCn);
        let (page, dumpling) = settings_page(
            &frame,
            frame,
            ApiClient::new(String::new(), text),
            text,
            SettingsValues {
                theme: ThemeMode::System,
                proxy: crate::config::OutboundProxyMode::System,
                local: crate::config::LocalConnectionMode::Standard,
                dumpling: true,
            },
            |_, _| {},
        );
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add(&page, 1, SizerFlag::Expand, 0);
        frame.set_sizer(outer, false);
        let theme_control = page.find_window_by_name("主题").unwrap().handle_ptr();
        for (width, height, wide) in [(1040, 650, true), (500, 410, false)] {
            frame.set_client_size(super::visual::dip_size(&frame, width, height));
            frame.layout();
            page.layout();
            page.fit_inside();
            let cards = ["外观与语言", "启动与集成", "连接与网络", "客户端与扩展"]
                .map(|name| page.find_window_by_name(name).unwrap());
            let pos = cards.each_ref().map(|card| card.get_position());
            assert_eq!(
                pos[0].y == pos[1].y,
                wide,
                "expected four settings sections to reflow"
            );
            assert_eq!(pos[0].x != pos[1].x, wide);
            assert!(pos[2].y > pos[0].y);
            for card in &cards {
                let origin = card.get_position();
                assert!(
                    origin.x >= 0
                        && origin.x + card.get_size().width <= page.get_client_size().width
                );
            }
            assert_eq!(
                page.find_window_by_name("主题").unwrap().handle_ptr(),
                theme_control
            );
            assert!(dumpling.is_checked());
            if !wide {
                assert!(page.get_virtual_size().height > page.get_client_size().height);
                page.scroll_coords(0, 10);
                assert!(page.view_start().y > 0);
                page.scroll_coords(0, 0);
            }
            super::native_model_review::capture(
                &page,
                if wide {
                    "settings-wide"
                } else {
                    "settings-narrow"
                },
            );
        }
        assert!(!frame.is_shown());
        frame.destroy();
    })
    .unwrap();
}
fn target_editor(frame: &Frame, api: ApiClient) {
    let dialog = Dialog::builder(frame, "其他客户端配置位置")
        .with_size(680, 360)
        .build();
    super::motion::install_dialog(dialog);
    let s = BoxSizer::builder(Orientation::Vertical).build();
    let client = Choice::builder(&dialog)
        .with_choices(
            crate::desktop::clients::ClientId::VISIBLE
                .into_iter()
                .map(|c| c.name().into())
                .collect(),
        )
        .build();
    client.set_selection(0);
    s.add(&client, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let path = TextCtrl::builder(&dialog).build();
    path.set_tooltip("客户端实际配置根目录");
    s.add(
        &path,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let status = StaticText::builder(&dialog).build();
    s.add(&status, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let save = Button::builder(&dialog).with_label("添加配置位置").build();
    s.add(&save, 0, SizerFlag::AlignRight | SizerFlag::All, 16);
    dialog.set_sizer(s, true);
    let job = dialog_job::bind(dialog, save, status, api, move || {
        let id = client
            .get_selection()
            .and_then(|index| crate::desktop::clients::ClientId::VISIBLE.get(index as usize))
            .ok_or("请选择客户端")?;
        if path.get_value().trim().is_empty() {
            return Err("请选择实际配置根目录".into());
        }
        Ok((
            "/api/desktop/targets/save".into(),
            json!({"client":id,"root":path.get_value()}),
        ))
    });
    dialog.show_modal();
    drop(job);
    dialog.destroy();
}
