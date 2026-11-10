//! Organization workbench presentation. Permissions and operations stay in the controller.
use super::*;
type Select = Rc<RefCell<Option<Rc<dyn Fn(usize)>>>>;
#[derive(Clone)]
pub(crate) struct Organization {
    pub panel: Panel,
    pub table: ListCtrl,
    pub summary: detail::Detail,
    pub status: StaticText,
    pub heading: StaticText,
    pub add: Button,
    pub edit: Button,
    pub remove: Button,
    pub more: Button,
    pub batch: Button,
    pub details: Button,
    pub reload: Button,
    pub close: Button,
    pub search: TextCtrl,
    pub start: filters::DateField,
    pub end: filters::DateField,
    pub apply: Button,
    pub filters: Panel,
    pub filter_bar: filters::Filters,
    pub previous: Button,
    pub next: Button,
    groups: Vec<Button>,
    tabs: Vec<Button>,
    entries: Vec<(String, String)>,
    select: Select,
    metric_panel: Panel,
    metric_labels: Vec<(Panel, StaticText, StaticText)>,
}
fn group(path: &str) -> usize {
    if path.starts_with("usage/") || path == "usage-report" {
        2
    } else if path.starts_with("settlement") {
        3
    } else if matches!(path, "memberships" | "departments" | "department-admins") {
        0
    } else {
        1
    }
}
impl Organization {
    pub fn new(parent: &dyn WxWidget, title: &str, entries: Vec<(String, String)>) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let header = BoxSizer::builder(Orientation::Horizontal).build();
        let close = Button::builder(&panel)
            .with_icon(Icon::Back)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(36, 36))
            .build();
        close.set_tooltip("返回组织列表");
        header.add(&close, 0, SizerFlag::Right, panel.from_dip(10));
        let title = StaticText::builder(&panel).with_label(title).build();
        title.set_font(&theme::font(theme::TextRole::PageTitle));
        title.set_foreground_color(theme::theme().ink_primary);
        header.add(&title, 1, SizerFlag::AlignCenterVertical, 0);
        let reload = Button::builder(&panel)
            .with_icon(Icon::Refresh)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(36, 36))
            .build();
        reload.set_tooltip("刷新当前内容");
        header.add(&reload, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &header,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(14),
        );
        let select: Select = Default::default();
        let main = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let mut groups = Vec::new();
        for (i, label) in ["成员与部门", "成员额度", "组织用量", "挂账与余额"]
            .into_iter()
            .enumerate()
        {
            let b = Button::builder(&panel)
                .with_label(label)
                .with_kind(ButtonKind::Tab)
                .with_size(Size::new(126, 36))
                .build();
            let cb = select.clone();
            let target = entries.iter().position(|e| group(&e.1) == i);
            b.on_click(move |_| {
                if let (Some(i), Some(f)) = (target, cb.borrow().clone()) {
                    f(i);
                }
            });
            b.show(target.is_some());
            main.add(&b, 0, SizerFlag::Right, panel.from_dip(4));
            groups.push(b);
        }
        col.add_sizer(
            &main,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(8),
        );
        let sub = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let mut tabs = Vec::new();
        for (i, (label, _)) in entries.iter().enumerate() {
            let b = Button::builder(&panel)
                .with_label(label)
                .with_kind(ButtonKind::Segment)
                .with_size(Size::new(
                    (label.chars().count() as i32 * 14 + 24).clamp(80, 144),
                    30,
                ))
                .build();
            let cb = select.clone();
            b.on_click(move |_| {
                let f = cb.borrow().clone();
                if let Some(f) = f {
                    f(i);
                }
            });
            sub.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(6),
            );
            tabs.push(b);
        }
        col.add_sizer(
            &sub,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(8),
        );
        let heading = StaticText::builder(&panel).build();
        heading.set_font(&theme::font(theme::TextRole::Title));
        heading.set_foreground_color(theme::theme().ink_primary);
        col.add(&heading, 0, SizerFlag::Bottom, panel.from_dip(10));
        let metric_panel = Panel::builder(&panel).build();
        metric_panel.set_background_color(theme::theme().bg_app);
        let metric_row = BoxSizer::builder(Orientation::Horizontal).build();
        let mut metric_labels = Vec::new();
        for _ in 0..4 {
            let host = Panel::builder(&metric_panel).build();
            super::super::controls::paint_surface(host, 10);
            let pair = BoxSizer::builder(Orientation::Vertical).build();
            let label = StaticText::builder(&host).build();
            label.set_font(&theme::font(theme::TextRole::Caption));
            label.set_foreground_color(theme::theme().ink_secondary);
            let value = StaticText::builder(&host)
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            value.set_min_size(Size::new(0, -1));
            value.set_font(&theme::font(theme::TextRole::Title));
            value.set_foreground_color(theme::theme().ink_primary);
            pair.add(
                &label,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                host.from_dip(12),
            );
            pair.add(
                &value,
                0,
                SizerFlag::Expand | SizerFlag::All,
                host.from_dip(12),
            );
            host.set_sizer(pair, false);
            metric_row.add(
                &host,
                1,
                SizerFlag::Expand | SizerFlag::Right,
                metric_panel.from_dip(8),
            );
            metric_labels.push((host, label, value));
        }
        metric_panel.set_sizer(metric_row, false);
        col.add(
            &metric_panel,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(12),
        );
        metric_panel.hide();
        let actions = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let buttons = [
            ("添加", ButtonKind::Primary),
            ("编辑", ButtonKind::Secondary),
            ("移除", ButtonKind::Ghost),
            ("相关操作", ButtonKind::Secondary),
            ("批量设置", ButtonKind::Ghost),
            ("查看详情", ButtonKind::Ghost),
        ]
        .map(|(label, kind)| {
            let b = Button::builder(&panel)
                .with_label(label)
                .with_kind(kind)
                .with_size(Size::new(124, 32))
                .build();
            actions.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(6),
            );
            b
        });
        col.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(4),
        );
        let filter_bar = filters::Filters::new(&panel);
        let (filters, search, start, end, apply) = (
            filter_bar.panel,
            filter_bar.search,
            filter_bar.start,
            filter_bar.end,
            filter_bar.apply,
        );
        col.add(
            &filters,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(12),
        );
        let summary = detail::Detail::new(&panel);
        col.add(&summary.panel, 1, SizerFlag::Expand, 0);
        summary.panel.hide();
        let table = ListCtrl::builder(&panel)
            .with_style(
                ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::BorderNone,
            )
            .build();
        table.set_background_color(theme::theme().bg_card);
        table.set_foreground_color(theme::theme().ink_primary);
        table.set_font(&theme::font(theme::TextRole::Body));
        col.add(&table, 1, SizerFlag::Expand, 0);
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        let status = StaticText::builder(&panel).build();
        status.set_foreground_color(theme::theme().ink_secondary);
        footer.add(&status, 1, SizerFlag::AlignCenterVertical, 0);
        let previous = Button::builder(&panel)
            .with_icon(Icon::Back)
            .with_label("上一页")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(92, 32))
            .build();
        let next = Button::builder(&panel)
            .with_icon(Icon::Chevron)
            .with_label("下一页")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(92, 32))
            .build();
        footer.add(&previous, 0, SizerFlag::Right, panel.from_dip(6));
        footer.add(&next, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(8),
        );
        panel.set_sizer(col, false);
        let [add, edit, remove, more, batch, details] = buttons;
        Self {
            panel,
            table,
            summary,
            status,
            heading,
            add,
            edit,
            remove,
            more,
            batch,
            details,
            reload,
            close,
            search,
            start,
            end,
            apply,
            filters,
            filter_bar,
            previous,
            next,
            groups,
            tabs,
            entries,
            select,
            metric_panel,
            metric_labels,
        }
    }
    pub fn on_select(&self, f: impl Fn(usize) + 'static) {
        self.select.replace(Some(Rc::new(f)));
    }
    pub fn set_metrics(&self, fields: &[(String, String)]) {
        self.metric_panel.show(!fields.is_empty());
        for (i, (host, label, value)) in self.metric_labels.iter().enumerate() {
            host.show(i < fields.len());
            if let Some((a, b)) = fields.get(i) {
                label.set_label(a);
                value.set_label(b);
                host.layout();
            }
        }
        self.metric_panel.layout();
        self.panel.layout();
    }
    pub fn select(&self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let current = group(&entry.1);
        for (i, b) in self.groups.iter().enumerate() {
            b.set_selected(i == current);
        }
        for (i, b) in self.tabs.iter().enumerate() {
            b.show(group(&self.entries[i].1) == current);
            b.set_selected(i == index);
        }
        self.heading.set_label(&entry.0);
        let dates = entry.1.starts_with("usage/");
        let search = matches!(
            entry.1.as_str(),
            "memberships" | "departments" | "department-admins" | "member-limit-details"
        );
        self.filter_bar.mode(
            if search {
                Some(if entry.1 == "departments" {
                    ("部门名称", "搜索部门名称")
                } else {
                    ("成员", "搜索姓名、邮箱或用户名")
                })
            } else {
                None
            },
            dates,
        );
        self.panel.layout();
    }
    pub fn fit_columns(&self) {
        let n = self.table.get_column_count().max(1);
        let width = (self.table.get_client_size().width - 4).max(n);
        let mut used = 0;
        for i in 0..n {
            let w = if i == n - 1 { width - used } else { width / n };
            self.table.set_column_width(i as i64, w);
            used += w;
        }
    }
}
