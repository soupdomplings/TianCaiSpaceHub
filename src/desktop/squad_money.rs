//! Account presentation currency. Never changes the site's ledger or payment payloads.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Money {}
impl Money {
    pub fn checkout(_info: &Value) -> Self {
        Self::default()
    }
    pub fn state(_state: &Value) -> Self {
        Self::default()
    }
    pub fn convert(self, value: f64) -> Option<f64> {
        // The site's numeric account amounts already represent yuan. This is
        // a label correction, never a foreign-exchange or recharge operation.
        value.is_finite().then_some(value)
    }
    pub fn usd(self, value: &Value) -> String {
        match value.as_f64() {
            Some(n) => self.convert(n).map(cny).unwrap_or("¥—".into()),
            _ => "—".into(),
        }
    }
    pub fn currency(self, value: &Value, _currency: &str) -> String {
        self.usd(value)
    }
    pub fn input(self, value: f64) -> Option<String> {
        let n = self.convert(value)?;
        let s = format!("{n:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        Some(if s.split('.').nth(1).is_some_and(|d| d.len() > 2) {
            s.into()
        } else {
            format!("{n:.2}")
        })
    }
    pub fn edit(self, original: &Value, text: &str) -> Result<Value, String> {
        if let Some(n) = original.as_f64() {
            if self.input(n).as_deref() == Some(text) {
                return Ok(original.clone());
            }
        }
        if original.is_null() && text.trim().is_empty() {
            return Ok(Value::Null);
        }
        let n = text
            .trim()
            .parse::<f64>()
            .map_err(|_| "请输入有效人民币金额")?;
        if !n.is_finite() || n.abs() > 1e12 {
            return Err("请输入有效人民币金额".into());
        }
        Ok(serde_json::json!(n))
    }
}
pub fn cny(n: f64) -> String {
    if n != 0. && n.abs() < 0.01 {
        format!("¥{n:.6}")
    } else if n.abs() < 1. && (n * 100. - (n * 100.).round()).abs() > 1e-7 {
        format!("¥{n:.4}")
    } else {
        format!("¥{n:.2}")
    }
}
pub fn ledger_field(key: &str) -> bool {
    matches!(
        key,
        "balance"
            | "quota"
            | "quota_used"
            | "actual_cost"
            | "total_cost"
            | "total_actual_cost"
            | "today_actual_cost"
            | "today_usage"
            | "month_usage"
            | "daily_limit"
            | "weekly_limit"
            | "monthly_limit"
            | "limit_amount"
            | "min_limit"
            | "max_limit"
            | "total_limit"
            | "budget"
            | "used"
            | "available_amount"
            | "week_usage"
            | "business_unit_today_usage"
            | "business_unit_week_usage"
            | "business_unit_month_usage"
            | "used_amount"
            | "reserved_amount"
            | "remaining_amount"
            | "budget_limit_amount"
            | "projected_amount"
            | "current_period_allocated_amount"
            | "current_period_used_amount"
            | "current_period_reserved_amount"
            | "current_period_remaining_amount"
            | "business_unit_cost"
            | "personal_balance_cost"
            | "subscription_cost"
            | "balance_notify_threshold"
            | "rate_limit_5h"
            | "rate_limit_1d"
            | "rate_limit_7d"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn currency_label_keeps_site_numbers_without_exchange_or_recharge_rates() {
        let m = Money::checkout(
            &json!({"subscription_usd_to_cny_rate":7.15,"balance_recharge_multiplier":0.14}),
        );
        assert_eq!(m.usd(&json!(10)), "¥10.00");
        assert_eq!(m.currency(&json!(68), "CNY"), "¥68.00");
        assert_eq!(m.currency(&json!(100), "USD"), "¥100.00");
        assert_eq!(
            Money::checkout(&json!({"balance_recharge_multiplier":0.14})).usd(&json!(10)),
            "¥10.00"
        );
        assert_eq!(Money::default().usd(&json!(0)), "¥0.00");
        assert_eq!(m.usd(&Value::Null), "—");
        for rate in [json!(0), json!(-1), json!("7.15"), json!(1e7)] {
            assert_eq!(
                Money::checkout(&json!({"subscription_usd_to_cny_rate":rate})).usd(&json!(10)),
                "¥10.00"
            );
        }
        assert!(!ledger_field("tokens") && !ledger_field("price") && !ledger_field("amount_cents"));
    }
    #[test]
    fn small_charges_do_not_disappear_when_displayed_in_yuan() {
        assert_eq!(cny(0.00015), "¥0.000150");
        assert_eq!(cny(0.0134), "¥0.0134");
    }
    #[test]
    fn editing_yuan_round_trips_without_rewriting_untouched_precise_balances() {
        let m = Money::default();
        let original = json!(0.123456789012);
        assert_eq!(
            m.edit(&original, &m.input(original.as_f64().unwrap()).unwrap())
                .unwrap(),
            original
        );
        assert_eq!(m.edit(&json!(1), "71.5").unwrap(), json!(71.5));
        assert!(m.edit(&original, "NaN").is_err());
        assert!(m.edit(&original, "inf").is_err());
        assert_eq!(m.edit(&Value::Null, "").unwrap(), Value::Null);
        assert_eq!(
            Money::default().edit(&original, "1000").unwrap(),
            json!(1000.)
        );
    }
}
