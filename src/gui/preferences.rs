use super::controls::Button;
use super::{
    api::ApiClient,
    dialog_job,
    shell::{PageId, Shell},
    text::{GuiLocale, GuiText},
    theme::{self, ThemeMode},
};
use crate::gui::controls::{Choice, TextCtrl};
use serde_json::json;
use wxdragon::prelude::*;

pub(super) fn create(
    shell: &Shell,
    frame: Frame,
    api: ApiClient,
    text: GuiText,
    resources: &super::resources::ResourceUi,
    export_diagnostics: std::rc::Rc<dyn Fn()>,
) -> CheckBox {
    let page = ScrolledWindow::builder(&shell.book)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_scroll_rate(0, 12);
    page.set_background_color(theme::theme().bg_card);
    let column = BoxSizer::builder(Orientation::Vertical).build();
    heading(&page, &column, "外观与语言");
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(14)
        .with_hgap(20)
        .build();
    grid.add_growable_col(1, 1);
    let appearance = Choice::builder(&page)
        .with_choices(vec!["跟随系统".into(), "浅色".into(), "深色".into()])
        .build();
    appearance.set_selection(match super::load_gui_theme() {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    });
    appearance.set_min_size(super::visual::dip_size(&appearance, 170, 36));
    appearance.set_max_size(super::visual::dip_size(&appearance, 200, -1));
    field(&page, &grid, "主题", &appearance);
    let language = Choice::builder(&page)
        .with_choices(vec!["简体中文".into(), "English".into()])
        .build();
    language.set_selection(if text.locale == GuiLocale::ZhCn { 0 } else { 1 });
    language.set_min_size(super::visual::dip_size(&language, 170, 36));
    language.set_max_size(super::visual::dip_size(&language, 200, -1));
    field(&page, &grid, "语言", &language);
    let motion = CheckBox::builder(&page)
        .with_label("减少界面动效")
        .with_value(!theme::motion_enabled())
        .build();
    field(&page, &grid, "动效", &motion);
    let save = Button::builder(&page).with_label("保存外观设置").build();
    grid.add_spacer(1);
    grid.add(&save, 0, SizerFlag::default(), 0);
    column.add_sizer(&grid, 0, SizerFlag::Expand | SizerFlag::All, 20);
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
        theme::set_motion_enabled(!motion.get_value());
        if let Some(path) = super::daemon_config_path() {
            let file = crate::desktop::state_root(&path).join("appearance.json");
            if let Err(e) = crate::desktop::transaction::atomic_write(
                &file,
                &serde_json::to_vec(&json!({"reduceMotion":motion.get_value()}))
                    .unwrap_or_default(),
            ) {
                super::show_error(&frame, &e.to_string());
                return;
            }
        }
        let result = super::save_gui_theme(mode).and_then(|_| super::save_gui_locale(locale));
        match result {
            Ok(()) => super::show_info(
                &frame,
                "外观设置已保存。主题及语言将在重新打开 Hub 后应用。",
            ),
            Err(e) => super::show_error(&frame, &e),
        }
    });
    heading(&page, &column, "连接与网络");
    let network = BoxSizer::builder(Orientation::Horizontal).build();
    let proxy = Choice::builder(&page)
        .with_choices(vec![
            "系统代理".into(),
            "直接连接".into(),
            "自定义代理".into(),
        ])
        .build();
    proxy.set_selection(match super::load_outbound_proxy_config().mode {
        crate::config::OutboundProxyMode::System => 0,
        crate::config::OutboundProxyMode::Direct => 1,
        crate::config::OutboundProxyMode::Custom => 2,
    });
    let apply = Button::builder(&page).with_label("保存网络设置").build();
    proxy.set_min_size(super::visual::dip_size(&proxy, 156, 36));
    proxy.set_max_size(super::visual::dip_size(&proxy, 200, -1));
    network.add(&proxy, 0, SizerFlag::Right, 12);
    network.add(&apply, 0, SizerFlag::default(), 0);
    column.add_sizer(&network, 0, SizerFlag::Expand | SizerFlag::All, 20);
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
    let local = Button::builder(&page)
        .with_label("切换本机连接兼容模式")
        .build();
    column.add(&local, 0, SizerFlag::Left | SizerFlag::Bottom, 20);
    local.on_click(move |_| {
        let mode = match super::load_local_connection_mode() {
            crate::config::LocalConnectionMode::Standard => {
                crate::config::LocalConnectionMode::VpnCompatible
            }
            _ => crate::config::LocalConnectionMode::Standard,
        };
        super::handle_local_connection_selected(&frame, text, mode);
    });
    heading(&page, &column, "客户端配置位置");
    let custom = Button::builder(&page)
        .with_label("添加其他配置位置")
        .build();
    column.add(&custom, 0, SizerFlag::All, 20);
    let a = api.clone();
    custom.on_click(move |_| target_editor(&frame, a.clone()));
    resources.add_recovery_control(&page, &column);
    heading(&page, &column, "可选功能");
    let dumpling = CheckBox::builder(&page)
        .with_label("显示 Dumpling-MCP 环境管理")
        .with_value(super::load_dumpling_mcp_visibility())
        .build();
    column.add(&dumpling, 0, SizerFlag::All, 20);
    heading(&page, &column, "系统集成");
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    for (register, title) in [(true, "启用网页导入"), (false, "解除网页导入")] {
        let b = Button::builder(&page).with_label(title).build();
        row.add(&b, 0, SizerFlag::Right, 10);
        b.on_click(move |_| {
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
    column.add_sizer(&row, 0, SizerFlag::All, 20);
    page.set_sizer(column, true);
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
            "先添加渠道，再到客户端选择模型。复用模型会创建独立副本，各端可分别调整。",
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
    help.set_sizer(s, true);
    shell.register(PageId::Help, &help);
    dumpling
}
fn heading<W: WxWidget>(parent: &W, sizer: &BoxSizer, label: &str) {
    let title = StaticText::builder(parent).with_label(label).build();
    title.set_font(&theme::font(theme::TextRole::Title));
    title.set_foreground_color(theme::theme().ink_primary);
    sizer.add(&title, 0, SizerFlag::Left | SizerFlag::Top, 20);
}
fn field<W: WxWidget>(parent: &ScrolledWindow, grid: &FlexGridSizer, label: &str, control: &W) {
    let name = StaticText::builder(parent).with_label(label).build();
    grid.add(&name, 0, SizerFlag::AlignCenterVertical, 0);
    grid.add(control, 1, SizerFlag::Expand, 0);
}
fn target_editor(frame: &Frame, api: ApiClient) {
    let dialog = Dialog::builder(frame, "其他客户端配置位置")
        .with_size(680, 360)
        .build();
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
