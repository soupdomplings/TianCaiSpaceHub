//! Persistent, opt-in WorkBuddy sharing controls inside client settings.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, paint_surface},
    dialog_job::InlineJob,
    loading::LoadingIndicator,
    squad_dialog, theme,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use wxdragon::prelude::*;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Read,
    Refresh,
    Hide,
    Enable,
}
impl Operation {
    fn request(self, view: &Value) -> Result<(&'static str, Value), String> {
        let (operation, mut body) = match self {
            Self::Read => ("read", json!({})),
            Self::Refresh => ("account-refresh", json!({})),
            Self::Hide => ("visibility", json!({"visible":false})),
            Self::Enable => {
                if view["builtinShareAllowed"] != true {
                    return Err("请切换到软萌猫娘后启用或重新授权分享".into());
                }
                ("enable", json!({}))
            }
        };
        if matches!(self, Self::Hide | Self::Enable) && view["revision"].is_string() {
            body["revision"] = view["revision"].clone();
        }
        let path = match operation {
            "read" => "/api/desktop/workbuddy-builtin/read",
            "account-refresh" => "/api/desktop/workbuddy-builtin/account-refresh",
            "visibility" => "/api/desktop/workbuddy-builtin/visibility",
            _ => "/api/desktop/workbuddy-builtin/enable",
        };
        Ok((path, body))
    }
}

