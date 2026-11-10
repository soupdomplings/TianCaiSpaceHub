//! The production account workspace, shared with hidden native layout review.
//! No network, session storage or synthetic fallback data lives in this module.
use super::{
    controls::{Button, ButtonKind, TextCtrl},
    squad_browser::friendly_error,
    theme,
    visual::{self, Canvas, Icon},
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::prelude::*;
use wxdragon::widgets::SimpleBook;
#[path = "squad_view/allocation.rs"]
pub(super) mod allocation;
#[path = "squad_view/calendar.rs"]
mod calendar;
#[path = "squad/dashboard.rs"]
mod dashboard;
#[path = "squad_view/data.rs"]
mod data;
#[path = "squad_view/detail.rs"]
pub(super) mod detail;
#[path = "squad_view/filters.rs"]
pub(super) mod filters;
#[path = "squad_view/member.rs"]
pub(super) mod member;
#[path = "squad_view/navigation.rs"]
mod navigation;
#[path = "squad_view/organization.rs"]
pub(super) mod organization;
#[path = "squad_view/organization_data.rs"]
pub(super) mod organization_data;
#[path = "squad_view/purchase.rs"]
mod purchase;
#[path = "squad_view/selection.rs"]
pub(super) mod selection;
#[path = "squad_view/summary.rs"]
mod summary;

// Keep page identities aligned when account features are added or removed.
#[repr(usize)]
enum BodyPage {
    Purchase,
    Dashboard,
    Summary,
    Table,
}

#[derive(Clone)]
pub(super) struct Workspace {
    pub page: Panel,
    navigation: navigation::Navigation,
    pub table: ListCtrl,
    pub heading: StaticText,
    pub status: StaticText,
    pub website: Button,
    pub refresh: Button,
    pub logout: Button,
    pub primary: Button,
    pub edit: Button,
    pub remove: Button,
    pub extra: Button,
    pub invoice: Button,
    pub previous: Button,
    pub next: Button,
    pub start: filters::DateField,
    pub end: filters::DateField,
    pub apply: Button,
    pub filters: Panel,
    pub filter_bar: filters::Filters,
    pub fallback: CheckBox,
    pub purchase: purchase::Purchase,
    dashboard: dashboard::Dashboard,
    summary: summary::Summary,
    toolbar: Panel,
    footer: Panel,
    body: SimpleBook,
    key: Rc<RefCell<String>>,
    rows: Rc<RefCell<Vec<Value>>>,
    available: Rc<Cell<bool>>,
    has_pending: Rc<Cell<bool>>,
}
impl Workspace {
    pub fn on_select(&self, f: impl Fn(String) + 'static) {
        self.navigation.on_select(f);
    }
    pub fn new(parent: &dyn WxWidget) -> Self {
        let t = theme::theme();
        let page = Panel::builder(parent).build();
        page.set_background_color(t.bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let navigation = navigation::Navigation::new(&page);
        col.add(
            &navigation.page,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let header = BoxSizer::builder(Orientation::Horizontal).build();
        let heading = StaticText::builder(&page).with_label("我的账户").build();
        heading.set_font(&theme::font(theme::TextRole::Title));
        heading.set_foreground_color(t.ink_primary);
        header.add(&heading, 1, SizerFlag::AlignCenterVertical, 0);
        let website = Button::builder(&page)
            .with_label("回到主站")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(100, 32))
            .build();
        let refresh = Button::builder(&page)
            .with_icon(Icon::Refresh)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(32, 32))
            .build();
        refresh.set_tooltip("重新读取当前页面");
        let logout = Button::builder(&page)
            .with_label("退出登录")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(104, 32))
            .build();
        for b in [website, refresh] {
            header.add(&b, 0, SizerFlag::Left, page.from_dip(4));
        }
        col.add_sizer(
            &header,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(12),
        );
        logout.hide();
        let toolbar = Panel::builder(&page).build();
        toolbar.set_background_color(t.bg_app);
        let actions = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let primary = Button::builder(&toolbar)
            .with_label("新建密钥")
            .with_kind(ButtonKind::Primary)
            .with_icon(Icon::Plus)
            .with_size(Size::new(116, 32))
            .build();
        let edit = Button::builder(&toolbar)
            .with_label("编辑")
            .with_size(Size::new(92, 32))
            .build();
        let remove = Button::builder(&toolbar)
            .with_label("删除")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(92, 32))
            .build();
        let extra = Button::builder(&toolbar)
            .with_label("导入 Hub")
            .with_size(Size::new(116, 32))
            .build();
        let invoice = Button::builder(&toolbar)
            .with_label("发票与文件")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(116, 32))
            .build();
        for b in [primary, extra, edit, remove, invoice] {
            actions.add(
                &b,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                page.from_dip(6),
            );
        }
        let fallback = CheckBox::builder(&toolbar)
            .with_label("允许个人余额兜底")
            .build();
        fallback.set_tooltip("开启后，组织额度不足时可使用个人余额；关闭后不使用个人余额兜底");
        actions.add(
            &fallback,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::All,
            page.from_dip(8),
        );
        toolbar.set_sizer(actions, false);
        col.add(
            &toolbar,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(8),
        );
        let filter_bar = filters::Filters::new(&page);
        let (filters, start, end, apply) = (
            filter_bar.panel,
            filter_bar.start,
            filter_bar.end,
            filter_bar.apply,
        );
        filter_bar.mode(None, false);
        col.add(
            &filters,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(10),
        );
        // A single expanding body prevents an inactive page's native minimum
        // size from reserving most of the list page's vertical space.
        let body = SimpleBook::builder(&page).build();
        body.set_min_size(Size::new(0, 0));
        let dashboard = dashboard::Dashboard::new(&body);
        let summary = summary::Summary::new(&body);
        let purchase = purchase::Purchase::new(&body);
        let table = ListCtrl::builder(&body)
            .with_style(
                ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::BorderNone,
            )
            .build();
        table.set_min_size(visual::dip_size(&table, 120, 90));
        table.set_background_color(t.bg_card);
        table.set_foreground_color(t.ink_primary);
        table.set_font(&theme::font(theme::TextRole::Body));
        body.add_page(&purchase.panel, "充值", false, None);
        body.add_page(&dashboard.panel, "账户", false, None);
        body.add_page(&summary.panel, "详情", false, None);
        body.add_page(&table, "记录", false, None);
        body.change_selection(BodyPage::Summary as usize);
        col.add(&body, 1, SizerFlag::Expand, 0);
        col.add(
            &logout,
            0,
            SizerFlag::AlignRight | SizerFlag::Top,
            page.from_dip(8),
        );
        let footer = Panel::builder(&page).build();
        footer.set_background_color(t.bg_app);
        let bottom = BoxSizer::builder(Orientation::Horizontal).build();
        let status = StaticText::builder(&footer).build();
        status.set_foreground_color(t.ink_secondary);
        bottom.add(
            &status,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            page.from_dip(8),
        );
        let previous = Button::builder(&footer)
            .with_icon(Icon::Back)
            .with_label("上一页")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(92, 30))
            .build();
        let next = Button::builder(&footer)
            .with_label("下一页")
            .with_icon(Icon::Chevron)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(92, 30))
            .build();
        bottom.add(&previous, 0, SizerFlag::Right, page.from_dip(6));
        bottom.add(&next, 0, SizerFlag::default(), 0);
        footer.set_sizer(bottom, false);
        col.add(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(8),
        );
        page.set_sizer(col, false);
        let ui = Self {
            page,
            navigation,
            table,
            heading,
            status,
            website,
            refresh,
            logout,
            primary,
            edit,
            remove,
            extra,
            invoice,
            previous,
            next,
            start,
            end,
            apply,
            filters,
            filter_bar,
            fallback,
            purchase,
            dashboard,
            summary,
            toolbar,
            footer,
            body,
            key: Default::default(),
            rows: Default::default(),
            available: Default::default(),
            has_pending: Default::default(),
        };
        let u = ui.clone();
        table.on_size(move |event| {
            u.fit_columns();
            event.skip(true);
        });
        let u = ui.clone();
        table.on_item_selected(move |_| u.enable_actions());
        let u = ui.clone();
        table.on_item_deselected(move |_| u.enable_actions());
        for p in [toolbar, filters, footer, ui.dashboard.panel] {
            p.hide();
        }
        table.hide();
        ui.purchase.panel.hide();
        ui.summary
            .render("loading", &Value::Null, &Value::Null, Default::default());
        ui
    }
    pub fn selected(&self) -> Option<Value> {
        let i = self
            .table
            .get_next_item(-1, ListNextItemFlag::All, ListItemState::Selected);
        (i >= 0)
            .then(|| self.rows.borrow().get(i as usize).cloned())
            .flatten()
    }
    fn enable_actions(&self) {
        let ready = self.available.get();
        let selected = self.selected().is_some();
        let key = self.key.borrow();
        self.primary.enable(
            ready
                && (!matches!(
                    key.as_str(),
                    "organization" | "organization-admin" | "organization-usage"
                ) || selected),
        );
        self.edit
            .enable(ready && (key.as_str() == "organization" || selected));
        self.remove.enable(ready && selected);
        self.fallback.enable(ready);
        self.extra
            .enable(ready && (key.as_str() != "keys" || selected || self.has_pending.get()));
        self.invoice
            .enable(ready && (key.as_str() == "organization" || selected));
    }
    pub fn clear(&self) {
        self.rows.borrow_mut().clear();
        self.table.delete_all_items();
        self.navigation.clear();
        self.available.set(false);
        self.dashboard
            .render(&Value::Null, &Value::Null, Default::default());
        self.purchase.clear();
        self.logout.hide();
        self.fallback.hide();
        self.summary
            .render("loading", &Value::Null, &Value::Null, Default::default());
        self.key.borrow_mut().clear();
        self.body.change_selection(BodyPage::Summary as usize);
    }
    pub fn loading(&self, key: &str, state: &Value) {
        self.available.set(false);
        self.enable_actions();
        let menus = state["menus"].as_array().cloned().unwrap_or_default();
        self.navigation.render(&menus, key);
        if *self.key.borrow() != key {
            self.key.replace(key.into());
            self.table.hide();
            self.purchase.panel.hide();
            self.logout.hide();
            self.dashboard.panel.hide();
            self.summary.panel.show(true);
            self.body.change_selection(BodyPage::Summary as usize);
            self.summary
                .render("loading", &Value::Null, &Value::Null, Default::default());
            self.toolbar.hide();
            self.filters.hide();
            self.footer.hide();
            self.heading.set_label(if key == "profile" {
                "个人资料"
            } else {
                menus
                    .iter()
                    .find(|m| m["id"] == key)
                    .and_then(|m| m["label"].as_str())
                    .unwrap_or("我的账户")
            });
        }
        self.page.layout();
    }
    pub fn error(&self, error: &str) {
        self.available.set(false);
        self.enable_actions();
        self.status.set_label(&friendly_error(error));
        self.status.set_foreground_color(theme::theme().error);
        self.status
            .wrap((self.page.get_client_size().width - self.page.from_dip(20)).max(120));
        self.previous.hide();
        self.next.hide();
        self.footer.show(true);
        self.page.layout();
    }
    pub fn notice(&self, notice: &str) {
        self.status.set_label(&friendly_error(notice));
        self.status
            .set_foreground_color(theme::theme().ink_secondary);
        self.status
            .wrap((self.page.get_client_size().width - self.page.from_dip(210)).max(120));
        self.footer.show(true);
        self.page.layout();
    }
    pub fn render(&self, key: &str, state: &Value, value: &Value, page_index: usize) {
        self.key.replace(key.into());
        let menus = state["menus"].as_array().cloned().unwrap_or_default();
        self.navigation.render(&menus, key);
        self.heading.set_label(if key == "profile" {
            "个人资料"
        } else {
            menus
                .iter()
                .find(|m| m["id"] == key)
                .and_then(|m| m["label"].as_str())
                .unwrap_or("我的账户")
        });
        self.has_pending
            .set(state["pending"].as_array().is_some_and(|p| !p.is_empty()));
        let failed = value["loadError"].is_string();
        self.available.set(!failed);
        let primary = match key {
            "keys" => Some("新建密钥"),
            "redeem" => Some("兑换"),
            "organization" => Some("选择身份"),
            "organization-admin" => Some("管理组织"),
            "organization-usage" => Some("查看用量"),
            "affiliate" => Some("转入余额"),
            key if key.starts_with("custom-") => Some("打开工具"),
            _ => None,
        };
        self.primary.show(primary.is_some());
        if let Some(label) = primary {
            self.primary.set_label(label);
            self.primary.set_icon(if key == "keys" {
                Icon::Plus
            } else {
                Icon::None
            });
        }
        self.edit
            .show(matches!(key, "keys" | "orders" | "organization"));
        self.edit.set_label(match key {
            "orders" => "核对付款",
            "organization" => "资金顺序",
            _ => "编辑",
        });
        self.remove.show(matches!(key, "keys" | "orders"));
        self.remove.set_label(if key == "orders" {
            "取消订单"
        } else {
            "删除"
        });
        self.extra
            .show(matches!(key, "keys" | "orders") || self.has_pending.get());
        self.extra.set_label(if self.has_pending.get() {
            "待确认操作"
        } else {
            match key {
                "keys" => "导入 Hub",
                "orders" => "退款申请",
                _ => "余额兜底",
            }
        });
        self.extra.set_kind(ButtonKind::Secondary);
        self.invoice.show(matches!(key, "orders" | "organization"));
        self.logout.show(key == "profile");
        self.fallback.show(key == "organization");
        self.fallback.set_value(
            state["organization"]["allow_personal_balance_fallback"]
                .as_bool()
                .unwrap_or(false),
        );
        self.toolbar.show(
            [
                self.primary,
                self.extra,
                self.edit,
                self.remove,
                self.invoice,
            ]
            .iter()
            .any(|b| b.is_shown())
                || self.fallback.is_shown(),
        );
        self.filter_bar.mode(None, data::date_filter(key));
        let columns = data::columns(key);
        let rows = if columns.is_empty() {
            vec![]
        } else {
            data::rows(key, value)
        };
        let table = !failed && !columns.is_empty() && !rows.is_empty();
        self.table.show(table);
        self.dashboard.panel.show(!failed && key == "account");
        self.summary
            .panel
            .show(failed || key != "account" && key != "purchase" && !table);
        self.purchase.panel.show(!failed && key == "purchase");
        self.body.change_selection(
            (if failed {
                BodyPage::Summary
            } else {
                match key {
                    "purchase" => BodyPage::Purchase,
                    "account" => BodyPage::Dashboard,
                    _ if table => BodyPage::Table,
                    _ => BodyPage::Summary,
                }
            }) as usize,
        );
        if key == "purchase" && !failed {
            self.purchase.render(value, true);
        }
        if key == "account" && !failed {
            self.dashboard.render(
                &state["user"],
                value,
                crate::desktop::squad_money::Money::state(state),
            );
        }
        if self.summary.panel.is_shown() {
            self.summary.render(
                if failed { "error" } else { key },
                &state["user"],
                value,
                crate::desktop::squad_money::Money::state(state),
            );
        }
        self.table.freeze();
        self.table.clear_all();
        for (i, (_, label, _)) in columns.iter().enumerate() {
            self.table
                .insert_column(i as i64, label, ListColumnFormat::Left, 80);
        }
        for (i, row) in rows.iter().enumerate() {
            for (j, (field, _, _)) in columns.iter().enumerate() {
                let text =
                    data::cell_money(row, field, crate::desktop::squad_money::Money::state(state));
                if j == 0 {
                    self.table.insert_item(i as i64, &text, None);
                } else {
                    self.table
                        .set_item_text_by_column(i as i64, j as i32, &text);
                }
            }
            if i % 2 == 1 {
                self.table
                    .set_item_background_colour(i as i64, &theme::theme().table_row_alt);
            }
        }
        self.rows.replace(rows.clone());
        self.table.thaw();
        let total = value["total"]
            .as_u64()
            .or_else(|| value["pagination"]["total"].as_u64());
        let pages = data::paginated(key)
            && (page_index > 0
                || total.is_some_and(|n| n > 50)
                || total.is_none() && rows.len() == 50);
        self.previous.show(pages);
        self.next.show(pages);
        self.previous.enable(!failed && page_index > 0);
        self.next.enable(
            !failed
                && (total.is_some_and(|n| n > ((page_index + 1) * 50) as u64)
                    || total.is_none() && rows.len() == 50),
        );
        let notice = if let Some(e) = value["loadError"].as_str() {
            friendly_error(e)
        } else if state["organizationUnknown"] == true {
            "组织权限未读取完成，可点击右上角刷新重试".into()
        } else if let Some(error) = value["usageLoadError"].as_str() {
            error.into()
        } else if pages {
            format!(
                "第 {} 页{}",
                page_index + 1,
                total.map(|n| format!(" · 共 {n} 条")).unwrap_or_default()
            )
        } else {
            String::new()
        };
        self.status.set_label(&notice);
        self.status.set_foreground_color(if failed {
            theme::theme().error
        } else {
            theme::theme().ink_secondary
        });
        self.status.wrap(
            (self.page.get_client_size().width - self.page.from_dip(if pages { 210 } else { 20 }))
                .max(120),
        );
        self.footer.show(!notice.is_empty() || pages);
        self.enable_actions();
        self.page.layout();
        self.fit_columns();
    }
    fn fit_columns(&self) {
        let columns = data::columns(&self.key.borrow());
        let width = (self.table.get_client_size().width - 4).max(1);
        let weights = columns.iter().map(|c| c.2).sum::<i32>().max(1);
        let mut used = 0;
        for (i, (_, _, weight)) in columns.iter().enumerate() {
            let w = if i + 1 == columns.len() {
                width - used
            } else {
                width * weight / weights
            };
            self.table.set_column_width(i as i64, w.max(1));
            used += w;
        }
    }
}

