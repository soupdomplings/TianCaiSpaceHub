//! Shared native command controls, rendered with the product visual system.
use super::{
    theme,
    visual::{Canvas, Icon, dip_size, scale},
};
use wxdragon::{dc::GraphicsDC, prelude::*, widgets::action_button::ActionButton};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum ButtonKind {
    Secondary,
    Primary,
    Ghost,
    Nav,
    Tab,
    Tile,
    Select,
    Segment,
    Caption,
    CaptionClose,
    CalendarDay,
}
#[derive(Clone, Copy)]
pub(super) struct Button {
    native: ActionButton,
}
impl Button {
    pub fn set_default(&self) {
        self.set_kind(ButtonKind::Primary);
        self.native.set_default();
    }
    pub fn builder(parent: &dyn WxWidget) -> ButtonBuilder<'_> {
        ButtonBuilder {
            parent,
            label: String::new(),
            size: None,
            kind: ButtonKind::Secondary,
            explicit_kind: false,
            id: ID_ANY,
            icon: Icon::None,
        }
    }
    pub fn set_label(&self, label: &str) {
        self.native.set_label(label);
        self.native.refresh(false, None);
    }
    pub fn get_label(&self) -> String {
        self.native.get_label().unwrap_or_default()
    }
    pub fn set_kind(&self, kind: ButtonKind) {
        self.native
            .set_presentation((self.native.presentation() & !0xff) | kind as u32);
    }
    pub fn set_icon(&self, icon: Icon) {
        self.native
            .set_presentation((self.native.presentation() & !0xff00) | ((icon as u32) << 8));
    }
    pub fn set_selected(&self, selected: bool) {
        self.native.set_presentation(
            (self.native.presentation() & !0x30000)
                | if selected { 0x10000 } else { 0 }
                | if theme::motion_enabled() { 0x20000 } else { 0 },
        );
    }
    pub fn selected(&self) -> bool {
        self.native.presentation() & 0x10000 != 0
    }
}
impl WxWidget for Button {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        self.native.handle_ptr()
    }
    fn is_valid(&self) -> bool {
        self.native.is_valid()
    }
}
impl WxEvtHandler for Button {
    unsafe fn get_event_handler_ptr(&self) -> *mut wxdragon::ffi::wxd_EvtHandler_t {
        self.handle_ptr().cast()
    }
}
impl WindowEvents for Button {}
impl ButtonEvents for Button {}

