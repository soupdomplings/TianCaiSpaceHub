//! Shared account filters. Labels remain visible; hints are never request values.
use super::super::controls::Choice;
use super::*;
type Apply = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

pub(crate) use super::calendar::DateField;

#[derive(Clone)]
pub(crate) struct Filters {
    pub panel: Panel,
    pub search: TextCtrl,
    pub start: DateField,
    pub end: DateField,
    pub apply: Button,
    pub reset: Button,
    preset: Choice,
    search_group: Panel,
    search_label: StaticText,
    date_groups: Vec<Panel>,
    error: StaticText,
    callback: Apply,
}
impl Filters {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let row = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let (search_group, search_label, search_col) = group(&panel, "搜索");
        let search = TextCtrl::builder(&search_group)
            .with_size(Size::new(200, 36))
            .with_style(TextCtrlStyle::ProcessEnter)
            .build();
        search_col.add(&search, 0, SizerFlag::Expand, 0);
        search_group.set_sizer(search_col, false);
        let (range, _, range_col) = group(&panel, "日期范围");
        let preset = Choice::builder(&range)
            .with_choices(
                ["不限日期", "今天", "近 7 天", "本月", "自定日期"]
                    .map(str::to_string)
                    .to_vec(),
            )
            .with_size(Size::new(120, 36))
            .build();
        preset.set_selection(1);
        range_col.add(&preset, 0, SizerFlag::Expand, 0);
        range.set_sizer(range_col, false);
        let mut date_groups = vec![range];
        let mut dates = Vec::new();
        for label in ["开始日期", "结束日期"] {
            let (host, _, c) = group(&panel, label);
            let input = DateField::new(&host, label);
            input.set_tooltip(&format!("选择{label}"));
            c.add(&input, 0, SizerFlag::Expand, 0);
            host.set_sizer(c, false);
            date_groups.push(host);
            dates.push(input);
        }
        for host in std::iter::once(&search_group).chain(date_groups.iter()) {
            row.add(
                host,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(8),
            );
        }
        let mut buttons = Vec::new();
        for (label, kind, width) in [
            ("筛选", ButtonKind::Secondary, 72),
            ("重置", ButtonKind::Ghost, 64),
        ] {
            let (host, _, c) = group(&panel, " ");
            let b = Button::builder(&host)
                .with_label(label)
                .with_kind(kind)
                .with_size(Size::new(width, 36))
                .build();
            c.add(&b, 0, SizerFlag::default(), 0);
            host.set_sizer(c, false);
            row.add(
                &host,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(8),
            );
            buttons.push(b);
        }
        let (apply, reset) = (buttons[0], buttons[1]);
        col.add_sizer(&row, 0, SizerFlag::Expand, 0);
        let error = StaticText::builder(&panel).build();
        error.set_foreground_color(theme::theme().error);
        error.hide();
        col.add(
            &error,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(8),
        );
        panel.set_sizer(col, false);
        let ui = Self {
            panel,
            search,
            start: dates[0],
            end: dates[1],
            apply,
            reset,
            preset,
            search_group,
            search_label,
            date_groups,
            error,
            callback: Default::default(),
        };
        let u = ui.clone();
        apply.on_click(move |_| u.submit());
        let u = ui.clone();
        reset.on_click(move |_| {
            u.search.change_value("");
            if let Ok((start, end)) = current_period(1) {
                u.start.change_value(&start);
                u.end.change_value(&end);
            }
            u.preset.set_selection(1);
            u.submit();
        });
        let u = ui.clone();
        search.on_text_enter(move |_| u.submit());
        for input in [ui.start, ui.end] {
            input.on_date_changed(move |_| preset.set_selection(4));
        }
        let u = ui.clone();
        preset.on_selection_changed(move |_| {
            let i = preset.get_selection().unwrap_or(0);
            if i == 4 {
                u.start.set_focus();
                return;
            }
            match current_period(i) {
                Ok((start, end)) => {
                    u.start.change_value(&start);
                    u.end.change_value(&end);
                    u.submit();
                }
                Err(error) => {
                    u.error.set_label(&error);
                    u.error.show(true);
                    u.panel.layout();
                }
            }
        });
        if let Ok((start, end)) = current_period(1) {
            ui.start.change_value(&start);
            ui.end.change_value(&end);
        }
        ui
    }
    pub fn on_apply(&self, f: impl Fn() + 'static) {
        self.callback.replace(Some(Rc::new(f)));
    }
    pub fn mode(&self, search: Option<(&str, &str)>, dates: bool) {
        self.search_group.show(search.is_some());
        self.search.show(search.is_some());
        if let Some((label, hint)) = search {
            self.search_label.set_label(label);
            self.search.set_hint(hint);
            self.search.set_tooltip(hint);
        }
        for host in &self.date_groups {
            host.show(dates);
        }
        self.start.show(dates);
        self.end.show(dates);
        self.panel.show(search.is_some() || dates);
        self.error.hide();
        self.panel.layout();
    }
    /// Give the wrapping row its actual width before the page asks for its
    /// height; a hidden book page may still carry the narrow creation width.
    pub fn reflow(&self, width: i32) {
        let height = self.panel.get_size().height;
        self.panel.set_size(Size::new(width.max(1), height));
        self.panel.layout();
        self.panel.set_min_size(Size::new(-1, -1));
    }
    fn submit(&self) {
        let error = if self.start.is_shown() {
            validate(&self.start.get_value(), &self.end.get_value()).err()
        } else {
            None
        };
        self.error.show(error.is_some());
        self.error.set_label(error.unwrap_or(""));
        self.panel.layout();
        if let Some(parent) = self.panel.get_parent() {
            parent.layout();
        }
        if error.is_none() {
            if let Some(f) = self.callback.borrow().clone() {
                f();
            }
        }
    }
}
fn group(parent: &Panel, name: &str) -> (Panel, StaticText, BoxSizer) {
    let host = Panel::builder(parent).build();
    host.set_background_color(parent.get_background_color());
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let label = StaticText::builder(&host).with_label(name).build();
    label.set_font(&theme::font(theme::TextRole::Caption));
    label.set_foreground_color(theme::theme().ink_secondary);
    col.add(&label, 0, SizerFlag::Bottom, host.from_dip(4));
    (host, label, col)
}
pub(crate) fn validate(start: &str, end: &str) -> Result<(), &'static str> {
    let (start, end) = (start.trim(), end.trim());
    for s in [start, end] {
        if !s.is_empty() && !valid_date(s) {
            return Err("请选择有效日期");
        }
    }
    if !start.is_empty() && !end.is_empty() && start > end {
        return Err("开始日期不能晚于结束日期");
    }
    Ok(())
}
pub(super) fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return false;
    }
    let y = s[..4].parse::<u32>().unwrap_or(0);
    let m = s[5..7].parse::<usize>().unwrap_or(0);
    let d = s[8..].parse::<u32>().unwrap_or(0);
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    y > 0
        && m > 0
        && m <= 12
        && d > 0
        && d <= [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ][m.saturating_sub(1).min(11)]
}
pub(super) fn current_period(index: u32) -> Result<(String, String), String> {
    let db = rusqlite::Connection::open_in_memory().map_err(|_| "无法确定日期范围".to_string())?;
    let today: String = db
        .query_row("SELECT date('now','localtime')", [], |r| r.get(0))
        .map_err(|_| "无法确定日期范围".to_string())?;
    period(index, &today)
}
fn period(index: u32, today: &str) -> Result<(String, String), String> {
    if index == 0 {
        return Ok((String::new(), String::new()));
    }
    let db = rusqlite::Connection::open_in_memory().map_err(|_| "无法确定日期范围".to_string())?;
    let modifier = match index {
        2 => "-6 days",
        3 => "start of month",
        _ => "+0 days",
    };
    let start: String = db
        .query_row("SELECT date(?1,?2)", [today, modifier], |r| r.get(0))
        .map_err(|_| "无法确定日期范围".to_string())?;
    Ok((start, today.into()))
}
#[cfg(all(test, windows))]
pub(super) fn review(suffix: &str) {
    let frame = Frame::builder()
        .with_title("hidden production filters")
        .build();
    frame.set_size(visual::dip_size(&frame, 700, 240));
    let ui = Filters::new(&frame);
    ui.mode(None, true);
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    ui.on_apply(move || calls.set(calls.get() + 1));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    col.add(
        &ui.panel,
        0,
        SizerFlag::Expand | SizerFlag::All,
        frame.from_dip(20),
    );
    frame.set_sizer(col, false);
    frame.layout();
    let today = current_period(1).unwrap();
    assert_eq!((ui.start.get_value(), ui.end.get_value()), today);
    assert_eq!(ui.preset.get_selection(), Some(1));
    super::calendar::review(suffix, ui.start);
    assert_eq!(ui.preset.get_selection(), Some(4));
    assert_eq!(count.get(), 0);
    ui.start.change_value("2026-11-10");
    ui.end.change_value("2026-11-09");
    super::super::native_model_review::click(&ui.apply);
    assert_eq!(count.get(), 0);
    assert!(ui.error.is_shown());
    super::super::native_model_review::click(&ui.reset);
    assert_eq!(count.get(), 1);
    assert!(!ui.error.is_shown());
    assert_eq!(
        (ui.start.get_value(), ui.end.get_value()),
        current_period(1).unwrap()
    );
    super::super::native_model_review::capture(
        &ui.panel,
        &format!("squad-filter-native-dates-{suffix}"),
    );
    ui.start.change_value("bad-date");
    ui.mode(Some(("成员", "搜索姓名、邮箱或用户名")), false);
    super::super::native_model_review::click(&ui.apply);
    assert_eq!(count.get(), 2);
    assert!(ui.search.is_shown() && !ui.start.is_shown());
    super::super::native_model_review::capture(&ui.panel, &format!("squad-filter-search-{suffix}"));
    frame.destroy();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn periods_and_manual_dates_keep_explicit_empty_and_inclusive_boundaries() {
        let today = "2026-11-09";
        assert_eq!(period(0, today).unwrap(), ("".into(), "".into()));
        assert_eq!(
            period(2, today).unwrap(),
            ("2026-11-03".into(), "2026-11-09".into())
        );
        assert_eq!(
            period(3, today).unwrap(),
            ("2026-11-01".into(), "2026-11-09".into())
        );
        for (s, e) in [("", ""), ("2024-02-29", ""), ("2026-11-09", "2026-11-09")] {
            assert!(validate(s, e).is_ok());
        }
        for (s, e) in [
            ("2026-02-29", ""),
            ("2026-1-3", ""),
            ("日期", ""),
            ("2026-11-09", "2026-11-08"),
        ] {
            assert!(validate(s, e).is_err());
        }
    }
}
