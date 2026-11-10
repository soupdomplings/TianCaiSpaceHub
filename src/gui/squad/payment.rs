//! Native checkout; verification continues an existing order, never creates a second one.
use super::super::controls::Button;
use super::*;
use crate::gui::controls::{Choice, TextCtrl};
use wxdragon::{
    event::WebViewEvents,
    widgets::{WebView, WebViewBackend, WebViewUserScriptInjectionTime},
};

pub(super) fn with_base(frame: &Frame, api: ApiClient, base: Value) {
    let Some(checkout) = query(frame, api.clone(), "读取支付方式", "/payment/checkout-info")
    else {
        return;
    };
    let methods = checkout["methods"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(_, v)| {
                    v["available"] != false
                        && v["daily_remaining"].as_f64().is_none_or(|v| v > 0.)
                        && v["currency"]
                            .as_str()
                            .unwrap_or("CNY")
                            .eq_ignore_ascii_case("CNY")
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if methods.is_empty() {
        super::super::show_info(frame, "当前没有可用支付方式，请重新读取或联系站点管理员");
        return;
    }
    let plans = checkout["plans"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|v| v["for_sale"] != false)
        .collect::<Vec<_>>();
    let forced = base["order_type"].as_str().unwrap_or("").to_string();
    let mut types = Vec::new();
    if forced.is_empty() {
        if checkout["balance_disabled"] != true {
            types.push(("充值", "balance"));
        }
        if !plans.is_empty() {
            types.push(("订阅", "subscription"));
        }
    } else {
        types.push((
            if forced == "organization_settlement" {
                "组织结算"
            } else {
                "组织预存"
            },
            forced.as_str(),
        ));
    }
    if types.is_empty() {
        super::super::show_info(frame, "当前没有可购买的充值或订阅方案");
        return;
    }
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(620, 500));
    let dialog = Dialog::builder(frame, "充值 / 订阅")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(14)
        .with_hgap(16)
        .build();
    grid.add_growable_col(1, 1);
    let kind = Choice::builder(&dialog)
        .with_choices(types.iter().map(|(label, _)| label.to_string()).collect())
        .with_selection(Some(0))
        .build();
    let method = Choice::builder(&dialog)
        .with_choices(
            methods
                .iter()
                .map(|(k, v)| {
                    v["display_name"]
                        .as_str()
                        .unwrap_or_else(|| method_label(k))
                        .to_string()
                })
                .collect(),
        )
        .with_selection(Some(0))
        .build();
    let plan = Choice::builder(&dialog)
        .with_choices(
            plans
                .iter()
                .map(|p| {
                    format!(
                        "{} · {} {} · {} 天",
                        p["name"].as_str().unwrap_or("订阅"),
                        "",
                        crate::desktop::squad_money::Money::checkout(&checkout).usd(&p["price"]),
                        p["validity_days"]
                    )
                })
                .collect(),
        )
        .with_selection(Some(0))
        .build();
    let amount = TextCtrl::builder(&dialog)
        .with_value(
            &base["amount"]
                .as_f64()
                .map(|v| format!("{v:.2}"))
                .unwrap_or_default(),
        )
        .build();
    super::super::clients::add_field(&dialog, &grid, "用途", &kind);
    super::super::clients::add_field(&dialog, &grid, "支付方式", &method);
    super::super::clients::add_field(&dialog, &grid, "订阅方案", &plan);
    super::super::clients::add_field(&dialog, &grid, "金额", &amount);
    column.add_sizer(&grid, 0, SizerFlag::Expand | SizerFlag::All, 20);
    let summary = StaticText::builder(&dialog).build();
    column.add(
        &summary,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        20,
    );
    let status = StaticText::builder(&dialog).build();
    column.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let save = Button::builder(&dialog)
        .with_label("确认并创建订单")
        .build();
    column.add(&save, 0, SizerFlag::AlignRight | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    let types = types.iter().map(|(_, v)| v.to_string()).collect::<Vec<_>>();
    let kinds = types.clone();
    let options = plans.clone();
    let limits = methods.clone();
    let info = checkout.clone();
    let update = Rc::new(move || {
        let order_kind = kinds
            .get(kind.get_selection().unwrap_or(0) as usize)
            .map(String::as_str)
            .unwrap_or("balance");
        let subscription = order_kind == "subscription";
        plan.enable(subscription);
        amount.enable(!subscription && forced != "organization_settlement");
        let limit = &limits[method.get_selection().unwrap_or(0) as usize].1;
        let price = if subscription {
            options
                .get(plan.get_selection().unwrap_or(0) as usize)
                .and_then(|v| v["price"].as_f64())
                .unwrap_or(0.)
        } else {
            amount.get_value().parse::<f64>().unwrap_or(0.)
        };
        match crate::desktop::squad_quote::quote(&info, limit, order_kind, price) {
            Ok(q) => {
                let credit = q
                    .credited
                    .map(|n| {
                        format!(
                            "\n预计到账 {}",
                            crate::desktop::squad_money::Money::checkout(&info).usd(&json!(n))
                        )
                    })
                    .unwrap_or_default();
                let validation = crate::desktop::squad_quote::validate_method(limit, &q)
                    .err()
                    .map(|e| format!("\n{e}"))
                    .unwrap_or_default();
                summary.set_label(&format!(
                    "预计支付 {}    手续费 {}{}{}",
                    crate::desktop::squad_money::cny(q.total),
                    crate::desktop::squad_money::cny(q.fee),
                    credit,
                    validation
                ));
            }
            Err(error) => summary.set_label(&error.to_string()),
        }
        summary.wrap(530);
        dialog.layout();
    });
    for c in [kind, method, plan] {
        let f = update.clone();
        c.on_selection_changed(move |_| f());
    }
    let f = update.clone();
    amount.on_text_changed(move |_| f());
    update();
    let info = checkout;
    let job = dialog_job::bind(dialog, save, status, api.clone(), move || {
        let order_type = types[kind.get_selection().unwrap_or(0) as usize].clone();
        let (payment_type, limit) = &methods[method.get_selection().unwrap_or(0) as usize];
        let selected = plans.get(plan.get_selection().unwrap_or(0) as usize);
        let value = if order_type == "subscription" {
            selected
                .and_then(|v| v["price"].as_f64())
                .ok_or("请选择订阅方案")?
        } else {
            amount
                .get_value()
                .parse::<f64>()
                .map_err(|_| "请输入有效金额")?
        };
        if !value.is_finite() || value <= 0. {
            return Err("金额需要大于零".into());
        }
        let quote = crate::desktop::squad_quote::quote(&info, limit, &order_type, value)
            .map_err(|e| e.to_string())?;
        crate::desktop::squad_quote::validate_method(limit, &quote).map_err(|e| e.to_string())?;
        let mut body = base.clone();
        body["order_type"] = order_type.clone().into();
        body["payment_type"] = payment_type.clone().into();
        body["amount"] = value.into();
        body["is_mobile"] = false.into();
        body["return_url"] = format!("{}/payment/result", crate::desktop::squad::ORIGIN).into();
        if order_type == "subscription" {
            body["plan_id"] = selected.unwrap()["id"].clone();
        }
        Ok((
            "/api/desktop/squad/request".into(),
            json!({"method":"POST","path":"/payment/orders","body":body}),
        ))
    });
    let accepted = dialog.show_modal() == ID_OK;
    let result = job.result.borrow().clone();
    drop(job);
    dialog.destroy();
    if accepted {
        if let Some(order) = result {
            continue_order(frame, api, &order);
        }
    }
}
fn method_label(key: &str) -> &str {
    match key {
        "alipay" | "alipay_direct" => "支付宝",
        "wxpay" | "wxpay_direct" => "微信支付",
        "stripe" => "银行卡 / Stripe",
        "airwallex" => "Airwallex",
        _ => key,
    }
}

pub(super) fn continue_order(frame: &Frame, api: ApiClient, order: &Value) {
    let id = order["order_id"].as_u64().or_else(|| order["id"].as_u64());
    let Some(id) = id else {
        super::super::show_error(
            frame,
            "站点未返回订单编号，请先回到主站核对订单，勿再次下单",
        );
        return;
    };
    let Some(merged) = dialog_job::run(
        frame,
        api.clone(),
        "读取已有订单",
        "/api/desktop/squad/payment-recovery",
        json!({"id":id}),
    )
    .filter(|v| v["order_id"].as_u64() == Some(id)) else {
        return;
    };
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(620, 720));
    let dialog = Dialog::builder(frame, "完成已有订单")
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let title = StaticText::builder(&dialog)
        .with_label(&format!(
            "订单 {}   {}",
            merged["out_trade_no"].as_str().unwrap_or(""),
            crate::desktop::squad_money::Money::default().currency(
                &merged["pay_amount"],
                merged["currency"].as_str().unwrap_or("CNY")
            )
        ))
        .build();
    title.set_font(&theme::font(theme::TextRole::Title));
    column.add(&title, 0, SizerFlag::Expand | SizerFlag::All, 20);
    if let Some(code) = merged["qr_code"].as_str().filter(|v| !v.is_empty()) {
        if let Some((bitmap, _)) = super::super::onboarding::qr_bitmap(code) {
            let qr = StaticBitmap::builder(&dialog)
                .with_bitmap(Some(bitmap))
                .build();
            column.add(&qr, 1, SizerFlag::AlignCentre | SizerFlag::All, 12);
        }
    }
    let status = StaticText::builder(&dialog)
        .with_label("付款后点击核对付款，订单结果以站点为准。")
        .build();
    column.add(&status, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    let open = Button::builder(&dialog).with_label("继续付款").build();
    let verify = Button::builder(&dialog).with_label("核对付款").build();
    buttons.add(&open, 0, SizerFlag::Right, 10);
    buttons.add(&verify, 0, SizerFlag::default(), 0);
    column.add_sizer(&buttons, 0, SizerFlag::AlignRight | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    let resumable = ["pay_url", "qr_code", "client_secret", "resume_token"]
        .iter()
        .any(|k| merged[*k].as_str().is_some_and(|v| !v.is_empty()));
    open.enable(resumable && merged["status"].as_str().is_none_or(|s| s == "PENDING"));
    if !resumable {
        status.set_label("此订单没有可恢复的付款凭据，可核对状态或取消待支付订单。");
    }
    let (a, o, f) = (api.clone(), merged.clone(), *frame);
    open.on_click(move |_| {
        let raw = payment_target(&o);
        official_window(&f, a.clone(), "继续已有订单", &raw, Some(&o));
    });
    let number = merged["out_trade_no"].as_str().unwrap_or("").to_string();
    let a = api.clone();
    let f = *frame;
    verify.on_click(move |_| {
        if number.is_empty() {
            status.set_label("请重新读取我的订单，核对对应订单号");
            return;
        }
        if let Some(result) = dialog_job::run(
            &f,
            a.clone(),
            "核对付款",
            "/api/desktop/squad/request",
            json!({"method":"POST","path":"/payment/orders/verify","body":{"out_trade_no":number}}),
        ) {
            status.set_label(match result["status"].as_str() {
                Some("COMPLETED") => "已完成到账",
                Some("PAID" | "RECHARGING") => "已支付，站点正在处理到账",
                Some("PENDING") => "等待付款",
                Some("EXPIRED") => "订单已过期",
                Some("CANCELLED") => "订单已取消",
                _ => "请在我的订单中查看当前结果",
            });
        }
    });
    dialog.show_modal();
    dialog.destroy();
}

/// Seed only the official origin. Initial provider HTTPS origin may handle its payment redirects.
pub(super) fn official_window(
    frame: &Frame,
    api: ApiClient,
    title: &str,
    raw: &str,
    order: Option<&Value>,
) {
    let Ok(url) = url::Url::parse(raw) else {
        return;
    };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        super::super::show_error(frame, "站点返回的地址无效，请重新读取订单");
        return;
    }
    let Some(seed) = dialog_job::run(
        frame,
        api,
        "准备安全窗口",
        "/api/desktop/squad/browser-session",
        json!({}),
    ) else {
        return;
    };
    let size = super::super::ai_gw_channel_dialog_size(frame, Size::new(940, 760));
    let dialog = Dialog::builder(frame, title)
        .with_size(size.width, size.height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let Some(view) = WebView::try_private(
        &dialog,
        if cfg!(windows) {
            WebViewBackend::Edge
        } else {
            WebViewBackend::WebKit
        },
    ) else {
        dialog.destroy();
        super::super::show_error(frame, "安全窗口不可用，请修复 WebView2 Runtime 后重试");
        return;
    };
    view.enable_access_to_dev_tools(false);
    view.enable_context_menu(false);
    let mut local = seed;
    if let Some(o) = order {
        local["payment.recovery.current"] = recovery(o);
    }
    let script = format!(
        r#"(()=>{{if(location.origin!=={origin})return;const seed={seed};const marker={marker};if(sessionStorage.getItem('hub.seed')===marker)return;for(const [k,v] of Object.entries(seed)){{localStorage.setItem(k,typeof v==='string'?v:JSON.stringify(v));}}sessionStorage.setItem('hub.seed',marker);}})();"#,
        origin = serde_json::to_string(crate::desktop::squad::ORIGIN).unwrap(),
        seed = local,
        marker = serde_json::to_string(&uuid::Uuid::new_v4().to_string()).unwrap()
    );
    view.add_user_script(&script, WebViewUserScriptInjectionTime::AtDocumentStart);
    let host = url.host_str().unwrap_or_default().to_string();
    let h = host.clone();
    view.on_navigating(move |event| {
        let allowed = event
            .get_string()
            .is_some_and(|s| payment_navigation(&s, &h));
        if !allowed {
            event.event.event.veto();
        }
    });
    view.on_new_window(move |event| {
        event.event.event.veto();
        if let Some(s) = event.get_string().filter(|s| payment_navigation(s, &host)) {
            view.load_url(&s);
        }
    });
    let column = BoxSizer::builder(Orientation::Vertical).build();
    column.add(&view, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(column, true);
    let target = if raw.ends_with("/purchase") {
        order
            .map(|o| {
                let mut u = url.clone();
                u.query_pairs_mut()
                    .append_pair("resume_token", o["resume_token"].as_str().unwrap_or(""));
                u.to_string()
            })
            .unwrap_or_else(|| raw.into())
    } else {
        raw.into()
    };
    view.load_url(&target);
    dialog.show_modal();
    view.stop();
    view.remove_all_user_scripts();
    dialog.destroy();
}
fn payment_navigation(raw: &str, initial_host: &str) -> bool {
    raw == "about:blank"
        || super::super::squad_login::same_origin(crate::desktop::squad::ORIGIN, raw)
        || url::Url::parse(raw).is_ok_and(|u| {
            u.scheme() == "https"
                && u.username().is_empty()
                && u.password().is_none()
                && u.host_str().is_some_and(|h| {
                    h == initial_host
                        || matches!(
                            h,
                            "checkout.stripe.com"
                                | "hooks.stripe.com"
                                | "js.stripe.com"
                                | "mapi.alipay.com"
                                | "openapi.alipay.com"
                                | "open.weixin.qq.com"
                                | "wx.tenpay.com"
                        )
                })
        })
}
fn recovery(order: &Value) -> Value {
    let mut result = json!({"orderId":order["order_id"].as_u64().or_else(||order["id"].as_u64()).unwrap_or(0),"amount":order["amount"].as_f64().unwrap_or(0.),"payAmount":order["pay_amount"].as_f64().unwrap_or(0.),"createdAt":crate::ai_gateway::request_log::now_ms()});
    for (target, source) in [
        ("qrCode", "qr_code"),
        ("expiresAt", "expires_at"),
        ("paymentType", "payment_type"),
        ("payUrl", "pay_url"),
        ("outTradeNo", "out_trade_no"),
        ("clientSecret", "client_secret"),
        ("intentId", "intent_id"),
        ("currency", "currency"),
        ("countryCode", "country_code"),
        ("paymentEnv", "payment_env"),
        ("orderType", "order_type"),
        ("paymentMode", "payment_mode"),
        ("resumeToken", "resume_token"),
    ] {
        result[target] = order[source].as_str().unwrap_or("").into();
    }
    result
}
fn payment_target(order: &Value) -> String {
    let origin = crate::desktop::squad::ORIGIN;
    if order["payment_type"] == "airwallex"
        && order["client_secret"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    {
        let mut u = url::Url::parse(&format!("{origin}/payment/airwallex")).unwrap();
        u.query_pairs_mut()
            .append_pair("order_id", &order["order_id"].to_string())
            .append_pair("out_trade_no", order["out_trade_no"].as_str().unwrap_or(""))
            .append_pair("resume_token", order["resume_token"].as_str().unwrap_or(""));
        return u.to_string();
    }
    if let Some(secret) = order["client_secret"].as_str().filter(|v| !v.is_empty()) {
        let mut u = url::Url::parse(&format!("{origin}/payment/stripe")).unwrap();
        u.query_pairs_mut()
            .append_pair("order_id", &order["order_id"].to_string())
            .append_pair("client_secret", secret)
            .append_pair("resume_token", order["resume_token"].as_str().unwrap_or(""));
        return u.to_string();
    }
    order["pay_url"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{origin}/purchase"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payment_redirects_require_exact_https_origin() {
        assert!(payment_navigation(
            "https://pay.example.invalid/continue",
            "pay.example.invalid"
        ));
        for raw in [
            "https://pay.example.invalid.evil/",
            "http://pay.example.invalid/",
            "https://user@pay.example.invalid/",
            "file:///c:/foo",
        ] {
            assert!(!payment_navigation(raw, "pay.example.invalid"));
        }
    }
    #[test]
    fn recovery_keeps_existing_order_identity() {
        let v = recovery(&json!({"order_id":42,"resume_token":"synthetic","pay_amount":2}));
        assert_eq!(v["orderId"], 42);
        assert_eq!(v["resumeToken"], "synthetic");
        assert_eq!(v["qrCode"], "");
    }
}