pub(super) fn confirm_checkout(
    before: &Value,
    current: &Value,
    draft: &Value,
) -> Result<Value, String> {
    let key = draft["payment_type"].as_str().ok_or("请选择支付方式")?;
    let kind = draft["order_type"].as_str().ok_or("请选择充值或订阅")?;
    if current["methods"][key].is_null() {
        return Err("此支付方式已关闭，请刷新后重新选择".into());
    }
    let plan = current["plans"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == draft["plan_id"]));
    let body = purchase::checkout_body(
        current,
        key,
        &current["methods"][key],
        kind,
        &draft["amount"].to_string(),
        plan,
    )?;
    let value = body["amount"].as_f64().ok_or("金额无效")?;
    let old = crate::desktop::squad_quote::quote(
        before,
        &before["methods"][key],
        kind,
        draft["amount"].as_f64().ok_or("金额无效")?,
    )
    .map_err(|e| e.to_string())?;
    let new = crate::desktop::squad_quote::quote(current, &current["methods"][key], kind, value)
        .map_err(|e| e.to_string())?;
    if (old.total - new.total).abs() > 1e-6
        || old.currency != new.currency
        || old.credited != new.credited
    {
        return Err("站点价格或充值规则已更新，请刷新后核对金额再付款".into());
    }
    Ok(body)
}
pub(super) fn query(key: &str, page: usize, start: String, end: String) -> Vec<(String, String)> {
    let mut query = Vec::new();
    if data::paginated(key) {
        query.extend([
            ("page".into(), (page + 1).to_string()),
            ("page_size".into(), "50".into()),
        ]);
    }
    if data::date_filter(key) {
        for (k, v) in [("start_date", start), ("end_date", end)] {
            if !v.trim().is_empty() {
                query.push((k.into(), v.trim().into()));
            }
        }
    }
    query
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_checkout_rejects_price_fee_method_and_plan_changes_before_any_write() {
        use serde_json::json;
        let info = json!({"balance_recharge_multiplier":0.14,"subscription_usd_to_cny_rate":7.15,"methods":{"alipay":{"available":true,"currency":"CNY"}},"plans":[{"id":7,"price":10,"for_sale":true}]});
        let draft = json!({"order_type":"balance","payment_type":"alipay","amount":100});
        assert_eq!(
            confirm_checkout(&info, &info, &draft).unwrap()["amount"],
            100.
        );
        let mut changed = info.clone();
        changed["recharge_fee_rate"] = 2.into();
        assert!(confirm_checkout(&info, &changed, &draft).is_err());
        let mut changed = info.clone();
        changed["methods"]["alipay"]["available"] = false.into();
        assert!(confirm_checkout(&info, &changed, &draft).is_err());
        let sub =
            json!({"order_type":"subscription","payment_type":"alipay","amount":10,"plan_id":7});
        assert_eq!(confirm_checkout(&info, &info, &sub).unwrap()["plan_id"], 7);
        let mut changed = info.clone();
        changed["plans"][0]["price"] = 11.into();
        assert!(confirm_checkout(&info, &changed, &sub).is_err());
        let mut changed = info.clone();
        changed["plans"][0]["for_sale"] = false.into();
        assert!(confirm_checkout(&info, &changed, &sub).is_err());
    }
    #[test]
    fn date_and_page_filters_do_not_leak_between_tasks() {
        assert!(query("profile", 5, "2026-11-01".into(), "2026-11-09".into()).is_empty());
        assert!(query("channels", 5, "2026-11-01".into(), "2026-11-09".into()).is_empty());
        let usage = query("usage", 2, "2026-11-01".into(), "2026-11-09".into());
        assert_eq!(usage.len(), 4);
        assert_eq!(usage[0].1, "3");
        assert_eq!(
            query("orders", 0, "2026-11-01".into(), String::new()).len(),
            2
        );
    }
}

