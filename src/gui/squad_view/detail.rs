//! Business summaries shared by real secondary pages and the hidden native review.
//! Internal identities, permissions and arbitrary response keys never become a field table.
use super::*;
use crate::desktop::squad_money::{self, Money};
#[derive(Clone, Debug)]
pub(crate) struct Group {
    pub title: String,
    pub fields: Vec<(String, String)>,
}

pub(crate) fn formatted(key: &str, v: &Value, money: Money) -> String {
    if key == "allocation_mode" {
        return super::organization_data::mode(v).into();
    }
    if key == "closed" {
        return match v.as_bool() {
            Some(true) => "已出账",
            Some(false) => "未出账",
            None => "—",
        }
        .into();
    }
    if key.ends_with("_cents") {
        return v
            .as_f64()
            .map(|n| squad_money::cny(n / 100.))
            .unwrap_or("—".into());
    }
    if squad_money::ledger_field(key) {
        return money.usd(v);
    }
    if key.ends_with("tokens") || matches!(key, "total_requests" | "today_requests") {
        return data::compact(v);
    }
    if let Some(s) = v.as_str() {
        return match s {
            "active" | "enabled" => "使用中",
            "inactive" | "disabled" => "未使用",
            "admin_locked" => "管理员设置",
            "self_service" | "user_selectable" => "自行选择",
            "daily" | "day" => "每天",
            "weekly" | "week" => "每周",
            "monthly" | "month" => "每月",
            "fixed" => "固定额度",
            "proportional" => "按比例",
            "unlimited" => "不限额",
            "business_unit" => "组织统一预算",
            "department" => "按部门管理",
            "equal" => "平均分配",
            "custom" => "自定义比例",
            "total" => "累计",
            "none" => "不分组",
            "PENDING" | "pending" => "待支付",
            "COMPLETED" | "completed" => "已完成",
            "PAID" | "paid" => "已支付",
            "EXPIRED" | "expired" => "已过期",
            "CANCELLED" | "cancelled" => "已取消",
            _ => s,
        }
        .into();
    }
    match v {
        Value::Bool(v) => if *v { "已开启" } else { "未开启" }.into(),
        Value::Number(v) => v.to_string(),
        _ => "—".into(),
    }
}
pub(crate) fn groups(data: &Value, money: Money) -> Vec<Group> {
    const SECTIONS: &[(&str, &[(&str, &str)])] = &[
        (
            "基本信息",
            &[
                ("name", "名称"),
                ("business_unit_name", "组织"),
                ("department_name", "部门"),
                ("username", "用户名"),
                ("user_email", "成员邮箱"),
                ("email", "邮箱"),
                ("group_name", "渠道分组"),
                ("status", "状态"),
                ("description", "说明"),
                ("is_active_billing", "当前扣费身份"),
                ("control_mode", "身份管理"),
                ("allow_personal_balance_fallback", "个人余额兜底"),
            ],
        ),
        (
            "用量与扣费",
            &[
                ("today_usage", "今日扣费"),
                ("month_usage", "本月扣费"),
                ("actual_cost", "实际扣费"),
                ("total_actual_cost", "累计扣费"),
                ("total_cost", "累计扣费"),
                ("today_requests", "今日请求"),
                ("total_requests", "累计请求"),
                ("input_tokens", "输入用量"),
                ("output_tokens", "输出用量"),
                ("today_tokens", "今日用量"),
                ("month_tokens", "本月用量"),
                ("business_unit_today_usage", "组织今日扣费"),
                ("business_unit_month_usage", "组织本月扣费"),
                ("business_unit_month_tokens", "组织本月用量"),
                ("total_tokens", "累计用量"),
            ],
        ),
        (
            "资金与额度",
            &[
                ("balance", "余额"),
                ("quota_used", "已用额度"),
                ("quota", "额度上限"),
                ("available_amount", "可用额度"),
                ("limit_amount", "额度"),
                ("used_amount", "已用额度"),
                ("remaining_amount", "剩余额度"),
                ("reserved_amount", "已预留额度"),
                ("budget_limit_amount", "组织额度"),
                ("configured_share", "配置比例"),
                ("projected_amount", "预计分配"),
                ("current_period_allocated_amount", "本期已分配"),
                ("current_period_remaining_amount", "本期剩余"),
                ("daily_limit", "日额度"),
                ("weekly_limit", "周额度"),
                ("monthly_limit", "月额度"),
                ("min_limit", "额度下限"),
                ("max_limit", "额度上限"),
                ("balance_cents", "预存余额"),
                ("available_cents", "可用余额"),
                ("reserved_cents", "预留金额"),
                ("outstanding_cents", "挂账余额"),
                ("due_cents", "应清金额"),
                ("amount_cents", "金额"),
                ("paid_cents", "已清金额"),
            ],
        ),
        (
            "规则与周期",
            &[
                ("period_type", "周期"),
                ("allocation_mode", "分配方式"),
                ("timezone", "时区"),
                ("boundary_day", "结算日"),
                ("grace_days", "宽限天数"),
                ("enforcement_enabled", "逾期限制"),
                ("daily_enabled", "每日限额"),
                ("weekly_enabled", "每周限额"),
                ("monthly_enabled", "每月限额"),
                ("enabled", "启用"),
                ("starts_at", "开始时间"),
                ("ends_at", "结束时间"),
                ("expires_at", "到期时间"),
                ("created_at", "创建时间"),
                ("closed", "账期状态"),
                ("remark", "备注"),
                ("effective_at", "生效时间"),
                ("period_start", "周期开始"),
                ("period_end", "周期结束"),
                ("recipient_email", "接收邮箱"),
            ],
        ),
        (
            "发票",
            &[
                ("title", "抬头"),
                ("tax_number", "税号"),
                ("tax_cents", "税费"),
                ("seller_name", "销售方"),
                ("item_name", "项目"),
                ("invoice_no", "发票号码"),
            ],
        ),
        (
            "请求",
            &[
                ("model", "模型"),
                ("model_name", "模型"),
                ("request_type", "请求类型"),
                ("stream", "流式响应"),
                ("duration_ms", "耗时（毫秒）"),
                ("out_trade_no", "订单号"),
                ("reason", "原因"),
                ("message", "结果"),
            ],
        ),
    ];
    let mut out = Vec::new();
    fn collect(
        data: &Value,
        money: Money,
        sections: &[(&str, &[(&str, &str)])],
        prefix: &str,
        out: &mut Vec<Group>,
        depth: usize,
    ) {
        if depth > 3 || out.len() >= 24 {
            return;
        }
        if let Some(items) = data.as_array() {
            for (i, item) in items.iter().take(30).enumerate() {
                let who = item["name"]
                    .as_str()
                    .or(item["user_email"].as_str())
                    .or(item["department_name"].as_str())
                    .map(str::to_string)
                    .unwrap_or(format!("第 {} 项", i + 1));
                collect(
                    item,
                    money,
                    sections,
                    &format!("{prefix}{who} · "),
                    out,
                    depth + 1,
                );
            }
            return;
        }
        for (title, fields) in sections {
            let fields = fields
                .iter()
                .filter_map(|(key, label)| {
                    data.get(*key)
                        .filter(|v| !v.is_null() && !v.is_array() && !v.is_object())
                        .map(|v| {
                            (
                                (*label).into(),
                                if *key == "input_tokens" {
                                    formatted(key, &super::data::input(data, ""), money)
                                } else {
                                    formatted(key, v, money)
                                },
                            )
                        })
                })
                .collect::<Vec<_>>();
            if !fields.is_empty() {
                out.push(Group {
                    title: format!("{prefix}{title}"),
                    fields,
                });
            }
        }
        if data["pay_amount"].is_number() {
            out.push(Group {
                title: format!("{prefix}支付金额"),
                fields: vec![(
                    "支付金额".into(),
                    money.currency(&data["pay_amount"], data["currency"].as_str().unwrap_or("")),
                )],
            });
        }
        for (key, label) in [
            ("budget_policy", "预算"),
            ("budget", "预算"),
            ("daily", "每日限额"),
            ("weekly", "每周限额"),
            ("monthly", "每月限额"),
            ("summary", "用量汇总"),
            ("members", "成员"),
            ("items", "条目"),
            ("funding_sources", "扣费来源"),
            ("departments", "部门"),
            ("lines", "账单明细"),
            ("wallets", "预存账户"),
            ("report", "用量报告"),
        ] {
            if let Some(v) = data.get(key).filter(|v| v.is_object() || v.is_array()) {
                collect(
                    v,
                    money,
                    sections,
                    &format!("{prefix}{label} · "),
                    out,
                    depth + 1,
                );
            }
        }
    }
    collect(data, money, SECTIONS, "", &mut out, 0);
    out
}

