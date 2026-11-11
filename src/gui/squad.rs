//! Native TianCai account workspace. WebView is limited to verification/payment.
use super::controls::Button;
use super::{
    api::ApiClient,
    dialog_job,
    shell::{PageId, Shell},
    squad_account::AccountBadge,
    squad_browser::BrowserLogin,
    squad_form as forms,
    squad_login::InlineLogin,
    squad_view::Workspace,
    theme,
};
use crate::gui::controls::TextCtrl;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use wxdragon::{prelude::*, timer::Timer};
mod finance;
mod organization;
mod payment;
type Slot = Arc<Mutex<Option<(u64, Result<(Value, Value, String), String>)>>>;
type InlineSlot = Arc<Mutex<Option<(u64, &'static str, String, Result<Value, String>)>>>;
#[derive(Clone)]
pub(super) struct SquadPage {
    page: Panel,
    book: wxdragon::widgets::SimpleBook,
    inline_login: InlineLogin,
    browser_login: BrowserLogin,
    restore: super::squad_browser::SessionRestore,
    builtin: Rc<Cell<bool>>,
    badge: AccountBadge,
    browser_pending: Arc<Mutex<Option<(u64, &'static str, Result<Value, String>)>>>,
    browser_serial: Rc<Cell<u64>>,
    browser_busy: Rc<Cell<bool>>,
    browser_waiting: Rc<Cell<bool>>,
    browser_url: Rc<RefCell<String>>,
    browser_next: Rc<Cell<Option<std::time::Instant>>>,
    frame: Frame,
    api: ApiClient,
    workspace: Workspace,
    capability_checked: Rc<Cell<bool>>,
    heading: StaticText,
    status: StaticText,
    invoice: Button,
    primary: Button,
    edit: Button,
    remove: Button,
    extra: Button,
    state: Rc<RefCell<Value>>,
    data: Rc<RefCell<Value>>,
    key: Rc<RefCell<String>>,
    page_index: Rc<Cell<usize>>,
    serial: Rc<Cell<u64>>,
    pending: Slot,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    active: Rc<Cell<bool>>,
    start: super::squad_view::filters::DateField,
    end: super::squad_view::filters::DateField,
    ready: Rc<Cell<bool>>,
    inline_pending: InlineSlot,
    inline_busy: Rc<Cell<bool>>,
    inline_notice: Rc<RefCell<Option<String>>>,
}
impl SquadPage {
    pub fn new(shell: &Shell, frame: Frame, api: ApiClient) -> Self {
        let t = theme::theme();
        let root = Panel::builder(&shell.book).build();
        root.set_background_color(t.bg_app);
        let book = wxdragon::widgets::SimpleBook::builder(&root).build();
        let browser_login = BrowserLogin::new(&book);
        book.add_page(&browser_login.page, "登录", true, None);
        let login_host = Panel::builder(&book).build();
        let login_col = BoxSizer::builder(Orientation::Vertical).build();
        let login_back = Button::builder(&login_host)
            .with_icon(super::visual::Icon::Back)
            .with_label("返回")
            .with_kind(super::controls::ButtonKind::Ghost)
            .build();
        login_col.add(&login_back, 0, SizerFlag::Bottom, login_host.from_dip(8));
        let inline_login = InlineLogin::new(&login_host, crate::desktop::squad::ORIGIN);
        login_col.add(&inline_login.page, 1, SizerFlag::Expand, 0);
        login_host.set_sizer(login_col, true);
        let workspace = Workspace::new(&book);
        let page = workspace.page;
        let (table, heading, status) = (workspace.table, workspace.heading, workspace.status);
        let (logout, website, refresh) = (workspace.logout, workspace.website, workspace.refresh);
        let (primary, edit, remove, extra, invoice) = (
            workspace.primary,
            workspace.edit,
            workspace.remove,
            workspace.extra,
            workspace.invoice,
        );
        let (back, next, start, end) = (
            workspace.previous,
            workspace.next,
            workspace.start,
            workspace.end,
        );
        book.add_page(&page, "账户", false, None);
        book.add_page(&login_host, "内置登录", false, None);
        let restore = super::squad_browser::SessionRestore::new(&book);
        book.add_page(&restore.page, "恢复账户", false, None);
        book.change_selection(3);
        let root_column = BoxSizer::builder(Orientation::Vertical).build();
        root_column.add(&book, 1, SizerFlag::Expand, 0);
        root.set_sizer(root_column, true);
        shell.register(PageId::Squad, &root);
        let ui = Self {
            page: root,
            book,
            inline_login,
            browser_login,
            restore,
            builtin: Default::default(),
            badge: AccountBadge::new(&shell.panel, shell.account, api.clone()),
            browser_pending: Default::default(),
            browser_serial: Default::default(),
            browser_busy: Default::default(),
            browser_waiting: Default::default(),
            browser_url: Default::default(),
            browser_next: Default::default(),
            frame,
            api,
            workspace,
            capability_checked: Default::default(),
            heading,
            status,
            invoice,
            primary,
            edit,
            remove,
            extra,
            state: Default::default(),
            data: Default::default(),
            key: Rc::new(RefCell::new("account".into())),
            page_index: Default::default(),
            serial: Default::default(),
            pending: Default::default(),
            timer: Default::default(),
            active: Default::default(),
            start,
            end,
            ready: Default::default(),
            inline_pending: Default::default(),
            inline_busy: Default::default(),
            inline_notice: Default::default(),
        };
        let u = ui.clone();
        let nav = shell.clone();
        shell.account.on_click(move |_| {
            *u.key.borrow_mut() = "profile".into();
            u.page_index.set(0);
            let was_active = u.active.get();
            nav.navigate(PageId::Squad);
            if was_active {
                u.load();
            }
        });
        let u = ui.clone();
        ui.browser_login.start.on_click(move |_| {
            if u.browser_waiting.get() && !u.browser_url.borrow().is_empty() {
                u.open_browser(&u.browser_url.borrow());
            } else {
                u.browser_job(
                    if u.browser_login.available.get() {
                        "start"
                    } else {
                        "capability"
                    },
                    if u.browser_login.available.get() {
                        "/api/desktop/squad/browser-start"
                    } else {
                        "/api/desktop/squad/browser-capability"
                    },
                );
            }
        });
        let u = ui.clone();
        ui.browser_login
            .cancel
            .on_click(move |_| u.cancel_browser());
        let u = ui.clone();
        ui.browser_login.builtin.on_click(move |_| {
            u.cancel_browser();
            u.builtin.set(true);
            u.book.change_selection(2);
            u.inline_login.activate();
        });
        let u = ui.clone();
        login_back.on_click(move |_| {
            u.builtin.set(false);
            u.inline_login.clear();
            u.book.change_selection(0);
        });
        let u = ui.clone();
        website.on_click(move |_| u.browser_job("website", "/api/desktop/squad/website-link"));
        let u = ui.clone();
        ui.inline_login.retry.on_click(move |_| {
            u.inline_login.clear();
            u.inline_login.activate();
            u.load();
        });
        let u = ui.clone();
        logout.on_click(move |_| {
            u.inline_operation(
                "logout",
                "退出账户",
                "/api/desktop/squad/logout",
                json!({}),
                None,
            );
        });
        let u = ui.clone();
        refresh.on_click(move |_| u.load_inner(None, true));
        let u = ui.clone();
        ui.workspace.fallback.on_toggled(move |_| {
            let old=u.state.borrow()["organization"]["allow_personal_balance_fallback"].as_bool().unwrap_or(false);
            let desired=u.workspace.fallback.get_value();
            u.workspace.fallback.set_value(old);
            u.inline_write("fallback",json!({"method":"PUT","path":"/user/organization/personal-fallback","body":{"allow":desired}}),None);
        });
        let u = ui.clone();
        ui.workspace
            .purchase
            .submit
            .on_click(move |_| match u.workspace.purchase.draft() {
                Ok(body) => u.inline_write("purchase", body, Some(u.workspace.purchase.info())),
                Err(e) => u.workspace.notice(&e),
            });
        let u = ui.clone();
        ui.workspace.filter_bar.on_apply(move || {
            u.page_index.set(0);
            u.load();
        });
        let u = ui.clone();
        ui.restore.retry.on_click(move |_| u.load());
        let u = ui.clone();
        ui.workspace.on_select(move |key| {
            if *u.key.borrow() != key {
                *u.key.borrow_mut() = key;
                u.page_index.set(0);
                u.load();
            }
        });
        let u = ui.clone();
        back.on_click(move |_| {
            u.page_index.set(u.page_index.get().saturating_sub(1));
            u.load();
        });
        let u = ui.clone();
        next.on_click(move |_| {
            u.page_index.set(u.page_index.get() + 1);
            u.load();
        });
        for (action, button) in [
            ("primary", primary),
            ("edit", edit),
            ("remove", remove),
            ("extra", extra),
            ("invoice", invoice),
        ] {
            let u = ui.clone();
            button.on_click(move |_| u.action(action));
        }
        let u = ui.clone();
        table.on_item_activated(move |_| {
            if !u.ready.get() || u.inline_busy.get() {
                return;
            }
            let Some(row) = u.selected() else {
                return;
            };
            let key = u.key.borrow().clone();
            if key.starts_with("organization-") && row["id"].as_u64().is_some() {
                organization::show(
                    &u.frame,
                    u.api.clone(),
                    &key,
                    &row,
                    crate::desktop::squad_money::Money::state(&u.state.borrow()),
                );
            } else if key == "orders" {
                payment::continue_order(&u.frame, u.api.clone(), &row);
            } else if key == "keys" {
                key_details(&u.frame, &row);
            } else if key == "organization" {
                let can_use = row["is_active_billing"] != true
                    && u.state.borrow()["organization"]["control_mode"] != "admin_locked";
                if forms::inspect_action(
                    &u.frame,
                    "扣费身份",
                    &row,
                    crate::desktop::squad_money::Money::state(&u.state.borrow()),
                    if can_use {
                        Some("使用此扣费身份")
                    } else {
                        None
                    },
                ) {
                    u.action("primary");
                }
            } else {
                forms::inspect_money(
                    &u.frame,
                    &u.heading.get_label(),
                    &row,
                    crate::desktop::squad_money::Money::state(&u.state.borrow()),
                );
            }
        });
        let timer = Timer::new(&page);
        let u = ui.clone();
        timer.on_tick(move |_| {
            u.browser_tick();
            u.inline_tick();
            if let Some(session) = u.inline_login.take_session() {
                u.inline_login.submitting();
                u.load_inner(Some(session), false);
            }
            let result = u.pending.lock().ok().and_then(|mut s| s.take());
            if let Some((serial, result)) = result {
                if serial != u.serial.get() {
                    return;
                }
                match result {
                    Ok((state, data, key)) => {
                        *u.state.borrow_mut() = state;
                        u.ready.set(data["loadError"].is_null());
                        *u.data.borrow_mut() = data;
                        *u.key.borrow_mut() = key;
                        u.render();
                        if u.ready.get() {
                            if let Some(notice) = u.inline_notice.borrow_mut().take() {
                                u.workspace.notice(&notice);
                            }
                        }
                    }
                    Err(e) => {
                        if u.state.borrow()["signedIn"].is_null() {
                            u.book.change_selection(3);
                            u.restore.error(&e);
                        } else if u.state.borrow()["signedIn"] != true {
                            if u.builtin.get() {
                                u.book.change_selection(2);
                                u.inline_login.activate();
                                u.inline_login.show_error(&format!(
                                    "未能确认登录：{e}。请核对网络后重新加载登录页。"
                                ));
                            } else {
                                u.book.change_selection(0);
                                u.browser_login.message(&format!("未能读取账户：{e}"), true);
                            }
                        }
                        u.workspace.error(&e);
                        u.page.layout();
                    }
                }
            }
        });
        ui.timer.borrow_mut().replace(timer);
        ui
    }
    fn open_browser(&self, url: &str) {
        if !super::squad_login::same_origin(crate::desktop::squad::ORIGIN, url) {
            self.browser_login
                .message("浏览器地址无效，请重新登录", true);
            return;
        }
        if super::browser::open_url_in_browser(
            super::text::GuiText::new(super::load_gui_locale()),
            url,
        )
        .is_err()
        {
            self.browser_login.message(
                "未能打开默认浏览器，请设置默认浏览器后重试，或使用内置登录",
                true,
            );
        }
    }
    fn browser_job(&self, kind: &'static str, path: &'static str) {
        if self.browser_busy.get() && kind != "cancel" {
            return;
        }
        self.browser_busy.set(true);
        let serial = self.browser_serial.get() + 1;
        self.browser_serial.set(serial);
        self.browser_login.start.enable(false);
        if kind == "start" {
            self.browser_login.message("正在打开浏览器登录…", false);
        }
        let (api, slot) = (self.api.clone(), self.browser_pending.clone());
        std::thread::spawn(move || {
            let result = api.desktop_post(path, &json!({}));
            if let Ok(mut value) = slot.lock() {
                if value.as_ref().is_none_or(|(old, _, _)| *old <= serial) {
                    *value = Some((serial, kind, result));
                }
            }
            wxdragon::wake_up_idle();
        });
    }
    fn cancel_browser(&self) {
        self.browser_waiting.set(false);
        self.browser_next.set(None);
        self.browser_url.borrow_mut().clear();
        self.browser_login.idle();
        self.browser_login.message(
            if self.browser_login.available.get() {
                "可使用浏览器中已登录的账号"
            } else {
                "请使用内置登录"
            },
            false,
        );
        self.browser_job("cancel", "/api/desktop/squad/browser-cancel");
    }
    fn browser_tick(&self) {
        let outcome = self
            .browser_pending
            .lock()
            .ok()
            .and_then(|mut value| value.take());
        if let Some((serial, kind, result)) = outcome {
            if serial != self.browser_serial.get() {
                return;
            }
            self.browser_busy.set(false);
            self.browser_login.start.enable(true);
            match result {
                Ok(data)
                    if kind == "capability" || kind == "start" && data["state"] != "pending" =>
                {
                    self.browser_waiting.set(false);
                    self.browser_login.capability(&data);
                }
                Ok(data) if kind == "start" => {
                    let url = data["url"].as_str().unwrap_or_default();
                    self.browser_url.replace(url.into());
                    self.browser_waiting.set(true);
                    self.browser_login
                        .waiting(data["code"].as_str().unwrap_or_default());
                    self.browser_next.set(Some(
                        std::time::Instant::now() + Duration::from_millis(1500),
                    ));
                    self.open_browser(url);
                }
                Ok(data) if kind == "poll" => match data["state"].as_str() {
                    Some("ready") => {
                        self.browser_waiting.set(false);
                        self.builtin.set(false);
                        self.badge
                            .set(&json!({"signedIn":true,"user":data["user"]}));
                        self.load();
                    }
                    Some("pending") => {
                        self.browser_next.set(Some(
                            std::time::Instant::now() + Duration::from_millis(1500),
                        ));
                    }
                    _ => {
                        self.browser_waiting.set(false);
                        self.browser_login.idle();
                        self.browser_login
                            .message("登录等待已结束，请重新登录", false);
                    }
                },
                Ok(data) if kind == "website" => {
                    self.open_browser(data["url"].as_str().unwrap_or_default());
                    if let Some(notice) = data["notice"].as_str() {
                        self.workspace.notice(notice);
                        self.page.layout();
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    let error = super::squad_browser::friendly_error(&error);
                    self.browser_login.message(&error, true);
                    if kind == "start" || kind == "capability" {
                        self.browser_waiting.set(false);
                        self.browser_login.capability(&json!({"state":"unavailable","message":super::squad_browser::friendly_error(&error)}));
                    }
                    if kind == "website" {
                        super::show_error(
                            &self.frame,
                            &format!("{error}。也可以直接在浏览器打开天才小分队主站登录。"),
                        );
                    }
                    self.browser_next
                        .set(Some(std::time::Instant::now() + Duration::from_secs(5)));
                }
            }
        }
        if self.browser_waiting.get()
            && !self.browser_busy.get()
            && self
                .browser_next
                .get()
                .is_some_and(|next| std::time::Instant::now() >= next)
        {
            self.browser_next.set(None);
            self.browser_job("poll", "/api/desktop/squad/browser-poll");
        }
    }
    pub fn enter(&self, id: PageId) {
        let active = id == PageId::Squad;
        self.active.set(active);
        if let Some(timer) = self.timer.borrow().as_ref() {
            if active {
                timer.start(120, false);
                self.load();
            } else {
                self.inline_login.clear();
            }
        }
    }
    fn load(&self) {
        self.load_inner(None, false);
    }
    fn inline_write(&self, kind: &'static str, body: Value, checkout: Option<Value>) {
        self.inline_operation(
            kind,
            if kind == "purchase" {
                "核对金额并创建订单"
            } else {
                "保存"
            },
            "/api/desktop/squad/request",
            body,
            checkout,
        );
    }
    fn inline_controls(&self, enabled: bool) {
        self.workspace.logout.enable(enabled);
        self.workspace.refresh.enable(enabled);
        self.workspace.fallback.enable(enabled);
        self.workspace.purchase.panel.enable(enabled);
        if !enabled {
            self.workspace.purchase.set_ready(false);
            for button in [
                self.primary,
                self.edit,
                self.remove,
                self.extra,
                self.invoice,
            ] {
                button.enable(false);
            }
        }
    }
    fn inline_operation(
        &self,
        kind: &'static str,
        title: &str,
        path: &str,
        body: Value,
        checkout: Option<Value>,
    ) -> bool {
        if !self.ready.get() || self.inline_busy.replace(true) {
            return false;
        }
        self.inline_controls(false);
        self.inline_notice.borrow_mut().take();
        self.workspace.notice(&format!("正在{title}…"));
        let expected = json!({"userId":self.state.borrow()["user"]["id"],"generation":self.state.borrow()["accountGeneration"]});
        let (api, slot, serial, path, title) = (
            self.api.clone(),
            self.inline_pending.clone(),
            self.serial.get(),
            path.to_string(),
            title.to_string(),
        );
        std::thread::spawn(move || {
            let result = (|| {
                let mut request = if let Some(before) = checkout {
                    let current:Value=api.desktop_post("/api/desktop/squad/request",&json!({"method":"GET","path":"/payment/checkout-info","expected_account":expected}))?;
                    let body = super::squad_view::confirm_checkout(&before, &current, &body)?;
                    json!({"method":"POST","path":"/payment/orders","body":body})
                } else {
                    body
                };
                if path == "/api/desktop/squad/request" {
                    request["expected_account"] = expected;
                }
                api.desktop_post(&path, &request)
            })();
            if let Ok(mut s) = slot.lock() {
                *s = Some((serial, kind, title, result));
            }
            wxdragon::wake_up_idle();
        });
        true
    }
    fn inline_tick(&self) {
        let result = self.inline_pending.lock().ok().and_then(|mut s| s.take());
        if let Some((serial, kind, title, result)) = result {
            self.inline_busy.set(false);
            self.inline_controls(true);
            if serial != self.serial.get() {
                if self.ready.get() {
                    self.render();
                }
                return;
            }
            match result {
                Ok(v) => {
                    if kind == "logout" {
                        *self.state.borrow_mut() = Value::Null;
                        *self.data.borrow_mut() = Value::Null;
                        self.workspace.clear();
                        self.capability_checked.set(false);
                        *self.key.borrow_mut() = "account".into();
                        self.badge.set(&json!({"signedIn":false}));
                        self.browser_waiting.set(false);
                        self.browser_login.idle();
                    }
                    // A result from a page the user has left must not open a
                    // payment/import window or overwrite the new page.
                    if !self.active.get() {
                        return;
                    }
                    if kind == "purchase" {
                        payment::continue_order(&self.frame, self.api.clone(), &v);
                    } else if kind == "import" {
                        match serde_json::from_value(v) {
                            Ok(draft) => {
                                super::external_import::preview(&self.frame, &self.api, draft)
                            }
                            Err(_) => {
                                self.render();
                                self.workspace.notice("导入预览格式不兼容，请重新读取密钥");
                                self.status.set_foreground_color(theme::theme().error);
                                return;
                            }
                        }
                    } else if kind == "verify" {
                        self.load();
                        *self.inline_notice.borrow_mut() =
                            Some(payment::verification_status(&v).into());
                        return;
                    }
                    self.load();
                    if kind != "logout" {
                        *self.inline_notice.borrow_mut() = Some(format!("{title}已完成"));
                    }
                }
                Err(e) => {
                    if !self.active.get() {
                        return;
                    }
                    self.render();
                    let e = if e.trim().is_empty() {
                        format!("{title}未完成，请刷新核对当前状态后再继续")
                    } else {
                        e
                    };
                    self.workspace.notice(&e);
                    self.status.set_foreground_color(theme::theme().error);
                    if kind == "purchase" {
                        self.workspace.purchase.set_ready(false);
                        self.workspace
                            .notice(&format!("{e}。请刷新核对金额或待确认操作后再继续"));
                    }
                }
            }
        }
    }
    fn load_inner(&self, session: Option<Value>, force: bool) {
        self.inline_notice.borrow_mut().take();
        if self.state.borrow()["signedIn"].is_null() {
            self.restore.loading();
            self.book.change_selection(3);
        }
        self.ready.set(false);
        self.workspace.purchase.set_ready(false);
        self.workspace
            .loading(&self.key.borrow(), &self.state.borrow());
        for button in [
            self.primary,
            self.edit,
            self.remove,
            self.extra,
            self.invoice,
        ] {
            button.enable(false);
        }
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        self.status.set_label("读取中…");
        let (api, slot, key, page, start, end) = (
            self.api.clone(),
            self.pending.clone(),
            self.key.borrow().clone(),
            self.page_index.get() + 1,
            self.start.get_value(),
            self.end.get_value(),
        );
        std::thread::spawn(move || {
            let result = (|| {
                if force {
                    api.desktop_post("/api/desktop/squad/refresh", &json!({}))?;
                }
                if let Some(session) = session {
                    let _: Value = api.desktop_post("/api/desktop/squad/session", &session)?;
                }
                let state: Value =
                    api.get_with_timeout("/api/desktop/squad/status", Duration::from_secs(70))?;
                if state["signedIn"] != true {
                    return Ok((state, Value::Null, key));
                }
                let key = if state["menus"]
                    .as_array()
                    .is_some_and(|m| m.iter().any(|m| m["id"] == key))
                {
                    key
                } else {
                    "account".into()
                };
                let path = endpoint(&key);
                let query = super::squad_view::query(&key, page - 1, start, end);
                let data = (|| -> Result<Value, String> {
                    Ok(if key == "account" {
                        let stats: Value = api.desktop_post(
                            "/api/desktop/squad/request",
                            &json!({"method":"GET","path":"/usage/dashboard/stats"}),
                        )?;
                        let trend: Value = api.desktop_post(
                            "/api/desktop/squad/request",
                            &json!({"method":"GET","path":"/usage/dashboard/trend","query":super::squad_view::recent_week()?}),
                        )?;
                        json!({"stats":stats,"trend":trend})
                    } else if key.starts_with("custom-") {
                        state["menus"]
                            .as_array()
                            .and_then(|m| m.iter().find(|m| m["id"] == key))
                            .map(|m| m["tool"].clone())
                            .unwrap_or(Value::Null)
                    } else {
                        api.desktop_post(
                            "/api/desktop/squad/request",
                            &json!({"method":"GET","path":path,"query":query}),
                        )?
                    })
                })()
                .unwrap_or_else(|error| json!({"loadError":error}));
                Ok((state, data, key))
            })();
            if let Ok(mut s) = slot.lock() {
                if s.as_ref().is_none_or(|(old, _)| *old <= serial) {
                    *s = Some((serial, result));
                }
            }
            wxdragon::wake_up_idle();
        });
    }
    fn render(&self) {
        let state = self.state.borrow();
        let signed = state["signedIn"] == true;
        self.book.change_selection(if signed {
            1
        } else if self.builtin.get() {
            2
        } else {
            0
        });
        self.badge.set(&state);
        if signed {
            self.inline_login.clear();
            self.browser_waiting.set(false);
            self.browser_login.idle();
            self.workspace.render(
                &self.key.borrow(),
                &state,
                &self.data.borrow(),
                self.page_index.get(),
            );
            if self.inline_busy.get() {
                self.inline_controls(false);
            }
        } else {
            self.workspace.clear();
            if self.active.get() && self.builtin.get() {
                self.inline_login.activate();
            }
            if self.active.get() && !self.capability_checked.replace(true) {
                self.browser_job("capability", "/api/desktop/squad/browser-capability");
            }
        }
        self.page.layout();
    }
    fn selected(&self) -> Option<Value> {
        self.workspace.selected()
    }
    fn request(&self, title: &str, method: &str, path: &str, body: Value) {
        self.inline_operation(
            if path == "/payment/orders/verify" {
                "verify"
            } else {
                "request"
            },
            title,
            "/api/desktop/squad/request",
            json!({"method":method,"path":path,"body":body}),
            None,
        );
    }
    fn form(&self, title: &str, method: &str, path: &str, fields: Value) -> Option<Value> {
        forms::edit_money(
            &self.frame,
            self.api.clone(),
            title,
            method,
            path,
            fields,
            Default::default(),
            crate::desktop::squad_money::Money::state(&self.state.borrow()),
        )
    }
    fn action(&self, action: &str) {
        if !self.ready.get() || self.inline_busy.get() {
            return;
        }
        let key = self.key.borrow().clone();
        let row = self.selected().unwrap_or(Value::Null);
        let id = row["id"].as_u64();
        if action == "primary"
            && matches!(key.as_str(), "organization-admin" | "organization-usage")
        {
            if id.is_some() {
                organization::show(
                    &self.frame,
                    self.api.clone(),
                    &key,
                    &row,
                    crate::desktop::squad_money::Money::state(&self.state.borrow()),
                );
                self.load();
            }
            return;
        }
        if action == "extra"
            && self.state.borrow()["pending"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        {
            pending(
                &self.frame,
                self.api.clone(),
                &self.state.borrow()["pending"],
                crate::desktop::squad_money::Money::state(&self.state.borrow()),
            );
            self.load();
            return;
        }
        if action == "invoice" {
            let scope = if key == "orders" && id.is_some() {
                let Some(scopes) = query(
                    &self.frame,
                    self.api.clone(),
                    "读取订单开票范围",
                    &format!("/invoices/orders/{}", id.unwrap()),
                ) else {
                    return;
                };
                let rows = scopes
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut s| {
                        s["name"] = format!(
                            "组织 {} / 部门 {}",
                            s["business_unit_id"], s["department_id"]
                        )
                        .into();
                        s
                    })
                    .collect::<Vec<_>>();
                let Some(scope) = organization::choose(&self.frame, "选择开票范围", &rows, "name")
                else {
                    return;
                };
                scope
            } else if key == "organization" && row["business_unit_id"].as_u64().is_some() {
                json!({"business_unit_id":row["business_unit_id"],"department_id":row["department_id"]})
            } else {
                json!({"business_unit_id":0,"department_id":0})
            };
            finance::invoices(&self.frame, self.api.clone(), scope);
            return;
        }
        match (key.as_str(), action) {
            ("keys", "primary") => {
                key_form(&self.frame, self.api.clone(), None);
            }
            ("keys", "edit") => {
                if id.is_some() {
                    key_form(&self.frame, self.api.clone(), Some(&row));
                }
            }
            ("keys", "remove") => {
                if let Some(id) = id {
                    if super::external_import::confirm(
                        &self.frame,
                        "删除此 API 密钥？使用此密钥的调用将失效。",
                    ) {
                        self.request("删除密钥", "DELETE", &format!("/keys/{id}"), Value::Null);
                    }
                }
            }
            ("keys", "extra") => {
                if let Some(id) = id {
                    self.inline_operation(
                        "import",
                        "准备导入",
                        "/api/desktop/squad/import",
                        json!({"id":id}),
                        None,
                    );
                }
            }
            ("orders", "edit") => {
                if let Some(number) = row["out_trade_no"].as_str() {
                    self.request(
                        "核对付款",
                        "POST",
                        "/payment/orders/verify",
                        json!({"out_trade_no":number}),
                    );
                }
            }
            ("orders", "remove") => {
                if let Some(id) = id {
                    if super::external_import::confirm(&self.frame, "取消当前待支付订单？")
                    {
                        self.request(
                            "取消订单",
                            "POST",
                            &format!("/payment/orders/{id}/cancel"),
                            json!({}),
                        );
                    }
                }
            }
            ("orders", "extra") => {
                if let Some(id) = id {
                    self.form(
                        "申请退款",
                        "POST",
                        &format!("/payment/orders/{id}/refund-request"),
                        json!({"reason":""}),
                    );
                }
            }
            ("redeem", "primary") => {
                self.form("兑换", "POST", "/redeem", json!({"code":""}));
            }
            ("organization", "primary") => {
                if self.state.borrow()["organization"]["control_mode"] == "admin_locked" {
                    super::show_info(
                        &self.frame,
                        "当前扣费身份由组织管理员锁定，请联系管理员调整",
                    );
                } else if id.is_some()
                    && super::external_import::confirm(
                        &self.frame,
                        &format!(
                            "将 {} / {} 设为扣费身份？",
                            row["business_unit_name"].as_str().unwrap_or(""),
                            row["department_name"].as_str().unwrap_or("")
                        ),
                    )
                {
                    self.request(
                        "选择扣费身份",
                        "PUT",
                        "/user/organization/active-membership",
                        json!({"membership_id":row["id"]}),
                    );
                }
            }
            ("organization", "edit") => {
                funding_order(
                    &self.frame,
                    self.api.clone(),
                    &self.state.borrow()["organization"],
                );
            }
            ("affiliate", "primary") => {
                if super::external_import::confirm(&self.frame, "将当前可用返利转入账户余额？")
                {
                    self.request("转入余额", "POST", "/user/aff/transfer", json!({}));
                }
            }
            (key, "primary") if key.starts_with("custom-") => {
                let raw = self.data.borrow()["url"].as_str().unwrap_or("").to_string();
                if url::Url::parse(&raw).is_ok_and(|u| {
                    u.scheme() == "https"
                        && u.host_str().is_some()
                        && u.username().is_empty()
                        && u.password().is_none()
                }) {
                    let _ = super::browser::open_url_in_browser(
                        super::text::GuiText::new(super::load_gui_locale()),
                        &raw,
                    );
                } else {
                    super::show_error(&self.frame, "此工具未提供有效网页地址，请联系站点管理员");
                }
            }
            _ => {}
        }
        if !self.inline_busy.get() {
            self.load();
        }
    }
}
fn endpoint(key: &str) -> &str {
    match key {
        "account" => "/usage/dashboard/stats",
        "keys" => "/keys",
        "usage" => "/usage",
        "organization" => "/user/organization",
        "organization-audit" => "/organization-audit/logs",
        "organization-admin"
        | "organization-usage"
        | "organization-accounts"
        | "organization-structure" => "/organization-admin/business-units",
        "channels" => "/channels/available",
        "purchase" => "/payment/checkout-info",
        "orders" => "/payment/orders/my",
        "subscriptions" => "/subscriptions",
        "redeem" => "/redeem/history",
        "affiliate" => "/user/aff",
        _ => "/user/profile",
    }
}
fn query(frame: &Frame, api: ApiClient, title: &str, path: &str) -> Option<Value> {
    dialog_job::run(
        frame,
        api,
        title,
        "/api/desktop/squad/request",
        json!({"method":"GET","path":path}),
    )
}
fn pending(
    frame: &Frame,
    api: ApiClient,
    items: &Value,
    money: crate::desktop::squad_money::Money,
) {
    let Some(items) = items.as_array() else {
        return;
    };
    for item in items {
        let path = item["path"].as_str().unwrap_or("");
        let Some(read_path) = crate::desktop::squad::result_path(path) else {
            super::show_info(
                frame,
                "请先到站内核对该操作的实际结果，此处不会重新提交。确认后再移除待确认标记。",
            );
            if super::external_import::confirm(frame, "已在站内核对实际结果，移除此待确认标记？")
            {
                dialog_job::run(
                    frame,
                    api.clone(),
                    "移除确认标记",
                    "/api/desktop/squad/confirmed",
                    json!({"id":item["id"]}),
                );
            }
            continue;
        };
        if let Some(value) = query(frame, api.clone(), "核对操作结果", &read_path) {
            forms::inspect_money(frame, "站点当前结果", &value, money);
            if super::external_import::confirm(frame, "已核对站点当前结果，移除此待确认标记？")
            {
                dialog_job::run(
                    frame,
                    api.clone(),
                    "移除确认标记",
                    "/api/desktop/squad/confirmed",
                    json!({"id":item["id"]}),
                );
            }
        }
    }
}
fn security(frame: &Frame, api: ApiClient) {
    let choices = ["修改密码", "通知邮箱", "双因素认证 / 通行密钥"];
    let dialog = SingleChoiceDialog::builder(frame, "账户安全", "选择操作", &choices).build();
    let selection = if dialog.show_modal() == ID_OK {
        Some(dialog.get_selection())
    } else {
        None
    };
    dialog.destroy();
    match selection {
        Some(0) => {
            forms::edit(
                frame,
                api,
                "修改密码",
                "PUT",
                "/user/password",
                json!({"old_password":"","new_password":""}),
            );
        }
        Some(1) => notify_email(frame, api),
        Some(2) => payment::official_window(
            frame,
            api,
            "账户安全验证",
            &format!("{}/profile", crate::desktop::squad::ORIGIN),
            None,
        ),
        _ => {}
    }
}
fn notify_email(frame: &Frame, api: ApiClient) {
    let Some(profile) = query(frame, api.clone(), "读取通知邮箱", "/user/profile") else {
        return;
    };
    forms::inspect(
        frame,
        "当前通知邮箱",
        &organization::pick(
            &profile,
            &[
                "email",
                "balance_notify_extra_emails",
                "balance_notify_enabled",
            ],
        ),
    );
    let choices = ["添加通知邮箱", "启用 / 停用邮箱", "移除通知邮箱"];
    let dialog = SingleChoiceDialog::builder(frame, "通知邮箱", "选择操作", &choices).build();
    let selection = if dialog.show_modal() == ID_OK {
        Some(dialog.get_selection())
    } else {
        None
    };
    dialog.destroy();
    match selection {
        Some(0) => {
            let prompt = TextEntryDialog::builder(frame, "填写接收通知的邮箱", "添加邮箱").build();
            let email = if prompt.show_modal() == ID_OK {
                prompt.get_value()
            } else {
                None
            };
            prompt.destroy();
            if let Some(email) = email {
                if dialog_job::run(frame,api.clone(),"发送验证码","/api/desktop/squad/request",json!({"method":"POST","path":"/user/notify-email/send-code","body":{"email":email}})).is_some(){forms::edit(frame,api,"验证通知邮箱","POST","/user/notify-email/verify",json!({"email":email,"code":""}));}
            }
        }
        Some(1 | 2) => {
            let rows = profile["balance_notify_extra_emails"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(row) = organization::choose(frame, "选择通知邮箱", &rows, "email") {
                if selection == Some(1) {
                    dialog_job::run(
                        frame,
                        api,
                        "调整邮箱通知",
                        "/api/desktop/squad/request",
                        json!({"method":"PUT","path":"/user/notify-email/toggle","body":{"email":row["email"],"disabled":row["disabled"]!=true}}),
                    );
                } else if super::external_import::confirm(frame, "移除此通知邮箱？") {
                    dialog_job::run(
                        frame,
                        api,
                        "移除邮箱",
                        "/api/desktop/squad/request",
                        json!({"method":"DELETE","path":"/user/notify-email","body":{"email":row["email"]}}),
                    );
                }
            }
        }
        _ => {}
    }
}
fn key_form(frame: &Frame, api: ApiClient, row: Option<&Value>) {
    let Some(groups) = query(frame, api.clone(), "读取可用渠道分组", "/groups/available")
    else {
        return;
    };
    let rows = groups
        .as_array()
        .or_else(|| groups["items"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut choices = vec![("不指定分组".into(), Value::Null)];
    choices.extend(rows.iter().map(|g| {
        (
            format!(
                "{} · {}",
                g["name"].as_str().unwrap_or("分组"),
                g["platform"].as_str().unwrap_or("")
            ),
            g["id"].clone(),
        )
    }));
    if let Some(row) = row {
        for key in ["group_id", "fallback_group_id"] {
            if !row[key].is_null() && !choices.iter().any(|(_, id)| *id == row[key]) {
                choices.push((
                    format!("当前分组 {}（无可用详情）", row[key]),
                    row[key].clone(),
                ));
            }
        }
    }
    let options = [
        ("group_id".into(), choices.clone()),
        ("fallback_group_id".into(), choices),
        (
            "status".into(),
            vec![
                ("使用中".into(), "active".into()),
                ("未使用".into(), "inactive".into()),
            ],
        ),
    ]
    .into_iter()
    .collect();
    let fields=row.map(|r|organization::pick(r,&["name","status","group_id","fallback_group_id","quota","expires_at","rate_limit_5h","rate_limit_1d","rate_limit_7d","ip_whitelist","ip_blacklist"])).unwrap_or_else(||json!({"name":"","group_id":null,"fallback_group_id":null,"quota":0.0,"expires_in_days":0,"rate_limit_5h":0.0,"rate_limit_1d":0.0,"rate_limit_7d":0.0,"ip_whitelist":[],"ip_blacklist":[]}));
    let path = row
        .map(|r| format!("/keys/{}", r["id"]))
        .unwrap_or("/keys".into());
    if let Some(value) = forms::edit_options(
        frame,
        api,
        if row.is_some() {
            "编辑 API 密钥"
        } else {
            "新建 API 密钥"
        },
        if row.is_some() { "PUT" } else { "POST" },
        &path,
        fields,
        options,
    ) {
        if row.is_none() {
            key_details(frame, &value);
        }
    }
}
fn key_details(frame: &Frame, key: &Value) {
    let dialog = Dialog::builder(frame, "API 密钥")
        .with_size(620, 260)
        .build();
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let title = StaticText::builder(&dialog)
        .with_label(key["name"].as_str().unwrap_or("API 密钥"))
        .build();
    column.add(&title, 0, SizerFlag::All, 18);
    let field = TextCtrl::builder(&dialog)
        .with_value("••••••••••••••••")
        .with_style(TextCtrlStyle::ReadOnly)
        .build();
    column.add(
        &field,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        18,
    );
    let bar = BoxSizer::builder(Orientation::Horizontal).build();
    let reveal = Button::builder(&dialog).with_label("显示密钥").build();
    let copy = Button::builder(&dialog).with_label("复制密钥").build();
    bar.add(&reveal, 0, SizerFlag::Right, 8);
    bar.add(&copy, 0, SizerFlag::default(), 0);
    column.add_sizer(&bar, 0, SizerFlag::AlignRight | SizerFlag::All, 18);
    dialog.set_sizer(column, true);
    let raw = key["key"].as_str().unwrap_or("").to_string();
    let text = raw.clone();
    reveal.on_click(move |_| field.set_value(&text));
    copy.on_click(move |_| {
        if !raw.is_empty() && wxdragon::clipboard::Clipboard::get().set_text(&raw) {
            copy.set_label("已复制");
        }
    });
    dialog.show_modal();
    dialog.destroy();
}
fn funding_order(frame: &Frame, api: ApiClient, state: &Value) {
    let items: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(
        state["funding_priorities"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    ));
    let dialog = Dialog::builder(frame, "资金使用顺序")
        .with_size(650, 480)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let list = ListBox::builder(&dialog).build();
    column.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let controls = BoxSizer::builder(Orientation::Horizontal).build();
    let up = Button::builder(&dialog).with_label("上移").build();
    let down = Button::builder(&dialog).with_label("下移").build();
    let toggle = Button::builder(&dialog).with_label("启用 / 停用").build();
    for b in [up, down, toggle] {
        controls.add(&b, 0, SizerFlag::Right, 8);
    }
    column.add_sizer(&controls, 0, SizerFlag::Left, 16);
    let status = StaticText::builder(&dialog).build();
    column.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 14);
    let save = Button::builder(&dialog).with_label("保存顺序").build();
    column.add(&save, 0, SizerFlag::AlignRight | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    let rows = items.clone();
    let refresh = Rc::new(move |selected: usize| {
        list.clear();
        for item in rows.borrow().iter() {
            let source = match item["source_type"].as_str() {
                Some("personal_balance") => "个人余额",
                Some("subscription") => "订阅",
                Some("department_budget") => "部门预算",
                _ => "组织资金",
            };
            list.append(&format!(
                "{}  {}  {} / {}",
                if item["enabled"] == true {
                    "●"
                } else {
                    "○"
                },
                source,
                item["business_unit_name"].as_str().unwrap_or(""),
                item["department_name"].as_str().unwrap_or("")
            ));
        }
        if selected < rows.borrow().len() {
            list.set_selection(selected as u32, true);
        }
    });
    refresh(0);
    for (action, b) in [(0, up), (1, down), (2, toggle)] {
        let rows = items.clone();
        let refresh = refresh.clone();
        b.on_click(move |_| {
            let Some(i) = list.get_selection().map(|v| v as usize) else {
                return;
            };
            let mut rows = rows.borrow_mut();
            let next = if action == 0 && i > 0 {
                i - 1
            } else if action == 1 && i + 1 < rows.len() {
                i + 1
            } else {
                i
            };
            if action == 2 {
                rows[i]["enabled"] = (rows[i]["enabled"] != true).into();
            } else {
                rows.swap(i, next);
            }
            drop(rows);
            refresh(next);
        });
    }
    let job = dialog_job::bind(dialog, save, status, api, move || {
        Ok((
            "/api/desktop/squad/request".into(),
            json!({"method":"PUT","path":"/user/organization/funding-priorities","body":{"items":items.borrow().iter().map(|r|organization::pick(r,&["source_type","business_unit_id","department_id","enabled"])).collect::<Vec<_>>()}}),
        ))
    });
    dialog.show_modal();
    drop(job);
    dialog.destroy();
}

#[cfg(test)]
mod tests {
    use super::super::squad_login::{login_navigation, same_origin};
    #[test]
    fn login_boundary_is_exact_origin_and_known_verifiers() {
        assert!(same_origin(
            crate::desktop::squad::ORIGIN,
            "https://tiancai.yc99.space/login"
        ));
        for url in [
            "http://tiancai.yc99.space",
            "https://tiancai.yc99.space.evil.invalid",
            "file:///tmp/login",
            "https://user@tiancai.yc99.space",
        ] {
            assert!(!same_origin(crate::desktop::squad::ORIGIN, url));
            assert!(!login_navigation(crate::desktop::squad::ORIGIN, url));
        }
    }
}