#[cfg(all(test, windows))]
mod motion_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    #[ignore = "isolated offscreen native menu motion fixture"]
    fn menu_selection_motion_is_interruptible_and_reduced_motion_is_immediate() {
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let prior = theme::motion_enabled();
            theme::set_motion_enabled(true);
            let frame = Frame::builder()
                .with_title("synthetic menu motion")
                .with_position(Point::new(-2400, -1800))
                .with_size(Size::new(320, 160))
                .build();
            let panel = Panel::builder(&frame).build();
            let column = BoxSizer::builder(Orientation::Vertical).build();
            let mut buttons = Vec::new();
            for kind in [ButtonKind::Nav, ButtonKind::Tab] {
                let button = Button::builder(&panel)
                    .with_label("合成菜单")
                    .with_kind(kind)
                    .build();
                column.add(&button, 0, SizerFlag::Expand, 0);
                buttons.push(button);
            }
            panel.set_sizer(column, true);
            let outer = BoxSizer::builder(Orientation::Vertical).build();
            outer.add(&panel, 1, SizerFlag::Expand, 0);
            frame.set_sizer(outer, true);
            frame.show(true);
            for button in buttons {
                let state = || button.native.visual_state();
                let set = |selected: bool, motion: bool| {
                    theme::set_motion_enabled(motion);
                    button.set_selected(selected);
                };
                let size = button.get_size();
                set(true, true);
                std::thread::sleep(Duration::from_millis(40));
                let forward = (state() >> 8) & 0xff;
                assert!(forward > 0 && forward < 255);
                set(false, true);
                let reversed = (state() >> 8) & 0xff;
                assert!(
                    forward.abs_diff(reversed) < 24,
                    "reversal must start at the current colour"
                );
                std::thread::sleep(Duration::from_millis(35));
                assert!(((state() >> 8) & 0xff) < reversed);
                set(true, true);
                std::thread::sleep(Duration::from_millis(180));
                assert_eq!((state() >> 8) & 0xff, 255);
                set(false, false);
                assert_eq!((state() >> 8) & 0xff, 0);
                assert_eq!(button.get_size(), size);
            }
            theme::set_motion_enabled(false);
            let direct = Button::builder(&panel).with_kind(ButtonKind::Nav).build();
            direct.set_selected(true);
            assert!(direct.selected());
            assert_eq!((direct.native.visual_state() >> 8) & 0xff, 255);
            theme::set_motion_enabled(prior);
            frame.destroy();
        })
        .unwrap();
    }
}
pub(super) struct ButtonBuilder<'a> {
    parent: &'a dyn WxWidget,
    label: String,
    size: Option<Size>,
    kind: ButtonKind,
    explicit_kind: bool,
    id: i64,
    icon: Icon,
}
impl<'a> ButtonBuilder<'a> {
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = label.into();
        self
    }
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }
    pub fn with_style(mut self, style: ButtonStyle) -> Self {
        if style.contains(ButtonStyle::BorderNone) {
            self.kind = ButtonKind::Ghost;
            self.explicit_kind = true;
        }
        self
    }
    pub fn with_kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self.explicit_kind = true;
        self
    }
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
    }
    pub fn with_id(mut self, id: impl Into<i64>) -> Self {
        self.id = id.into();
        self
    }
    pub fn build(self) -> Button {
        let kind = if !self.explicit_kind
            && self.kind == ButtonKind::Secondary
            && ["保存", "添加", "导入", "登录", "确认", "复用"]
                .iter()
                .any(|p| self.label.starts_with(p))
        {
            ButtonKind::Primary
        } else {
            self.kind
        };
        let native = ActionButton::new(self.parent, self.id as i32, &self.label);
        native.set_font(&theme::font(theme::TextRole::Body));
        native.set_background_color(self.parent.get_background_color());
        native.set_foreground_color(theme::theme().ink_primary);
        native.set_presentation(kind as u32 | ((self.icon as u32) << 8));
        let button = Button { native };
        let size = self.size.unwrap_or(Size::new(
            (self
                .label
                .chars()
                .map(|c| if c.is_ascii() { 7 } else { 14 })
                .sum::<i32>()
                + 32
                + if self.icon == Icon::None { 0 } else { 28 })
            .max(72),
            theme::CONTROL_HEIGHT,
        ));
        button.set_min_size(dip_size(&button, size.width, size.height));
        button.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&button);
            let dc = GraphicsDC::new(&base);
            let scale = scale(&button);
            let size = button.get_client_size();
            let c = Canvas::new(&dc, scale);
            c.clear(
                button
                    .get_parent()
                    .map(|p| p.get_background_color())
                    .unwrap_or(theme::theme().bg_app),
            );
            let p = native.presentation();
            draw_button(
                &c,
                Rect::new(
                    0,
                    0,
                    (size.width as f64 / scale).round() as i32,
                    (size.height as f64 / scale).round() as i32,
                ),
                &button.get_label(),
                p,
                native.visual_state(),
                button.get_foreground_color(),
            );
        });
        let b = button;
        button.on_size(move |e| {
            b.refresh(false, None);
            e.skip(true);
        });

        button
    }
}

