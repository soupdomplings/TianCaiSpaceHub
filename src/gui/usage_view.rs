//! The actual statistics layout, shared with isolated native layout checks.
//! No configuration, API, timers, or application startup is performed here.
use super::{
    controls::{Button, ButtonKind, Choice, paint_surface},
    theme, visual,
};
use crate::desktop::request_source;
use std::{cell::Cell, rc::Rc};
use wxdragon::prelude::*;

pub(super) const PERIOD_DAYS: [i64; 3] = [1, 7, 30];
pub(super) const DEFAULT_DAYS: i64 = 7;

#[derive(Clone)]
pub(super) struct UsageView {
    pub page: Panel,
    pub periods: [Button; 3],
    pub client: Choice,
    pub filters: Button,
    pub reset: Button,
    pub stats: [StaticText; 4],
    pub chart: Panel,
    pub dimension: Choice,
    pub breakdown: ListCtrl,
    detail: Panel,
    distribution: Button,
    detail_only: Rc<Cell<bool>>,
}
impl UsageView {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let t = theme::theme();
        let page = Panel::builder(parent).build();
        page.set_background_color(t.bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let toolbar = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let periods = std::array::from_fn(|i| {
            let b = Button::builder(&page)
                .with_label(["今天", "近 7 天", "近 30 天"][i])
                .with_kind(ButtonKind::Segment)
                .with_size(Size::new([60, 72, 80][i], theme::CONTROL_HEIGHT))
                .build();
            b.set_selected(PERIOD_DAYS[i] == DEFAULT_DAYS);
            toolbar.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                page.from_dip(4),
            );
            b
        });
        let client = Choice::builder(&page)
            .with_size(Size::new(160, theme::CONTROL_HEIGHT))
            .with_choices(
                std::iter::once("全部来源".into())
                    .chain(
                        request_source::keys()
                            .filter_map(request_source::name)
                            .map(Into::into),
                    )
                    .collect(),
            )
            .build();
        client.set_selection(0);
        toolbar.add(
            &client,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(4),
        );
        let filters = Button::builder(&page)
            .with_label("模型 / 渠道")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(100, theme::CONTROL_HEIGHT))
            .build();
        toolbar.add(
            &filters,
            0,
            SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(4),
        );
        let reset = Button::builder(&page)
            .with_label("清除")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(56, theme::CONTROL_HEIGHT))
            .build();
        reset.hide();
        toolbar.add(
            &reset,
            0,
            SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(4),
        );
        let distribution = Button::builder(&page)
            .with_label("用量分布")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(80, theme::CONTROL_HEIGHT))
            .build();
        distribution.set_tooltip("切换趋势与分布");
        toolbar.add(&distribution, 0, SizerFlag::Bottom, page.from_dip(4));
        column.add_sizer(
            &toolbar,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(4),
        );

        let metrics = Panel::builder(&page).build();
        paint_surface(metrics, theme::SURFACE_RADIUS);
        let numbers = BoxSizer::builder(Orientation::Horizontal).build();
        let stats = std::array::from_fn(|i| {
            let cell = Panel::builder(&metrics).build();
            cell.set_background_color(t.bg_card);
            cell.set_min_size(visual::dip_size(&cell, 100, 56));
            let c = BoxSizer::builder(Orientation::Vertical).build();
            let label = StaticText::builder(&cell)
                .with_label(["请求数", "成功率", "输入用量", "输出用量"][i])
                .build();
            label.set_font(&theme::font(theme::TextRole::Caption));
            label.set_foreground_color(t.ink_secondary);
            let number = StaticText::builder(&cell).with_label("—").build();
            number.set_font(&theme::font(theme::TextRole::Metric));
            number.set_foreground_color(t.ink_primary);
            c.add(
                &label,
                0,
                SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                cell.from_dip(4),
            );
            c.add(
                &number,
                0,
                SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                cell.from_dip(4),
            );
            cell.set_sizer(c, true);
            numbers.add(
                &cell,
                1,
                SizerFlag::Expand | SizerFlag::All,
                cell.from_dip(8),
            );
            number
        });
        metrics.set_sizer(numbers, true);
        column.add(
            &metrics,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(8),
        );

        let chart = Panel::builder(&page).build();
        chart.set_min_size(visual::dip_size(&chart, 280, 168));
        chart.set_background_style(BackgroundStyle::Paint);
        visual::redraw_on_resize(chart);
        column.add(
            &chart,
            1,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );

        let detail = Panel::builder(&page).build();
        paint_surface(detail, theme::SURFACE_RADIUS);
        let detail_col = BoxSizer::builder(Orientation::Vertical).build();
        let dimension = Choice::builder(&detail)
            .with_size(Size::new(126, 32))
            .with_choices(vec!["按来源".into(), "按模型".into(), "按渠道".into()])
            .build();
        dimension.set_selection(0);
        detail_col.add(
            &dimension,
            0,
            SizerFlag::Left | SizerFlag::Top | SizerFlag::Bottom,
            detail.from_dip(10),
        );
        let breakdown = ListCtrl::builder(&detail)
            .with_style(
                ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::BorderNone,
            )
            .build();
        breakdown.set_min_size(visual::dip_size(&breakdown, 280, 100));
        breakdown.set_background_color(t.bg_card);
        breakdown.set_foreground_color(t.ink_primary);
        for (i, name) in ["对象", "请求数", "输入用量", "输出用量"]
            .into_iter()
            .enumerate()
        {
            breakdown.insert_column(
                i as i64,
                name,
                if i == 0 {
                    ListColumnFormat::Left
                } else {
                    ListColumnFormat::Right
                },
                breakdown.from_dip(if i == 0 { 230 } else { 120 }),
            );
        }
        breakdown.on_size(move |event| {
            let width = (breakdown.get_client_size().width - breakdown.from_dip(12))
                .max(breakdown.from_dip(300));
            for (i, weight) in [43, 15, 21, 21].into_iter().enumerate() {
                breakdown.set_column_width(i as i64, width * weight / 100);
            }
            event.skip(true);
        });
        detail_col.add(
            &breakdown,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            detail.from_dip(10),
        );
        detail.set_sizer(detail_col, true);
        column.add(&detail, 1, SizerFlag::Expand, 0);
        page.set_sizer(column, true);
        let view = Self {
            page,
            periods,
            client,
            filters,
            reset,
            stats,
            chart,
            dimension,
            breakdown,
            detail,
            distribution,
            detail_only: Default::default(),
        };
        let v = view.clone();
        page.on_size(move |e| {
            v.adapt();
            e.skip(true);
        });
        let v = view.clone();
        distribution.on_click(move |_| {
            v.detail_only.set(!v.detail_only.get());
            v.adapt();
        });
        view.adapt();
        view
    }
    fn adapt(&self) {
        // A real fixed viewport, rather than a scroll area with hidden overflow.
        // Short windows show one useful view at a time; the distribution toggle
        // keeps the same data and drill-down accessible without growing the page.
        let compact = self.page.get_client_size().height < self.page.from_dip(560);
        let detail_only = compact && self.detail_only.get();
        self.distribution.show(compact);
        self.distribution.set_label(if detail_only {
            "用量趋势"
        } else {
            "用量分布"
        });
        self.distribution.set_selected(detail_only);
        self.chart.show(!detail_only);
        self.detail.show(!compact || detail_only);
        self.detail.set_max_size(visual::dip_size(
            &self.detail,
            -1,
            if compact { -1 } else { 205 },
        ));
        self.page.layout();
    }
    pub fn update_filter_label(&self, model: &str, channel: &str) {
        let n = usize::from(!model.is_empty()) + usize::from(!channel.is_empty());
        self.filters.set_label(&if n > 0 {
            format!("模型 / 渠道 · {n}")
        } else {
            "模型 / 渠道".into()
        });
        self.filters.set_selected(n > 0);
        self.filters.set_tooltip(&format!(
            "模型：{}\n渠道：{}",
            if model.is_empty() { "全部" } else { model },
            if channel.is_empty() {
                "全部"
            } else {
                channel
            }
        ));
        self.reset
            .show(n > 0 || self.client.get_selection().unwrap_or(0) > 0);
        self.adapt();
    }
}

