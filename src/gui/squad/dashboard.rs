//! Production account overview. Pure presentation; no requests or configuration.
use super::*;
use wxdragon::dc::GraphicsDC;

#[derive(Clone)]
pub(super) struct Dashboard {
    pub panel: Panel,
    snapshot: Rc<RefCell<(Value, Value, crate::desktop::squad_money::Money)>>,
    hover: Rc<Cell<Option<usize>>>,
}
impl Dashboard {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_style(BackgroundStyle::Paint);
        panel.set_background_color(theme::theme().bg_app);
        panel.set_min_size(visual::dip_size(&panel, 240, 160));
        visual::redraw_on_resize(panel);
        let ui = Self {
            panel,
            snapshot: Default::default(),
            hover: Default::default(),
        };
        let u = ui.clone();
        panel.on_paint(move |_| u.paint());
        let u = ui.clone();
        panel.on_mouse_motion(move |e| {
            if let WindowEventData::MouseMotion(ref event) = e
                && let Some(pos) = event.get_position()
            {
                let s = visual::scale(&panel);
                let w = panel.get_client_size().width as f64 / s;
                let snapshot = u.snapshot.borrow();
                let rows = snapshot.1["trend"]["trend"].as_array();
                let next = rows.filter(|r| !r.is_empty()).and_then(|r| {
                    let x = pos.x as f64 / s;
                    let y = pos.y as f64 / s;
                    (y >= 162. && x >= 60. && x <= w - 24.)
                        .then(|| (((x - 60.) / (w - 84.)) * (r.len() - 1) as f64).round() as usize)
                });
                if u.hover.replace(next) != next {
                    panel.refresh(false, None);
                }
            }
            e.skip(true);
        });
        let hover = ui.hover.clone();
        panel.on_mouse_leave(move |e| {
            hover.set(None);
            panel.refresh(false, None);
            e.skip(true);
        });
        ui
    }
    pub fn render(&self, account: &Value, data: &Value, money: crate::desktop::squad_money::Money) {
        self.snapshot
            .replace((account.clone(), data.clone(), money));
        self.hover.set(None);
        self.panel.refresh(false, None);
    }
    fn paint(&self) {
        let base = AutoBufferedPaintDC::new(&self.panel);
        let dc = GraphicsDC::new(&base);
        let scale = visual::scale(&self.panel);
        let c = Canvas::new(&dc, scale);
        let t = theme::theme();
        c.clear(t.bg_app);
        let size = self.panel.get_client_size();
        let w = (size.width as f64 / scale).round() as i32;
        let h = (size.height as f64 / scale).round() as i32;
        let snapshot = self.snapshot.borrow();
        let (user, data, money) = &*snapshot;
        let stats = &data["stats"];
        let labels = ["账户余额", "今日扣费", "今日请求", "可用 API 密钥"];
        let values = [
            money.usd(&user["balance"]),
            money.usd(&stats["today_actual_cost"]),
            data::compact(&stats["today_requests"]),
            data::compact(&stats["active_api_keys"]),
        ];
        let gap = 10;
        let cw = (w - gap * 3) / 4;
        for i in 0..4 {
            let x = i as i32 * (cw + gap);
            c.rounded(
                Rect::new(x, 0, cw, 84),
                12,
                if i == 0 { t.bg_muted } else { t.bg_card },
                None,
            );
            c.label(
                labels[i],
                Rect::new(x + 16, 12, cw - 28, 20),
                12,
                false,
                t.ink_secondary,
                false,
            );
            c.label(
                &values[i],
                Rect::new(x + 16, 37, cw - 28, 31),
                if cw < 160 { 21 } else { 25 },
                true,
                if i == 0 { t.accent } else { t.ink_primary },
                false,
            );
        }
        if h < 140 {
            return;
        }
        c.rounded(Rect::new(0, 98, w, (h - 98).max(1)), 12, t.bg_card, None);
        c.label(
            "站点扣费趋势（元）",
            Rect::new(20, 108, w - 150, 26),
            14,
            true,
            t.ink_primary,
            false,
        );
        c.label(
            "最近 7 天",
            Rect::new(w - 100, 108, 80, 26),
            12,
            false,
            t.ink_secondary,
            true,
        );
        let Some(rows) = data["trend"]["trend"].as_array().filter(|r| !r.is_empty()) else {
            c.label(
                "暂无用量记录",
                Rect::new(20, 153, w - 40, (h - 170).max(24)),
                14,
                false,
                t.ink_secondary,
                true,
            );
            return;
        };
        let gx = 60;
        let gy = 158;
        let gw = (w - 84).max(1);
        let gh = (h - 203).max(20);
        let max = rows
            .iter()
            .filter_map(|r| r["actual_cost"].as_f64())
            .fold(0., f64::max)
            .max(0.01)
            * 1.15;
        for tick in 0..=3 {
            let y = gy + gh * tick / 3;
            c.line(Point::new(gx, y), Point::new(gx + gw, y), t.divider, false);
            c.label(
                &money
                    .convert(max * (3 - tick) as f64 / 3.)
                    .map(|n| format!("{n:.2}"))
                    .unwrap_or("—".into()),
                Rect::new(8, y - 9, 44, 18),
                11,
                false,
                t.ink_secondary,
                false,
            );
        }
        let points: Vec<Point> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                Point::new(
                    gx + if rows.len() > 1 {
                        gw * i as i32 / (rows.len() - 1) as i32
                    } else {
                        gw / 2
                    },
                    gy + gh
                        - (r["actual_cost"].as_f64().unwrap_or(0.) / max * gh as f64).round()
                            as i32,
                )
            })
            .collect();
        // A monotone ease between samples avoids overshooting into negative costs.
        let mut line = Vec::new();
        for pair in points.windows(2) {
            for step in 0..=20 {
                let v = step as f64 / 20.;
                let ease = v * v * (3. - 2. * v);
                line.push(wxdragon::dc::Point::new(
                    c.px(pair[0].x + ((pair[1].x - pair[0].x) as f64 * v).round() as i32),
                    c.px(pair[0].y + ((pair[1].y - pair[0].y) as f64 * ease).round() as i32),
                ));
            }
        }
        if line.len() > 1 {
            let mut area = line.clone();
            area.push(wxdragon::dc::Point::new(
                c.px(points.last().unwrap().x),
                c.px(gy + gh),
            ));
            area.push(wxdragon::dc::Point::new(c.px(points[0].x), c.px(gy + gh)));
            dc.set_pen(t.bg_muted, 1, PenStyle::Transparent);
            dc.set_brush(t.bg_muted, BrushStyle::Solid);
            dc.draw_polygon(&area, 0, 0, wxdragon::dc::PolygonFillMode::OddEven);
            dc.set_pen(t.accent, c.px(2).max(1), PenStyle::Solid);
            dc.draw_lines(&line, 0, 0);
        }
        for (i, p) in points.iter().enumerate() {
            c.dot(p.x, p.y, 3, t.accent);
            if i == 0 || i + 1 == points.len() || i == points.len() / 2 {
                let label = rows[i]["date"].as_str().unwrap_or("");
                let short = if label.len() >= 10 {
                    &label[5..10]
                } else {
                    label
                };
                c.label(
                    short,
                    Rect::new((p.x - 30).clamp(2, (w - 62).max(2)), gy + gh + 10, 60, 20),
                    11,
                    false,
                    t.ink_secondary,
                    true,
                );
            }
        }
        if let Some(i) = self.hover.get().filter(|i| *i < rows.len()) {
            let p = points[i];
            c.line(
                Point::new(p.x, gy),
                Point::new(p.x, gy + gh),
                t.accent,
                true,
            );
            let x = (p.x - 90).clamp(60, (w - 206).max(60));
            c.rounded(Rect::new(x, gy, 182, 64), 8, t.bg_app, Some(t.divider));
            c.label(
                rows[i]["date"].as_str().unwrap_or(""),
                Rect::new(x + 12, gy + 7, 158, 20),
                12,
                false,
                t.ink_secondary,
                false,
            );
            c.label(
                &money.usd(&rows[i]["actual_cost"]),
                Rect::new(x + 12, gy + 30, 158, 24),
                16,
                true,
                t.accent,
                false,
            );
        }
    }
}