pub(super) fn draw_button(
    c: &Canvas<'_>,
    r: Rect,
    label: &str,
    p: u32,
    state: u32,
    foreground: Colour,
) {
    let t = theme::theme();
    let kind = p & 0xff;
    let selected = p & 0x10000 != 0;
    let transition = matches!(kind, 3 | 4 | 7);
    let selection = if transition && state & 0x10000 != 0 {
        ((state >> 8) & 0xff) as f64 / 255.0
    } else if selected {
        1.0
    } else {
        0.0
    };
    let hover = state & 1 != 0;
    let pressed = state & 2 != 0;
    let focus = state & 4 != 0;
    let enabled = state & 8 != 0;
    let icon = Icon::from_code((p >> 8) & 0xff);
    if kind == ButtonKind::Caption as u32 || kind == ButtonKind::CaptionClose as u32 {
        let closing = kind == ButtonKind::CaptionClose as u32 && (hover || pressed);
        c.rounded(
            r,
            0,
            if closing {
                t.error
            } else if hover || pressed {
                t.bg_muted
            } else {
                t.bg_card
            },
            None,
        );
        if focus {
            c.rounded(
                Rect::new(r.x + 5, r.y + 5, r.width - 10, r.height - 10),
                5,
                if closing { t.error } else { t.bg_card },
                Some(t.accent),
            );
        }
        c.icon(
            icon,
            Rect::new(r.x + (r.width - 14) / 2, r.y + (r.height - 14) / 2, 14, 14),
            if closing {
                t.on_accent
            } else {
                t.ink_secondary
            },
        );
        return;
    }
    let fill = if !enabled {
        t.bg_card_alt
    } else if kind == ButtonKind::Primary as u32 {
        if pressed || hover {
            t.accent_hover
        } else {
            t.accent
        }
    } else if selected {
        t.bg_muted
    } else if hover || pressed {
        t.bg_card_alt
    } else if kind == ButtonKind::Secondary as u32 || kind == ButtonKind::Tile as u32 {
        if kind == ButtonKind::Tile as u32 {
            t.bg_card
        } else {
            t.bg_card_alt
        }
    } else {
        t.bg_app
    };
    let text = if !enabled {
        t.ink_muted
    } else if kind == ButtonKind::Primary as u32 {
        t.on_accent
    } else if selected {
        t.accent
    } else {
        foreground
    };
    let blend = |from: Colour, to: Colour, amount: f64| {
        let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * amount).round() as u8;
        Colour::rgb(mix(from.r, to.r), mix(from.g, to.g), mix(from.b, to.b))
    };
    let fill = if transition && enabled {
        blend(
            if hover || pressed {
                t.bg_card_alt
            } else {
                t.bg_card
            },
            t.bg_muted,
            selection,
        )
    } else {
        fill
    };
    let text = if transition && enabled {
        blend(foreground, t.accent, selection)
    } else {
        text
    };
    let rr = Rect::new(
        r.x + 1,
        r.y + 1,
        (r.width - 2).max(1),
        (r.height - 2).max(1),
    );
    if kind != ButtonKind::Tab as u32
        && !((kind == ButtonKind::Ghost as u32
            || kind == ButtonKind::Nav as u32
            || kind == ButtonKind::CalendarDay as u32)
            && selection == 0.0
            && !hover
            && !focus
            && enabled)
    {
        let background = if kind == ButtonKind::Nav as u32 && selection == 0.0 && !hover {
            t.bg_card
        } else {
            fill
        };
        c.rounded(
            rr,
            if kind == ButtonKind::Tile as u32 {
                theme::SURFACE_RADIUS
            } else {
                theme::BUTTON_RADIUS
            },
            background,
            if kind == ButtonKind::Segment as u32 && selected {
                Some(t.accent)
            } else if focus && kind != ButtonKind::Segment as u32 {
                Some(t.accent)
            } else if kind == ButtonKind::Tile as u32 && selected {
                Some(t.accent)
            } else {
                None
            },
        );
    }
    if kind == ButtonKind::Tab as u32 {
        if hover || focus {
            c.rounded(
                Rect::new(r.x + 2, r.y + 2, r.width - 4, r.height - 6),
                theme::BUTTON_RADIUS,
                t.bg_card_alt,
                if focus { Some(t.accent) } else { None },
            );
        }
        c.label(
            label,
            Rect::new(r.x + 2, r.y, r.width - 4, r.height - 4),
            13,
            selected,
            text,
            true,
        );
        if selection > 0.0 {
            c.font(13, true);
            let text_width = (c.dc.get_text_extent(label).0 as f64 / c.scale).ceil() as i32;
            let width = text_width.clamp(20, (r.width - 16).max(20));
            c.rounded(
                Rect::new(r.x + (r.width - width) / 2, r.y + r.height - 3, width, 2),
                1,
                blend(t.bg_card, t.accent, selection),
                None,
            );
        }
    } else if kind == ButtonKind::Select as u32 {
        c.rounded(
            rr,
            7,
            if enabled { t.bg_card } else { t.bg_card_alt },
            Some(if focus {
                t.accent
            } else if hover && enabled {
                t.border
            } else {
                t.divider
            }),
        );
        c.label(
            label,
            Rect::new(r.x + 12, r.y, r.width - 38, r.height),
            13,
            false,
            text,
            false,
        );
        let x = r.x + r.width - 18;
        let y = r.y + r.height / 2;
        c.line(Point::new(x - 4, y - 2), Point::new(x, y + 2), text, false);
        c.line(Point::new(x, y + 2), Point::new(x + 4, y - 2), text, false);
    } else if kind == ButtonKind::Tile as u32 || label.contains('\n') {
        let name = label.lines().last().unwrap_or(label);
        let glyph = if icon == Icon::None {
            Icon::Model
        } else {
            icon
        };
        c.icon(
            glyph,
            Rect::new(r.x + (r.width - 22) / 2, r.y + 9, 22, 22),
            text,
        );
        c.label(
            name,
            Rect::new(r.x + 5, r.y + 33, r.width - 10, r.height - 36),
            11,
            selected,
            text,
            true,
        );
        if selected {
            c.dot(r.x + r.width - 9, r.y + 9, 3, t.ok);
        }
    } else {
        let has_icon = icon != Icon::None;
        if has_icon && label.is_empty() {
            c.icon(
                icon,
                Rect::new(r.x + (r.width - 24) / 2, r.y + (r.height - 24) / 2, 24, 24),
                text,
            );
            return;
        }
        if has_icon {
            c.icon(
                icon,
                Rect::new(r.x + 12, r.y + (r.height - 17) / 2, 17, 17),
                text,
            );
        }
        c.label(
            label,
            Rect::new(
                r.x + if has_icon {
                    39
                } else if kind == ButtonKind::CalendarDay as u32 {
                    4
                } else {
                    12
                },
                r.y,
                r.width
                    - if has_icon {
                        49
                    } else if kind == ButtonKind::CalendarDay as u32 {
                        8
                    } else {
                        24
                    },
                r.height,
            ),
            13,
            selected,
            text,
            kind != ButtonKind::Nav as u32 && !has_icon,
        );
        if kind == ButtonKind::Nav as u32 && selection > 0.0 {
            c.rounded(
                Rect::new(r.x + 2, r.y + 10, 3, (r.height - 20).max(8)),
                2,
                blend(fill, t.accent, selection),
                None,
            );
        }
        if kind == ButtonKind::Segment as u32 && focus {
            // A small inner focus mark stays distinct from the accent border
            // used for the selected statistics period.
            c.rounded(
                Rect::new(r.x + r.width / 2 - 8, r.y + r.height - 6, 16, 2),
                1,
                if selected { t.accent } else { t.ink_secondary },
                None,
            );
        }
    }
}