pub(super) fn choose_filters(
    parent: &dyn WxWidget,
    model: &str,
    channel: &str,
    options: &serde_json::Value,
) -> Option<(String, String)> {
    let dialog = Dialog::builder(parent, "筛选用量")
        .with_size(430, 265)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let fields = std::array::from_fn::<_, 2, _>(|i| {
        let label = StaticText::builder(&dialog)
            .with_label(["模型名称", "渠道名称"][i])
            .build();
        label.set_foreground_color(theme::theme().ink_secondary);
        column.add(
            &label,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            dialog.from_dip(16),
        );
        let current = [model, channel][i];
        let mut choices = vec![["全部模型", "全部渠道"][i].to_string()];
        choices.extend(
            options[["models", "channels"][i]]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .map(str::to_string),
        );
        if !current.is_empty() && !choices.iter().any(|s| s == current) {
            choices.push(current.into());
        }
        let selection = choices.iter().position(|s| s == current).unwrap_or(0);
        let field = Choice::builder(&dialog)
            .with_choices(choices)
            .with_size(Size::new(340, 36))
            .build();
        field.set_selection(selection as u32);
        column.add(
            &field,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            dialog.from_dip(8),
        );
        field
    });
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    actions.add_stretch_spacer(1);
    for (name, id) in [("取消", ID_CANCEL), ("应用", ID_OK)] {
        let b = Button::builder(&dialog)
            .with_label(name)
            .with_kind(if id == ID_OK {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            })
            .build();
        b.on_click(move |_| dialog.end_modal(id));
        actions.add(&b, 0, SizerFlag::Left, dialog.from_dip(8));
    }
    column.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(16),
    );
    dialog.set_sizer(column, true);
    dialog.layout();
    dialog.fit();
    dialog.centre();
    fields[0].set_focus();
    let result = if dialog.show_modal() == ID_OK {
        Some((
            if fields[0].get_selection() == Some(0) {
                String::new()
            } else {
                fields[0].get_value()
            },
            if fields[1].get_selection() == Some(0) {
                String::new()
            } else {
                fields[1].get_value()
            },
        ))
    } else {
        None
    };
    dialog.destroy();
    result
}
