use super::super::{
    controls::{Button, ButtonKind, paint_surface},
    theme,
};
use wxdragon::prelude::*;

pub(super) fn card_pair(page: &ScrolledWindow, column: &BoxSizer) -> GridSizer {
    let gap = page.from_dip(theme::SPACE_MD);
    let pair = GridSizer::builder(0, 1)
        .with_hgap(gap)
        .with_vgap(gap)
        .build();
    column.add_sizer(&pair, 0, SizerFlag::Expand | SizerFlag::Bottom, gap);
    pair
}

pub(super) fn resize_pairs(page: ScrolledWindow, pairs: [GridSizer; 2]) {
    let cols = if page.get_client_size().width >= page.from_dip(880) {
        2
    } else {
        1
    };
    if pairs[0].get_cols() != cols {
        for pair in pairs {
            pair.set_cols(cols);
        }
        page.layout();
        page.fit_inside();
    }
}

pub(super) struct SettingsCard {
    pub panel: Panel,
    pub body: BoxSizer,
}

impl SettingsCard {
    pub fn new(page: &ScrolledWindow, pair: &GridSizer, title: &str) -> Self {
        let panel = Panel::builder(page)
            .with_style(PanelStyle::TabTraversal)
            .build();
        panel.set_name(title);
        paint_surface(panel, theme::SURFACE_RADIUS);
        let body = BoxSizer::builder(Orientation::Vertical).build();
        let heading = StaticText::builder(&panel).with_label(title).build();
        heading.set_font(&theme::font(theme::TextRole::Title));
        heading.set_foreground_color(theme::theme().ink_primary);
        body.add(
            &heading,
            0,
            SizerFlag::Bottom,
            panel.from_dip(theme::SPACE_MD),
        );
        let padding = BoxSizer::builder(Orientation::Vertical).build();
        padding.add_sizer(
            &body,
            1,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(theme::PANEL_INSET),
        );
        panel.set_sizer(padding, false);
        pair.add(&panel, 0, SizerFlag::Expand, 0);
        Self { panel, body }
    }

    pub fn row<W: WxWidget>(&self, title: &str, control: &W) {
        let panel = self.panel;
        let label = StaticText::builder(&panel).with_label(title).build();
        label.set_font(&theme::font(theme::TextRole::Body));
        label.set_foreground_color(theme::theme().ink_primary);
        label.set_min_size(Size::new(panel.from_dip(164), -1));
        control.set_name(title);
        let row = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        row.add(
            &label,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            panel.from_dip(theme::SPACE_MD),
        );
        row.add(control, 0, SizerFlag::AlignCenterVertical, 0);
        self.body.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(theme::SPACE_SM),
        );
    }

    pub fn action(&self, title: &str, kind: ButtonKind) -> Button {
        let button = Button::builder(&self.panel)
            .with_label(title)
            .with_kind(kind)
            .build();
        self.body.add(
            &button,
            0,
            SizerFlag::Bottom,
            self.panel.from_dip(theme::SPACE_SM),
        );
        button
    }
}

pub(super) fn caption(panel: &Panel, column: &BoxSizer, text: &str) -> StaticText {
    let hint = StaticText::builder(panel).with_label(text).build();
    hint.set_font(&theme::font(theme::TextRole::Caption));
    hint.set_foreground_color(theme::theme().ink_secondary);
    hint.wrap(panel.from_dip(300));
    column.add(&hint, 0, SizerFlag::Bottom, panel.from_dip(theme::SPACE_SM));
    hint
}
