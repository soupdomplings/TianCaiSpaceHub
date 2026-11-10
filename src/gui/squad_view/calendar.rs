//! Themed, anchored date selection. API values remain ISO dates, never display labels.
use super::*;
use wxdragon::widgets::transient_popup::TransientPopup;

#[derive(Clone, Copy)]
pub(crate) struct DateField {
    button: Button,
    value: wxdragon::widgets::textctrl::TextCtrl,
}
impl WxWidget for DateField {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        self.button.handle_ptr()
    }
    fn is_valid(&self) -> bool {
        self.button.is_valid()
    }
}
impl DateField {
    pub(super) fn new(parent: &dyn WxWidget, label: &str) -> Self {
        let button = Button::builder(parent)
            .with_label("不限日期")
            .with_icon(Icon::Calendar)
            .with_kind(ButtonKind::Secondary)
            .with_size(Size::new(152, theme::CONTROL_HEIGHT))
            .build();
        button.set_name(label);
        let value = wxdragon::widgets::textctrl::TextCtrl::builder(&button).build();
        value.hide();
        let field = Self { button, value };
        let calendar = Calendar::new(field);
        button.on_click(move |_| {
            calendar.open();
        });
        field
    }
    pub fn get_value(&self) -> String {
        self.value.get_value()
    }
    pub fn change_value(&self, value: &str) {
        let value = if filters::valid_date(value) {
            value
        } else {
            ""
        };
        self.value.change_value(value);
        self.update_label();
    }
    fn choose(&self, value: &str) {
        // Native text events preserve the existing filter callback contract.
        self.button.set_label(if value.is_empty() {
            "不限日期"
        } else {
            value
        });
        self.value.set_value(value);
    }
    fn update_label(&self) {
        let value = self.get_value();
        self.button.set_label(if value.is_empty() {
            "不限日期"
        } else {
            &value
        });
    }
    pub fn on_date_changed(
        &self,
        f: impl Fn(wxdragon::widgets::textctrl::TextCtrlEventData) + 'static,
    ) {
        self.value.on_text_changed(f);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Date {
    year: i32,
    month: i32,
    day: i32,
}
impl Date {
    fn parse(s: &str) -> Option<Self> {
        filters::valid_date(s).then(|| Self {
            year: s[..4].parse().unwrap(),
            month: s[5..7].parse().unwrap(),
            day: s[8..].parse().unwrap(),
        })
    }
    fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    fn today() -> Self {
        let dt = wxdragon::datetime::DateTime::now();
        Self {
            year: dt.year(),
            month: dt.month() as i32,
            day: dt.day() as i32,
        }
    }
    fn month_days(self) -> i32 {
        match self.month {
            4 | 6 | 9 | 11 => 30,
            2 if self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0) => 29,
            2 => 28,
            _ => 31,
        }
    }
    fn months(self, delta: i32) -> Self {
        let index = ((self.year - 1) * 12 + self.month - 1 + delta).clamp(0, 9999 * 12 - 1);
        let mut date = Self {
            year: index / 12 + 1,
            month: index % 12 + 1,
            day: self.day,
        };
        date.day = date.day.min(date.month_days());
        date
    }
    fn days(self, delta: i32) -> Self {
        let mut date = self;
        for _ in 0..delta.abs() {
            if delta > 0 {
                if date.day < date.month_days() {
                    date.day += 1;
                } else if date.year < 9999 || date.month < 12 {
                    date = date.months(1);
                    date.day = 1;
                }
            } else if date.day > 1 {
                date.day -= 1;
            } else if date.year > 1 || date.month > 1 {
                date = date.months(-1);
                date.day = date.month_days();
            }
        }
        date
    }
    /// Monday first, matching the displayed weekday headings.
    fn month_offset(self) -> usize {
        let offsets = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = self.year - if self.month < 3 { 1 } else { 0 };
        ((y + y / 4 - y / 100 + y / 400 + offsets[(self.month - 1) as usize] + 1 + 6) % 7) as usize
    }
}

#[derive(Clone)]
struct Calendar {
    popup: TransientPopup,
    panel: Panel,
    field: DateField,
    title: Button,
    book: SimpleBook,
    days_host: Panel,
    days: Vec<Button>,
    months: Vec<Button>,
    date: Rc<Cell<Date>>,
    month_mode: Rc<Cell<bool>>,
}
impl Calendar {
    fn new(field: DateField) -> Self {
        let popup = TransientPopup::new(&field.button);
        popup.set_background_color(theme::theme().bg_card);
        let panel = Panel::builder(&popup)
            .with_style(PanelStyle::TabTraversal)
            .build();
        panel.set_background_color(theme::theme().bg_card);
        panel.set_background_style(BackgroundStyle::Paint);
        visual::redraw_on_resize(panel);
        panel.on_paint(move |_| {
            let base = AutoBufferedPaintDC::new(&panel);
            let dc = wxdragon::dc::GraphicsDC::new(&base);
            let scale = visual::scale(&panel);
            let c = Canvas::new(&dc, scale);
            let size = panel.get_client_size();
            let t = theme::theme();
            c.clear(t.bg_card);
            c.rounded(
                Rect::new(
                    0,
                    0,
                    (size.width as f64 / scale) as i32 - 1,
                    (size.height as f64 / scale) as i32 - 1,
                ),
                10,
                t.bg_card,
                Some(t.divider),
            );
        });
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let header = BoxSizer::builder(Orientation::Horizontal).build();
        let prev = Button::builder(&panel)
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(34, 34))
            .build();
        prev.set_tooltip("上一月；选择月份时为上一年");
        let title = Button::builder(&panel)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(188, 34))
            .build();
        title.set_tooltip("选择年份和月份");
        let next = Button::builder(&panel)
            .with_icon(Icon::Chevron)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(34, 34))
            .build();
        next.set_tooltip("下一月；选择月份时为下一年");
        header.add(&prev, 0, SizerFlag::default(), 0);
        header.add(&title, 1, SizerFlag::Expand, 0);
        header.add(&next, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &header,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(10),
        );
        let book = SimpleBook::builder(&panel).build();
        book.set_min_size(visual::dip_size(&book, 276, 250));
        let days_host = Panel::builder(&book).build();
        days_host.set_background_color(theme::theme().bg_card);
        let grid = GridSizer::builder(0, 7)
            .with_vgap(panel.from_dip(4))
            .with_hgap(panel.from_dip(4))
            .build();
        for name in ["一", "二", "三", "四", "五", "六", "日"] {
            let label = StaticText::builder(&days_host)
                .with_label(name)
                .with_style(StaticTextStyle::AlignCenterHorizontal)
                .build();
            label.set_foreground_color(theme::theme().ink_secondary);
            label.set_font(&theme::font(theme::TextRole::Caption));
            grid.add(&label, 0, SizerFlag::AlignCentre, 0);
        }
        let mut days = vec![];
        for _ in 0..42 {
            let day = Button::builder(&days_host)
                .with_kind(ButtonKind::CalendarDay)
                .with_size(Size::new(36, 32))
                .build();
            grid.add(
                &day,
                0,
                SizerFlag::Expand | SizerFlag::ReserveSpaceEvenIfHidden,
                0,
            );
            days.push(day);
        }
        days_host.set_sizer(grid, false);
        let months_host = Panel::builder(&book).build();
        months_host.set_background_color(theme::theme().bg_card);
        let grid = GridSizer::builder(0, 3)
            .with_vgap(panel.from_dip(8))
            .with_hgap(panel.from_dip(8))
            .build();
        let mut months = vec![];
        for i in 1..=12 {
            let b = Button::builder(&months_host)
                .with_label(&format!("{i} 月"))
                .with_kind(ButtonKind::Ghost)
                .with_size(Size::new(80, 48))
                .build();
            grid.add(&b, 0, SizerFlag::Expand, 0);
            months.push(b);
        }
        months_host.set_sizer(grid, false);
        book.add_page(&days_host, "日期", true, None);
        book.add_page(&months_host, "月份", false, None);
        col.add(&book, 0, SizerFlag::Expand, 0);
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        let clear = Button::builder(&panel)
            .with_label("清除")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(68, 32))
            .build();
        let today = Button::builder(&panel)
            .with_label("今天")
            .with_kind(ButtonKind::Secondary)
            .with_size(Size::new(68, 32))
            .build();
        footer.add(&clear, 0, SizerFlag::default(), 0);
        footer.add_stretch_spacer(1);
        footer.add(&today, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(10),
        );
        let padding = BoxSizer::builder(Orientation::Vertical).build();
        padding.add_sizer(
            &col,
            1,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(14),
        );
        panel.set_sizer(padding, false);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        outer.add(&panel, 1, SizerFlag::Expand, 0);
        popup.set_sizer_and_fit(outer, false);
        let ui = Self {
            popup,
            panel,
            field,
            title,
            book,
            days_host,
            days,
            months,
            date: Rc::new(Cell::new(Date::today())),
            month_mode: Default::default(),
        };
        let u = ui.clone();
        prev.on_click(move |_| u.shift(-1));
        let u = ui.clone();
        next.on_click(move |_| u.shift(1));
        let u = ui.clone();
        title.on_click(move |_| {
            u.month_mode.set(!u.month_mode.get());
            u.render();
        });
        let u = ui.clone();
        clear.on_click(move |_| u.choose(""));
        let u = ui.clone();
        today.on_click(move |_| u.choose(&Date::today().iso()));
        for (i, button) in ui.days.iter().copied().enumerate() {
            let u = ui.clone();
            button.on_click(move |_| {
                if let Some(date) = u.day_at(i) {
                    u.choose(&date.iso());
                }
            });
            let u = ui.clone();
            button.on_key_down(move |event| {
                let WindowEventData::Keyboard(ref key) = event else {
                    event.skip(true);
                    return;
                };
                let delta = match key.get_key_code() {
                    Some(314) => -1,
                    Some(316) => 1,
                    Some(315) => -7,
                    Some(317) => 7,
                    _ => {
                        event.skip(true);
                        return;
                    }
                };
                if let Some(date) = u.day_at(i) {
                    u.date.set(date.days(delta));
                    u.render();
                    u.focus_day();
                }
            });
        }
        for (i, button) in ui.months.iter().copied().enumerate() {
            let u = ui.clone();
            button.on_click(move |_| {
                let date = u.date.get();
                u.date.set(date.months(i as i32 + 1 - date.month));
                u.month_mode.set(false);
                u.render();
                u.focus_day();
            });
        }
        // A native popup already dismisses on outside click. Escape from any
        // child is also handled explicitly, including month navigation.
        for button in [prev, next, title, today, clear]
            .into_iter()
            .chain(ui.days.iter().copied())
            .chain(ui.months.iter().copied())
        {
            button.on_key_down(move |event| {
                if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) { popup.dismiss(); field.button.set_focus(); }
                else { event.skip(true); }
            });
        }
        ui.render();
        ui
    }
    fn open(&self) {
        self.date
            .set(Date::parse(&self.field.get_value()).unwrap_or_else(Date::today));
        self.month_mode.set(false);
        self.render();
        let focus = self.days[self.date.get().month_offset() + self.date.get().day as usize - 1];
        self.popup.popup_for(&self.field.button, &focus);
    }
    fn shift(&self, delta: i32) {
        self.date.set(
            self.date
                .get()
                .months(delta * if self.month_mode.get() { 12 } else { 1 }),
        );
        self.render();
    }
    fn day_at(&self, i: usize) -> Option<Date> {
        let date = self.date.get();
        let day = i as i32 - date.month_offset() as i32 + 1;
        (day > 0 && day <= date.month_days()).then_some(Date { day, ..date })
    }
    fn choose(&self, value: &str) {
        let restore_focus = self.popup.is_shown();
        self.popup.dismiss();
        self.field.choose(value);
        if restore_focus {
            self.field.button.set_focus();
        }
    }
    fn focus_day(&self) {
        if !self.popup.is_shown() {
            return;
        }
        let date = self.date.get();
        self.days[date.month_offset() + date.day as usize - 1].set_focus();
    }
    fn render(&self) {
        let date = self.date.get();
        self.title.set_label(&if self.month_mode.get() {
            format!("{} 年", date.year)
        } else {
            format!("{} 年 {} 月", date.year, date.month)
        });
        self.book
            .change_selection(usize::from(self.month_mode.get()));
        let selected = Date::parse(&self.field.get_value());
        let today = Date::today();
        for (i, button) in self.days.iter().enumerate() {
            let day = self.day_at(i);
            button.set_label(&day.map(|d| d.day.to_string()).unwrap_or_default());
            button.enable(day.is_some());
            button.show(day.is_some());
            button.set_selected(day.is_some() && day == selected);
            button.set_foreground_color(if day == Some(today) {
                theme::theme().accent
            } else {
                theme::theme().ink_primary
            });
            button.set_tooltip(&day.map(|d| d.iso()).unwrap_or_default());
            button.set_name(&day.map(|d| d.iso()).unwrap_or_default());
        }
        for (i, button) in self.months.iter().enumerate() {
            button.set_selected(i as i32 + 1 == date.month);
        }
        self.panel.layout();
        self.days_host.layout();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_handles_leap_days_months_and_keyboard_boundaries() {
        let date = |s| Date::parse(s).unwrap();
        assert_eq!(date("2024-02-29").months(12).iso(), "2025-02-28");
        assert_eq!(date("2026-01-31").months(1).iso(), "2026-02-28");
        assert_eq!(date("2026-12-31").days(1).iso(), "2027-01-01");
        assert_eq!(date("2024-03-01").days(-1).iso(), "2024-02-29");
        assert_eq!(date("0001-01-01").days(-7).iso(), "0001-01-01");
        assert_eq!(date("9999-12-31").days(7).iso(), "9999-12-31");
        assert_eq!(date("2026-11-10").month_offset(), 6); // Nov 1 is Sunday.
        assert_eq!(date("2024-01-01").month_offset(), 0);
        assert!(Date::parse("2026-02-29").is_none());
    }
    #[cfg(windows)]
    pub(super) fn review(suffix: &str, field: DateField) {
        let ui = Calendar::new(field);
        field.change_value("2024-02-29");
        ui.date.set(Date::parse("2024-02-29").unwrap());
        ui.render();
        let index = ui.date.get().month_offset() + 28;
        assert_eq!(ui.day_at(index).unwrap().iso(), "2024-02-29");
        let bounds = ui.days_host.get_client_size();
        for (i, day) in ui.days.iter().enumerate() {
            if ui.day_at(i).is_some() {
                let p = day.get_position();
                let s = day.get_size();
                assert!(
                    p.x >= 0
                        && p.y >= 0
                        && p.x + s.width <= bounds.width
                        && p.y + s.height <= bounds.height,
                    "calendar date {} is outside {}x{}: {},{} {}x{}",
                    day.get_label(),
                    bounds.width,
                    bounds.height,
                    p.x,
                    p.y,
                    s.width,
                    s.height
                );
            }
        }
        super::super::super::native_model_review::capture(
            &ui.panel,
            &format!("squad-calendar-days-{suffix}"),
        );
        ui.month_mode.set(true);
        ui.render();
        super::super::super::native_model_review::capture(
            &ui.panel,
            &format!("squad-calendar-months-{suffix}"),
        );
        super::super::super::native_model_review::click(&ui.months[2]);
        assert!(!ui.month_mode.get());
        assert_eq!(ui.date.get().iso(), "2024-03-29");
        // Click the actual production day button: one callback, no auto-submit.
        let index = ui.date.get().month_offset() + 9;
        super::super::super::native_model_review::click(&ui.days[index]);
        assert_eq!(field.get_value(), "2024-03-10");
        ui.popup.destroy();
    }
}

#[cfg(all(test, windows))]
pub(super) fn review(suffix: &str, field: DateField) {
    tests::review(suffix, field);
}
