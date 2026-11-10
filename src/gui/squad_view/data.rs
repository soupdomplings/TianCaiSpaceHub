//! Account presentation contracts. Never enumerate arbitrary response fields.
use serde_json::{Value, json};

pub(super) fn rows(key: &str, data: &Value) -> Vec<Value> {
    if key == "organization" {
        return data["memberships"].as_array().cloned().unwrap_or_default();
    }
    for field in [
        "items",
        "orders",
        "subscriptions",
        "channels",
        "plans",
        "data",
    ] {
        if let Some(rows) = data[field].as_array() {
            return rows.clone();
        }
    }
    data.as_array().cloned().unwrap_or_default()
}

// (response field, user-facing heading, relative column width)
pub(super) fn columns(key: &str) -> Vec<(&'static str, &'static str, i32)> {
    match key {
        "keys" => vec![
            ("name", "密钥名称", 120),
            ("status", "状态", 65),
            ("group_name", "渠道分组", 90),
            ("today_actual_cost", "今日已用", 100),
            ("total_actual_cost", "近30天已用", 110),
            ("quota_usage", "限额使用", 160),
            ("expires_at", "到期时间", 110),
        ],
        "usage" => vec![
            ("created_at", "时间", 130),
            ("model", "模型", 180),
            ("input_usage", "输入用量", 100),
            ("output_tokens", "输出用量", 100),
            ("actual_cost", "实际扣费", 100),
        ],
        "orders" => vec![
            ("out_trade_no", "订单号", 190),
            ("status", "状态", 90),
            ("pay_amount", "支付金额", 100),
            ("order_type", "类型", 100),
            ("created_at", "创建时间", 145),
        ],
        "organization" => vec![
            ("business_unit_name", "组织", 170),
            ("department_name", "部门", 150),
            ("is_active_billing", "扣费身份", 100),
            ("today_usage", "今日使用", 100),
            ("month_usage", "本月使用", 100),
        ],
        "organization-audit" => vec![
            ("created_at", "时间", 140),
            ("actor_email", "操作人", 180),
            ("action", "操作", 170),
            ("description", "内容", 210),
        ],
        key if key.starts_with("organization-") => vec![
            ("name", "组织", 200),
            ("status", "状态", 90),
            ("today_usage", "今日使用", 110),
            ("month_usage", "本月使用", 110),
            ("description", "说明", 200),
        ],
        "subscriptions" => vec![
            ("group_name", "订阅", 190),
            ("status", "状态", 100),
            ("starts_at", "生效时间", 140),
            ("expires_at", "到期时间", 140),
        ],
        "channels" => vec![
            ("name", "渠道", 200),
            ("platform", "厂商", 120),
            ("description", "说明", 350),
        ],
        "redeem" => vec![
            ("created_at", "时间", 160),
            ("type", "类型", 150),
            ("value", "额度", 120),
            ("status", "状态", 100),
        ],
        _ => vec![],
    }
}
pub(super) fn paginated(key: &str) -> bool {
    matches!(
        key,
        "keys" | "usage" | "orders" | "organization-audit" | "redeem"
    )
}
pub(super) fn date_filter(key: &str) -> bool {
    matches!(key, "usage" | "organization-audit")
}
pub(super) fn amount(v: &Value) -> String {
    v.as_f64()
        .filter(|n| n.is_finite())
        .map(|n| format!("${n:.4}"))
        .unwrap_or_else(|| "—".into())
}
pub(super) fn compact(v: &Value) -> String {
    let Some(n) = v.as_f64().filter(|n| n.is_finite()) else {
        return "—".into();
    };
    if n.abs() >= 1_000_000. {
        format!("{:.2}M", n / 1_000_000.)
    } else if n.abs() >= 10_000. {
        format!("{:.1}K", n / 1000.)
    } else {
        format!("{n:.0}")
    }
}
pub(super) fn input(row: &Value, prefix: &str) -> Value {
    let base = &row[format!("{prefix}input_tokens")];
    if base.is_null() {
        return Value::Null;
    }
    json!(
        ["input_tokens", "cache_read_tokens", "cache_creation_tokens"]
            .iter()
            .filter_map(|key| row[format!("{prefix}{key}")].as_i64())
            .fold(0i64, i64::saturating_add)
    )
}
pub(super) fn text(v: &Value) -> String {
    match v {
        Value::String(s) if !s.trim().is_empty() => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(true) => "已启用".into(),
        Value::Bool(false) => "未启用".into(),
        _ => "—".into(),
    }
}
pub(super) fn cell(row: &Value, key: &str) -> String {
    let v = &row[key];
    match key {
        "group_name" => row["group_name"]
            .as_str()
            .or(row["group"]["name"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if row["group_id"].is_null() {
                    "未指定".into()
                } else {
                    format!("分组 {}", row["group_id"])
                }
            }),
        "status" => match v.as_str().unwrap_or("") {
            "active" | "enabled" => "使用中".into(),
            "inactive" | "disabled" => "未使用".into(),
            "pending" => "待支付".into(),
            "paid" | "completed" => "已完成".into(),
            "expired" => "已到期".into(),
            "cancelled" | "canceled" => "已取消".into(),
            "refunded" => "已退款".into(),
            "failed" => "未完成".into(),
            _ => text(v),
        },
        "is_active_billing" => {
            if v == true {
                "当前使用".into()
            } else {
                "未使用".into()
            }
        }
        "quota" if v.as_f64() == Some(0.) => "不限额".into(),
        "quota_used" | "quota" | "actual_cost" | "today_usage" | "month_usage" => amount(v),
        "pay_amount" => v
            .as_f64()
            .map(|n| format!("{n:.2} {}", row["currency"].as_str().unwrap_or("")))
            .unwrap_or("—".into()),
        "input_usage" => compact(&input(row, "")),
        "output_tokens" => compact(v),
        "platform" if v.is_null() => row["platforms"]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(|p| p["platform"].as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or("—".into()),
        "created_at" | "expires_at" | "starts_at" => {
            if key == "expires_at" && v.is_null() {
                return "长期有效".into();
            }
            v.as_str()
                .filter(|s| s.is_ascii() && s.len() >= 16)
                .map(|s| s[5..16].replace('T', " "))
                .unwrap_or_else(|| text(v))
        }
        "billing_type" => match v.as_str().unwrap_or("") {
            "balance" => "账户余额".into(),
            "subscription" => "订阅额度".into(),
            "organization" => "组织资金".into(),
            _ => text(v),
        },
        "order_type" => match v.as_str().unwrap_or("") {
            "balance" | "recharge" => "余额充值".into(),
            "subscription" => "订阅".into(),
            _ => text(v),
        },
        _ => text(v),
    }
}
pub(super) fn cell_money(
    row: &Value,
    key: &str,
    money: crate::desktop::squad_money::Money,
) -> String {
    if key == "quota_usage" {
        return match row["quota"].as_f64() {
            Some(0.) => "不限额".into(),
            Some(n) if n > 0. => format!(
                "{} / {}",
                money.usd(&row["quota_used"]),
                money.usd(&row["quota"])
            ),
            _ => "—".into(),
        };
    }
    if key == "quota" && row[key].as_f64() == Some(0.) {
        return "不限额".into();
    }
    if crate::desktop::squad_money::ledger_field(key) {
        return money.usd(&row[key]);
    }
    if key == "pay_amount" {
        return money.currency(&row[key], row["currency"].as_str().unwrap_or(""));
    }
    cell(row, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_costs_and_limit_counter_have_separate_labels_and_yuan_values() {
        let money = crate::desktop::squad_money::Money::default();
        let row = json!({"quota":0,"quota_used":0,"today_actual_cost":0.000057,"total_actual_cost":15.75});
        assert_eq!(cell_money(&row, "today_actual_cost", money), "¥0.000057");
        assert_eq!(cell_money(&row, "total_actual_cost", money), "¥15.75");
        assert_eq!(cell_money(&row, "quota_usage", money), "不限额");
        assert_eq!(cell_money(&json!({}), "total_actual_cost", money), "—");
        assert_eq!(
            cell_money(&json!({"quota":20,"quota_used":3}), "quota_usage", money),
            "¥3.00 / ¥20.00"
        );
    }
    #[test]
    fn usage_has_five_columns_and_yuan_presentation_keeps_raw_ledger() {
        let money = crate::desktop::squad_money::Money::default();
        assert_eq!(columns("usage").len(), 5);
        assert!(!columns("usage").iter().any(|v| v.0 == "billing_type"));
        let row = json!({"actual_cost":1.25,"input_tokens":10,"cache_read_tokens":15,"cache_creation_tokens":5,"output_tokens":9});
        assert_eq!(cell_money(&row, "actual_cost", money), "¥1.25");
        assert_eq!(row["actual_cost"], 1.25);
        assert_eq!(cell_money(&row, "input_usage", money), "30");
        assert_eq!(cell_money(&row, "output_tokens", money), "9");
    }
    #[test]
    fn profile_is_never_an_arbitrary_field_table() {
        assert!(rows("profile", &json!({"auth_bindings":[1],"allowed_groups":[2],"email":"synthetic@example.invalid"})).is_empty());
        assert!(columns("profile").is_empty());
        assert!(!date_filter("profile") && !paginated("profile"));
        assert!(date_filter("usage") && paginated("usage"));
        assert!(!paginated("channels"));
    }
    #[test]
    fn lists_keep_server_identities_and_combine_input_only_once() {
        let rows = rows(
            "keys",
            &json!({"items":[{"id":17,"status":"active","group":{"name":"示例分组"}}]}),
        );
        assert_eq!(rows[0]["id"], 17);
        assert_eq!(cell(&rows[0], "group_name"), "示例分组");
        assert_eq!(cell(&rows[0], "status"), "使用中");
        assert_eq!(
            input(
                &json!({"input_tokens":10,"cache_read_tokens":30,"cache_creation_tokens":5}),
                ""
            ),
            45
        );
        assert!(input(&json!({}), "").is_null());
    }
}
