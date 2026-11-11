use super::{
    controls::{Button, ButtonKind},
    theme,
    visual::{self, Canvas, Icon},
};
use crate::desktop::clients::ClientId;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::dc::GraphicsDC;
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PageId {
    Overview,
    Squad,
    Assistant,
    Client(ClientId),
    Sources,
    Skills,
    Mcp,
    Chat,
    Logs,
    Settings,
    Help,
    Dumpling,
}

impl PageId {
    fn visible_destination(self) -> Self {
        match self {
            Self::Client(id) if !id.is_visible() => Self::Overview,
            _ => self,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Squad => "天才小分队",
            Self::Assistant => "天才搭档",
            Self::Client(id) => id.name(),
            Self::Sources => "模型与渠道",
            Self::Skills => "Skill 管理",
            Self::Mcp => "MCP 管理",
            Self::Chat => "聊天接入",
            Self::Logs => "请求日志",
            Self::Settings => "设置",
            Self::Help => "帮助",
            Self::Dumpling => "Dumpling-MCP",
        }
    }
}

type Changed = Rc<dyn Fn(PageId)>;

fn add_window_controls(frame: &Frame, parent: &Panel, row: &BoxSizer) {
    let minimize = Button::builder(parent)
        .with_label("最小化")
        .with_icon(Icon::WindowMinimize)
        .with_kind(ButtonKind::Caption)
        .with_size(Size::new(46, 44))
        .build();
    let maximize = Button::builder(parent)
        .with_label("最大化")
        .with_icon(Icon::WindowMaximize)
        .with_kind(ButtonKind::Caption)
        .with_size(Size::new(46, 44))
        .build();
    let close = Button::builder(parent)
        .with_label("关闭")
        .with_icon(Icon::WindowClose)
        .with_kind(ButtonKind::CaptionClose)
        .with_size(Size::new(46, 44))
        .build();
    for (button, name, tip) in [
        (minimize, "window-minimize", "最小化"),
        (
            maximize,
            "window-maximize",
            "最大化 / 还原；悬停选择贴靠布局",
        ),
        (close, "window-close", "关闭窗口（保留原托盘行为）"),
    ] {
        button.set_name(name);
        button.set_tooltip(tip);
        row.add(&button, 0, SizerFlag::Expand, 0);
    }
    frame.register_chrome_maximize_button(&maximize);
    let f = *frame;
    minimize.on_click(move |_| f.iconize(true));
    maximize.on_click(move |_| f.maximize(!f.is_maximized()));
    close.on_click(move |_| f.close(false));
    frame.on_size(move |event| {
        let maximized = f.is_maximized();
        maximize.set_icon(if maximized {
            Icon::WindowRestore
        } else {
            Icon::WindowMaximize
        });
        maximize.set_label(if maximized { "还原" } else { "最大化" });
        event.skip(true);
    });
}

#[derive(Clone)]
pub(super) struct Shell {
    pub panel: Panel,
    pub book: SimpleBook,
    pub service: Button,
    pub account: Button,
    pub assistant_launcher: Button,
    pub assistant_host: Panel,
    assistant_footer: Panel,
    title: StaticText,
    back: Button,
    secondary_origin: Rc<Cell<PageId>>,
    pub status: StaticText,
    breadcrumb: StaticText,
    sidebar: ScrolledWindow,
    pages: Rc<RefCell<Vec<PageId>>>,
    current: Rc<Cell<PageId>>,
    buttons: Rc<RefCell<Vec<(PageId, Button)>>>,
    changed: Rc<RefCell<Option<Changed>>>,
    source_focus: Rc<RefCell<Option<String>>>,
}

