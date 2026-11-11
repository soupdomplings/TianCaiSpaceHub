//! Native account forms keep server identities and unknown values while editing fields.
use super::controls::{Button, ButtonKind, Choice, TextCtrl, paint_surface};
use super::{api::ApiClient, dialog_job, squad_dialog::SquadDialog, theme};
use serde_json::{Value, json};
use wxdragon::prelude::*;

pub(super) fn label(key: &str) -> &str {
    match key {
        "id" => "编号",
        "name" => "名称",
        "username" => "用户名",
        "query" => "邮箱或用户名",
        "email" => "邮箱",
        "balance" => "余额",
        "status" => "状态",
        "created_at" => "创建时间",
        "expires_at" => "到期时间",
        "updated_at" => "更新时间",
        "group_id" => "渠道分组",
        "fallback_group_id" => "备用渠道分组",
        "quota" => "额度上限",
        "quota_used" => "已用额度",
        "expires_in_days" => "有效天数",
        "ip_whitelist" => "允许的 IP",
        "ip_blacklist" => "禁止的 IP",
        "rate_limit_5h" => "五小时限额",
        "rate_limit_1d" => "日限额",
        "rate_limit_7d" => "周限额",
        "allow" => "允许个人余额兜底",
        "membership_id" => "部门身份编号",
        "user_id" => "成员编号",
        "department_id" => "部门编号",
        "business_unit_id" => "组织编号",
        "description" => "说明",
        "code" => "代码",
        "enabled" => "启用",
        "amount" => "金额",
        "pay_amount" => "支付金额",
        "payment_type" => "支付方式",
        "order_type" => "订单类型",
        "plan_id" => "订阅方案编号",
        "out_trade_no" => "订单号",
        "reason" => "原因",
        "source_type" => "资金来源",
        "old_password" => "原密码",
        "new_password" => "新密码",
        "balance_notify_enabled" => "余额提醒",
        "balance_notify_threshold" => "提醒阈值",
        "avatar_url" => "头像地址",
        "verify_code" | "verification_code" => "验证码",
        "password" => "密码",
        "totp_code" => "动态验证码",
        "code_value" => "兑换码",
        "model" | "model_name" => "模型",
        "actual_cost" => "实际扣费",
        "input_tokens" => "输入用量",
        "output_tokens" => "输出用量",
        "concurrency" => "并发数",
        "control_mode" => "控制方式",
        "context_version" => "配置版本",
        "allow_personal_balance_fallback" => "个人余额兜底",
        "business_unit_name" => "组织",
        "department_name" => "部门",
        "user_email" => "成员邮箱",
        "member_name" => "成员",
        "configured_share" => "配置比例",
        "projected_amount" => "预计分配",
        "current_period_allocated_amount" => "本期已分配",
        "current_period_used_amount" => "本期已用",
        "current_period_remaining_amount" => "本期剩余",
        "due_cents" => "应清金额（元）",
        "paid_cents" => "已清金额（元）",
        "closed" => "账期状态",
        "today_usage" => "今日使用",
        "month_usage" => "本月使用",
        "budget_mode" => "预算方式",
        "daily_limit" => "日额度",
        "weekly_limit" => "周额度",
        "monthly_limit" => "月额度",
        "sort_order" => "排序",
        "timezone" => "时区",
        "is_active_billing" => "当前扣费身份",
        "max_tokens" => "输出上限",
        "period" => "周期",
        "operation" => "调整方式",
        "effective_mode" => "生效方式",
        "group_ids" => "可用渠道分组",
        "source_department_id" => "转出部门",
        "target_department_id" => "转入部门",
        "kind" => "资金类型",
        "items" => "条目",
        "order_id" => "订单编号",
        "paid" => "已支付",
        "total" => "总计",
        "total_requests" => "请求数",
        "total_actual_cost" => "实际扣费合计",
        "today_actual_cost" => "今日扣费",
        "total_tokens" => "总用量",
        "today_tokens" => "今日用量",
        "total_api_keys" => "密钥数",
        "active_api_keys" => "使用中的密钥",
        "today_requests" => "今日请求",
        "total_input_tokens" => "累计输入",
        "total_output_tokens" => "累计输出",
        "title" => "标题",
        "message" => "消息",
        "action" => "操作",
        "actor_email" => "操作人",
        "validity_days" => "有效天数",
        "price" => "价格",
        "features" => "包含内容",
        "platform" => "平台",
        "key" => "API 密钥",
        "billing_type" => "扣费方式",
        "duration_ms" => "耗时",
        "request_id" => "请求编号",
        "request_type" => "请求类型",
        "stream" => "流式",
        "request_body" => "请求内容",
        "budget" => "预算",
        "used" => "已使用",
        "limit" => "额度",
        "reset_period" => "重置周期",
        "profile" => "开票信息",
        "title_type" => "抬头类型",
        "tax_number" => "税号",
        "phone" => "电话",
        "address" => "地址",
        "bank" => "开户银行",
        "bank_account" => "银行账号",
        "remark" => "备注",
        "tax_method" => "税费结算",
        "save_profile" => "保存开票信息",
        "amount_cents" => "金额（元）",
        "outstanding_cents" => "挂账余额（元）",
        "period_type" => "周期",
        "limit_amount" => "限额",
        "boundary_day" => "结算日",
        "grace_days" => "宽限天数",
        "enforcement_enabled" => "逾期限制",
        "daily_enabled" => "每日限额",
        "weekly_enabled" => "每周限额",
        "monthly_enabled" => "每月限额",
        "threshold_percents" => "提醒阈值",
        "recipient_user_ids" => "接收成员",
        "recipient_email" => "接收邮箱",
        "frequency" => "频率",
        "group_by" => "汇总维度",
        "include_settlement" => "包含结算",
        "send_hour" => "发送小时",
        "send_minute" => "发送分钟",
        "can_view_structure" => "查看组织架构",
        "can_view_billing" => "查看组织扣费",
        "can_view_accounts" => "查看组织账号",
        "allocation_mode" => "分配方式",
        "members" => "成员分配",
        "share_basis_points" => "分配比例（%）",
        "gradient" => "自动调整限额",
        "day" => "每日",
        "week" => "每周",
        "month" => "每月",
        "up_threshold_percent" => "上调触发用量（%）",
        "up_percent" => "上调幅度（%）",
        "down_threshold_percent" => "下调触发用量（%）",
        "down_percent" => "下调幅度（%）",
        "min_limit" => "限额下限",
        "max_limit" => "限额上限",
        "total_limit" => "累计限额",
        "apply_current_period" => "应用本期",
        "locked" => "锁定此身份",
        "balance_cents" => "预存余额（元）",
        "available_cents" => "可用金额（元）",
        "reserved_cents" => "预留金额（元）",
        "starts_at" => "开始时间",
        "ends_at" => "结束时间",
        "aff_quota" => "可用返利",
        "aff_code" => "邀请码",
        "aff_count" => "已邀请人数",
        "aff_history_quota" => "累计返利",
        _ => key,
    }
}
pub(super) fn value(value: &Value) -> String {
    match value {
        Value::Null => "—".into(),
        Value::Bool(v) => if *v { "是" } else { "否" }.into(),
        Value::String(v) => v.clone(),
        Value::Number(v) => v.to_string(),
        Value::Array(v) => format!("{} 项", v.len()),
        Value::Object(v) => format!("{} 项", v.len()),
    }
}