pub(super) fn paint_surface(panel: Panel, radius: i32) {
    super::visual::redraw_on_resize(panel);
    panel.set_background_style(BackgroundStyle::Paint);
    panel.set_background_color(theme::theme().bg_card);
    panel.on_paint(move |_| {
        let base = AutoBufferedPaintDC::new(&panel);
        let dc = GraphicsDC::new(&base);
        let scale = scale(&panel);
        let size = panel.get_client_size();
        let c = Canvas::new(&dc, scale);
        c.clear(
            panel
                .get_parent()
                .map(|p| p.get_background_color())
                .unwrap_or(theme::theme().bg_app),
        );
        c.rounded(
            Rect::new(
                0,
                0,
                (size.width as f64 / scale) as i32,
                (size.height as f64 / scale) as i32,
            ),
            radius,
            theme::theme().bg_card,
            None,
        );
    });
}

/// A compact rendered selector backed by a real wxChoice. The item menu and
/// selection events remain native; the hidden child is never cast to a button.
#[derive(Clone, Copy)]
pub(super) struct Choice {
    button: Button,
    native: wxdragon::widgets::choice::Choice,
}
impl Choice {
    pub fn builder(parent: &dyn WxWidget) -> ChoiceBuilder<'_> {
        ChoiceBuilder {
            parent,
            choices: vec![],
            size: Size::new(190, 34),
            selection: None,
            style: ChoiceStyle::Default,
            id: ID_ANY as i32,
        }
    }
    fn update_label(&self) {
        self.button.set_label(
            &self
                .native
                .get_string_selection()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "请选择".into()),
        );
    }
    pub fn set_selection(&self, i: u32) {
        self.native.set_selection(i);
        self.update_label();
    }
    pub fn get_selection(&self) -> Option<u32> {
        self.native.get_selection()
    }
    pub fn get_string_selection(&self) -> Option<String> {
        self.native.get_string_selection()
    }
    pub fn get_string(&self, i: u32) -> Option<String> {
        self.native.get_string(i)
    }
    pub fn get_count(&self) -> u32 {
        self.native.get_count()
    }
    pub fn get_value(&self) -> String {
        self.get_string_selection().unwrap_or_default()
    }
    pub fn set_value(&self, s: &str) {
        if s.is_empty() {
            return;
        }
        let i = (0..self.get_count()).find(|i| self.get_string(*i).as_deref() == Some(s));
        let index = i.unwrap_or_else(|| {
            let n = self.get_count();
            self.append(s);
            n
        });
        self.set_selection(index);
    }
    pub fn append(&self, s: &str) {
        self.native.append(s);
        self.update_label();
    }
    pub fn insert(&self, s: &str, pos: usize) {
        self.native.insert(s, pos);
        self.update_label();
    }
    pub fn clear(&self) {
        self.native.clear();
        self.update_label();
    }
    pub fn on_selection_changed<F: Fn(wxdragon::widgets::choice::ChoiceEventData) + 'static>(
        &self,
        f: F,
    ) {
        let this = *self;
        self.native.on_selection_changed(move |event| {
            this.update_label();
            f(event);
        });
    }
    pub fn popup(&self) {
        self.native.popup_for(&self.button);
        self.update_label();
    }
}
impl WxWidget for Choice {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        self.button.handle_ptr()
    }
    fn is_valid(&self) -> bool {
        self.button.is_valid()
    }
}
impl WxEvtHandler for Choice {
    unsafe fn get_event_handler_ptr(&self) -> *mut wxdragon::ffi::wxd_EvtHandler_t {
        self.button.handle_ptr().cast()
    }
}
impl WindowEvents for Choice {}
pub(super) struct ChoiceBuilder<'a> {
    parent: &'a dyn WxWidget,
    choices: Vec<String>,
    size: Size,
    selection: Option<u32>,
    style: ChoiceStyle,
    id: i32,
}
impl ChoiceBuilder<'_> {
    pub fn with_choices(mut self, choices: Vec<String>) -> Self {
        self.choices = choices;
        self
    }
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    pub fn with_selection(mut self, value: impl Into<Option<u32>>) -> Self {
        self.selection = value.into();
        self
    }
    pub fn with_style(mut self, style: ChoiceStyle) -> Self {
        self.style = style;
        self
    }
    pub fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
    }
    pub fn build(self) -> Choice {
        let button = Button::builder(self.parent)
            .with_label("请选择")
            .with_kind(ButtonKind::Select)
            .with_size(Size::new(self.size.width.max(90), self.size.height.max(34)))
            .build();
        let native = wxdragon::widgets::choice::Choice::builder(&button)
            .with_id(self.id)
            .with_style(self.style)
            .with_choices(self.choices)
            .build();
        native.hide();
        let value = Choice { button, native };
        if let Some(i) = self.selection {
            value.set_selection(i);
        }
        value.update_label();
        button.on_click(move |_| value.popup());
        // Arrow keys work without opening a menu. Tab/Shift+Tab retain the
        // normal native focus order and Escape dismisses the native menu.
        button.on_key_down(move |e| {
            let WindowEventData::Keyboard(ref event) = e else {
                e.skip(true);
                return;
            };
            let Some(key) = event.get_key_code() else {
                e.skip(true);
                return;
            };
            if key == 317 || key == 315 {
                // WXK_DOWN / WXK_UP
                let count = value.get_count();
                if count > 0 {
                    let current = value.get_selection().unwrap_or(0);
                    let index = if key == 317 {
                        current.saturating_add(1).min(count - 1)
                    } else {
                        current.saturating_sub(1)
                    };
                    value.native.select_and_notify(index);
                    value.update_label();
                }
            } else {
                e.skip(true);
            }
        });
        value
    }
}

