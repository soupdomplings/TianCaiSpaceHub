use super::{
    controls::{Button, ButtonKind},
    theme,
    visual::{self, Canvas, Icon},
};
use crate::desktop::{
    clients::{ClientId, RecentClients},
    transaction,
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
use wxdragon::dc::GraphicsDC;
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PageId {
    Overview,
    Squad,
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
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Squad => "天才小分队",
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
    title: StaticText,
    back: Button,
    secondary_origin: Rc<Cell<PageId>>,
    pub status: StaticText,
    breadcrumb: StaticText,
    sidebar: ScrolledWindow,
    pages: Rc<RefCell<Vec<PageId>>>,
    current: Rc<Cell<PageId>>,
    buttons: Rc<RefCell<Vec<(PageId, Button)>>>,
    recent: Rc<RefCell<RecentClients>>,
    displayed_slots: Rc<Cell<[ClientId; 3]>>,
    slots: [Button; 3],
    extras: Vec<(ClientId, Button)>,
    more: Button,
    collapse: Button,
    expanded: Rc<Cell<bool>>,
    changed: Rc<RefCell<Option<Changed>>>,
    recent_path: Option<PathBuf>,
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
        for id in [PageId::Overview, PageId::Squad] {
            let b = nav_button(&sidebar, id.title());
            b.set_icon(if id == PageId::Overview {
                Icon::Overview
            } else {
                Icon::Squad
            });
            side.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                sidebar.from_dip(4),
            );
            buttons.push((id, b));
        }
        let client_heading = BoxSizer::builder(Orientation::Horizontal).build();
        let label = StaticText::builder(&sidebar).with_label("客户端").build();
        label.set_font(&theme::font(theme::TextRole::Caption));
        label.set_foreground_color(t.ink_secondary);
        let collapse = Button::builder(&sidebar)
            .with_label("")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(32, 30))
            .build();
        collapse.set_icon(Icon::Collapse);
        collapse.set_name("collapse-clients");
        collapse.set_tooltip("收起客户端");
        collapse.hide();
        client_heading.add(&label, 1, SizerFlag::AlignCenterVertical, 0);
        client_heading.add(&collapse, 0, SizerFlag::AlignCenterVertical, 0);
        side.add_sizer(
            &client_heading,
            0,
            SizerFlag::Expand
                | SizerFlag::Left
                | SizerFlag::Right
                | SizerFlag::Top
                | SizerFlag::Bottom,
            sidebar.from_dip(12),
        );
        let recent_path = super::daemon_config_path()
            .map(|p| crate::desktop::state_root(&p).join("recent-clients.json"));
        let recent = recent_path
            .as_ref()
            .and_then(|p| transaction::read(p).ok().flatten())
            .and_then(|b| serde_json::from_slice::<RecentClients>(&b).ok())
            .filter(|v| v.validate())
            .unwrap_or_default();
        let slots = std::array::from_fn(|i| {
            let b = nav_button(&sidebar, visual::client_short(recent.slots()[i]));
            side.add(
                &b,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                sidebar.from_dip(4),
            );
            b
        });
        let more = Button::builder(&sidebar)
            .with_label("")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(180, 34))
            .build();
        more.set_icon(Icon::More);
        more.set_name("expand-clients");
        more.set_min_size(visual::dip_size(&more, 180, 34));
        more.set_tooltip("展开其余客户端");
        side.add(
            &more,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            sidebar.from_dip(10),
        );
        let extras = ClientId::ALL
            .into_iter()
            .map(|id| {
                let b = nav_button(&sidebar, visual::client_short(id));
                b.set_icon(Icon::client(id));
                side.add(
                    &b,
                    0,
                    SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                    sidebar.from_dip(4),
                );
                b.hide();
                (id, b)
            })
            .collect();
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
            (PageId::Skills, Icon::Skill),
            (PageId::Mcp, Icon::Mcp),
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
        content.set_sizer(column, true);
        row.add(&content, 1, SizerFlag::Expand, 0);
        panel.set_sizer(row, true);
        let shell = Self {
            panel,
            book,
            service,
            account,
            title,
            back,
            secondary_origin: Rc::new(Cell::new(PageId::Mcp)),
            status,
            breadcrumb,
            sidebar,
            pages: Default::default(),
            current: Rc::new(Cell::new(PageId::Overview)),
            buttons: Rc::new(RefCell::new(buttons)),
            displayed_slots: Rc::new(Cell::new(*recent.slots())),
            recent: Rc::new(RefCell::new(recent)),
            slots,
            extras,
            more,
            collapse,
            expanded: Default::default(),
            changed: Default::default(),
            recent_path,
            source_focus: Default::default(),
        };
        for (id, b) in shell.buttons.borrow().iter() {
            let (id, nav) = (*id, shell.clone());
            b.on_click(move |_| nav.navigate(id));
        }
        for (i, b) in shell.slots.iter().enumerate() {
            let nav = shell.clone();
            b.on_click(move |_| {
                let id = nav.displayed_slots.get()[i];
                nav.navigate(PageId::Client(id));
            });
        }
        for (id, b) in &shell.extras {
            let (id, nav) = (*id, shell.clone());
            b.on_click(move |_| nav.navigate(PageId::Client(id)));
        }
        for b in [more, collapse] {
            let nav = shell.clone();
            b.on_click(move |_| {
                nav.expanded.set(!nav.expanded.get());
                nav.refresh_navigation();
            });
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
    pub fn navigate(&self, id: PageId) {
        let Some(index) = self.pages.borrow().iter().position(|v| *v == id) else {
            return;
        };
        if id == PageId::Dumpling && self.current.get() != PageId::Dumpling {
            self.secondary_origin.set(self.current.get());
        }
        self.back.show(id == PageId::Dumpling);
        if let PageId::Client(client) = id {
            self.recent.borrow_mut().visit(client);
            if let Some(path) = &self.recent_path {
                if let Ok(bytes) = serde_json::to_vec(&*self.recent.borrow()) {
                    if transaction::atomic_write(path, &bytes).is_err() {
                        tracing::warn!("could not save recent-client preference");
                    }
                }
            }
        }
        self.book.change_selection(index);
        self.current.set(id);
        self.title.set_label(id.title());
        self.breadcrumb.set_label(match id {
            PageId::Client(_) => "客户端",
            PageId::Sources | PageId::Skills | PageId::Mcp | PageId::Chat | PageId::Logs => {
                "共用能力"
            }
            PageId::Squad => "账户服务",
            PageId::Dumpling => "MCP 管理",
            _ => "工作台",
        });
        self.status.set_label("");
        self.refresh_navigation();
        self.panel.layout();
        if let Some(callback) = self.changed.borrow().clone() {
            callback(id);
        }
    }
    fn refresh_navigation(&self) {
        let t = theme::theme();
        for (id, button) in self.buttons.borrow().iter() {
            nav_colour(button, *id == self.current.get());
        }
        // Keep the expanded list stationary, including the three visible slots.
        // Apply accumulated recency only after collapsing it again.
        if !self.expanded.get() {
            self.displayed_slots.set(*self.recent.borrow().slots());
        }
        let slots = self.displayed_slots.get();
        for (i, b) in self.slots.iter().enumerate() {
            let client = slots[i];
            b.set_label(visual::client_short(client));
            b.set_icon(Icon::client(client));
            nav_colour(b, self.current.get() == PageId::Client(client));
        }
        for (id, b) in &self.extras {
            b.show(self.expanded.get() && !slots.contains(id));
            nav_colour(b, self.current.get() == PageId::Client(*id));
        }
        self.more.show(!self.expanded.get());
        self.collapse.show(self.expanded.get());
        self.title.set_foreground_color(t.ink_primary);
        self.sidebar.layout();
        self.sidebar.fit_inside();
        self.panel.layout();
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
            "选择模型即可配置到当前客户端。复用其他客户端的模型会建立独立副本，各端可以分别调整。Skill 和 MCP 只有实际配置成功才点亮；如需重开客户端会在操作结果中提示。接入设置和历史维护在本页对应区域。"
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
