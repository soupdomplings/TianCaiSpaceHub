mod rail;
mod strip;
pub(super) use strip::SlidingStrip;

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
    Remote,
    Client(ClientId),
    Sources,
    Skills,
    Mcp,
    #[cfg(windows)]
    XshellB,
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
            Self::Remote => "远程连接",
            Self::Client(id) => id.name(),
            Self::Sources => "模型与渠道",
            Self::Skills => "Skill 管理",
            Self::Mcp => "MCP 管理",
            #[cfg(windows)]
            Self::XshellB => crate::xshellb::NAME,
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
    page_area: Panel,
    rail: rail::Rail,
    pub status: StaticText,
    breadcrumb: StaticText,
    sidebar: ScrolledWindow,
    pages: Rc<RefCell<Vec<(PageId, bool)>>>,
    viewport: Rc<Cell<bool>>,
    current: Rc<Cell<PageId>>,
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
        let navigation = rail::Rail::new(sidebar);
        rail_column.add(&sidebar, 1, SizerFlag::Expand, 0);
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
        let breadcrumb = StaticText::builder(&bar)
            .with_label("工作空间  /  概览")
            .build();
        breadcrumb.set_name("shell-page-path");
        breadcrumb.set_font(&theme::font(theme::TextRole::Caption));
        breadcrumb.set_foreground_color(t.ink_secondary);
        let service = Button::builder(&bar)
            .with_label("本机服务 · 读取中")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(130, 30))
            .build();
        service.set_foreground_color(t.ink_secondary);
        let account = Button::builder(&bar)
            .with_label("登录账户")
            .with_icon(Icon::Squad)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(149, 30))
            .build();
        account.set_name("squad-account-profile");
        account.set_tooltip("天才小分队账户、资料和余额");
        top.add(&breadcrumb, 1, SizerFlag::AlignCenterVertical, 0);
        for button in [service, account] {
            top.add(&button, 0, SizerFlag::AlignCenterVertical, 0);
        }
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
        let status = StaticText::builder(&content).with_label("").build();
        status.set_foreground_color(t.ink_secondary);
        status.set_font(&theme::font(theme::TextRole::Caption));
        status.hide();
        column.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Top,
            content.from_dip(12),
        );
        column.add_spacer(content.from_dip(9));
        // Full-page viewports own their insets so their scrollbar reaches the
        // client edge. Fixed pages keep the same gutter outside the book.
        let page_area = Panel::builder(&content).build();
        page_area.set_background_color(t.bg_app);
        let book = SimpleBook::builder(&page_area).build();
        book.set_background_color(t.bg_app);
        column.add(&page_area, 1, SizerFlag::Expand, 0);
        // Only this 44-DIP native button participates in hit testing. It is
        // not a sizer item and never reserves space in the active page.
        let assistant_launcher = Button::builder(&content)
            .with_icon(Icon::Chat)
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(44, 44))
            .build();
        assistant_launcher.set_name("assistant-floating-chat");
        assistant_launcher.set_tooltip("打开天才搭档，继续本次对话");
        assistant_launcher.hide();
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
            page_area,
            rail: navigation,
            status,
            breadcrumb,
            sidebar,
            pages: Default::default(),
            viewport: Default::default(),
            current: Rc::new(Cell::new(PageId::Overview)),
            changed: Default::default(),
            source_focus: Default::default(),
        };
        let nav = shell.clone();
        shell.rail.bind(Rc::new(move |id| nav.navigate(id)));
        let viewport = shell.viewport.clone();
        page_area.on_size(move |event| {
            size_page_book(page_area, book, viewport.get());
            if assistant_launcher.is_shown() {
                place_launcher(content, assistant_launcher, viewport.get());
            }
            event.skip(true);
        });
        shell.refresh_navigation();
        size_page_book(page_area, book, shell.viewport.get());
        shell
    }
    pub fn register<W: WxWidget>(&self, id: PageId, page: &W) {
        self.add_page(
            id,
            page,
            std::any::TypeId::of::<W>() == std::any::TypeId::of::<ScrolledWindow>(),
        );
    }
    /// A composite page contains its own page-wide scrolling surface.
    pub fn register_viewport<W: WxWidget>(&self, id: PageId, page: &W) {
        self.add_page(id, page, true);
    }
    fn add_page<W: WxWidget>(&self, id: PageId, page: &W, viewport: bool) {
        assert!(
            !self.pages.borrow().iter().any(|entry| entry.0 == id),
            "duplicate desktop page"
        );
        let first = self.pages.borrow().is_empty();
        self.book.add_page(page, id.title(), first, None);
        self.pages.borrow_mut().push((id, viewport));
        if first {
            self.viewport.set(viewport);
            size_page_book(self.page_area, self.book, viewport);
        }
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
        if self.assistant_launcher.is_shown() == ready {
            return;
        }
        if ready {
            place_launcher(
                self.assistant_host,
                self.assistant_launcher,
                self.viewport.get(),
            );
            self.assistant_launcher.show(true);
            self.assistant_launcher.raise_child_overlay();
        } else {
            self.assistant_launcher.hide();
        }
    }
    /// Bounds of the client-area overlay. The title bar, rail and launcher
    /// remain outside the chat so native window controls stay reachable.
    pub fn assistant_overlay_area(&self) -> Rect {
        let anchor = launcher_bounds(self.assistant_host, self.viewport.get());
        let inset = self.assistant_host.from_dip(theme::SPACE_SM);
        let top = self.assistant_host.from_dip(56);
        Rect::new(
            inset,
            top,
            (anchor.x + anchor.width - inset).max(1),
            (anchor.y - inset - top).max(1),
        )
    }
    pub fn set_dumpling_visible(&self, visible: bool) {
        self.rail.set_dumpling(visible);
        if !visible && self.current() == PageId::Dumpling {
            self.navigate(PageId::Overview);
        } else {
            self.refresh_navigation();
        }
    }
    pub fn navigate(&self, id: PageId) {
        let id = id.visible_destination();
        // General capability management is intentionally hidden. The backend
        // remains available to existing client integrations.
        if matches!(id, PageId::Skills | PageId::Mcp) {
            return;
        }
        let Some((index, (_, viewport))) = self
            .pages
            .borrow()
            .iter()
            .copied()
            .enumerate()
            .find(|(_, v)| v.0 == id)
        else {
            return;
        };
        if id == PageId::Dumpling && !self.rail.dumpling() {
            return;
        }
        let previous = self.book.selection();
        self.book.change_selection(index);
        self.current.set(id);
        self.viewport.set(viewport);
        self.breadcrumb
            .set_label(&format!("{}  /  {}", rail::group_name(id), id.title()));
        self.status.set_label("");
        self.status.show(matches!(id, PageId::Client(_)));
        self.rail.select(id);
        self.refresh_navigation();
        size_page_book(self.page_area, self.book, viewport);
        if self.assistant_launcher.is_shown() {
            place_launcher(self.assistant_host, self.assistant_launcher, viewport);
            self.assistant_launcher.raise_child_overlay();
        }
        if let Some(callback) = self.changed.borrow().clone() {
            callback(id);
        }
        super::motion::page_changed(self.book, previous);
    }
    fn refresh_navigation(&self) {
        self.sidebar.layout();
        self.sidebar.fit_inside();
        self.panel.layout();
    }
}

