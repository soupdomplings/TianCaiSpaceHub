//! Site organization response contracts. No guessed field names or currency rates.
use crate::desktop::squad_money::{Money, cny};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(crate) struct Section {
    pub name: &'static str,
    pub path: &'static str,
    pub rows: &'static str,
    pub columns: &'static [&'static str],
}
pub(crate) fn sections(menu: &str) -> Vec<Section> {
    let entries = [
        (
            "成员",
            "memberships",
            "",
            &["user_email", "username", "department_name", "status"][..],
        ),
        (
            "部门",
            "departments",
            "",
            &["name", "code", "status", "month_usage"],
        ),
        (
            "部门管理员",
            "department-admins",
            "",
            &["email", "username", "department_name", "status"],
        ),
        (
            "额度分配",
            "allocation-policy",
            "members",
            &[
                "member_name",
                "configured_share",
                "projected_amount",
                "current_period_allocated_amount",
                "current_period_used_amount",
                "current_period_remaining_amount",
            ],
        ),
        (
            "成员额度",
            "member-limit-details",
            "",
            &[
                "email",
                "username",
                "daily_limit",
                "weekly_limit",
                "monthly_limit",
            ],
        ),
        ("用量汇总", "usage/summary", "", &["name", "value"]),
        (
            "成员用量",
            "usage/members",
            "",
            &["username", "departments", "total_cost", "total_tokens"],
        ),
        (
            "使用明细",
            "usage/details",
            "",
            &["created_at", "user_email", "model", "actual_cost"],
        ),
        (
            "挂账余额",
            "settlement",
            "lines",
            &[
                "department_name",
                "starts_at",
                "ends_at",
                "due_cents",
                "paid_cents",
                "outstanding_cents",
                "closed",
            ],
        ),
        (
            "预存余额",
            "settlement/prepayment",
            "wallets",
            &[
                "department_name",
                "balance_cents",
                "available_cents",
                "reserved_cents",
            ],
        ),
    ];
    entries
        .into_iter()
        .filter(|(_, path, _, _)| match menu {
            "organization-structure" => matches!(*path, "memberships" | "departments"),
            "organization-usage" => path.starts_with("usage/"),
            "organization-accounts" => false,
            _ => true,
        })
        .map(|(name, path, rows, columns)| Section {
            name,
            path,
            rows,
            columns,
        })
        .collect()
}

