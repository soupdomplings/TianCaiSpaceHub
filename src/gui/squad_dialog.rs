//! Common account dialog chrome. Business controls stay native and local to
//! their feature; only the body scrolls, keeping feedback and actions visible.
use super::{controls::paint_surface, theme};
use wxdragon::prelude::*;

pub(super) struct SquadDialog {
    pub dialog: Dialog,
    pub body: ScrolledWindow,
    pub content: BoxSizer,
    pub status: StaticText,
    pub actions: BoxSizer,
}

impl SquadDialog {
    /// Preferred size is in DIP, then limited to the containing Hub/work area.
    pub fn new(parent: &impl WxWidget, title: &str, subtitle: &str, size: Size) -> Self {
        let parent_size = parent.get_client_size();
        let mut width = parent.from_dip(size.width);
        let mut height = parent.from_dip(size.height);
        if parent_size.width > 0 {
            width = width.min((parent_size.width - parent.from_dip(16)).max(1));
        }
        if parent_size.height > 0 {
            height = height.min((parent_size.height - parent.from_dip(16)).max(1));
        }
        if let Some((work_width, work_height)) = super::screen_work_area_size() {
            width = width.min((work_width - parent.from_dip(24)).max(1));
            height = height.min((work_height - parent.from_dip(40)).max(1));
        }
        let dialog = Dialog::builder(parent, title)
            .with_size(width, height)
            .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
            .build();
        let t = theme::theme();
        dialog.set_background_color(t.bg_app);
        dialog.set_font(&theme::font(theme::TextRole::Body));
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        let heading = StaticText::builder(&dialog).with_label(title).build();
        heading.set_font(&theme::font(theme::TextRole::PageTitle));
        heading.set_foreground_color(t.ink_primary);
        outer.add(
            &heading,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            dialog.from_dip(theme::PAGE_GUTTER),
        );
        let description = note(&dialog, subtitle);
        description.show(!subtitle.is_empty());
        description.wrap((width - dialog.from_dip(theme::PAGE_GUTTER * 2)).max(1));
        outer.add_spacer(dialog.from_dip(6));
        outer.add(
            &description,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dialog.from_dip(theme::PAGE_GUTTER),
        );
        let body = ScrolledWindow::builder(&dialog)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        body.set_background_color(t.bg_app);
        body.set_min_size(Size::new(0, 0));
        body.set_scroll_rate(0, body.from_dip(12));
        body.show_scroll_indicators(false);
        let content = BoxSizer::builder(Orientation::Vertical).build();
        body.set_sizer(content, false);
        outer.add_spacer(dialog.from_dip(16));
        outer.add(
            &body,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dialog.from_dip(theme::PAGE_GUTTER),
        );
        let status = note(&dialog, "");
        status.set_min_size(Size::new(0, status.get_char_height().max(1)));
        outer.add_spacer(dialog.from_dip(8));
        outer.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dialog.from_dip(theme::PAGE_GUTTER),
        );
        let actions = BoxSizer::builder(Orientation::Horizontal).build();
        actions.add_stretch_spacer(1);
        outer.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dialog.from_dip(theme::PAGE_GUTTER),
        );
        dialog.set_sizer(outer, false);
        let subtitle = subtitle.to_string();
        dialog.on_size(move |event| {
            let width =
                (dialog.get_client_size().width - dialog.from_dip(theme::PAGE_GUTTER * 2)).max(1);
            description.set_label(&subtitle);
            description.wrap(width);
            dialog.layout();
            body.layout();
            body.fit_inside();
            event.skip(true);
        });
        Self {
            dialog,
            body,
            content,
            status,
            actions,
        }
    }

    /// Return the card as the parent for all of its controls.
    pub fn section(&self, title: &str) -> (Panel, BoxSizer) {
        let card = Panel::builder(&self.body).build();
        paint_surface(card, theme::SURFACE_RADIUS);
        let inner = BoxSizer::builder(Orientation::Vertical).build();
        if !title.is_empty() {
            let title = field_label(&card, title);
            title.set_font(&theme::font(theme::TextRole::Title));
            inner.add(&title, 0, SizerFlag::Bottom, card.from_dip(16));
        }
        let padding = BoxSizer::builder(Orientation::Vertical).build();
        padding.add_sizer(
            &inner,
            1,
            SizerFlag::Expand | SizerFlag::All,
            card.from_dip(theme::PANEL_INSET),
        );
        card.set_sizer(padding, false);
        self.content.add(
            &card,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            card.from_dip(12),
        );
        (card, inner)
    }

    /// Call once after controls, actions and their callbacks are in place.
    pub fn finish(&self) {
        self.dialog.layout();
        self.body.layout();
        self.body.fit_inside();
        self.dialog.centre();
    }
}

pub(super) fn field_label(parent: &impl WxWidget, text: &str) -> StaticText {
    let label = StaticText::builder(parent).with_label(text).build();
    label.set_font(&theme::font(theme::TextRole::Body));
    label.set_foreground_color(theme::theme().ink_primary);
    label
}

pub(super) fn note(parent: &impl WxWidget, text: &str) -> StaticText {
    let label = StaticText::builder(parent).with_label(text).build();
    label.set_font(&theme::font(theme::TextRole::Caption));
    label.set_foreground_color(theme::theme().ink_secondary);
    label
}
