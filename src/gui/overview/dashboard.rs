use crate::desktop::clients::ClientId;
use crate::gui::{
    controls::{Button, ButtonKind, paint_surface},
    shell::{PageId, Shell, SlidingStrip},
    theme::{self, TextRole},
    visual::{self, Canvas, Icon},
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};
use wxdragon::{dc::GraphicsDC, prelude::*};

#[derive(Clone)]
pub(super) struct Dashboard {
    pub page: ScrolledWindow,
    pub periods: SlidingStrip<2>,
    pub runtime_link: Button,
    pub usage_link: Button,
    chart: Panel,
    trend: Rc<RefCell<Value>>,
    values: [StaticText; 3],
    clients: Vec<(ClientId, StaticText, StaticText)>,
}
fn label(parent: &impl WxWidget, text: &str, role: TextRole) -> StaticText {
    let widget = StaticText::builder(parent).with_label(text).build();
    widget.set_font(&theme::font(role));
    widget.set_background_color(theme::theme().bg_card);
    widget.set_foreground_color(theme::theme().ink_primary);
    widget
}
fn card(parent: &impl WxWidget) -> Panel {
    let panel = Panel::builder(parent).build();
    panel.set_background_color(theme::theme().bg_card);
    paint_surface(panel, theme::SURFACE_RADIUS);
    panel
}
fn detail_link(card: &Panel, layout: &BoxSizer, name: &str, hint: &str) -> Button {
    let link = Button::builder(card)
        .with_label(name)
        .with_kind(ButtonKind::Ghost)
        .with_size(Size::new(82, 30))
        .build();
    link.set_foreground_color(theme::theme().accent);
    link.set_tooltip(hint);
    layout.add(
        &link,
        0,
        SizerFlag::AlignRight | SizerFlag::Right | SizerFlag::Bottom,
        card.from_dip(6),
    );
    link
}
fn set(label: &StaticText, value: &str) {
    if label.get_label() != value {
        label.set_label(value);
    }
}
fn client_labels(row: Option<&Value>) -> (String, String) {
    let Some(row) = row else {
        return ("—".into(), "状态未知".into());
    };
    let n = row["requests"].as_array().map_or(0, Vec::len);
    let status = if n > 0 {
        format!("{n} 个请求中")
    } else {
        match row["opened"].as_bool() {
            Some(true) => "已打开",
            Some(false) => "未运行",
            None => "状态未知",
        }
        .into()
    };
    (
        row["modelCount"]
            .as_u64()
            .map(|n| format!("{n} 个模型"))
            .unwrap_or_else(|| "—".into()),
        status,
    )
}
impl Dashboard {
    pub fn new(parent: &impl WxWidget, shell: &Shell) -> Self {
        let t = theme::theme();
        let page = ScrolledWindow::builder(parent)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        page.set_background_color(t.bg_app);
        page.set_scroll_rate(0, page.from_dip(12));
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let upper = BoxSizer::builder(Orientation::Horizontal).build();
        let chart_card = card(&page);
        let layout = BoxSizer::builder(Orientation::Vertical).build();
        let toolbar = BoxSizer::builder(Orientation::Horizontal).build();
        toolbar.add(
            &label(&chart_card, "请求趋势", TextRole::Title),
            1,
            SizerFlag::AlignCenterVertical,
            0,
        );
        let periods = SlidingStrip::new(chart_card, [("今天", 54), ("近 7 天", 72)], 30, false);
        for button in periods.keys() {
            toolbar.add(&button, 0, SizerFlag::AlignCenterVertical, 0);
        }
        layout.add_sizer(
            &toolbar,
            0,
            SizerFlag::Expand | SizerFlag::All,
            chart_card.from_dip(12),
        );
        let chart = Panel::builder(&chart_card).build();
        chart.set_min_size(visual::dip_size(&chart, 190, 130));
        chart.set_background_style(BackgroundStyle::Paint);
        visual::redraw_on_resize(chart);
        layout.add(
            &chart,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            chart_card.from_dip(12),
        );
        let usage_link = detail_link(&chart_card, &layout, "使用统计", "查看详细用量及请求分布");
        chart_card.set_sizer(layout, true);
        upper.add(
            &chart_card,
            3,
            SizerFlag::Expand | SizerFlag::Right,
            page.from_dip(12),
        );
        let health = card(&page);
        let data = BoxSizer::builder(Orientation::Vertical).build();
        data.add(
            &label(&health, "状态", TextRole::Title),
            0,
            SizerFlag::Left | SizerFlag::Top,
            health.from_dip(14),
        );
        let values = ["本机网关", "请求成功率", "正在处理"].map(|name| {
            let row = BoxSizer::builder(Orientation::Horizontal).build();
            let field = label(&health, name, TextRole::Body);
            field.set_foreground_color(t.ink_secondary);
            row.add(&field, 1, SizerFlag::AlignCenterVertical, 0);
            let value = label(&health, "—", TextRole::Body);
            row.add(&value, 0, SizerFlag::AlignCenterVertical, 0);
            data.add_sizer(
                &row,
                1,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
                health.from_dip(14),
            );
            value
        });
        let runtime_link = detail_link(&health, &data, "运行状态", "查看实时连接和当前请求");
        health.set_sizer(data, true);
        upper.add(&health, 2, SizerFlag::Expand, 0);
        col.add_sizer(
            &upper,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(14),
        );
        let table = card(&page);
        let rows = BoxSizer::builder(Orientation::Vertical).build();
        rows.add(
            &label(&table, "客户端", TextRole::Title),
            0,
            SizerFlag::Left | SizerFlag::Top | SizerFlag::Bottom,
            table.from_dip(12),
        );
        let clients = ClientId::VISIBLE
            .into_iter()
            .map(|id| {
                let row = BoxSizer::builder(Orientation::Horizontal).build();
                let jump = Button::builder(&table)
                    .with_kind(ButtonKind::Ghost)
                    .with_icon(Icon::client(id))
                    .with_label(visual::client_short(id))
                    .with_size(Size::new(186, 36))
                    .build();
                jump.set_tooltip(&format!("打开 {}", visual::client_short(id)));
                let navigation = shell.clone();
                jump.on_click(move |_| navigation.navigate(PageId::Client(id)));
                row.add(&jump, 0, SizerFlag::AlignCenterVertical, 0);
                row.add_stretch_spacer(1);
                let models = label(&table, "—", TextRole::Body);
                models.set_foreground_color(t.ink_secondary);
                let status = label(&table, "—", TextRole::Body);
                row.add(&models, 1, SizerFlag::AlignCenterVertical, 0);
                row.add(&status, 1, SizerFlag::AlignCenterVertical, 0);
                rows.add_sizer(
                    &row,
                    0,
                    SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                    table.from_dip(4),
                );
                (id, models, status)
            })
            .collect();
        table.set_sizer(rows, true);
        col.add(&table, 0, SizerFlag::Expand, 0);
        page.set_sizer(col, true);
        let trend = Rc::new(RefCell::new(Value::Null));
        let draw = trend.clone();
        chart.on_paint(move |_| {
            let dc = AutoBufferedPaintDC::new(&chart);
            let graph = GraphicsDC::new(&dc);
            let scale = visual::scale(&chart);
            let c = Canvas::new(&graph, scale);
            let sz = chart.get_client_size();
            draw_trend(
                &c,
                (sz.width as f64 / scale) as i32,
                (sz.height as f64 / scale) as i32,
                &draw.borrow(),
            );
        });
        Self {
            page,
            periods,
            runtime_link,
            usage_link,
            chart,
            trend,
            values,
            clients,
        }
    }
    pub fn activity(&self, activity: &Value) {
        let clients = activity["clients"].as_array();
        for (id, count, state) in &self.clients {
            let row = clients.and_then(|rows| rows.iter().find(|r| r["client"] == id.key()));
            let (n, s) = client_labels(row);
            set(count, &n);
            set(state, &s);
        }
        set(
            &self.values[0],
            match activity["gatewayEnabled"].as_bool() {
                Some(true) => "已启用",
                Some(false) => "已停用",
                None => "—",
            },
        );
        let active = activity["requests"]
            .as_array()
            .map(|v| format!("{} 个", v.len()))
            .unwrap_or_else(|| "—".into());
        set(&self.values[2], &active);
    }
    pub fn usage(&self, usage: Value) {
        let total = usage["summary"]["requests"].as_i64().unwrap_or(0);
        let success = usage["summary"]["successful"]
            .as_i64()
            .filter(|_| total > 0)
            .map(|n| format!("{:.1}%", 100. * n as f64 / total as f64));
        set(&self.values[1], success.as_deref().unwrap_or("—"));
        *self.trend.borrow_mut() = usage;
        self.chart.refresh(false, None);
    }
}
fn draw_trend(c: &Canvas<'_>, w: i32, h: i32, usage: &Value) {
    let t = theme::theme();
    c.clear(t.bg_card);
    let total = usage["summary"]["requests"]
        .as_i64()
        .map(|n| format!("{n} 次请求"))
        .unwrap_or_else(|| "—".into());
    c.text(&total, 6, 0, 22, true, t.ink_primary);
    let Some(rows) = usage["trend"].as_array().filter(|r| !r.is_empty()) else {
        return;
    };
    let (left, right, top, bottom) = (7, (w - 8).max(8), 35, (h - 20).max(36));
    let max = rows
        .iter()
        .filter_map(|r| r["requests"].as_i64())
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    for i in 0..=3 {
        let y = top + (bottom - top) * i / 3;
        c.line(Point::new(left, y), Point::new(right, y), t.divider, true);
    }
    let mut previous = None;
    for (i, row) in rows.iter().enumerate() {
        let Some(n) = row["requests"].as_i64() else {
            previous = None;
            continue;
        };
        let x = left + ((right - left) as usize * i / rows.len().saturating_sub(1).max(1)) as i32;
        let point = Point::new(
            x,
            bottom - ((bottom - top) as f64 * n as f64 / max).round() as i32,
        );
        if let Some(prev) = previous {
            c.curve(prev, point, t.accent, 0., false, false)
        }
        if i + 1 == rows.len() {
            c.dot(point.x, point.y, 3, t.accent)
        }
        previous = Some(point);
    }
    for (row, x) in [(&rows[0], left), (&rows[rows.len() - 1], right - 73)] {
        c.label(
            row["label"].as_str().unwrap_or(""),
            Rect::new(x, bottom + 2, 76, 17),
            10,
            false,
            t.ink_muted,
            false,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn hidden_overview_card_details_return_to_summary_from_sidebar() {
        use crate::gui::{
            api::ApiClient,
            native_model_review::capture,
            overview::Overview,
            text::{GuiLocale, GuiText},
        };
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let frame = Frame::builder().with_title("offline overview").build();
            frame.set_font(&theme::font(TextRole::Body));
            let root = Panel::builder(&frame).build();
            let nav = Shell::new(&root, &frame);
            let text = GuiText::new(GuiLocale::ZhCn);
            let ui = Overview::new(&nav, frame, ApiClient::new(String::new(), text), text);
            let s = BoxSizer::builder(Orientation::Vertical).build();
            s.add(&nav.panel, 1, SizerFlag::Expand, 0);
            root.set_sizer(s, false);
            let outer = BoxSizer::builder(Orientation::Vertical).build();
            outer.add(&root, 1, SizerFlag::Expand, 0);
            frame.set_sizer(outer, false);
            let d = &ui.dashboard;
            assert!(d.page.is_shown(), "initial selection is the actual dashboard page");
            assert_eq!(d.runtime_link.get_label(), "运行状态");
            assert_eq!(d.usage_link.get_label(), "使用统计");
            // Same destination as clicking the Overview rail entry. Suppress HTTP
            // in the hidden fixture; details continue to use the real view's filters.
            ui.busy.store(true, std::sync::atomic::Ordering::SeqCst);
            let view = ui.clone();
            nav.on_changed(move |id| view.enter(id));
            ui.summary_days.set(7);
            ui.filters.borrow_mut().channel = Some("fixture-channel".into());
            for index in [0, 1] {
                ui.select_panel(index);
                assert_eq!(ui.book.selection(), index as i32);
                nav.navigate(PageId::Overview);
                assert!(d.page.is_shown());
                assert_eq!(ui.mode.get(), super::super::SUMMARY);
                assert_eq!(ui.filter().calendar_days, 7);
                assert_eq!(ui.filters.borrow().channel.as_deref(), Some("fixture-channel"));
            }
            ui.enter(PageId::Logs);
            ui.summary_days.set(1);
            assert_eq!(d.clients.len(), 5);
            d.activity(&serde_json::json!({"clients":[{"client":d.clients[0].0.key(),"opened":true,"modelCount":3,"requests":[]}],"gatewayEnabled":true,"requests":[]}));
            d.usage(serde_json::json!({"summary":{"requests":4,"successful":3},"trend":[{"label":"08:00","requests":1},{"label":"09:00","requests":3}]}));
            assert_eq!(d.values[1].get_label(), "75.0%");
            assert_eq!(d.clients[0].1.get_label(), "3 个模型");
            assert_eq!(d.clients[0].2.get_label(), "已打开");
            assert_eq!(d.clients[4].2.get_label(), "状态未知");
            for (w,h) in [(1040,720),(900,370)] {
                frame.set_size(visual::dip_size(&frame,w,h));
                frame.layout();
                root.layout();
                nav.panel.layout();
                ui.page.layout();
                d.page.layout();
                d.page.fit_inside();
                let size=d.page.get_client_size();
                assert!(size.width > d.page.from_dip(430));
                if h==370 {
                    assert!(d.page.get_virtual_size().height > size.height, "last client must be reachable by vertical scroll");
                } else {
                    capture(&frame,"v44-overview-native");
                }
            }
            assert!(!frame.is_shown());
            frame.destroy();
        }).unwrap();
    }
    #[test]
    fn summary_separates_unknown_state_from_idle_client_and_zero_models() {
        assert_eq!(client_labels(None), ("—".into(), "状态未知".into()));
        let idle = serde_json::json!({"opened":false,"modelCount":0,"requests":[]});
        assert_eq!(
            client_labels(Some(&idle)),
            ("0 个模型".into(), "未运行".into())
        );
        let working = serde_json::json!({"opened":null,"requests":[{}]});
        assert_eq!(
            client_labels(Some(&working)),
            ("—".into(), "1 个请求中".into())
        );
    }
}