fn number(value: &Value) -> Option<String> {
    match value {
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}
fn timestamp(value: &Value) -> Option<String> {
    let seconds = value.as_u64()? / 1000;
    (seconds > 0 && seconds <= 253_402_300_799).then(|| {
        crate::codex_app_config::format_rfc3339_utc(seconds)
            .replace('T', " ")
            .replace('Z', " UTC")
    })
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct AccountSummary {
    pub account: String,
    pub authorization: String,
    pub credits: String,
    pub updated: String,
    pub detail: String,
}
pub(super) fn account_summary(view: &Value, refresh_failed: bool) -> AccountSummary {
    let account = &view["account"];
    let auth = &account["authorization"];
    let credits = &account["credits"];
    let account_name = account["accountName"]
        .as_str()
        .or_else(|| view["accountName"].as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("尚未获取")
        .to_owned();
    let mut authorization = match auth["status"].as_str() {
        Some("available") => "已授权",
        Some("refreshable") => "可自动续期",
        Some("expired") => "已过期，请重新授权",
        Some("rejected") => "授权失效，请重新授权",
        Some("missing") => "尚未授权",
        _ => "尚未读取",
    }
    .to_owned();
    if auth["status"] == "refreshable" {
        if let Some(expiry) = timestamp(&auth["refreshExpiresAt"]) {
            authorization.push_str(&format!(" · 续期有效至 {expiry}"));
        }
    } else if let Some(expiry) = timestamp(&auth["expiresAt"]) {
        authorization.push_str(&format!(" · 到期 {expiry}"));
    }
    let known_credits = matches!(credits["status"].as_str(), Some("ready" | "error"));
    let mut remaining = if !known_credits {
        "尚未获取".into()
    } else if credits["unlimited"] == true {
        "不限量".into()
    } else {
        number(&credits["remaining"]).unwrap_or_else(|| "尚未获取".into())
    };
    let stale = refresh_failed || credits["status"] == "error";
    if stale {
        remaining.push_str("（未更新）");
    }
    let updated = timestamp(&credits["updatedAt"]).unwrap_or_else(|| "尚未获取".into());
    let mut details = Vec::new();
    let totals = [("总额", &credits["total"]), ("已用", &credits["used"])]
        .into_iter()
        .filter(|_| known_credits)
        .filter_map(|(label, value)| number(value).map(|value| format!("{label} {value}")))
        .collect::<Vec<_>>();
    if !totals.is_empty() {
        details.push(totals.join(" · "));
    }
    let buckets = credits["quotaItems"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|_| known_credits)
        .filter_map(|bucket| {
            let remaining = number(&bucket["remaining"])?;
            let label = bucket["label"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("积分包");
            Some(format!("{label} {remaining}"))
        })
        .collect::<Vec<_>>();
    if !buckets.is_empty()
        && number(&credits["remaining"]).is_none()
        && credits["unlimited"] != true
    {
        remaining = if stale {
            "见下方积分包（未更新）"
        } else {
            "见下方积分包"
        }
        .into();
    }
    if buckets.len() > 1 || (buckets.len() == 1 && number(&credits["remaining"]).is_none()) {
        details.push(buckets.join(" · "));
    }
    if stale {
        details.push(
            credits["detail"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("本次未能更新积分，保留上次结果；可稍后刷新。")
                .to_owned(),
        );
    }
    details.push(
        if view["hostConnected"] == true {
            "WorkBuddy 已连接"
        } else {
            "WorkBuddy 未连接"
        }
        .into(),
    );
    AccountSummary {
        account: account_name,
        authorization,
        credits: remaining,
        updated,
        detail: details.join("\n"),
    }
}

fn automatic_refresh_due(view: &Value, elapsed: Duration, now: u64) -> bool {
    view["featureVisible"] == true
        && view["enabled"] == true
        && matches!(
            view["account"]["authorization"]["status"].as_str(),
            Some("available" | "refreshable")
        )
        && elapsed >= REFRESH_INTERVAL
        && view["account"]["credits"]["updatedAt"]
            .as_u64()
            .is_none_or(|updated| {
                now.saturating_sub(updated) >= REFRESH_INTERVAL.as_millis() as u64
            })
}

type Pending = Arc<Mutex<Option<(u64, Operation, Result<Value, String>)>>>;
#[derive(Clone)]
pub(super) struct ShareSettings {
    pub panel: Panel,
    visible: CheckBox,
    labels: [StaticText; 5],
    manage: Button,
    refresh: Button,
    authorize: Button,
    job: Rc<InlineJob>,
    loading: LoadingIndicator,
    api: ApiClient,
    frame: Frame,
    view: Rc<RefCell<Value>>,
    refresh_failed: Rc<Cell<bool>>,
    busy: Arc<AtomicBool>,
    refreshing: Rc<Cell<bool>>,
    manual_refresh: Rc<Cell<bool>>,
    epoch: Rc<Cell<u64>>,
    pending: Pending,
    last_read: Rc<Cell<Instant>>,
    last_attempt: Rc<Cell<Instant>>,
}
impl ShareSettings {
    pub fn new(parent: &Panel, frame: Frame, api: ApiClient) -> Self {
        let panel = Panel::builder(parent).build();
        panel.set_name("workbuddy-share-settings");
        paint_surface(panel, theme::SURFACE_RADIUS);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let visible = CheckBox::builder(&panel).with_label("内置模型分享").build();
        visible.set_font(&theme::font(theme::TextRole::Title));
        visible.set_foreground_color(theme::theme().ink_primary);
        visible.set_value(true);
        visible.set_tooltip(
            "取消勾选只隐藏此入口，分享、授权和已保存模型继续保留；可在天才搭档对话中再次打开。",
        );
        column.add(
            &visible,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            panel.from_dip(14),
        );
        let grid = FlexGridSizer::builder(0, 2)
            .with_hgap(panel.from_dip(18))
            .with_vgap(panel.from_dip(10))
            .build();
        grid.add_growable_col(1, 1);
        let labels: [StaticText; 4] = std::array::from_fn(|index| {
            let label = squad_dialog::note(
                &panel,
                ["授权账号", "本机授权", "剩余积分", "积分更新时间"][index],
            );
            grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
            let value = squad_dialog::field_label(&panel, "尚未获取");
            value.set_min_size(Size::new(0, 0));
            grid.add(&value, 0, SizerFlag::Expand, 0);
            value
        });
        column.add_sizer(&grid, 0, SizerFlag::Expand, 0);
        let detail = squad_dialog::note(&panel, "");
        detail.set_min_size(Size::new(0, 0));
        column.add(
            &detail,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(8),
        );
        let actions = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let manage = Button::builder(&panel)
            .with_label("管理分享")
            .with_kind(ButtonKind::Secondary)
            .build();
        let refresh = Button::builder(&panel)
            .with_label("刷新状态与积分")
            .with_kind(ButtonKind::Ghost)
            .build();
        let authorize = Button::builder(&panel)
            .with_label("重新授权")
            .with_kind(ButtonKind::Ghost)
            .build();
        for button in [manage, refresh, authorize] {
            actions.add(
                &button,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                panel.from_dip(8),
            );
        }
        column.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            panel.from_dip(12),
        );
        let status = squad_dialog::note(&panel, "");
        status.hide();
        column.add(&status, 0, SizerFlag::Expand, 0);
        let padding = BoxSizer::builder(Orientation::Vertical).build();
        padding.add_sizer(
            &column,
            0,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(theme::PANEL_INSET),
        );
        panel.set_sizer(padding, false);
        panel.hide();
        if let Some(column) = parent.get_sizer() {
            column.add(
                &panel,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
                parent.from_dip(18),
            );
        }
        let ui = Self {
            panel,
            visible,
            labels: [labels[0], labels[1], labels[2], labels[3], detail],
            manage,
            refresh,
            authorize,
            job: Rc::new(InlineJob::new(panel, status)),
            loading: LoadingIndicator::for_button(&panel, refresh),
            api,
            frame,
            view: Default::default(),
            refresh_failed: Default::default(),
            busy: Default::default(),
            refreshing: Default::default(),
            manual_refresh: Default::default(),
            epoch: Default::default(),
            pending: Default::default(),
            last_read: Rc::new(Cell::new(Instant::now() - Duration::from_secs(5))),
            last_attempt: Rc::new(Cell::new(Instant::now() - REFRESH_INTERVAL)),
        };
        let u = ui.clone();
        visible.on_toggled(move |event| {
            if !event.is_checked() {
                u.run(Operation::Hide);
            }
        });
        let u = ui.clone();
        refresh.on_click(move |_| u.run(Operation::Refresh));
        let u = ui.clone();
        authorize.on_click(move |_| u.run(Operation::Enable));
        let u = ui.clone();
        manage.on_click(move |_| {
            super::workbuddy_builtin::show(&u.frame, &u.api, |_| {});
            u.read();
        });
        let u = ui.clone();
        panel.on_size(move |event| {
            u.reflow();
            event.skip(true);
        });
        ui
    }
    pub fn read(&self) {
        self.background(Operation::Read);
    }
    fn background(&self, operation: Operation) {
        if self.job.is_running() || self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.last_read.set(Instant::now());
        if operation == Operation::Refresh {
            self.last_attempt.set(Instant::now());
            self.refreshing.set(true);
            self.loading.start("刷新积分中…");
            self.controls();
        }
        let (path, body) = operation
            .request(&self.view.borrow())
            .expect("read-only WorkBuddy operation");
        let (api, pending, epoch) = (self.api.clone(), self.pending.clone(), self.epoch.get());
        std::thread::spawn(move || {
            let result = api.desktop_post(path, &body);
            if let Ok(mut pending) = pending.lock() {
                *pending = Some((epoch, operation, result));
            }
            wxdragon::wake_up_idle();
        });
    }
    pub fn tick(&self, settings_visible: bool) {
        let result = self.pending.lock().ok().and_then(|mut p| p.take());
        if let Some((epoch, operation, result)) = result {
            self.busy.store(false, Ordering::SeqCst);
            if operation == Operation::Refresh {
                self.refreshing.set(false);
                self.loading.finish();
            }
            let manual = operation == Operation::Refresh && self.manual_refresh.replace(false);
            if epoch == self.epoch.get() {
                match result {
                    Ok(view) => {
                        let recovered =
                            operation == Operation::Refresh && self.refresh_failed.replace(false);
                        self.apply(view);
                        if recovered {
                            self.render();
                        }
                    }
                    Err(error) if operation == Operation::Refresh => {
                        self.refresh_failed.set(true);
                        self.render();
                        if manual {
                            super::feedback::error(&self.panel, &error);
                        }
                    }
                    Err(_) => {}
                }
            }
            self.controls();
        }
        if !settings_visible || self.job.is_running() || self.busy.load(Ordering::SeqCst) {
            return;
        }
        if automatic_refresh_due(
            &self.view.borrow(),
            self.last_attempt.get().elapsed(),
            now_ms(),
        ) {
            self.background(Operation::Refresh);
        } else if self.last_read.get().elapsed() >= Duration::from_secs(5) {
            self.read();
        }
    }
    fn run(&self, operation: Operation) {
        if self.job.is_running() || (operation == Operation::Refresh && self.refreshing.get()) {
            return;
        }
        if operation == Operation::Refresh {
            if self.busy.load(Ordering::SeqCst) {
                return;
            }
            self.manual_refresh.set(true);
            self.background(operation);
            return;
        }
        let request = operation.request(&self.view.borrow());
        let (path, body) = match request {
            Ok(request) => request,
            Err(message) => {
                super::feedback::error(&self.panel, &message);
                return;
            }
        };
        self.epoch.set(self.epoch.get() + 1);
        let u = self.clone();
        self.job.start_with_feedback(
            self.api.clone(),
            match operation {
                Operation::Hide => "隐藏内置模型分享入口",
                Operation::Enable => "授权 WorkBuddy 分享",
                _ => "刷新状态与积分",
            },
            path,
            body,
            operation == Operation::Hide,
            move |result| {
                match result {
                    Ok(view) => u.apply(view),
                    Err(_) => u.render(),
                }
                u.controls();
            },
        );
        self.controls();
    }
    fn apply(&self, view: Value) {
        if *self.view.borrow() == view {
            self.controls();
            return;
        }
        *self.view.borrow_mut() = view;
        self.render();
    }
    fn render(&self) {
        let view = self.view.borrow();
        let summary = account_summary(&view, self.refresh_failed.get());
        for (label, text) in self.labels.iter().zip([
            summary.account,
            summary.authorization,
            summary.credits,
            summary.updated,
            summary.detail,
        ]) {
            label.set_label(&text);
        }
        let shown = view["featureVisible"] == true;
        self.visible.set_value(shown);
        self.panel.show(shown);
        drop(view);
        self.controls();
        self.reflow();
        if let Some(parent) = self.panel.get_parent() {
            parent.layout();
            if let Some(viewport) = parent.get_parent() {
                viewport.layout();
            }
        }
    }
    fn controls(&self) {
        let view = self.view.borrow();
        let idle = !self.job.is_running();
        self.visible.enable(idle);
        self.manage.enable(idle);
        self.refresh
            .enable(idle && !self.busy.load(Ordering::SeqCst));
        let needs_auth = view["reauthorizationRequired"] == true || view["enabled"] != true;
        self.authorize.show(needs_auth);
        self.authorize
            .set_label(if view["reauthorizationRequired"] == true {
                "重新授权"
            } else {
                "启用分享"
            });
        self.authorize
            .enable(idle && view["builtinShareAllowed"] == true);
    }
    fn reflow(&self) {
        let width = (self.panel.get_client_size().width
            - self.panel.from_dip(theme::PANEL_INSET * 2 + 108))
        .max(self.panel.from_dip(180));
        let summary = account_summary(&self.view.borrow(), self.refresh_failed.get());
        for (label, text) in self.labels.iter().zip([
            summary.account,
            summary.authorization,
            summary.credits,
            summary.updated,
            summary.detail,
        ]) {
            label.set_label(&text);
            label.wrap(width);
        }
        self.panel.layout();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_values_keep_real_credit_units_unknown_and_unlimited_distinct() {
        let mut view = json!({"hostConnected":false,"account":{"accountName":"合成账号","authorization":{"status":"refreshable"},"credits":{"status":"ready","remaining":125.5,"total":150,"used":24.5,"updatedAt":1794355200000_u64}}});
        let summary = account_summary(&view, false);
        assert_eq!(summary.account, "合成账号");
        assert_eq!(summary.authorization, "可自动续期");
        assert_eq!(summary.credits, "125.5");
        assert!(summary.updated.contains("UTC"));
        assert!(!summary.detail.contains("已登录"));
        view["account"]["credits"]["status"] = "error".into();
        assert_eq!(account_summary(&view, false).credits, "125.5（未更新）");
        view["account"]["credits"] = json!({"status":"unknown","remaining":100});
        assert_eq!(account_summary(&view, false).credits, "尚未获取");
        view["account"]["credits"] = json!({"status":"ready","remaining":"100"});
        assert_eq!(account_summary(&view, false).credits, "尚未获取");
        view["account"]["credits"] = json!({"status":"ready","remaining":0,"unlimited":true});
        assert_eq!(account_summary(&view, false).credits, "不限量");
        view["account"]["credits"]["unlimited"] = false.into();
        assert_eq!(account_summary(&view, false).credits, "0");
        view["account"]["credits"] = json!({"status":"ready","quotaItems":[{"label":"每日积分","remaining":20},{"label":"订阅积分","remaining":100}]});
        let summary = account_summary(&view, false);
        assert_eq!(summary.credits, "见下方积分包");
        assert!(summary.detail.contains("每日积分 20 · 订阅积分 100"));
    }
    #[test]
    fn cached_reads_refresh_and_hiding_never_enable_or_disable_sharing() {
        let view = json!({"revision":"fixture","builtinShareAllowed":false});
        assert_eq!(
            Operation::Read.request(&view).unwrap(),
            ("/api/desktop/workbuddy-builtin/read", json!({}))
        );
        assert_eq!(
            Operation::Refresh.request(&view).unwrap(),
            ("/api/desktop/workbuddy-builtin/account-refresh", json!({}))
        );
        assert_eq!(
            Operation::Hide.request(&view).unwrap(),
            (
                "/api/desktop/workbuddy-builtin/visibility",
                json!({"visible":false,"revision":"fixture"})
            )
        );
        assert!(Operation::Enable.request(&view).is_err());
        let mut visible = json!({"featureVisible":true,"enabled":true,"account":{"authorization":{"status":"available"},"credits":{"status":"unknown"}}});
        assert!(automatic_refresh_due(
            &visible,
            Duration::from_secs(60),
            120000
        ));
        assert!(!automatic_refresh_due(
            &visible,
            Duration::from_secs(59),
            120000
        ));
        visible["account"]["credits"]["updatedAt"] = 119000.into();
        assert!(!automatic_refresh_due(
            &visible,
            Duration::from_secs(60),
            120000
        ));
        visible["featureVisible"] = false.into();
        assert!(!automatic_refresh_due(
            &visible,
            Duration::from_secs(1000),
            900000
        ));
    }

    #[cfg(windows)]
    #[test]
    fn hidden_settings_card_keeps_controls_identity_and_allows_reading_after_personality_change() {
        wxdragon::app::render_offscreen(|| {
            for mode in [theme::ThemeMode::Light, theme::ThemeMode::Dark] {
                theme::init(mode);
                let frame = Frame::builder().with_title("WorkBuddy settings fixture").with_size(Size::new(760, 680)).build();
                let host = Panel::builder(&frame).build();
                host.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), false);
                let layout = BoxSizer::builder(Orientation::Vertical).build();
                layout.add(&host, 1, SizerFlag::Expand, 0);
                frame.set_sizer(layout, true);
                let api = ApiClient::new("http://127.0.0.1:1".into(), super::super::text::GuiText::new(super::super::text::GuiLocale::ZhCn));
                let ui = ShareSettings::new(&host, frame, api);
                assert!(!ui.panel.is_shown());
                let manage_handle = ui.manage.handle_ptr();
                let view = json!({"revision":"fixture","featureVisible":true,"enabled":true,"ready":true,"builtinShareAllowed":false,
                    "account":{"accountName":"合成账号","authorization":{"status":"available"},"credits":{"status":"ready","remaining":125.5}}});
                ui.apply(view.clone());
                frame.layout(); host.layout(); ui.reflow();
                assert!(ui.panel.is_shown());
                assert!(ui.visible.is_checked());
                assert!(ui.manage.is_enabled());
                assert_eq!(ui.labels[2].get_label(), "125.5");
                assert!(!ui.authorize.is_shown());
                assert_eq!(ui.manage.handle_ptr(), manage_handle);
                ui.refresh_failed.set(true);
                ui.render();
                assert_eq!(ui.labels[2].get_label(), "125.5（未更新）");
                let mut hidden = view;
                hidden["featureVisible"] = false.into();
                ui.apply(hidden);
                assert!(!ui.panel.is_shown());
                assert_eq!(ui.view.borrow()["enabled"], true);
                assert_eq!(ui.manage.handle_ptr(), manage_handle);
                assert!(!frame.is_shown());
                drop(ui);
                frame.destroy();
            }
        }).unwrap();
    }
}
