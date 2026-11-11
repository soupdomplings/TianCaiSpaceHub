//! Production resource cards; also rendered by the isolated native review.
use super::{
    controls::{Button, ButtonKind, paint_surface},
    theme,
    visual::{self, Icon},
};
use crate::desktop::clients::ClientId;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::prelude::*;

#[derive(Clone)]
pub(super) struct ResourceCard {
    pub panel: Panel,
    pub edit: Button,
    pub delete: Button,
    pub manage: Button,
    pub state: StaticText,
}
impl ResourceCard {
    pub fn new(
        parent: &dyn WxWidget,
        name: &str,
        description: &str,
        skill: bool,
        clients: &[(ClientId, String, Option<bool>)],
        scoped: bool,
    ) -> Self {
        let panel = Panel::builder(parent).build();
        paint_surface(panel, theme::SURFACE_RADIUS);
        panel.set_min_size(visual::dip_size(&panel, 280, -1));
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let top = BoxSizer::builder(Orientation::Horizontal).build();
        let symbol = Panel::builder(&panel).build();
        symbol.set_min_size(visual::dip_size(&symbol, 32, 32));
        symbol.set_background_color(theme::theme().bg_card);
        symbol.set_background_style(BackgroundStyle::Paint);
        symbol.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&symbol);
            let dc = wxdragon::dc::GraphicsDC::new(&base);
            let c = visual::Canvas::new(&dc, visual::scale(&symbol));
            c.clear(theme::theme().bg_card);
            c.rounded(Rect::new(0, 0, 32, 32), 9, theme::theme().bg_muted, None);
            c.icon(
                if skill { Icon::Skill } else { Icon::Mcp },
                Rect::new(7, 7, 18, 18),
                theme::theme().accent,
            );
        });
        let title = StaticText::builder(&panel)
            .with_label(name)
            .with_style(StaticTextStyle::EllipsizeEnd)
            .build();
        title.set_font(&theme::font(theme::TextRole::Title));
        title.set_foreground_color(theme::theme().ink_primary);
        title.set_min_size(Size::new(0, -1));
        title.set_tooltip(name);
        top.add(
            &symbol,
            0,
            SizerFlag::Right | SizerFlag::AlignCenterVertical,
            panel.from_dip(8),
        );
        top.add(&title, 1, SizerFlag::AlignCenterVertical, 0);
        let state = StaticText::builder(&panel)
            .with_label(if clients.iter().any(|v| v.2 == Some(true)) {
                "使用中"
            } else if clients.iter().any(|v| v.2.is_none()) {
                "需核对"
            } else {
                "未使用"
            })
            .build();
        state.set_font(&theme::font(theme::TextRole::Caption));
        state.set_foreground_color(if clients.iter().any(|v| v.2 == Some(true)) {
            theme::theme().accent
        } else {
            theme::theme().ink_secondary
        });
        top.add(
            &state,
            0,
            SizerFlag::Left | SizerFlag::AlignCenterVertical,
            panel.from_dip(8),
        );
        col.add_sizer(
            &top,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(theme::PANEL_INSET),
        );
        let short = description.split_whitespace().collect::<Vec<_>>().join(" ");
        let short = if short.chars().count() > 96 {
            short.chars().take(96).collect::<String>() + "…"
        } else {
            short
        };
        let description = StaticText::builder(&panel).with_label(&short).build();
        description.set_foreground_color(theme::theme().ink_secondary);
        description.set_font(&theme::font(theme::TextRole::Caption));
        description.wrap(panel.from_dip(280));
        col.add(
            &description,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            panel.from_dip(theme::PANEL_INSET),
        );
        col.add_stretch_spacer(1);
        let actions = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let edit = Button::builder(&panel)
            .with_label("编辑配置")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(86, 30))
            .build();
        edit.show(!skill);
        let manage = Button::builder(&panel)
            .with_label(if scoped {
                "管理使用"
            } else {
                "管理客户端"
            })
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(104, 32))
            .build();
        manage.set_foreground_color(theme::theme().accent);
        let delete = Button::builder(&panel)
            .with_label(if scoped { "移除" } else { "删除" })
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(58, 30))
            .build();
        delete.set_foreground_color(theme::theme().error);
        delete.set_tooltip(if scoped {
            "从当前客户端移除此项，保留 Hub 中的内容及其他客户端"
        } else {
            "删除 Hub 中的此项，须先从所有使用端移除"
        });
        if !scoped {
            let names = clients
                .iter()
                .filter(|c| c.2 == Some(true))
                .map(|c| c.1.as_str())
                .collect::<Vec<_>>();
            let text = StaticText::builder(&panel)
                .with_label(&if names.is_empty() {
                    "尚未用于客户端".into()
                } else {
                    format!("用于 {}", names.join("、"))
                })
                .build();
            text.set_foreground_color(theme::theme().ink_secondary);
            text.set_font(&theme::font(theme::TextRole::Caption));
            text.wrap(panel.from_dip(280));
            col.add(
                &text,
                0,
                SizerFlag::Left | SizerFlag::Right,
                panel.from_dip(14),
            );
        }
        for b in [manage, edit, delete] {
            actions.add(&b, 0, SizerFlag::Right, panel.from_dip(2));
        }
        col.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(10),
        );
        panel.set_sizer(col, true);
        Self {
            panel,
            edit,
            delete,
            manage,
            state,
        }
    }
    pub fn enable(&self, enabled: bool) {
        for b in [self.edit, self.delete, self.manage] {
            b.enable(enabled);
        }
    }
}

#[derive(Clone)]
pub(super) struct Gallery {
    pub page: ScrolledWindow,
    grid: GridSizer,
    pub cards: Rc<RefCell<Vec<ResourceCard>>>,
    empty: StaticText,
}
impl Gallery {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = ScrolledWindow::builder(parent)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        page.set_background_color(theme::theme().bg_app);
        page.set_scroll_rate(0, page.from_dip(12));
        page.set_min_size(visual::dip_size(&page, 0, 0));
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let empty = StaticText::builder(&page).with_label("正在读取…").build();
        empty.set_foreground_color(theme::theme().ink_secondary);
        col.add(&empty, 0, SizerFlag::All, page.from_dip(24));
        let grid = GridSizer::builder(0, 1)
            .with_hgap(page.from_dip(12))
            .with_vgap(page.from_dip(12))
            .build();
        col.add_sizer(
            &grid,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(4),
        );
        page.set_sizer(col, true);
        let columns = Rc::new(Cell::new(1));
        page.on_size(move |event| {
            let count = ((page.get_client_size().width - page.from_dip(8)) / page.from_dip(332))
                .clamp(1, 3);
            if columns.replace(count) != count {
                grid.set_cols(count);
                page.layout();
                page.fit_inside();
            }
            event.skip(true);
        });
        Self {
            page,
            grid,
            cards: Default::default(),
            empty,
        }
    }
    pub fn clear(&self) {
        self.page.freeze();
        for card in self.cards.borrow_mut().drain(..) {
            card.panel.destroy();
        }
    }
    pub fn push(&self, card: ResourceCard) {
        self.grid.add(&card.panel, 0, SizerFlag::Expand, 0);
        self.cards.borrow_mut().push(card);
    }
    pub fn finish(&self, empty: &str) {
        self.empty.set_label(empty);
        self.empty.show(self.cards.borrow().is_empty());
        self.page.layout();
        self.page.fit_inside();
        self.page.thaw();
    }
}