#[derive(Clone)]
pub(crate) struct Detail {
    pub panel: ScrolledWindow,
    body: BoxSizer,
    children: Rc<RefCell<Vec<Panel>>>,
}
impl Detail {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let panel = ScrolledWindow::builder(parent)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        panel.set_background_color(theme::theme().bg_app);
        panel.set_scroll_rate(0, 12);
        panel.show_scroll_indicators(false);
        let body = BoxSizer::builder(Orientation::Vertical).build();
        panel.set_sizer(body, false);
        Self {
            panel,
            body,
            children: Default::default(),
        }
    }
    pub fn render(&self, title: &str, data: &Value, money: Money) {
        self.panel.freeze();
        for p in self.children.borrow_mut().drain(..) {
            p.destroy();
        }
        let mut sections = groups(data, money);
        if sections.is_empty() {
            sections.push(Group {
                title: title.into(),
                fields: vec![(
                    "当前内容".into(),
                    "暂无可展示的业务数据，请刷新后重试".into(),
                )],
            });
        }
        if sections.len() > 1 && sections.iter().map(|s| s.fields.len()).sum::<usize>() <= 6 {
            sections = vec![Group {
                title: title.into(),
                fields: sections.into_iter().flat_map(|s| s.fields).collect(),
            }];
        }
        for section in sections {
            let card = Panel::builder(&self.panel).build();
            card.set_background_color(theme::theme().bg_card);
            let col = BoxSizer::builder(Orientation::Vertical).build();
            let heading = StaticText::builder(&card)
                .with_label(&section.title)
                .build();
            heading.set_font(&theme::font(theme::TextRole::Title));
            heading.set_foreground_color(theme::theme().accent);
            col.add(&heading, 0, SizerFlag::All, card.from_dip(16));
            let grid = FlexGridSizer::builder(0, 2)
                .with_vgap(card.from_dip(14))
                .with_hgap(card.from_dip(22))
                .build();
            grid.add_growable_col(0, 1);
            grid.add_growable_col(1, 1);
            for (label, value) in section.fields {
                let field = Panel::builder(&card).build();
                field.set_background_color(theme::theme().bg_card);
                let pair = BoxSizer::builder(Orientation::Vertical).build();
                let caption = StaticText::builder(&field).with_label(&label).build();
                caption.set_foreground_color(theme::theme().ink_secondary);
                caption.set_font(&theme::font(theme::TextRole::Caption));
                let text = StaticText::builder(&field).with_label(&value).build();
                text.set_foreground_color(theme::theme().ink_primary);
                text.set_font(&theme::font(theme::TextRole::Body));
                text.wrap(field.from_dip(280));
                pair.add(&caption, 0, SizerFlag::Bottom, field.from_dip(5));
                pair.add(&text, 0, SizerFlag::Expand, 0);
                field.set_sizer(pair, false);
                let resize = text;
                field.on_size(move |e| {
                    resize.wrap((field.get_client_size().width - 4).max(60));
                    e.skip(true);
                });
                grid.add(&field, 1, SizerFlag::Expand, 0);
            }
            col.add_sizer(
                &grid,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                card.from_dip(16),
            );
            card.set_sizer(col, false);
            self.body.add(
                &card,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                self.panel.from_dip(12),
            );
            self.children.borrow_mut().push(card);
        }
        self.panel.layout();
        self.panel.fit_inside();
        self.panel.thaw();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn membership_detail_is_business_content_without_internal_fields_or_credentials() {
        let v = json!({"id":72,"business_unit_id":88,"business_unit_name":"合成团队","department_name":"研发","month_usage":12,"month_tokens":5000,"can_manage":true,"context_version":7,"secret":"must-hide","request_body":"must-hide","unknown_internal_flag":true});
        let s = groups(&v, Money::default());
        let all = format!("{s:?}");
        assert!(
            all.contains("合成团队")
                && all.contains("研发")
                && all.contains("¥12.00")
                && all.contains("本月用量")
        );
        for hidden in [
            "business_unit_id",
            "can_manage",
            "context_version",
            "must-hide",
            "unknown_internal_flag",
        ] {
            assert!(!all.contains(hidden));
        }
        assert_eq!(
            formatted("available_cents", &json!(1250), Money::default()),
            "¥12.50"
        );
    }
}