enum Input {
    Text(String, TextCtrl, Value),
    Check(String, CheckBox),
    Choice(String, Choice, Vec<Value>),
    Lines(String, TextCtrl, bool),
    Multi(String, CheckListBox, Vec<Value>),
}
fn pointer_key(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
fn leaf_key(key: &str) -> String {
    key.rsplit('/')
        .next()
        .unwrap_or(key)
        .replace("~1", "/")
        .replace("~0", "~")
}
fn field_label(key: &str) -> String {
    key.split('/')
        .map(|k| {
            let k = k.replace("~1", "/").replace("~0", "~");
            if let Ok(n) = k.parse::<usize>() {
                format!("第 {} 项", n + 1)
            } else {
                label(&k).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}
fn field_entries(fields: &Value, prefix: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (k, v) in fields.as_object().into_iter().flatten() {
        let key = format!("{prefix}{}", pointer_key(k));
        if v.as_object().is_some_and(|v| !v.is_empty()) {
            out.extend(field_entries(v, &format!("{key}/")));
        } else if v
            .as_array()
            .is_some_and(|v| !v.is_empty() && v.iter().all(Value::is_object))
        {
            for (i, item) in v.as_array().unwrap().iter().enumerate() {
                out.extend(field_entries(item, &format!("{key}/{i}/")));
            }
        } else {
            out.push((key, v.clone()));
        }
    }
    out
}
fn field_group(key: &str) -> &'static str {
    let leaf = leaf_key(key);
    if key.starts_with("members/") {
        return "成员分配";
    }
    if key.starts_with("profile/") {
        return "开票信息";
    }
    if key.starts_with("gradient/") {
        return "用量与限额";
    }
    match leaf.as_str() {
        "quota" | "balance" | "amount" | "amount_cents" | "share_basis_points" => "资金与额度",
        "rate_limit_5h" | "rate_limit_1d" | "rate_limit_7d" | "daily_limit" | "weekly_limit"
        | "monthly_limit" | "total_limit" | "daily_enabled" | "weekly_enabled"
        | "monthly_enabled" | "min_limit" | "max_limit" | "limit_amount" => "用量与限额",
        "group_id" | "fallback_group_id" | "status" | "enabled" | "control_mode" => "渠道与状态",
        "expires_at" | "expires_in_days" | "starts_at" | "ends_at" | "validity_days" => "有效期",
        "ip_whitelist" | "ip_blacklist" | "old_password" | "new_password" | "password"
        | "totp_code" | "verification_code" | "verify_code" => "访问与验证",
        _ => "基本信息",
    }
}
fn assign(body: &mut Value, key: &str, value: Value) {
    let pointer = format!("/{key}");
    if let Some(slot) = body.pointer_mut(&pointer) {
        *slot = value;
    }
}
pub(super) fn edit(
    parent: &Frame,
    api: ApiClient,
    title: &str,
    method: &str,
    path: &str,
    fields: Value,
) -> Option<Value> {
    edit_options(parent, api, title, method, path, fields, Default::default())
}
pub(super) fn edit_options(
    parent: &Frame,
    api: ApiClient,
    title: &str,
    method: &str,
    path: &str,
    fields: Value,
    options: std::collections::BTreeMap<String, Vec<(String, Value)>>,
) -> Option<Value> {
    edit_checked(
        parent,
        api,
        title,
        method,
        path,
        fields,
        options,
        |_| Ok(()),
    )
}
pub(super) fn edit_checked(
    parent: &Frame,
    api: ApiClient,
    title: &str,
    method: &str,
    path: &str,
    fields: Value,
    options: std::collections::BTreeMap<String, Vec<(String, Value)>>,
    validate: impl Fn(&Value) -> Result<(), String> + 'static,
) -> Option<Value> {
    edit_checked_money(
        parent,
        api,
        title,
        method,
        path,
        fields,
        options,
        Default::default(),
        validate,
    )
}
pub(super) fn edit_money(
    parent: &Frame,
    api: ApiClient,
    title: &str,
    method: &str,
    path: &str,
    fields: Value,
    options: std::collections::BTreeMap<String, Vec<(String, Value)>>,
    money: crate::desktop::squad_money::Money,
) -> Option<Value> {
    edit_checked_money(
        parent,
        api,
        title,
        method,
        path,
        fields,
        options,
        money,
        |_| Ok(()),
    )
}
pub(super) fn edit_checked_money(
    parent: &Frame,
    api: ApiClient,
    title: &str,
    method: &str,
    path: &str,
    fields: Value,
    options: std::collections::BTreeMap<String, Vec<(String, Value)>>,
    money: crate::desktop::squad_money::Money,
    validate: impl Fn(&Value) -> Result<(), String> + 'static,
) -> Option<Value> {
    let BuiltForm {
        dialog,
        save,
        status,
        editor,
    } = build_form(parent, title, path, &fields, &options, money);
    let (method, path) = (method.to_string(), path.to_string());
    let job = dialog_job::bind(dialog, save, status, api, move || {
        let body = editor.body()?;
        validate(&body)?;
        Ok((
            "/api/desktop/squad/request".into(),
            json!({"method":method,"path":path,"body":body}),
        ))
    });
    let accepted = dialog.show_modal() == ID_OK;
    let result = job.result.borrow().clone();
    drop(job);
    dialog.destroy();
    if accepted { result } else { None }
}

#[derive(Clone)]
pub(super) struct FieldEditor {
    pub page: ScrolledWindow,
    inputs: std::rc::Rc<Vec<Input>>,
    fields: Value,
    path: String,
    money: crate::desktop::squad_money::Money,
}
impl FieldEditor {
    pub fn new(
        parent: &dyn WxWidget,
        path: &str,
        fields: &Value,
        options: &std::collections::BTreeMap<String, Vec<(String, Value)>>,
        money: crate::desktop::squad_money::Money,
    ) -> Self {
        let scroll = ScrolledWindow::builder(parent)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        let column = BoxSizer::builder(Orientation::Vertical).build();
        scroll.set_sizer(column, false);
        Self::build(scroll, column, path, fields, options, money)
    }

    fn build(
        scroll: ScrolledWindow,
        column: BoxSizer,
        path: &str,
        fields: &Value,
        options: &std::collections::BTreeMap<String, Vec<(String, Value)>>,
        money: crate::desktop::squad_money::Money,
    ) -> Self {
        scroll.set_scroll_rate(0, scroll.from_dip(12));
        scroll.set_background_color(theme::theme().bg_app);
        scroll.show_scroll_indicators(false);
        scroll.set_min_size(Size::new(0, 0));
        let mut inputs = Vec::new();
        let entries = field_entries(&fields, "").into_iter().filter(|(key, default)| {
            let leaf = leaf_key(key);
            !(matches!(key.as_str(), "request_key" | "user_ids")
                || leaf.starts_with("expected_")
                || key.starts_with("members/")
                    && matches!(
                        leaf.as_str(),
                        "id" | "user_id" | "email" | "username" | "user_email"
                    )
                || matches!(leaf.as_str(), "business_unit_id" | "department_id")
                    && !options.contains_key(&leaf))
                // Unknown structured metadata remains in the original body;
                // it is not exposed as a raw JSON editor in business forms.
                && !default.is_object()
                && !(default.is_array() && !default.as_array().unwrap().iter().all(|v| v.is_string() || v.is_number()))
        }).collect::<Vec<_>>();
        for section in [
            "基本信息",
            "渠道与状态",
            "资金与额度",
            "用量与限额",
            "有效期",
            "访问与验证",
            "开票信息",
            "成员分配",
        ] {
            let rows = entries
                .iter()
                .filter(|(key, _)| field_group(key) == section)
                .collect::<Vec<_>>();
            if rows.is_empty() {
                continue;
            }
            let form = Panel::builder(&scroll).build();
            paint_surface(form, theme::SURFACE_RADIUS);
            form.set_foreground_color(theme::theme().ink_primary);
            let wrap = BoxSizer::builder(Orientation::Vertical).build();
            if entries.len() > 3 {
                let heading = super::squad_dialog::field_label(&form, section);
                heading.set_font(&theme::font(theme::TextRole::Title));
                wrap.add(
                    &heading,
                    0,
                    SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                    form.from_dip(theme::PANEL_INSET),
                );
            }
            let grid = FlexGridSizer::builder(0, 2)
                .with_vgap(scroll.from_dip(14))
                .with_hgap(scroll.from_dip(16))
                .build();
            grid.add_growable_col(1, 1);
            for (key, default) in rows {
                let leaf = leaf_key(key);
                let caption = if key.starts_with("members/") {
                    let row = fields
                        .pointer(&format!(
                            "/members/{}",
                            key.split('/').nth(1).unwrap_or("0")
                        ))
                        .unwrap_or(&Value::Null);
                    let who = row["user_email"]
                        .as_str()
                        .or_else(|| row["email"].as_str())
                        .or_else(|| row["username"].as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("成员 {}", row["user_id"]));
                    format!("{who} · {}", label(&leaf))
                } else {
                    let scope = key.split('/').rev().nth(1);
                    if key.starts_with("gradient/") {
                        format!("{} · {}", label(scope.unwrap_or_default()), label(&leaf))
                    } else {
                        label(&leaf).to_string()
                    }
                };
                let caption = if ledger_input(&leaf, &path) {
                    format!("{caption}（元）")
                } else {
                    caption
                };
                let name = StaticText::builder(&form).with_label(&caption).build();
                name.set_min_size(Size::new(scroll.from_dip(108), -1));
                name.wrap(scroll.from_dip(128));
                name.set_font(&theme::font(theme::TextRole::Body));
                name.set_foreground_color(theme::theme().ink_secondary);
                grid.add(&name, 0, SizerFlag::AlignCenterVertical, 0);
                if let Some(choices) = options.get(key).or_else(|| options.get(&leaf)) {
                    let mut choices = choices.clone();
                    let current = default
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| vec![default.clone()]);
                    for v in current {
                        if !choices.iter().any(|(_, old)| *old == v) {
                            choices.push((format!("保留当前：{}", value(&v)), v));
                        }
                    }
                    if let Some(selected) = default.as_array() {
                        let input = CheckListBox::builder(&form)
                            .with_choices(choices.iter().map(|(label, _)| label.clone()).collect())
                            .build();
                        input.set_min_size(Size::new(scroll.from_dip(160), scroll.from_dip(120)));
                        for (i, (_, v)) in choices.iter().enumerate() {
                            input.check(i as u32, selected.contains(v));
                        }
                        grid.add(&input, 1, SizerFlag::Expand, 0);
                        inputs.push(Input::Multi(
                            key.clone(),
                            input,
                            choices.into_iter().map(|(_, v)| v).collect(),
                        ));
                        continue;
                    }
                    let field = Panel::builder(&form).build();
                    field.set_background_color(theme::theme().bg_card);
                    let row = BoxSizer::builder(Orientation::Horizontal).build();
                    let input = Choice::builder(&field)
                        .with_size(Size::new(
                            (choices
                                .iter()
                                .map(|(label, _)| label.chars().count())
                                .max()
                                .unwrap_or(6) as i32
                                * 12
                                + 40)
                                .clamp(160, 280),
                            36,
                        ))
                        .with_choices(choices.iter().map(|(label, _)| label.clone()).collect())
                        .build();
                    input.set_background_color(theme::theme().bg_card_alt);
                    input.set_foreground_color(theme::theme().ink_primary);
                    input.set_max_size(Size::new(scroll.from_dip(280), -1));
                    input.set_selection(
                        choices
                            .iter()
                            .position(|(_, value)| value == default)
                            .unwrap_or(0) as u32,
                    );
                    row.add(&input, 0, SizerFlag::default(), 0);
                    field.set_sizer(row, false);
                    grid.add(&field, 1, SizerFlag::Expand, 0);
                    inputs.push(Input::Choice(
                        key.clone(),
                        input,
                        choices.iter().map(|(_, v)| v.clone()).collect(),
                    ));
                } else if matches!(
                    leaf.as_str(),
                    "ip_whitelist" | "ip_blacklist" | "emails" | "report_emails"
                ) || default
                    .as_array()
                    .is_some_and(|a| a.iter().all(|v| v.is_string() || v.is_number()))
                {
                    let numeric = matches!(
                        leaf.as_str(),
                        "threshold_percents" | "recipient_user_ids" | "group_ids"
                    ) || default
                        .as_array()
                        .is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_number));
                    let input = TextCtrl::builder(&form)
                        .with_style(TextCtrlStyle::MultiLine)
                        .with_value(
                            &default
                                .as_array()
                                .map(|a| a.iter().map(value).collect::<Vec<_>>().join("\n"))
                                .unwrap_or_default(),
                        )
                        .build();
                    input.set_min_size(Size::new(0, scroll.from_dip(82)));
                    input.set_tooltip("每行一项，留空表示不设置");
                    grid.add(&input, 1, SizerFlag::Expand, 0);
                    inputs.push(Input::Lines(key.clone(), input, numeric));
                } else if let Some(value) = default.as_bool() {
                    let input = CheckBox::builder(&form)
                        .with_label("启用")
                        .with_value(value)
                        .build();
                    input.set_foreground_color(theme::theme().ink_primary);
                    input.set_background_color(theme::theme().bg_card);
                    grid.add(&input, 1, SizerFlag::Expand, 0);
                    inputs.push(Input::Check(key.clone(), input));
                } else {
                    let secret = key.contains("password") || key.contains("secret") || key == "key";
                    let text = if ledger_input(&leaf, &path) && default.is_number() {
                        money.input(default.as_f64().unwrap()).unwrap_or_default()
                    } else if matches!(leaf.as_str(), "amount_cents" | "share_basis_points") {
                        format!("{:.2}", default.as_f64().unwrap_or(0.) / 100.)
                    } else if default.is_null() {
                        String::new()
                    } else if let Some(s) = default.as_str() {
                        s.into()
                    } else {
                        default.to_string()
                    };
                    let style = if secret {
                        TextCtrlStyle::Password
                    } else {
                        TextCtrlStyle::default()
                    };
                    let input = TextCtrl::builder(&form)
                        .with_value(&text)
                        .with_style(style)
                        .build();
                    input.set_name(&format!("squad-field-{}", key.replace('/', "-")));
                    grid.add(&input, 1, SizerFlag::Expand, 0);
                    inputs.push(Input::Text(key.clone(), input, default.clone()));
                }
            }
            wrap.add_sizer(
                &grid,
                0,
                SizerFlag::Expand | SizerFlag::All,
                scroll.from_dip(theme::PANEL_INSET),
            );
            form.set_sizer(wrap, false);
            column.add(
                &form,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                scroll.from_dip(12),
            );
        }
        scroll.layout();
        scroll.fit_inside();
        Self {
            page: scroll,
            inputs: std::rc::Rc::new(inputs),
            fields: fields.clone(),
            path: path.into(),
            money,
        }
    }
    pub fn body(&self) -> Result<Value, String> {
        let fields = &self.fields;
        let inputs = self.inputs.as_ref();
        let path = self.path.as_str();
        let money = self.money;
        let mut body = fields.clone();
        for input in inputs {
            match input {
                Input::Choice(key, input, choices) => {
                    assign(
                        &mut body,
                        key,
                        choices
                            .get(input.get_selection().unwrap_or(0) as usize)
                            .cloned()
                            .unwrap_or(Value::Null),
                    );
                }
                Input::Multi(key, input, choices) => {
                    assign(
                        &mut body,
                        key,
                        choices
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| input.is_checked(*i as u32))
                            .map(|(_, v)| v.clone())
                            .collect(),
                    );
                }
                Input::Lines(key, input, numeric) => {
                    let lines = input.get_value();
                    let values = lines
                        .lines()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            if *numeric {
                                serde_json::from_str::<Value>(s)
                                    .ok()
                                    .filter(Value::is_number)
                                    .ok_or_else(|| {
                                        format!("{}：每行填写一个数字", field_label(key))
                                    })
                            } else {
                                Ok(Value::String(s.into()))
                            }
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    assign(&mut body, key, values.into());
                }
                Input::Check(key, input) => {
                    assign(&mut body, key, input.get_value().into());
                }
                Input::Text(key, input, default) => {
                    let text = input.get_value();
                    let leaf = leaf_key(key);
                    if ledger_input(&leaf, &path) {
                        assign(&mut body, key, money.edit(default, &text)?);
                        continue;
                    }
                    let v = if default.is_number() {
                        serde_json::from_str::<Value>(&text)
                            .ok()
                            .filter(Value::is_number)
                            .ok_or_else(|| format!("{}需要填写数字", label(key)))?
                    } else if default.is_object() || default.is_array() {
                        serde_json::from_str(&text)
                            .map_err(|_| format!("{}格式无效", label(key)))?
                    } else if default.is_null() {
                        if text.trim().is_empty() {
                            Value::Null
                        } else {
                            serde_json::from_str(&text).unwrap_or(Value::String(text))
                        }
                    } else {
                        Value::String(text)
                    };
                    let leaf = leaf_key(key);
                    let v = if matches!(leaf.as_str(), "amount_cents" | "share_basis_points") {
                        let n = v.as_f64().ok_or("请填写有效金额")?;
                        if !n.is_finite()
                            || n.abs() > 1e12
                            || (n * 100. - (n * 100.).round()).abs() > 0.00001
                        {
                            return Err(format!("{}最多保留两位小数", field_label(key)));
                        }
                        if leaf == "share_basis_points" && !(0.0..=100.0).contains(&n) {
                            return Err("分配比例应在 0–100% 之间".into());
                        }
                        json!((n * 100.).round() as i64)
                    } else {
                        v
                    };
                    assign(&mut body, key, v);
                }
            }
        }
        if path == "/keys" && body["expires_in_days"] == 0 {
            body.as_object_mut().map(|v| v.remove("expires_in_days"));
        }
        if path.ends_with("/allocation-policy") {
            let rows = body["members"].as_array().cloned().unwrap_or_default();
            body["members"]=rows.iter().map(|r|serde_json::json!({"user_id":r["user_id"],"share_basis_points":r["share_basis_points"]})).collect();
        }
        Ok(body)
    }
}