pub(crate) fn mode(value: &Value) -> &'static str {
    match value.as_str() {
        Some("none") => "不分配",
        Some("equal") => "平均分配",
        Some("custom") => "按比例分配",
        _ => "—",
    }
}
pub(crate) fn percent(v: &Value) -> String {
    v.as_u64()
        .filter(|n| *n <= 10000)
        .map(|n| format!("{:.2}%", n as f64 / 100.))
        .unwrap_or("—".into())
}
pub(crate) fn allocation_rows(data: &Value) -> Vec<Value> {
    data["members"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|member| {
            let mut row = member.clone();
            row["member_name"] = member["username"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or(member["email"].as_str())
                .unwrap_or("成员")
                .into();
            row["configured_share"] = if data["allocation_mode"] == "none" {
                "—".into()
            } else {
                percent(
                    &member[if data["allocation_mode"] == "equal" {
                        "share_basis_points"
                    } else {
                        "custom_share_basis_points"
                    }],
                )
                .into()
            };
            row
        })
        .collect()
}
pub(crate) fn metrics(path: &str, data: &Value) -> Vec<(String, String)> {
    let money = Money::default();
    let fields: Vec<(&str, String)> = match path {
        "allocation-policy" => vec![
            ("组织额度", money.usd(&data["budget_limit_amount"])),
            (
                "本期分配方式",
                mode(&data["current_period_allocation_mode"]).into(),
            ),
            ("配置分配比例", percent(&data["allocated_basis_points"])),
            ("预留比例", percent(&data["reserve_basis_points"])),
        ],
        "settlement" => {
            let sum = |key: &str, closed: Option<bool>| -> String {
                let Some(lines) = data["lines"].as_array() else {
                    return "—".into();
                };
                let mut total: i64 = 0;
                for row in lines {
                    let Some(is_closed) = row["closed"].as_bool() else {
                        return "—".into();
                    };
                    if closed.is_some_and(|b| b != is_closed) {
                        continue;
                    }
                    let Some(n) = row[key].as_i64() else {
                        return "—".into();
                    };
                    let Some(sum) = total.checked_add(n) else {
                        return "—".into();
                    };
                    total = sum;
                }
                cny(total as f64 / 100.)
            };
            vec![
                ("挂账余额", sum("outstanding_cents", None)),
                ("已出账待清", sum("outstanding_cents", Some(true))),
                ("未出账累计", sum("outstanding_cents", Some(false))),
                ("已清金额", sum("paid_cents", Some(true))),
            ]
        }
        "settlement/prepayment" => {
            let sum = |key: &str| {
                data["wallets"]
                    .as_array()
                    .and_then(|rows| {
                        rows.iter()
                            .try_fold(0i64, |total, row| total.checked_add(row[key].as_i64()?))
                    })
                    .map(|n| cny(n as f64 / 100.))
                    .unwrap_or("—".into())
            };
            vec![
                ("预存余额", sum("balance_cents")),
                ("可用余额", sum("available_cents")),
                ("预留处理中", sum("reserved_cents")),
            ]
        }
        _ => vec![],
    };
    fields.into_iter().map(|(a, b)| (a.into(), b)).collect()
}
pub(crate) fn allocation_body(
    data: &Value,
    custom: bool,
    values: &[String],
    apply: bool,
) -> Result<Value, String> {
    if data["configured"] != true {
        return Err("请先在主站配置组织额度，再分配成员额度".into());
    }
    let members = data["members"]
        .as_array()
        .ok_or("成员列表未读取成功，请重试")?;
    if members.len() != values.len() {
        return Err("成员列表已变化，请重新读取".into());
    }
    let mut total = 0u64;
    let mut ids = std::collections::HashSet::new();
    let mut body = Vec::new();
    for (member, text) in members.iter().zip(values) {
        let id = member["user_id"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("成员身份不完整，请重新读取")?;
        if !ids.insert(id) {
            return Err("成员重复，请重新读取".into());
        }
        if !custom {
            continue;
        }
        let n = text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && (0. ..=100.).contains(v))
            .ok_or("比例须为 0–100% 的数字")?;
        if (n * 100. - (n * 100.).round()).abs() > 1e-6 {
            return Err("比例最多保留两位小数".into());
        }
        let bp = (n * 100.).round() as u64;
        total += bp;
        body.push(json!({"user_id":id,"share_basis_points":bp}));
    }
    if total > 10000 {
        return Err("分配比例合计不能超过 100%".into());
    }
    Ok(
        json!({"allocation_mode":if custom {"custom"} else {"none"},"members":body,"apply_current_period":apply}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn organization_navigation_excludes_distribution_and_settlement_rules() {
        for menu in ["organization-admin", "organization-usage"] {
            assert!(
                !sections(menu)
                    .iter()
                    .any(|s| s.path == "usage/breakdown" || s.rows == "policies")
            );
        }
        assert!(
            sections("organization-admin")
                .iter()
                .any(|s| s.path == "settlement/prepayment")
        );
    }
    #[test]
    fn allocation_uses_configured_shares_but_keeps_current_period_amounts() {
        let data = json!({"allocation_mode":"custom","members":[{"user_id":4,"email":"member@example.invalid","custom_share_basis_points":2500,"share_basis_points":5000,"projected_amount":25,"current_period_allocated_amount":50,"current_period_used_amount":12,"current_period_remaining_amount":38}]});
        let rows = allocation_rows(&data);
        assert_eq!(rows[0]["configured_share"], "25.00%");
        assert_eq!(rows[0]["current_period_allocated_amount"], 50);
        assert_eq!(rows[0]["projected_amount"], 25);
        assert_eq!(
            Money::default().usd(&rows[0]["current_period_remaining_amount"]),
            "¥38.00"
        );
        let mut legacy = data.clone();
        legacy["allocation_mode"] = "equal".into();
        assert_eq!(allocation_rows(&legacy)[0]["configured_share"], "50.00%");
    }
    #[test]
    fn settlement_separates_closed_debt_from_unbilled_accrual_without_double_counting() {
        let data = json!({"lines":[{"closed":true,"outstanding_cents":150,"paid_cents":50},{"closed":false,"outstanding_cents":250,"paid_cents":20}]});
        assert_eq!(
            metrics("settlement", &data)
                .iter()
                .map(|x| x.1.as_str())
                .collect::<Vec<_>>(),
            vec!["¥4.00", "¥1.50", "¥2.50", "¥0.50"]
        );
        assert_eq!(metrics("settlement", &json!({}))[0].1, "—");
        assert_eq!(metrics("settlement", &json!({"lines":[]}))[0].1, "¥0.00");
    }
    #[test]
    fn allocation_write_contains_only_site_fields_and_rejects_invalid_totals() {
        let data = json!({"configured":true,"members":[{"user_id":1,"email":"one@example.invalid"},{"user_id":2,"current_period_allocated_amount":300}]});
        assert_eq!(
            allocation_body(&data, true, &["25.05".into(), "74.95".into()], true).unwrap(),
            json!({"allocation_mode":"custom","members":[{"user_id":1,"share_basis_points":2505},{"user_id":2,"share_basis_points":7495}],"apply_current_period":true})
        );
        for values in [["100", "1"], ["NaN", "0"], ["0.001", "0"], ["-1", "0"]] {
            assert!(allocation_body(&data, true, &values.map(String::from), false).is_err());
        }
        assert_eq!(
            allocation_body(&data, false, &["invalid".into(), "invalid".into()], false).unwrap()["members"],
            json!([])
        );
        assert!(
            allocation_body(&json!({"configured":false,"members":[]}), true, &[], false).is_err()
        );
    }
}