impl Shell {
    pub fn new(parent: &Panel, frame: &Frame) -> Self {
        let t = theme::theme();
        let integrated = frame.enable_integrated_chrome(t.is_dark, t.divider);
        let panel = Panel::builder(parent).build();
        panel.set_background_color(t.bg_app);
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let rail = Panel::builder(&panel).build();
        rail.set_background_color(t.bg_card);
        rail.set_min_size(visual::dip_size(&rail, 200, -1));
        let rail_column = BoxSizer::builder(Orientation::Vertical).build();
        let brand = Panel::builder(&rail).build();
        brand.set_min_size(visual::dip_size(&brand, 200, 44));
        brand.set_background_style(BackgroundStyle::Paint);
        visual::redraw_on_resize(brand);
        brand.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&brand);
            let dc = GraphicsDC::new(&base);
            let c = Canvas::new(&dc, visual::scale(&brand));
            c.clear(t.bg_card);
            visual::draw_brand(&c, 18, 9);
        });
        rail_column.add(&brand, 0, SizerFlag::Expand, 0);
        let sidebar = ScrolledWindow::builder(&rail)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        sidebar.set_scroll_rate(0, sidebar.from_dip(10));
        sidebar.show_scroll_indicators(false);
        sidebar.set_background_color(t.bg_card);
        let side = BoxSizer::builder(Orientation::Vertical).build();
        let mut buttons = Vec::new();
        for id in [PageId::Overview, PageId::Squad, PageId::Assistant] {
            let b = nav_button(&sidebar, id.title());
            b.set_icon(match id {
                PageId::Overview => Icon::Overview,
                PageId::Assistant => Icon::Chat,
                _ => Icon::Squad,
            });
            side.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                sidebar.from_dip(4),
            );
            buttons.push((id, b));
        }
        let label = StaticText::builder(&sidebar).with_label("客户端").build();
        label.set_font(&theme::font(theme::TextRole::Caption));
        label.set_foreground_color(t.ink_secondary);
        side.add(
            &label,
            0,
            SizerFlag::Expand
                | SizerFlag::Left
                | SizerFlag::Right
                | SizerFlag::Top
                | SizerFlag::Bottom,
            sidebar.from_dip(12),
        );
        // The five supported destinations stay visible and stationary.
        for id in ClientId::VISIBLE {
            let b = nav_button(&sidebar, visual::client_short(id));
            b.set_icon(Icon::client(id));
            side.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                sidebar.from_dip(4),
            );
            buttons.push((PageId::Client(id), b));
        }
        let shared = StaticText::builder(&sidebar).with_label("共用能力").build();
        shared.set_font(&theme::font(theme::TextRole::Caption));
        shared.set_foreground_color(t.ink_secondary);
        side.add(
            &shared,
            0,
            SizerFlag::Left | SizerFlag::Top | SizerFlag::Bottom,
            sidebar.from_dip(12),
        );
        for (id, icon) in [
            (PageId::Sources, Icon::Model),
            (PageId::Dumpling, Icon::Mcp),
            (PageId::Chat, Icon::Chat),
            (PageId::Logs, Icon::Logs),
        ] {
            let b = nav_button(&sidebar, id.title());
            b.set_icon(icon);
            side.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                sidebar.from_dip(4),
            );
            buttons.push((id, b));
            if id == PageId::Dumpling {
                // The resource controller applies the saved optional visibility.
                b.hide();
            }
        }
        sidebar.set_sizer(side, true);
        rail_column.add(&sidebar, 1, SizerFlag::Expand, 0);
        for (id, icon) in [
            (PageId::Settings, Icon::Settings),
            (PageId::Help, Icon::Help),
        ] {
            let b = nav_button(&rail, id.title());
            b.set_icon(icon);
            rail_column.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                rail.from_dip(4),
            );
            buttons.push((id, b));
        }
        let version = StaticText::builder(&rail)
            .with_label(&format!("TianCaiSpaceHub  {}", env!("CARGO_PKG_VERSION")))
            .build();
        version.set_font(&theme::font(theme::TextRole::Caption));
        version.set_foreground_color(t.ink_muted);
        rail_column.add(
            &version,
            0,
            SizerFlag::Left | SizerFlag::Bottom | SizerFlag::Top,
            rail.from_dip(12),
        );
        rail.set_sizer(rail_column, true);
        row.add(&rail, 0, SizerFlag::Expand, 0);
        let content = Panel::builder(&panel).build();
        content.set_background_color(t.bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let bar = Panel::builder(&content).build();
        bar.set_background_color(t.bg_card);
        let top = BoxSizer::builder(Orientation::Horizontal).build();
        let breadcrumb = StaticText::builder(&bar).with_label("工作台").build();
        breadcrumb.set_font(&theme::font(theme::TextRole::Caption));
        breadcrumb.set_foreground_color(t.ink_secondary);
        let service = Button::builder(&bar)
            .with_label("本机服务 · 读取中")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(152, 30))
            .build();
        service.set_foreground_color(t.ink_secondary);
        let account = Button::builder(&bar)
            .with_label("登录账户")
            .with_icon(Icon::Squad)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(184, 30))
            .build();
        account.set_name("squad-account-profile");
        account.set_tooltip("天才小分队账户、资料和余额");
        top.add(&breadcrumb, 1, SizerFlag::AlignCenterVertical, 0);
        top.add(&service, 0, SizerFlag::AlignCenterVertical, 0);
        top.add(
            &account,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Left,
            bar.from_dip(8),
        );
        if integrated {
            frame.register_chrome_drag_region(&brand);
            frame.register_chrome_drag_region(&bar);
            frame.register_chrome_drag_region(&breadcrumb);
            top.add_spacer(bar.from_dip(12));
            add_window_controls(frame, &bar, &top);
        }
        let bs = BoxSizer::builder(Orientation::Vertical).build();
        bs.add_sizer(
            &top,
            1,
            SizerFlag::Expand
                | SizerFlag::Left
                | if integrated {
                    SizerFlag::empty()
                } else {
                    SizerFlag::Right
                },
            bar.from_dip(theme::PAGE_GUTTER),
        );
        bar.set_min_size(visual::dip_size(&bar, -1, 44));
        bar.set_sizer(bs, true);
        column.add(&bar, 0, SizerFlag::Expand, 0);
        let heading = BoxSizer::builder(Orientation::Horizontal).build();
        let back = Button::builder(&content)
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(34, 34))
            .build();
        back.set_name("back-to-parent");
        back.set_tooltip("返回上一级");
        back.hide();
        heading.add(
            &back,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            content.from_dip(8),
        );
        let title = StaticText::builder(&content).with_label("概览").build();
        title.set_foreground_color(t.ink_primary);
        title.set_font(&theme::font(theme::TextRole::PageTitle));
        let status = StaticText::builder(&content).with_label("").build();
        status.set_foreground_color(t.ink_secondary);
        status.set_font(&theme::font(theme::TextRole::Caption));
        let help = Button::builder(&content)
            .with_label("?")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(30, 30))
            .build();
        help.set_tooltip("本页使用说明");
        heading.add(
            &title,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            content.from_dip(20),
        );
        heading.add(&status, 1, SizerFlag::AlignCenterVertical, 0);
        heading.add(&help, 0, SizerFlag::AlignCenterVertical, 0);
        column.add_sizer(
            &heading,
            0,
            SizerFlag::Expand | SizerFlag::All,
            content.from_dip(theme::PAGE_GUTTER),
        );
        let book = SimpleBook::builder(&content).build();
        book.set_background_color(t.bg_app);
        column.add(
            &book,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            content.from_dip(theme::PAGE_GUTTER),
        );
        // Keep the assistant entrance in the bottom-right corner with a small
        // reserved row, so it never covers a focused configuration control.
        let assistant_footer = Panel::builder(&content).build();
        assistant_footer.set_background_color(t.bg_app);
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        footer.add_stretch_spacer(1);
        let assistant_launcher = Button::builder(&assistant_footer)
            .with_icon(Icon::Chat)
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(44, 44))
            .build();
        assistant_launcher.set_name("assistant-floating-chat");
        assistant_launcher.set_tooltip("打开天才搭档，继续本次对话");
        footer.add(
            &assistant_launcher,
            0,
            SizerFlag::Right | SizerFlag::Bottom,
            content.from_dip(theme::PAGE_GUTTER),
        );
        assistant_footer.set_sizer(footer, true);
        column.add(&assistant_footer, 0, SizerFlag::Expand, 0);
        assistant_footer.hide();
        content.set_sizer(column, true);
        row.add(&content, 1, SizerFlag::Expand, 0);
        panel.set_sizer(row, true);
        let shell = Self {
            panel,
            book,
            service,
            account,
            assistant_launcher,
            assistant_host: content,
            assistant_footer,
            title,
            back,
            secondary_origin: Rc::new(Cell::new(PageId::Overview)),
            status,
            breadcrumb,
            sidebar,
            pages: Default::default(),
            current: Rc::new(Cell::new(PageId::Overview)),
            buttons: Rc::new(RefCell::new(buttons)),
            changed: Default::default(),
            source_focus: Default::default(),
        };
        for (id, b) in shell.buttons.borrow().iter() {
            let (id, nav) = (*id, shell.clone());
            b.on_click(move |_| nav.navigate(id));
        }
        let nav = shell.clone();
        back.on_click(move |_| nav.navigate(nav.secondary_origin.get()));
        let nav = shell.clone();
        help.on_click(move |_| super::show_info(&nav.panel, &help_text(nav.current())));
        shell.refresh_navigation();
        shell
    }
    pub fn register<W: WxWidget>(&self, id: PageId, page: &W) {
        assert!(!self.pages.borrow().contains(&id), "duplicate desktop page");
        self.book
            .add_page(page, id.title(), self.pages.borrow().is_empty(), None);
        self.pages.borrow_mut().push(id);
    }
    pub fn current(&self) -> PageId {
        self.current.get()
    }
    pub fn focus_source(&self, route: String) {
        *self.source_focus.borrow_mut() = Some(route);
        self.navigate(PageId::Sources);
    }
    pub fn take_source_focus(&self) -> Option<String> {
        self.source_focus.borrow().clone()
    }
    pub fn clear_source_focus(&self) {
        self.source_focus.borrow_mut().take();
    }
    pub fn on_changed(&self, changed: impl Fn(PageId) + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(changed));
    }
    pub fn set_assistant_ready(&self, ready: bool) {
        self.assistant_launcher.show(ready);
        if self.assistant_footer.is_shown() != ready {
            self.assistant_footer.show(ready);
            self.assistant_host.layout();
        }
    }
    /// Bounds of the client-area overlay. The title bar, rail and launcher
    /// remain outside the chat so native window controls stay reachable.
    pub fn assistant_overlay_area(&self) -> Rect {
        let size = self.assistant_host.get_client_size();
        let inset = self.assistant_host.from_dip(theme::PAGE_GUTTER);
        let top = self.assistant_host.from_dip(56);
        let bottom = size.height
            - self.assistant_footer.get_best_size().height
            - self.assistant_host.from_dip(theme::SPACE_SM);
        Rect::new(
            inset,
            top,
            (size.width - inset * 2).max(1),
            (bottom - top).max(1),
        )
    }
    pub fn set_dumpling_visible(&self, visible: bool) {
        if let Some((_, button)) = self
            .buttons
            .borrow()
            .iter()
            .find(|(id, _)| *id == PageId::Dumpling)
        {
            button.show(visible);
        }
        self.refresh_navigation();
    }
    pub fn navigate(&self, id: PageId) {
        let id = id.visible_destination();
        // General capability management is intentionally hidden. The backend
        // remains available to existing client integrations.
        if matches!(id, PageId::Skills | PageId::Mcp) {
            return;
        }
        let Some(index) = self.pages.borrow().iter().position(|v| *v == id) else {
            return;
        };
        self.back.hide();
        let previous = self.book.selection();
        self.book.change_selection(index);
        self.current.set(id);
        self.title.set_label(id.title());
        self.breadcrumb.set_label(match id {
            PageId::Client(_) => "客户端",
            PageId::Sources | PageId::Skills | PageId::Mcp | PageId::Chat | PageId::Logs => {
                "共用能力"
            }
            PageId::Squad => "账户服务",
            PageId::Dumpling | PageId::Assistant => "共用能力",
            _ => "工作台",
        });
        self.status.set_label("");
        self.refresh_navigation();
        self.panel.layout();
        if let Some(callback) = self.changed.borrow().clone() {
            callback(id);
        }
        super::motion::page_changed(self.book, previous);
    }
    fn refresh_navigation(&self) {
        let t = theme::theme();
        for (id, button) in self.buttons.borrow().iter() {
            nav_colour(button, *id == self.current.get());
        }
        self.title.set_foreground_color(t.ink_primary);
        self.sidebar.layout();
        self.sidebar.fit_inside();
        self.panel.layout();
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;

    #[test]
    fn hidden_client_navigation_returns_to_overview() {
        for id in ClientId::ALL {
            assert_eq!(
                PageId::Client(id).visible_destination(),
                if id.is_visible() {
                    PageId::Client(id)
                } else {
                    PageId::Overview
                }
            );
        }
        assert_eq!(PageId::Assistant.visible_destination(), PageId::Assistant);
    }
}

