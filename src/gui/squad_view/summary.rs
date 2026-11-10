use super::*;
use wxdragon::dc::GraphicsDC;

#[derive(Clone)]
pub(super) struct Summary {
    pub panel: Panel,
    snapshot: Rc<RefCell<(String, Value, Value, crate::desktop::squad_money::Money)>>,
}
impl Summary {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_style(BackgroundStyle::Paint);
        panel.set_background_color(theme::theme().bg_app);
        visual::redraw_on_resize(panel);
        let ui = Self {
            panel,
            snapshot: Default::default(),
        };
        let u = ui.clone();
        panel.on_paint(move |_| u.paint());
        ui
    }
    pub fn render(
        &self,
        key: &str,
        user: &Value,
        data: &Value,
        money: crate::desktop::squad_money::Money,
    ) {
        self.snapshot
            .replace((key.into(), user.clone(), data.clone(), money));
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
        let (key, user, data, money) = &*snapshot;
        if key == "profile" {
            let profile = if data["email"].is_string() {
                data
            } else {
                user
            };
            let card_h = h.min(366);
            let compact = card_h < 280;
            c.rounded(Rect::new(0, 0, w, card_h), 12, t.bg_card, None);
            c.rounded(Rect::new(22, 16, 44, 44), 14, t.bg_muted, None);
            c.icon(Icon::Squad, Rect::new(32, 26, 24, 24), t.accent);
            c.label(
                profile["username"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("我的账户"),
                Rect::new(82, 12, w - 106, 26),
                19,
                true,
                t.ink_primary,
                false,
            );
            c.label(
                profile["email"].as_str().unwrap_or("未设置邮箱"),
                Rect::new(82, 39, w - 106, 22),
                13,
                false,
                t.ink_secondary,
                false,
            );
            let divider = if compact { 72 } else { 85 };
            c.line(
                Point::new(22, divider),
                Point::new(w - 22, divider),
                t.divider,
                false,
            );
            let extra = profile["balance_notify_extra_emails"]
                .as_array()
                .map(|a| a.iter().filter(|e| e["disabled"] != true).count());
            let fields = [
                ("账户状态", data::cell(profile, "status")),
                (
                    "余额提醒",
                    match profile["balance_notify_enabled"].as_bool() {
                        Some(true) => "已开启".into(),
                        Some(false) => "已关闭".into(),
                        None => "—".into(),
                    },
                ),
                ("提醒阈值", money.usd(&profile["balance_notify_threshold"])),
                ("并发上限", data::text(&profile["concurrency"])),
                (
                    "额外通知邮箱",
                    extra.map(|n| format!("{n} 个")).unwrap_or("—".into()),
                ),
                (
                    "加入时间",
                    profile["created_at"]
                        .as_str()
                        .map(|s| s.chars().take(10).collect())
                        .unwrap_or("—".into()),
                ),
            ];
            let first_y = divider + 12;
            let rh = ((card_h - first_y - 10) / 3).clamp(34, 78);
            for (i, (label, value)) in fields.iter().enumerate() {
                let x = 24 + (i % 2) as i32 * (w - 48) / 2;
                let y = first_y + (i / 2) as i32 * rh;
                c.label(
                    label,
                    Rect::new(x, y, (w - 64) / 2, if compact { 15 } else { 18 }),
                    if compact { 11 } else { 12 },
                    false,
                    t.ink_secondary,
                    false,
                );
                c.label(
                    value,
                    Rect::new(
                        x,
                        y + if compact { 17 } else { 21 },
                        (w - 64) / 2,
                        if compact { 21 } else { 25 },
                    ),
                    if compact { 14 } else { 15 },
                    true,
                    t.ink_primary,
                    false,
                );
            }
            return;
        }
        if key == "affiliate" {
            let fields = [
                ("可转入余额", money.usd(&data["aff_quota"])),
                ("累计返利", money.usd(&data["aff_history_quota"])),
                ("已邀请用户", data::compact(&data["aff_count"])),
            ];
            let cw = (w - 20) / 3;
            for (i, (label, value)) in fields.iter().enumerate() {
                let x = i as i32 * (cw + 10);
                c.rounded(Rect::new(x, 0, cw, 110), 12, t.bg_card, None);
                c.label(
                    label,
                    Rect::new(x + 16, 18, cw - 32, 22),
                    13,
                    false,
                    t.ink_secondary,
                    false,
                );
                c.label(
                    value,
                    Rect::new(x + 16, 54, cw - 32, 32),
                    24,
                    true,
                    t.accent,
                    false,
                );
            }
            c.label(
                "邀请码",
                Rect::new(20, 145, w - 40, 24),
                13,
                false,
                t.ink_secondary,
                false,
            );
            c.label(
                data["aff_code"].as_str().unwrap_or("—"),
                Rect::new(20, 175, w - 40, 40),
                22,
                true,
                t.ink_primary,
                false,
            );
            return;
        }
        let (title, note) = match key.as_str() {
            "loading" => ("正在读取账户数据", ""),
            "error" => ("未能读取此页", "点击右上角的刷新按钮重试"),
            "keys" => ("还没有 API 密钥", "新建密钥后，可直接导入 Hub"),
            "usage" => ("暂无使用记录", "可以调整日期范围"),
            "orders" => ("暂时没有订单", ""),
            "subscriptions" => ("还没有订阅", ""),
            "redeem" => ("暂无兑换记录", ""),
            "organization" => ("暂无可用的扣费身份", ""),
            key if key.starts_with("custom-") => (
                data["label"].as_str().unwrap_or("站内工具"),
                data["description"]
                    .as_str()
                    .or(data["url"].as_str())
                    .unwrap_or("通过右上角入口打开工具"),
            ),
            _ => ("暂无记录", ""),
        };
        c.rounded(Rect::new(0, 0, w, h.max(1)), 12, t.bg_card, None);
        let cy = (h / 2 - 55).max(20);
        c.rounded(Rect::new(w / 2 - 24, cy, 48, 48), 14, t.bg_muted, None);
        c.icon(
            if key == "keys" {
                Icon::Channel
            } else if key == "error" {
                Icon::Help
            } else {
                Icon::Squad
            },
            Rect::new(w / 2 - 12, cy + 12, 24, 24),
            t.accent,
        );
        c.label(
            title,
            Rect::new(20, cy + 62, w - 40, 28),
            17,
            true,
            t.ink_primary,
            true,
        );
        c.label(
            note,
            Rect::new(24, cy + 98, w - 48, 26),
            13,
            false,
            t.ink_secondary,
            true,
        );
    }
}
