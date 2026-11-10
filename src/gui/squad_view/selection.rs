//! Themed, searchable single/multiple selection; filtering never changes chosen objects.
use super::*;
use std::collections::BTreeSet;
#[derive(Clone)]
pub(crate) struct Selection {
    pub panel: Panel,
    pub accept: Button,
    pub back: Button,
    pub search: TextCtrl,
    labels: Vec<String>,
    rows: Vec<Button>,
    selected: Rc<RefCell<BTreeSet<usize>>>,
    count: StaticText,
    empty: StaticText,
    scroll: ScrolledWindow,
    multi: bool,
}
impl Selection {
    pub fn new(parent: &dyn WxWidget, title: &str, labels: Vec<String>, multi: bool) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let title = StaticText::builder(&panel).with_label(title).build();
        title.set_font(&theme::font(theme::TextRole::PageTitle));
        title.set_foreground_color(theme::theme().ink_primary);
        col.add(&title, 0, SizerFlag::Bottom, panel.from_dip(18));
        let search = TextCtrl::builder(&panel)
            .with_size(Size::new(-1, 36))
            .build();
        search.set_name("organization-picker-search");
        search.set_tooltip("按名称、邮箱或用户名搜索");
        search.set_hint("搜索名称、邮箱或用户名");
        col.add(
            &search,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(12),
        );
        let tools = BoxSizer::builder(Orientation::Horizontal).build();
        let count = StaticText::builder(&panel).build();
        count.set_foreground_color(theme::theme().ink_secondary);
        tools.add(&count, 1, SizerFlag::AlignCenterVertical, 0);
        let all = Button::builder(&panel)
            .with_label("选择当前结果")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(136, 30))
            .build();
        let clear = Button::builder(&panel)
            .with_label("清空选择")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(100, 30))
            .build();
        all.show(multi);
        clear.show(multi);
        tools.add(&all, 0, SizerFlag::Right, panel.from_dip(6));
        tools.add(&clear, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &tools,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(10),
        );
        let scroll = ScrolledWindow::builder(&panel)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_background_color(theme::theme().bg_app);
        scroll.set_min_size(Size::new(0, 0));
        scroll.set_scroll_rate(0, scroll.from_dip(12));
        scroll.show_scroll_indicators(false);
        let list = BoxSizer::builder(Orientation::Vertical).build();
        let mut rows = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            let row = Button::builder(&scroll)
                .with_label(label)
                .with_kind(ButtonKind::Nav)
                .with_size(Size::new(220, 42))
                .build();
            row.set_name(&format!("organization-picker-row-{i}"));
            row.set_tooltip(label);
            list.add(
                &row,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                scroll.from_dip(6),
            );
            rows.push(row);
        }
        let empty = StaticText::builder(&scroll)
            .with_label("没有匹配的条目")
            .build();
        empty.set_foreground_color(theme::theme().ink_secondary);
        list.add(&empty, 0, SizerFlag::All, scroll.from_dip(16));
        scroll.set_sizer(list, false);
        col.add(&scroll, 1, SizerFlag::Expand, 0);
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        footer.add_stretch_spacer(1);
        let back = Button::builder(&panel)
            .with_label("返回")
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(96, 36))
            .build();
        let accept = Button::builder(&panel)
            .with_label("继续")
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(96, 36))
            .build();
        footer.add(&back, 0, SizerFlag::Right, panel.from_dip(8));
        footer.add(&accept, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(16),
        );
        panel.set_sizer(col, false);
        let ui = Self {
            panel,
            accept,
            back,
            search,
            labels,
            rows,
            selected: Default::default(),
            count,
            empty,
            scroll,
            multi,
        };
        for (i, row) in ui.rows.iter().enumerate() {
            let u = ui.clone();
            row.on_click(move |_| {
                let mut selected = u.selected.borrow_mut();
                if !selected.remove(&i) {
                    if !u.multi {
                        selected.clear();
                    }
                    selected.insert(i);
                }
                drop(selected);
                u.update();
            });
        }
        let u = ui.clone();
        ui.search.on_text_changed(move |_| u.update());
        let u = ui.clone();
        all.on_click(move |_| {
            for (i, row) in u.rows.iter().enumerate() {
                if row.is_shown() {
                    u.selected.borrow_mut().insert(i);
                }
            }
            u.update();
        });
        let u = ui.clone();
        clear.on_click(move |_| {
            u.selected.borrow_mut().clear();
            u.update();
        });
        ui.update();
        ui
    }
    pub fn indices(&self) -> Vec<usize> {
        self.selected.borrow().iter().copied().collect()
    }
    fn update(&self) {
        let query = self.search.get_value().to_lowercase();
        let mut visible = 0;
        let selected = self.selected.borrow();
        for (i, row) in self.rows.iter().enumerate() {
            let shown = self.labels[i].to_lowercase().contains(query.trim());
            row.show(shown);
            row.set_selected(selected.contains(&i));
            row.set_label(&format!(
                "{}  {}",
                if selected.contains(&i) { "✓" } else { "○" },
                self.labels[i]
            ));
            // ActionButton updates its default name when the label changes.
            row.set_name(&format!("organization-picker-row-{i}"));
            visible += usize::from(shown);
        }
        self.count
            .set_label(&format!("已选 {} 项    共 {visible} 项", selected.len()));
        self.accept.enable(!selected.is_empty());
        self.empty.show(visible == 0);
        self.scroll.layout();
        self.scroll.fit_inside();
    }
}
