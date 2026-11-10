//! Browser login presentation, also used by the hidden production layout review.
use super::{
    controls::{Button, ButtonKind},
    theme,
    visual::{self, Icon},
};
use serde_json::Value;
use std::{cell::Cell, rc::Rc};
use wxdragon::prelude::*;

pub(super) fn render_profile(button: Button, state: &Value) {
    if state["signedIn"].is_null() {
        button.set_label("恢复账户…");
        button.set_tooltip("正在恢复已保存的登录状态；点击可查看进度或重试");
        return;
    }
    if state["signedIn"] == true {
        let user = &state["user"];
        let name = user["username"]
            .as_str()
            .filter(|v| !v.trim().is_empty())
            .or(user["email"].as_str())
            .unwrap_or("我的账户");
        // Keep the balance readable even at the minimum window width.
        let mut units = 0;
        let mut short = String::new();
        for ch in name.chars() {
            let width = if ch.is_ascii() { 1 } else { 2 };
            if units + width > 8 {
                break;
            }
            units += width;
            short.push(ch);
        }
        if short.len() < name.len() {
            short.push('…');
        }
        let balance = crate::desktop::squad_money::Money::state(state).usd(&user["balance"]);
        button.set_label(&format!("{short} · {balance}"));
        button.set_tooltip(&format!("{name}\n账户余额 {balance}\n查看个人资料"));
    } else {
        button.set_label(if state["expired"] == true {
            "重新登录"
        } else {
            "登录账户"
        });
        button.set_tooltip("登录天才小分队，查看个人资料和余额");
    }
}

/// Shown before the saved session has been checked, including temporary failures.
#[derive(Clone)]
pub(super) struct SessionRestore {
    pub page: Panel,
    pub retry: Button,
    status: StaticText,
}
impl SessionRestore {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        col.add_stretch_spacer(1);
        let title = StaticText::builder(&page).with_label("天才小分队").build();
        title.set_font(&theme::font(theme::TextRole::PageTitle));
        title.set_foreground_color(theme::theme().ink_primary);
        col.add(
            &title,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(16),
        );
        let status = StaticText::builder(&page)
            .with_label("正在恢复已保存的登录状态…")
            .build();
        status.set_foreground_color(theme::theme().ink_secondary);
        col.add(
            &status,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(16),
        );
        let retry = Button::builder(&page)
            .with_label("重试")
            .with_icon(Icon::Refresh)
            .build();
        col.add(&retry, 0, SizerFlag::AlignCenterHorizontal, 0);
        col.add_stretch_spacer(1);
        page.set_sizer(col, false);
        let ui = Self {
            page,
            retry,
            status,
        };
        ui.loading();
        ui
    }
    pub fn loading(&self) {
        self.status.set_label("正在恢复已保存的登录状态…");
        self.status
            .set_foreground_color(theme::theme().ink_secondary);
        self.retry.hide();
        self.page.layout();
    }
    pub fn error(&self, error: &str) {
        self.status.set_label(&format!(
            "{}\n登录状态已保留，网络恢复后可重试。",
            friendly_error(error)
        ));
        self.status.wrap(self.page.from_dip(420));
        self.status.set_foreground_color(theme::theme().error);
        self.retry.show(true);
        self.page.layout();
    }
}