fn nav_button(parent: &dyn WxWidget, label: &str) -> Button {
    Button::builder(parent)
        .with_label(label)
        .with_kind(ButtonKind::Nav)
        .with_size(Size::new(184, 36))
        .build()
}
fn nav_colour(b: &Button, selected: bool) {
    b.set_selected(selected);
    b.set_foreground_color(theme::theme().ink_secondary);
}
fn help_text(page: PageId) -> String {
    let text = match page {
        PageId::Overview => {
            "运行状态只展示打开或有请求的客户端；有进行中请求时连线流动。用量统计来自本机已记录的请求，输入已包含协议报告的缓存用量，不另列缓存或首字。站点余额和账单请到天才小分队查看。"
        }
        PageId::Client(_) => {
            "选择模型即可配置到当前客户端。复用其他客户端的模型会建立独立副本，各端可以分别调整。如需重开客户端会在操作结果中提示。天工本机协助、接入设置和历史维护在本页对应区域。"
        }
        PageId::Sources => {
            "添加来源时按厂商选择账号、API Key 或专用接入方式。普通模型来源与客户端独立副本分别管理；更新来源后可选择同步哪些客户端，已有修改不会被静默覆盖。模型可见不代表上游支持所有能力。"
        }
        PageId::Skills => {
            "从本地目录、ZIP 或仓库导入 Skill。在卡片打开“管理客户端”，选择要使用的端后点击“应用更改”；返回会放弃未应用选择。只有成功的端显示使用中。未读取项目可查看原因及打开所在文件夹，原客户端文件不会因读取失败被删除。"
        }
        PageId::Mcp => {
            "添加 MCP 可使用配置表单或 JSON，两种视图共用草稿。远程服务填写地址及认证；本地服务填写命令、参数和环境变量。选择客户端后真实配置，失败端可单独重试。Dumpling-MCP 先配置环境与认证，再接入客户端。"
        }
        PageId::Squad => {
            "登录天才小分队后管理 API 密钥、站点用量及充值订阅；组织与扣费按账户实际权限显示。登录和付款由官网验证。退出账户不会删除已经导入的模型渠道。"
        }
        PageId::Assistant => {
            "启用后检查共享的天才搭档 Server；需要安装时由你确认。登录天才小分队后使用管理员提供的模型，费用由管理员承担。右下角可继续本次对话，收起不会结束；停止会中断执行，清空会删除本次临时内容。真正退出 Hub 后统一清理。密钥通过本机表单输入，删除配置会集中确认一次。"
        }
        PageId::Chat => {
            "添加聊天账号后，可在聊天平台内选择目录、模型和会话并处理审批。WorkBuddy 暂无外部任务执行；天工的执行授权、桌面同步与模型连接分别显示。"
        }
        PageId::Logs => {
            "记录开关沿用原设置。筛选和分页针对全部已记录请求，详情保留客户端请求与实际上游请求、缓存和首字等原始字段。未记录的历史无法补回。"
        }
        PageId::Dumpling => {
            "先添加环境并选择认证方式，再连接到客户端。保留密码、应用和浏览器认证，连接时自动检查配置；高级设置及撤销只影响当前环境和客户端。"
        }
        PageId::Settings | PageId::Help => {
            "设置包含外观、语言、本机服务、网络及系统集成。修复、恢复与诊断保留原功能；导出资料前可选择保存位置。"
        }
    };
    format!("{}\n\n{}", page.title(), text)
}