pub(super) fn inset_page_content<W: WxWidget>(host: &W, body: BoxSizer) {
    let padded = BoxSizer::builder(Orientation::Vertical).build();
    padded.add_sizer(
        &body,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        host.from_dip(theme::PAGE_GUTTER),
    );
    host.set_sizer(padded, true);
}

fn page_gutter(host: Panel, viewport: bool) -> i32 {
    if viewport {
        0
    } else {
        host.from_dip(theme::PAGE_GUTTER)
    }
}

fn size_page_book(host: Panel, book: SimpleBook, viewport: bool) {
    let s = host.get_client_size();
    let inset = page_gutter(host, viewport);
    let width = (s.width - 2 * inset).max(1);
    let height = (s.height - inset).max(1);
    if book.get_position() != Point::new(inset, 0) || book.get_size() != Size::new(width, height) {
        book.set_size_with_pos(inset, 0, width, height);
    }
}

fn launcher_bounds(host: Panel, viewport: bool) -> Rect {
    let size = host.get_client_size();
    let button = host.from_dip(44).min(size.width).min(size.height).max(1);
    // Inside the active page but clear of its right and bottom scrollbar lanes.
    let margin = page_gutter(host, viewport) + host.from_dip(24);
    Rect::new(
        (size.width - margin - button).max(0),
        (size.height - margin - button).max(0),
        button,
        button,
    )
}

fn place_launcher(host: Panel, button: Button, viewport: bool) {
    let r = launcher_bounds(host, viewport);
    if button.get_position() != Point::new(r.x, r.y)
        || button.get_size() != Size::new(r.width, r.height)
    {
        button.set_size_with_pos(r.x, r.y, r.width, r.height);
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