#[derive(Clone)]
pub(super) struct BrowserLogin {
    pub page: Panel,
    pub start: Button,
    pub builtin: Button,
    pub cancel: Button,
    pub code: StaticText,
    pub status: StaticText,
    pub available: Rc<Cell<bool>>,
}
impl BrowserLogin {
    pub fn new(parent: &dyn WxWidget) -> Self {
        let t = theme::theme();
        let page = Panel::builder(parent).build();
        page.set_background_color(t.bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        col.add_stretch_spacer(1);
        let title = StaticText::builder(&page)
            .with_label("天才小分队账户")
            .build();
        title.set_font(&theme::font(theme::TextRole::PageTitle));
        title.set_foreground_color(t.ink_primary);
        col.add(
            &title,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(14),
        );
        let info = StaticText::builder(&page)
            .with_label("登录后查看余额、API 密钥与组织服务")
            .build();
        info.set_foreground_color(t.ink_secondary);
        col.add(
            &info,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(20),
        );
        let start = Button::builder(&page)
            .with_label("检查浏览器登录")
            .with_kind(ButtonKind::Ghost)
            .with_icon(Icon::Squad)
            .with_size(Size::new(200, 40))
            .build();
        start.set_name("squad-browser-login");
        col.add(
            &start,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(8),
        );
        let code = StaticText::builder(&page).with_label("").build();
        code.set_foreground_color(t.accent);
        let mut code_font = theme::font(theme::TextRole::Title);
        code_font.set_point_size(23);
        code.set_font(&code_font);
        code.set_min_size(visual::dip_size(&page, -1, 36));
        code.hide();
        col.add(
            &code,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(8),
        );
        let status = StaticText::builder(&page)
            .with_label("正在检查可用的登录方式…")
            .build();
        status.set_foreground_color(t.ink_secondary);
        status.wrap(page.from_dip(420));
        col.add(
            &status,
            0,
            SizerFlag::AlignCenterHorizontal | SizerFlag::Bottom,
            page.from_dip(14),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let builtin = Button::builder(&page)
            .with_label("内置登录")
            .with_kind(ButtonKind::Primary)
            .with_size(Size::new(200, 40))
            .build();
        let cancel = Button::builder(&page)
            .with_label("取消等待")
            .with_kind(ButtonKind::Ghost)
            .build();
        cancel.hide();
        row.add(&builtin, 0, SizerFlag::Right, page.from_dip(8));
        row.add(&cancel, 0, SizerFlag::default(), 0);
        col.add_sizer(&row, 0, SizerFlag::AlignCenterHorizontal, 0);
        col.add_stretch_spacer(2);
        page.set_sizer(col, true);
        Self {
            page,
            start,
            builtin,
            cancel,
            code,
            status,
            available: Default::default(),
        }
    }
    pub fn capability(&self, data: &Value) {
        self.available.set(data["state"] == "available");
        self.idle();
        self.message(
            if self.available.get() {
                "可使用浏览器中已登录的账号"
            } else {
                data["message"].as_str().unwrap_or("可先使用内置登录")
            },
            false,
        );
    }
    pub fn waiting(&self, code: &str) {
        let left: String = code.chars().take(3).collect();
        let right: String = code.chars().skip(3).collect();
        self.code.set_label(&format!("{left}  {right}"));
        self.code.show(true);
        self.status
            .set_label("请在浏览器确认核对码，完成后会自动进入账户");
        self.status
            .set_foreground_color(theme::theme().ink_secondary);
        self.start.set_label("重新打开浏览器");
        self.cancel.show(true);
        self.page.layout();
    }
    pub fn message(&self, text: &str, error: bool) {
        self.status.set_label(&friendly_error(text));
        self.status.set_foreground_color(if error {
            theme::theme().error
        } else {
            theme::theme().ink_secondary
        });
        self.status.wrap(self.page.from_dip(420));
        self.page.layout();
    }
    pub fn idle(&self) {
        self.code.set_label("");
        self.code.hide();
        self.start.set_label(if self.available.get() {
            "在浏览器中登录"
        } else {
            "重新检查浏览器登录"
        });
        self.start.set_kind(if self.available.get() {
            ButtonKind::Primary
        } else {
            ButtonKind::Ghost
        });
        self.builtin.set_kind(if self.available.get() {
            ButtonKind::Ghost
        } else {
            ButtonKind::Primary
        });
        self.cancel.hide();
        self.start.enable(true);
        self.page.layout();
    }
}

pub(super) fn friendly_error(error: &str) -> String {
    // The local transport wraps application errors in HTTP/JSON. Show the
    // application message rather than leaking the wrapper into account pages.
    error
        .find('{')
        .and_then(|i| serde_json::from_str::<Value>(&error[i..]).ok())
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_transport_wrapper_is_not_account_copy() {
        assert_eq!(
            friendly_error(r#"HTTP 409 Conflict: {"error":"请使用内置登录"}"#),
            "请使用内置登录"
        );
        assert_eq!(
            friendly_error("暂时无法读取，请重试"),
            "暂时无法读取，请重试"
        );
    }
}