pub(super) fn recent_week() -> Result<Vec<(String, String)>, String> {
    // Reuse the bundled SQLite calendar, as the local statistics page does.
    // This connection is purely in memory and never opens the user's log DB.
    let db = rusqlite::Connection::open_in_memory().map_err(|_| "无法确定日期范围".to_string())?;
    let (start, end): (String, String) = db
        .query_row(
            "SELECT date('now','localtime','-6 days'), date('now','localtime')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "无法确定日期范围".to_string())?;
    Ok(vec![
        ("start_date".into(), start),
        ("end_date".into(), end),
        ("granularity".into(), "day".into()),
    ])
}

#[cfg(all(test, windows))]
pub(super) fn review_account_components(suffix: &str) {
    filters::review(suffix);
    let frame = Frame::builder()
        .with_title("hidden account restoration")
        .build();
    frame.set_size(visual::dip_size(&frame, 700, 420));
    let view = super::squad_browser::SessionRestore::new(&frame);
    let col = BoxSizer::builder(Orientation::Vertical).build();
    col.add(&view.page, 1, SizerFlag::Expand, 0);
    frame.set_sizer(col, false);
    frame.layout();
    assert!(!view.retry.is_shown());
    super::native_model_review::capture(&view.page, &format!("squad-restore-loading-{suffix}"));
    view.error("站点暂时无法响应，请稍后重试");
    assert!(view.retry.is_shown());
    super::native_model_review::capture(&view.page, &format!("squad-restore-retry-{suffix}"));
    frame.destroy();
}