/// Padded, rounded surface around the native editor. IME, selection, password,
/// undo and text events remain those of wxTextCtrl, including multiline JSON.
#[derive(Clone, Copy)]
pub(super) struct TextCtrl {
    host: Panel,
    native: wxdragon::widgets::textctrl::TextCtrl,
}
impl TextCtrl {
    pub fn builder(parent: &dyn WxWidget) -> TextCtrlBuilder<'_> {
        TextCtrlBuilder {
            parent,
            value: String::new(),
            size: Size::new(-1, -1),
            style: TextCtrlStyle::Default,
            id: ID_ANY as i32,
        }
    }
    pub fn get_value(&self) -> String {
        self.native.get_value()
    }
    pub fn set_hint(&self, hint: &str) {
        self.native.set_hint(hint);
    }
    pub fn set_value(&self, s: &str) {
        self.native.set_value(s);
    }
    pub fn change_value(&self, s: &str) {
        self.native.change_value(s);
    }
    pub fn clear(&self) {
        self.native.clear();
    }
    pub fn append_text(&self, s: &str) {
        self.native.append_text(s);
    }
    pub fn set_editable(&self, value: bool) {
        self.native.set_editable(value);
    }
    pub fn is_editable(&self) -> bool {
        self.native.is_editable()
    }
    pub fn set_insertion_point_end(&self) {
        self.native.set_insertion_point_end();
    }
    pub fn set_insertion_point(&self, pos: i64) {
        self.native.set_insertion_point(pos);
    }
    pub fn set_selection(&self, from: i64, to: i64) {
        self.native.set_selection(from, to);
    }
    pub fn get_selection(&self) -> (i64, i64) {
        self.native.get_selection()
    }
    pub fn select_all(&self) {
        self.native.select_all();
    }
    pub fn set_max_length(&self, len: usize) {
        self.native.set_max_length(len);
    }
    pub fn set_focus(&self) {
        self.native.set_focus();
    }
    pub fn set_background_color(&self, c: Colour) {
        self.host.set_background_color(c);
        self.native.set_background_color(c);
        self.host.refresh(false, None);
    }
    pub fn set_foreground_color(&self, c: Colour) {
        self.native.set_foreground_color(c);
    }
    pub fn set_font(&self, f: &wxdragon::font::Font) {
        self.host.set_font(f);
        self.native.set_font(f);
        self.set_min_size(self.host.get_min_size());
    }
    /// Legacy forms may request a 30 physical-pixel row. Preserve sufficient
    /// room for the real editor, its platform font metrics and scaled padding.
    pub fn set_min_size(&self, size: Size) {
        let minimum = self.native.get_best_size().height + self.host.from_dip(12);
        self.host.set_min_size(Size::new(
            size.width,
            size.height
                .max(minimum)
                .max(self.host.from_dip(theme::CONTROL_HEIGHT)),
        ));
    }
    pub(super) fn editor_size(&self) -> Size {
        self.native.get_client_size()
    }
    pub(super) fn editor_best_size(&self) -> Size {
        self.native.get_best_size()
    }
    pub fn set_tooltip(&self, text: &str) {
        self.host.set_tooltip(text);
        self.native.set_tooltip(text);
        self.native.set_name(text);
    }
    pub fn on_text_changed<F: Fn(wxdragon::widgets::textctrl::TextCtrlEventData) + 'static>(
        &self,
        f: F,
    ) {
        self.native.on_text_changed(f);
    }
    pub fn on_text_enter<F: Fn(wxdragon::widgets::textctrl::TextCtrlEventData) + 'static>(
        &self,
        f: F,
    ) {
        self.native.on_text_enter(f);
    }
    pub fn on_text_updated<F: Fn(wxdragon::event::text_events::TextEventData) + 'static>(
        &self,
        f: F,
    ) {
        self.native.on_text_updated(f);
    }
}
impl WxWidget for TextCtrl {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        self.host.handle_ptr()
    }
    fn is_valid(&self) -> bool {
        self.host.is_valid()
    }
}
impl WxEvtHandler for TextCtrl {
    unsafe fn get_event_handler_ptr(&self) -> *mut wxdragon::ffi::wxd_EvtHandler_t {
        self.native.handle_ptr().cast()
    }
}
impl WindowEvents for TextCtrl {}
pub(super) struct TextCtrlBuilder<'a> {
    parent: &'a dyn WxWidget,
    value: String,
    size: Size,
    style: TextCtrlStyle,
    id: i32,
}
impl TextCtrlBuilder<'_> {
    pub fn with_value(mut self, value: &str) -> Self {
        self.value = value.into();
        self
    }
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    pub fn with_style(mut self, style: TextCtrlStyle) -> Self {
        self.style = style;
        self
    }
    pub fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
    }
    pub fn build(self) -> TextCtrl {
        let host = Panel::builder(self.parent)
            .with_style(PanelStyle::BorderNone)
            .build();
        host.set_background_style(BackgroundStyle::Paint);
        host.set_background_color(theme::theme().bg_card);
        let multi = self.style.contains(TextCtrlStyle::MultiLine);
        let native = wxdragon::widgets::textctrl::TextCtrl::builder(&host)
            .with_id(self.id)
            .with_style(self.style | TextCtrlStyle::BorderNone)
            .with_value(&self.value)
            .build();
        native.set_font(&theme::font(theme::TextRole::Body));
        native.set_background_color(theme::theme().bg_card);
        native.set_foreground_color(theme::theme().ink_primary);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(
            &native,
            1,
            (if multi {
                SizerFlag::Expand
            } else {
                SizerFlag::AlignCenterVertical
            }) | SizerFlag::Left
                | SizerFlag::Right,
            host.from_dip(10),
        );
        col.add_sizer(
            &row,
            1,
            SizerFlag::Expand | SizerFlag::Top | SizerFlag::Bottom,
            host.from_dip(if multi { 9 } else { 6 }),
        );
        host.set_sizer(col, true);
        let value = TextCtrl { host, native };
        value.set_min_size(dip_size(
            &host,
            if self.size.width > 0 {
                self.size.width
            } else {
                100
            },
            self.size.height.max(if multi { 90 } else { 34 }),
        ));
        host.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&host);
            let dc = GraphicsDC::new(&base);
            let s = scale(&host);
            let c = Canvas::new(&dc, s);
            let size = host.get_client_size();
            c.clear(
                host.get_parent()
                    .map(|p| p.get_background_color())
                    .unwrap_or(theme::theme().bg_app),
            );
            c.rounded(
                Rect::new(
                    1,
                    1,
                    (size.width as f64 / s) as i32 - 2,
                    (size.height as f64 / s) as i32 - 2,
                ),
                theme::BUTTON_RADIUS,
                host.get_background_color(),
                Some(if native.has_focus() {
                    theme::theme().accent
                } else {
                    theme::theme().border
                }),
            );
        });
        super::visual::redraw_on_resize(host);
        for focus in [true, false] {
            if focus {
                native.on_set_focus(move |e| {
                    host.refresh(false, None);
                    e.skip(true);
                });
            } else {
                native.on_kill_focus(move |e| {
                    host.refresh(false, None);
                    e.skip(true);
                });
            }
        }
        value
    }
}
