//! Native preview of the deployed site's PaymentView/rechargeBonus rules.
//! The server-created order remains authoritative for the final payment.
use anyhow::{Result, ensure};
use serde_json::Value;

#[derive(Debug)]
pub struct Quote {
    pub currency: String,
    pub pay_base: f64,
    pub fee: f64,
    pub total: f64,
    pub credited: Option<f64>,
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.trim().parse().ok())
        .filter(|n| n.is_finite())
}
fn round(v: f64, digits: i32) -> f64 {
    let factor = 10f64.powi(digits);
    ((v + f64::EPSILON) * factor).round() / factor
}
pub fn quote(info: &Value, method: &Value, kind: &str, amount: f64) -> Result<Quote> {
    ensure!(
        amount.is_finite() && amount > 0. && amount <= 1e12,
        "请输入有效金额"
    );
    let currency = method["currency"]
        .as_str()
        .unwrap_or("CNY")
        .trim()
        .to_ascii_uppercase();
    let digits = match currency.as_str() {
        "JPY" | "KRW" | "VND" | "CLP" | "XAF" | "XOF" | "XPF" | "RWF" | "UGX" | "PYG" | "GNF"
        | "KMF" | "DJF" | "BIF" | "VUV" | "ISK" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        "CLF" | "UYW" => 4,
        _ => 2,
    };
    let mut pay_base = amount;
    let mut credited = None;
    if kind == "balance" {
        let multiplier = number(&info["balance_recharge_multiplier"])
            .filter(|n| *n > 0.)
            .unwrap_or(1.);
        let base = round(amount * multiplier, 2);
        let mut tiers = Vec::<(f64, f64)>::new();
        for row in info["recharge_bonus_tiers"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let (Some(min), Some(percent)) =
                (number(&row["min_amount"]), number(&row["bonus_percent"]))
            {
                if min >= 0.
                    && (round(min, 2) - min).abs() < 1e-9
                    && (0. ..=1000.).contains(&percent)
                    && (round(percent, 2) - percent).abs() < 1e-9
                    && !tiers.iter().any(|(m, _)| *m == min)
                {
                    tiers.push((min, percent));
                }
            }
        }
        let percent = tiers
            .iter()
            .filter(|(min, _)| amount + 1e-9 >= *min)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| *p)
            .unwrap_or(0.);
        if info["recharge_bonus_mode"]
            .as_str()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("discount")
        {
            if percent > 0. && percent < 100. {
                let discounted = round(amount * (100. - percent) / 100., digits);
                if discounted > 0. && discounted < amount {
                    pay_base = discounted;
                }
            }
            credited = Some(base);
        } else {
            credited = Some(round(base + round(base * percent / 100., 2), 2));
        }
    } else if kind == "subscription" {
        if currency == "CNY" {
            if let Some(rate) = number(&info["subscription_usd_to_cny_rate"]).filter(|n| *n > 0.) {
                pay_base *= rate;
            }
        }
        pay_base = round(pay_base, digits);
    }
    let rate = number(&info["recharge_fee_rate"])
        .filter(|n| *n > 0.)
        .unwrap_or(0.);
    // The deployed recharge view rounds fees to cents; subscriptions use currency digits.
    let factor = 10f64.powi(if kind == "balance" { 2 } else { digits });
    let fee = (pay_base * rate / 100. * factor).ceil() / factor;
    let total = round(pay_base + fee, if kind == "balance" { 2 } else { digits });
    ensure!(
        pay_base.is_finite() && total.is_finite() && credited.is_none_or(f64::is_finite),
        "站点价格设置无效，请重新读取"
    );
    Ok(Quote {
        currency,
        pay_base,
        fee,
        total,
        credited,
    })
}
pub fn validate_method(method: &Value, quote: &Quote) -> Result<()> {
    ensure!(
        method["available"] != false,
        "此支付方式当前不可用，请重新选择"
    );
    if let Some(min) = number(&method["single_min"]).filter(|n| *n > 0.) {
        ensure!(quote.pay_base >= min, "金额低于此支付方式的单笔下限 {min}");
    }
    if let Some(max) = number(&method["single_max"]).filter(|n| *n > 0.) {
        ensure!(quote.pay_base <= max, "金额超过此支付方式的单笔上限 {max}");
    }
    Ok(())
}
/// Mirrors invoiceTax.ts integer rounding, including half-cent boundaries.
pub fn invoice_tax(cents: i64, rate_bps: i64) -> Result<i64> {
    ensure!(
        (1..=1_000_000_000_000).contains(&cents) && (0..=10000).contains(&rate_bps),
        "站点开票金额或税率无效，请重新读取"
    );
    let denominator = 10000 + rate_bps as i128;
    let net = (cents as i128 * 10000 + denominator / 2) / denominator;
    Ok(cents - net as i64)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn invoice_tax_matches_integer_site_rounding_and_rejects_unknown_rates() {
        assert_eq!(invoice_tax(10, 600).unwrap(), 1);
        assert_eq!(invoice_tax(10000, 600).unwrap(), 566);
        assert_eq!(invoice_tax(1, 0).unwrap(), 0);
        assert!(invoice_tax(100, -1).is_err());
        assert!(invoice_tax(100, 10001).is_err());
    }
    #[test]
    fn recharge_matches_site_discount_multiplier_bonus_fee_and_method_limits() {
        let info = json!({"balance_recharge_multiplier":0.14,"recharge_fee_rate":1.5,"recharge_bonus_mode":"discount","recharge_bonus_tiers":[{"min_amount":100,"bonus_percent":20}]});
        let method = json!({"currency":"CNY","single_max":85});
        let q = quote(&info, &method, "balance", 100.).unwrap();
        assert_eq!(
            (q.pay_base, q.credited, q.fee, q.total),
            (80., Some(14.), 1.2, 81.2)
        );
        assert!(validate_method(&method, &q).is_ok());
        let mut info = info;
        info["recharge_bonus_mode"] = "bonus".into();
        let q = quote(&info, &method, "balance", 100.).unwrap();
        assert_eq!(q.credited, Some(16.8));
        assert!(validate_method(&method, &q).is_err());
    }
    #[test]
    fn subscriptions_only_convert_configured_cny_and_round_fees_to_currency() {
        let info = json!({"subscription_usd_to_cny_rate":7.2,"recharge_fee_rate":1.5});
        assert_eq!(
            quote(&info, &json!({"currency":"CNY"}), "subscription", 10.)
                .unwrap()
                .total,
            73.08
        );
        assert_eq!(
            quote(&info, &json!({"currency":"USD"}), "subscription", 10.)
                .unwrap()
                .total,
            10.15
        );
        assert_eq!(
            quote(&info, &json!({"currency":"JPY"}), "subscription", 100.)
                .unwrap()
                .total,
            102.
        );
        assert!(quote(&info, &Value::Null, "balance", f64::NAN).is_err());
    }
}