struct BuiltForm {
    dialog: Dialog,
    save: Button,
    status: StaticText,
    editor: FieldEditor,
}
fn build_form(
    parent: &Frame,
    title: &str,
    path: &str,
    fields: &Value,
    options: &std::collections::BTreeMap<String, Vec<(String, Value)>>,
    money: crate::desktop::squad_money::Money,
) -> BuiltForm {
    let count = field_entries(&fields, "").len() as i32;
    let chrome = SquadDialog::new(
        parent,
        title,
        "填写相关内容后保存。",
        Size::new(640, (210 + count * 62).clamp(310, 720)),
    );
    let dialog = chrome.dialog;
    let editor = FieldEditor::build(chrome.body, chrome.content, path, fields, options, money);
    let status = chrome.status;
    let cancel = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label("取消")
        .with_icon(super::visual::Icon::Back)
        .with_kind(ButtonKind::Ghost)
        .with_size(Size::new(92, 36))
        .build();
    cancel.on_click(move |_| dialog.end_modal(ID_CANCEL));
    let save = Button::builder(&dialog)
        .with_label("保存更改")
        .with_kind(ButtonKind::Primary)
        .with_size(Size::new(116, 36))
        .build();
    save.set_default();
    chrome
        .actions
        .add(&cancel, 0, SizerFlag::Right, dialog.from_dip(8));
    chrome.actions.add(&save, 0, SizerFlag::default(), 0);
    chrome.finish();
    BuiltForm {
        dialog,
        save,
        status,
        editor,
    }
}
#[allow(dead_code)]
pub(super) fn review_form(
    parent: &Frame,
    title: &str,
    path: &str,
    fields: &Value,
    options: &std::collections::BTreeMap<String, Vec<(String, Value)>>,
    money: crate::desktop::squad_money::Money,
) -> Dialog {
    build_form(parent, title, path, fields, options, money).dialog
}
pub(super) fn choice(parent: &Frame, title: &str, choices: &[&str]) -> Option<usize> {
    let chrome = SquadDialog::new(
        parent,
        title,
        "选择需要进行的操作。",
        Size::new(
            600,
            (160 + ((choices.len() + 1) / 2) as i32 * 70).clamp(280, 680),
        ),
    );
    let dialog = chrome.dialog;
    let (card, column) = chrome.section("");
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(card.from_dip(10))
        .with_hgap(card.from_dip(12))
        .build();
    grid.add_growable_col(0, 1);
    grid.add_growable_col(1, 1);
    let selected = std::rc::Rc::new(std::cell::Cell::new(None));
    for (i, label) in choices.iter().enumerate() {
        let b = Button::builder(&card)
            .with_label(label)
            .with_kind(ButtonKind::Select)
            .with_size(Size::new(200, 54))
            .build();
        let s = selected.clone();
        b.on_click(move |_| {
            s.set(Some(i));
            dialog.end_modal(ID_OK);
        });
        grid.add(&b, 1, SizerFlag::Expand, 0);
    }
    column.add_sizer(&grid, 0, SizerFlag::Expand, 0);
    let back = Button::builder(&dialog)
        .with_label("返回")
        .with_icon(super::visual::Icon::Back)
        .with_id(ID_CANCEL)
        .with_kind(ButtonKind::Ghost)
        .build();
    back.on_click(move |_| dialog.end_modal(ID_CANCEL));
    chrome.actions.add(&back, 0, SizerFlag::default(), 0);
    chrome.finish();
    let ok = dialog.show_modal() == ID_OK;
    let result = if ok { selected.get() } else { None };
    dialog.destroy();
    result
}
fn ledger_input(key: &str, path: &str) -> bool {
    crate::desktop::squad_money::ledger_field(key) || key == "amount" && path.contains("/balance/")
}
pub(super) fn select(parent: &Frame, title: &str, labels: Vec<String>, multi: bool) -> Vec<usize> {
    let size = super::ai_gw_channel_dialog_size(parent, Size::new(680, 600));
    let dialog = Dialog::builder(parent, title)
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let view = super::squad_view::selection::Selection::new(&dialog, title, labels, multi);
    let col = BoxSizer::builder(Orientation::Vertical).build();
    col.add(
        &view.panel,
        1,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(22),
    );
    dialog.set_sizer(col, false);
    view.back.on_click(move |_| dialog.end_modal(ID_CANCEL));
    view.accept.on_click(move |_| dialog.end_modal(ID_OK));
    let accepted = dialog.show_modal() == ID_OK;
    let result = if accepted { view.indices() } else { vec![] };
    dialog.destroy();
    result
}
pub(super) fn prompt(parent: &Frame, title: &str, key: &str, value: &str) -> Option<String> {
    let fields = json!({key:value});
    let form = build_form(
        parent,
        title,
        "",
        &fields,
        &Default::default(),
        Default::default(),
    );
    let result: std::rc::Rc<std::cell::RefCell<Option<String>>> = Default::default();
    let (r, editor, key, dialog) = (result.clone(), form.editor, key.to_string(), form.dialog);
    form.save
        .set_label(if key == "query" { "查找" } else { "确定" });
    form.save.on_click(move |_| {
        if let Ok(body) = editor.body() {
            *r.borrow_mut() = body[&key].as_str().map(str::to_string);
            dialog.end_modal(ID_OK);
        }
    });
    let accepted = dialog.show_modal() == ID_OK;
    let text = result.borrow().clone();
    dialog.destroy();
    if accepted { text } else { None }
}
pub(super) fn inspect(parent: &Frame, title: &str, data: &Value) {
    inspect_money(parent, title, data, Default::default());
}
pub(super) fn inspect_money(
    parent: &Frame,
    title: &str,
    data: &Value,
    money: crate::desktop::squad_money::Money,
) {
    inspect_action(parent, title, data, money, None);
}
pub(super) fn inspect_action(
    parent: &Frame,
    title: &str,
    data: &Value,
    money: crate::desktop::squad_money::Money,
    action: Option<&str>,
) -> bool {
    let chrome = SquadDialog::new(parent, title, "查看当前信息。", Size::new(720, 620));
    let dialog = chrome.dialog;
    let mut groups = super::squad_view::detail::groups(data, money);
    if groups.is_empty() {
        groups.push(super::squad_view::detail::Group {
            title: "当前内容".into(),
            fields: vec![("状态".into(), "暂无可展示的信息".into())],
        });
    }
    for group in groups {
        let (card, column) = chrome.section(&group.title);
        let grid = FlexGridSizer::builder(0, 2)
            .with_hgap(card.from_dip(20))
            .with_vgap(card.from_dip(16))
            .build();
        grid.add_growable_col(0, 1);
        grid.add_growable_col(1, 1);
        for (label, value) in group.fields {
            let field = Panel::builder(&card).build();
            field.set_background_color(theme::theme().bg_card);
            let pair = BoxSizer::builder(Orientation::Vertical).build();
            let caption = super::squad_dialog::note(&field, &label);
            let value = super::squad_dialog::field_label(&field, &value);
            value.wrap(field.from_dip(260));
            pair.add(&caption, 0, SizerFlag::Bottom, field.from_dip(5));
            pair.add(&value, 0, SizerFlag::Expand, 0);
            field.set_sizer(pair, false);
            field.on_size(move |event| {
                value.wrap(field.get_client_size().width.max(1));
                event.skip(true);
            });
            grid.add(&field, 1, SizerFlag::Expand, 0);
        }
        column.add_sizer(&grid, 0, SizerFlag::Expand, 0);
    }
    let close = Button::builder(&dialog)
        .with_label("返回")
        .with_icon(super::visual::Icon::Back)
        .with_id(ID_CANCEL)
        .with_kind(ButtonKind::Ghost)
        .with_size(Size::new(96, 36))
        .build();
    close.on_click(move |_| dialog.end_modal(ID_CANCEL));
    chrome
        .actions
        .add(&close, 0, SizerFlag::Right, dialog.from_dip(8));
    if let Some(label) = action {
        let button = Button::builder(&dialog)
            .with_label(label)
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(160, 36))
            .build();
        button.on_click(move |_| dialog.end_modal(ID_OK));
        button.set_default();
        chrome.actions.add(&button, 0, SizerFlag::default(), 0);
    }
    chrome.finish();
    let accepted = dialog.show_modal() == ID_OK;
    dialog.destroy();
    accepted
}

