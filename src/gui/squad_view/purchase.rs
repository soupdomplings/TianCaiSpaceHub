//! The real checkout draft lives in the page. This component never sends requests.
use super::super::controls::{Choice, paint_surface};
use super::*;
use crate::desktop::{squad_money::Money, squad_quote};
use serde_json::json;

#[derive(Clone)]
pub(crate) struct Purchase {
    pub panel: Panel,
    pub submit: Button,
    pub status: StaticText,
    balance: Button,
    subscription: Button,
    amount_row: Panel,
    plan_row: Panel,
    amount: TextCtrl,
    method: Choice,
    plan: Choice,
    preview: StaticText,
    breakdown: StaticText,
    offer: StaticText,
    content: ScrolledWindow,
    columns: GridSizer,
    plan_grid: GridSizer,
    plan_cards: Rc<RefCell<Vec<Panel>>>,
    plan_buttons: Rc<RefCell<Vec<Button>>>,
    presets: Vec<(i32, Button)>,
    info: Rc<RefCell<Value>>,
    kind: Rc<Cell<bool>>,
    methods: Rc<RefCell<Vec<(String, Value)>>>,
    plans: Rc<RefCell<Vec<Value>>>,
    available: Rc<Cell<bool>>,
}
impl Purchase {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let tabs = BoxSizer::builder(Orientation::Horizontal).build();
        let balance = Button::builder(&panel)
            .with_label("余额充值")
            .with_kind(ButtonKind::Tab)
            .with_size(Size::new(112, 36))
            .build();
        let subscription = Button::builder(&panel)
            .with_label("订阅方案")
            .with_kind(ButtonKind::Tab)
            .with_size(Size::new(112, 36))
            .build();
        for b in [balance, subscription] {
            tabs.add(&b, 0, SizerFlag::Right, panel.from_dip(8));
        }
        col.add_sizer(&tabs, 0, SizerFlag::Bottom, panel.from_dip(12));
        let content = ScrolledWindow::builder(&panel)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        content.set_background_color(theme::theme().bg_app);
        content.set_scroll_rate(0, content.from_dip(12));
        content.set_min_size(Size::new(0, 0));
        let columns = GridSizer::builder(0, 2)
            .with_hgap(panel.from_dip(16))
            .with_vgap(panel.from_dip(16))
            .build();
        let input = Panel::builder(&content).build();
        paint_surface(input, theme::SURFACE_RADIUS);
        input.set_min_size(Size::new(0, -1));
        let input_col = BoxSizer::builder(Orientation::Vertical).build();
        let amount_row = Panel::builder(&input).build();
        amount_row.set_background_color(theme::theme().bg_card);
        let amounts = BoxSizer::builder(Orientation::Vertical).build();
        let title = StaticText::builder(&amount_row)
            .with_label("充值金额（元）")
            .build();
        title.set_foreground_color(theme::theme().ink_primary);
        title.set_background_color(theme::theme().bg_card);
        title.set_font(&theme::font(theme::TextRole::Title));
        amounts.add(&title, 0, SizerFlag::Bottom, panel.from_dip(8));
        let amount = TextCtrl::builder(&amount_row)
            .with_value("100")
            .with_size(Size::new(220, 42))
            .build();
        amount.set_name("squad-checkout-amount-cny");
        amount.set_font(&theme::font(theme::TextRole::Metric));
        amounts.add(&amount, 0, SizerFlag::Bottom, panel.from_dip(6));
        let presets = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let mut preset_buttons = Vec::new();
        for n in [50, 100, 200, 500] {
            let b = Button::builder(&amount_row)
                .with_label(&format!("¥{n}"))
                .with_kind(ButtonKind::Segment)
                .with_size(Size::new(64, 30))
                .build();
            b.on_click(move |_| amount.set_value(&n.to_string()));
            presets.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(6),
            );
            preset_buttons.push((n, b));
        }
        amounts.add_sizer(&presets, 0, SizerFlag::Expand, 0);
        amount_row.set_sizer(amounts, false);
        input_col.add(
            &amount_row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(16),
        );
        let plan_row = Panel::builder(&input).build();
        plan_row.set_background_color(theme::theme().bg_card);
        let plans_col = BoxSizer::builder(Orientation::Vertical).build();
        let plan = Choice::builder(&plan_row).build();
        plan.hide();
        let plan_grid = GridSizer::builder(0, 1)
            .with_hgap(panel.from_dip(10))
            .with_vgap(panel.from_dip(10))
            .build();
        plans_col.add_sizer(&plan_grid, 0, SizerFlag::Expand, 0);
        plan_row.set_sizer(plans_col, false);
        input_col.add(
            &plan_row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(16),
        );
        let offer = StaticText::builder(&input).build();
        offer.set_foreground_color(theme::theme().accent);
        offer.set_background_color(theme::theme().bg_card);
        offer.set_min_size(Size::new(0, -1));
        input_col.add(
            &offer,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            panel.from_dip(16),
        );
        let method_row = BoxSizer::builder(Orientation::Horizontal).build();
        let label = StaticText::builder(&input).with_label("支付方式").build();
        label.set_foreground_color(theme::theme().ink_secondary);
        label.set_background_color(theme::theme().bg_card);
        method_row.add(
            &label,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            panel.from_dip(16),
        );
        let method = Choice::builder(&input)
            .with_size(Size::new(142, 34))
            .build();
        method_row.add(&method, 0, SizerFlag::default(), 0);
        input_col.add_sizer(
            &method_row,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            panel.from_dip(16),
        );
        input.set_sizer(input_col, false);
        let summary = Panel::builder(&content).build();
        paint_surface(summary, theme::SURFACE_RADIUS);
        summary.set_min_size(Size::new(0, -1));
        let summary_col = BoxSizer::builder(Orientation::Vertical).build();
        let summary_title = StaticText::builder(&summary).with_label("订单预览").build();
        summary_title.set_font(&theme::font(theme::TextRole::Title));
        summary_title.set_foreground_color(theme::theme().ink_primary);
        summary_title.set_background_color(theme::theme().bg_card);
        summary_col.add(&summary_title, 0, SizerFlag::All, panel.from_dip(16));
        let preview = StaticText::builder(&summary).build();
        preview.set_font(&theme::font(theme::TextRole::Metric));
        preview.set_min_size(Size::new(0, -1));
        preview.set_foreground_color(theme::theme().accent);
        preview.set_background_color(theme::theme().bg_card);
        summary_col.add(
            &preview,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            panel.from_dip(16),
        );
        let breakdown = StaticText::builder(&summary).build();
        breakdown.set_foreground_color(theme::theme().ink_secondary);
        breakdown.set_background_color(theme::theme().bg_card);
        breakdown.set_min_size(Size::new(0, -1));
        summary_col.add(
            &breakdown,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(16),
        );
        let status = StaticText::builder(&summary).build();
        status.set_foreground_color(theme::theme().error);
        status.set_background_color(theme::theme().bg_card);
        summary_col.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            panel.from_dip(16),
        );
        let submit = Button::builder(&summary)
            .with_label("立即充值")
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(144, 38))
            .build();
        summary_col.add(
            &submit,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(16),
        );
        summary.set_sizer(summary_col, false);
        input.set_name("squad-purchase-selection");
        summary.set_name("squad-purchase-preview");
        columns.add(&input, 1, SizerFlag::Expand, 0);
        columns.add(&summary, 1, SizerFlag::Expand, 0);
        let scroll_col = BoxSizer::builder(Orientation::Vertical).build();
        scroll_col.add_sizer(&columns, 0, SizerFlag::Expand, 0);
        content.set_sizer(scroll_col, false);
        col.add(&content, 1, SizerFlag::Expand, 0);
        panel.set_sizer(col, false);
        let ui = Self {
            panel,
            submit,
            status,
            balance,
            subscription,
            amount_row,
            plan_row,
            amount,
            method,
            plan,
            preview,
            breakdown,
            offer,
            content,
            columns,
            plan_grid,
            plan_cards: Default::default(),
            plan_buttons: Default::default(),
            presets: preset_buttons,
            info: Default::default(),
            kind: Default::default(),
            methods: Default::default(),
            plans: Default::default(),
            available: Default::default(),
        };
        for (b, kind) in [(balance, false), (subscription, true)] {
            let u = ui.clone();
            b.on_click(move |_| u.select_subscription(kind));
        }
        for c in [method, plan] {
            let u = ui.clone();
            c.on_selection_changed(move |_| u.update());
        }
        let u = ui.clone();
        amount.on_text_changed(move |_| u.update());
        let u = ui.clone();
        content.on_size(move |event| {
            u.adapt();
            event.skip(true);
        });
        ui
    }
    pub fn clear(&self) {
        self.available.set(false);
        self.info.replace(Value::Null);
        self.amount.change_value("100");
        self.kind.set(false);
        self.update();
    }
    pub fn render(&self, info: &Value, ready: bool) {
        if *self.info.borrow() != *info {
            self.info.replace(info.clone());
            let methods = available_methods(info);
            let old_method = self
                .methods
                .borrow()
                .get(self.method.get_selection().unwrap_or(0) as usize)
                .map(|m| m.0.clone());
            let plans = available_plans(info);
            let old_plan = self
                .plans
                .borrow()
                .get(self.plan.get_selection().unwrap_or(0) as usize)
                .map(|p| p["id"].clone());
            self.method.clear();
            self.plan.clear();
            for (key, m) in &methods {
                self.method
                    .append(m["display_name"].as_str().unwrap_or(match key.as_str() {
                        "alipay" | "alipay_direct" => "支付宝",
                        "wxpay" | "wxpay_direct" => "微信支付",
                        "stripe" => "银行卡",
                        _ => key,
                    }));
            }
            for p in &plans {
                self.plan.append(&format!(
                    "{} · {} · {} 天",
                    p["name"].as_str().unwrap_or("订阅"),
                    Money::checkout(info).usd(&p["price"]),
                    p["validity_days"]
                        .as_i64()
                        .map(|n| n.to_string())
                        .unwrap_or("—".into())
                ));
            }
            if !methods.is_empty() {
                self.method.set_selection(
                    methods
                        .iter()
                        .position(|m| Some(&m.0) == old_method.as_ref())
                        .unwrap_or(0) as u32,
                );
            }
            if !plans.is_empty() {
                self.plan.set_selection(
                    plans
                        .iter()
                        .position(|p| Some(&p["id"]) == old_plan.as_ref())
                        .unwrap_or(0) as u32,
                );
            }
            self.methods.replace(methods);
            self.plans.replace(plans);
            self.rebuild_plans();
            if info["balance_disabled"] == true {
                self.kind.set(true);
            }
        }
        self.available.set(ready);
        self.update();
    }
    pub fn draft(&self) -> Result<Value, String> {
        let info = self.info.borrow();
        let methods = self.methods.borrow();
        let (key, method) = methods
            .get(self.method.get_selection().unwrap_or(0) as usize)
            .ok_or("当前没有可用的人民币支付方式")?;
        let plans = self.plans.borrow();
        let plan = plans.get(self.plan.get_selection().unwrap_or(0) as usize);
        checkout_body(
            &info,
            key,
            method,
            if self.kind.get() {
                "subscription"
            } else {
                "balance"
            },
            &self.amount.get_value(),
            plan,
        )
    }
    pub fn select_subscription(&self, enabled: bool) {
        self.kind.set(enabled);
        self.update();
    }
    pub fn set_ready(&self, ready: bool) {
        self.available.set(ready);
        self.update();
    }
    pub fn info(&self) -> Value {
        self.info.borrow().clone()
    }
    fn rebuild_plans(&self) {
        for card in self.plan_cards.borrow_mut().drain(..) {
            card.destroy();
        }
        self.plan_buttons.borrow_mut().clear();
        for (i, p) in self.plans.borrow().iter().enumerate() {
            let card = Panel::builder(&self.plan_row).build();
            paint_surface(card, 8);
            card.set_min_size(Size::new(0, -1));
            let col = BoxSizer::builder(Orientation::Vertical).build();
            let button = Button::builder(&card)
                .with_label(p["name"].as_str().unwrap_or("订阅方案"))
                .with_kind(ButtonKind::Segment)
                .with_size(Size::new(180, 34))
                .build();
            button.set_name(&format!("squad-subscription-plan-{i}"));
            button.set_min_size(Size::new(0, self.panel.from_dip(34)));
            button.set_tooltip(p["name"].as_str().unwrap_or("订阅方案"));
            let u = self.clone();
            button.on_click(move |_| {
                u.plan.set_selection(i as u32);
                u.update();
            });
            col.add(
                &button,
                0,
                SizerFlag::Expand | SizerFlag::All,
                card.from_dip(8),
            );
            let cost = StaticText::builder(&card)
                .with_label(&Money::checkout(&self.info.borrow()).usd(&p["price"]))
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            cost.set_min_size(Size::new(0, -1));
            cost.set_foreground_color(theme::theme().ink_primary);
            cost.set_background_color(theme::theme().bg_card);
            cost.set_font(&theme::font(theme::TextRole::Title));
            col.add(
                &cost,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                card.from_dip(12),
            );
            if let Some(days) = p["validity_days"].as_i64() {
                let duration = StaticText::builder(&card)
                    .with_label(&format!("{days} 天"))
                    .build();
                duration.set_foreground_color(theme::theme().ink_secondary);
                duration.set_background_color(theme::theme().bg_card);
                col.add(
                    &duration,
                    0,
                    SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                    card.from_dip(12),
                );
            }
            let quota = StaticText::builder(&card)
                .with_label(&plan_terms(p))
                .with_style(StaticTextStyle::EllipsizeEnd)
                .build();
            quota.set_tooltip(&plan_terms(p));
            quota.set_min_size(Size::new(0, -1));
            quota.set_foreground_color(theme::theme().ink_secondary);
            quota.set_background_color(theme::theme().bg_card);
            col.add(
                &quota,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                card.from_dip(12),
            );
            card.set_sizer(col, false);
            self.plan_grid.add(&card, 0, SizerFlag::Expand, 0);
            self.plan_cards.borrow_mut().push(card);
            self.plan_buttons.borrow_mut().push(button);
        }
    }
    fn adapt(&self) {
        let columns = if self.content.get_client_size().width >= self.panel.from_dip(580) {
            2
        } else {
            1
        };
        if columns != self.columns.get_cols() {
            self.columns.set_cols(columns);
        }
        self.content.layout();
        let plan_columns = (self.plan_row.get_client_size().width / self.panel.from_dip(120))
            .clamp(1, self.plans.borrow().len().clamp(1, 3) as i32);
        if plan_columns != self.plan_grid.get_cols() {
            self.plan_grid.set_cols(plan_columns);
        }
        self.content.layout();
        self.content.fit_inside();
    }
    fn update(&self) {
        let subscription = self.kind.get();
        let info = self.info.borrow();
        self.balance.show(info["balance_disabled"] != true);
        self.subscription.show(!self.plans.borrow().is_empty());
        self.balance.set_selected(!subscription);
        self.subscription.set_selected(subscription);
        self.amount_row.show(!subscription);
        self.plan_row.show(subscription);
        self.submit.set_label(if subscription {
            "购买订阅"
        } else {
            "立即充值"
        });
        for (i, b) in self.plan_buttons.borrow().iter().enumerate() {
            b.set_selected(self.plan.get_selection() == Some(i as u32));
        }
        for (n, b) in &self.presets {
            b.set_selected(self.amount.get_value().trim().parse::<i32>().ok() == Some(*n));
            b.enable(self.available.get());
        }
        let offers = info["recharge_bonus_tiers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tier| {
                Some(format!(
                    "满 {} 元{} {}%",
                    tier["min_amount"].as_f64()?,
                    if info["recharge_bonus_mode"] == "discount" {
                        "优惠"
                    } else {
                        "赠送"
                    },
                    tier["bonus_percent"].as_f64()?
                ))
            })
            .collect::<Vec<_>>()
            .join("  ·  ");
        self.offer.set_label(&offers);
        self.offer.show(!subscription && !offers.is_empty());
        self.method.enable(self.available.get());
        self.plan.enable(self.available.get());
        self.amount.enable(self.available.get());
        let result = self.draft();
        match result {
            Ok(body) => {
                let methods = self.methods.borrow();
                let method = &methods[self.method.get_selection().unwrap_or(0) as usize].1;
                let q = squad_quote::quote(
                    &info,
                    method,
                    body["order_type"].as_str().unwrap(),
                    body["amount"].as_f64().unwrap(),
                )
                .unwrap();
                let credit = q
                    .credited
                    .map(|v| format!("  ·  到账 {}", Money::checkout(&info).usd(&json!(v))))
                    .unwrap_or_default();
                self.preview
                    .set_label(&crate::desktop::squad_money::cny(q.total));
                let item = if subscription {
                    self.plans
                        .borrow()
                        .get(self.plan.get_selection().unwrap_or(0) as usize)
                        .map(|p| data::text(&p["name"]))
                        .unwrap_or_default()
                } else {
                    "余额充值".into()
                };
                self.breakdown.set_label(&format!(
                    "{item}\n金额    {}\n手续费    {}{}",
                    crate::desktop::squad_money::cny(q.pay_base),
                    crate::desktop::squad_money::cny(q.fee),
                    credit.replace("  ·  ", "\n")
                ));
                self.status.set_label("");
            }
            Err(e) => {
                self.preview.set_label("");
                self.breakdown.set_label("");
                self.status.set_label(&e);
            }
        }
        let width = self
            .preview
            .get_parent()
            .map(|p| p.get_client_size().width - self.panel.from_dip(32))
            .unwrap_or(self.panel.from_dip(220))
            .max(self.panel.from_dip(160));
        self.preview.wrap(width);
        self.status.wrap(width);
        self.breakdown.wrap(width);
        self.status.show(!self.status.get_label().is_empty());
        self.submit
            .enable(self.available.get() && self.draft().is_ok());
        self.panel.layout();
        self.adapt();
    }
}
fn plan_terms(plan: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(name) = plan["group_name"].as_str() {
        if !name.is_empty() {
            parts.push(name.to_string());
        }
    }
    for (key, label) in [
        ("daily_limit_usd", "每日"),
        ("weekly_limit_usd", "每周"),
        ("monthly_limit_usd", "每月"),
    ] {
        if plan[key].is_number() {
            parts.push(format!("{label} {}", Money::default().usd(&plan[key])));
        }
    }
    if parts.is_empty() {
        plan["description"]
            .as_str()
            .unwrap_or("按站点方案额度生效")
            .to_string()
    } else {
        parts.join("\n")
    }
}
pub(crate) fn available_methods(info: &Value) -> Vec<(String, Value)> {
    info["methods"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| {
            v["available"] != false
                && v["daily_remaining"].as_f64().is_none_or(|v| v > 0.)
                && v["currency"]
                    .as_str()
                    .unwrap_or("CNY")
                    .eq_ignore_ascii_case("CNY")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
fn available_plans(info: &Value) -> Vec<Value> {
    info["plans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["for_sale"] != false)
        .cloned()
        .collect()
}
pub(crate) fn checkout_body(
    info: &Value,
    method_key: &str,
    method: &Value,
    kind: &str,
    amount: &str,
    plan: Option<&Value>,
) -> Result<Value, String> {
    if kind == "balance" && info["balance_disabled"] == true {
        return Err("站点当前未开放余额充值".into());
    }
    if !matches!(kind, "balance" | "subscription") {
        return Err("请选择充值或订阅".into());
    }
    if method["daily_remaining"].as_f64().is_some_and(|n| n <= 0.) {
        return Err("此支付方式今日额度已用完，请刷新后更换".into());
    }
    let price = if kind == "subscription" {
        let p = plan
            .filter(|p| p["for_sale"] != false && p["id"].as_u64().is_some())
            .ok_or("请选择在售订阅")?;
        p["price"].as_f64().ok_or("站点未返回订阅价格")?
    } else {
        amount.trim().parse::<f64>().map_err(|_| "请填写充值金额")?
    };
    if !method["currency"]
        .as_str()
        .unwrap_or("CNY")
        .eq_ignore_ascii_case("CNY")
    {
        return Err("请选择人民币支付方式".into());
    }
    if kind == "balance" {
        if info["global_min"]
            .as_f64()
            .is_some_and(|n| n > 0. && price < n)
        {
            return Err(format!("最低充值 {} 元", info["global_min"]));
        }
        if info["global_max"]
            .as_f64()
            .is_some_and(|n| n > 0. && price > n)
        {
            return Err(format!("最高充值 {} 元", info["global_max"]));
        }
    }
    let q = squad_quote::quote(info, method, kind, price).map_err(|e| e.to_string())?;
    squad_quote::validate_method(method, &q).map_err(|e| e.to_string())?;
    let mut body = json!({"order_type":kind,"payment_type":method_key,"amount":price,"is_mobile":false,
        "return_url":format!("{}/payment/result",crate::desktop::squad::ORIGIN)});
    if kind == "subscription" {
        body["plan_id"] = plan.unwrap()["id"].clone();
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_checkout_preserves_server_amount_and_validates_limits() {
        let info = json!({"global_min":10,"global_max":1000,"subscription_usd_to_cny_rate":7.15});
        let m = json!({"currency":"CNY","single_max":500});
        let b = checkout_body(&info, "alipay", &m, "balance", "100", None).unwrap();
        assert_eq!(b["amount"], 100.);
        assert_eq!(b["payment_type"], "alipay");
        let p = json!({"id":7,"price":10,"for_sale":true});
        let b = checkout_body(&info, "alipay", &m, "subscription", "", Some(&p)).unwrap();
        assert_eq!(b["amount"], 10.);
        assert_eq!(b["plan_id"], 7);
        for value in ["", "NaN", "inf", "-1", "0", "5", "1001"] {
            assert!(checkout_body(&info, "alipay", &m, "balance", value, None).is_err());
        }
        assert!(
            checkout_body(
                &info,
                "alipay",
                &m,
                "subscription",
                "",
                Some(&json!({"id":7,"price":10,"for_sale":false}))
            )
            .is_err()
        );
        assert!(
            checkout_body(
                &json!({"balance_disabled":true}),
                "alipay",
                &m,
                "balance",
                "100",
                None
            )
            .is_err()
        );
    }
}