#[cfg(all(test, windows))]
mod native_review_tests {
    use super::*;

    #[test]
    #[ignore = "isolated hidden native account forms; run with --test-threads=1"]
    fn squad_forms_keep_footer_visible_and_preserve_unedited_server_fields() {
        wxdragon::app::render_offscreen(|| {
            for (mode, suffix) in [
                (theme::ThemeMode::Light, "light"),
                (theme::ThemeMode::Dark, "dark"),
            ] {
                theme::init(mode);
                for (width, height) in [(900, 560), (1168, 800)] {
                    let parent = Frame::builder()
                        .with_title("hidden synthetic account form")
                        .build();
                    parent.set_client_size(super::super::visual::dip_size(&parent, width, height));
                    let fields = json!({"name":"合成密钥", "quota":0.12345, "expires_in_days":0,
                        "status":"active", "rate_limit_5h":0.0, "rate_limit_1d":0.0,
                        "rate_limit_7d":0.0, "ip_whitelist":["127.0.0.1"], "ip_blacklist":[],
                        "expected_updated_at":"fixture-revision", "metadata":{}});
                    let form = build_form(
                        &parent,
                        "编辑 API 密钥（合成）",
                        "/keys/7",
                        &fields,
                        &Default::default(),
                        Default::default(),
                    );
                    form.dialog.layout();
                    form.editor.page.layout();
                    form.editor.page.fit_inside();
                    let size = form.dialog.get_client_size();
                    let point = form
                        .dialog
                        .screen_to_client(form.save.client_to_screen(Point::new(0, 0)));
                    let button = form.save.get_size();
                    assert!(
                        point.x >= 0
                            && point.y >= 0
                            && point.x + button.width <= size.width
                            && point.y + button.height <= size.height,
                        "fixed save button was clipped"
                    );
                    let body_size = form.editor.page.get_client_size();
                    assert!(body_size.width > 0 && body_size.height > 0);
                    assert!(
                        form.editor.page.get_virtual_size().height > body_size.height,
                        "long grouped forms should scroll inside their body"
                    );
                    for input in form.editor.inputs.iter() {
                        let control: &dyn WxWidget = match input {
                            Input::Text(_, widget, _) | Input::Lines(_, widget, _) => widget,
                            Input::Choice(_, widget, _) => widget,
                            Input::Check(_, widget) => widget,
                            Input::Multi(_, widget, _) => widget,
                        };
                        let point = form
                            .editor
                            .page
                            .screen_to_client(control.client_to_screen(Point::new(0, 0)));
                        assert!(
                            point.x >= 0 && point.x + control.get_size().width <= body_size.width,
                            "native field overflowed its body horizontally"
                        );
                    }
                    assert_eq!(
                        form.editor.body().unwrap(),
                        fields,
                        "visual groups must preserve exact amounts and server-managed fields"
                    );
                    let Input::Text(_, name, _) = form
                        .editor
                        .inputs
                        .iter()
                        .find(|input| matches!(input, Input::Text(key, _, _) if key == "name"))
                        .unwrap()
                    else {
                        unreachable!()
                    };
                    name.change_value("修改后的合成名称");
                    let saved = form.editor.body().unwrap();
                    assert_eq!(saved["name"], "修改后的合成名称");
                    assert_eq!(saved["metadata"], fields["metadata"]);
                    assert_eq!(saved["expected_updated_at"], fields["expected_updated_at"]);
                    assert_eq!(saved["quota"], fields["quota"]);
                    super::super::native_model_review::capture(
                        &form.dialog,
                        &format!("squad-grouped-form-{width}-{suffix}"),
                    );
                    form.dialog.destroy();
                    parent.destroy();
                }
            }
        })
        .unwrap();
    }
}
