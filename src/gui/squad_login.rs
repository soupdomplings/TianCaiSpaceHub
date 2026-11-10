//! Inline, private website login. Created only when this page is activated.
use super::{
    controls::{Button, ButtonKind},
    theme,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wxdragon::{
    event::WebViewEvents,
    prelude::*,
    widgets::{WebView, WebViewBackend, WebViewUserScriptInjectionTime},
};

#[derive(Clone)]
pub(super) struct InlineLogin {
    pub page: Panel,
    pub retry: Button,
    pub status: StaticText,
    host: Panel,
    view: Rc<RefCell<Option<WebView>>>,
    session: Rc<RefCell<Option<Value>>>,
    origin: &'static str,
}
impl InlineLogin {
    pub fn new(parent: &dyn WxWidget, origin: &'static str) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let title = StaticText::builder(&page)
            .with_label("登录天才小分队")
            .build();
        title.set_font(&theme::font(theme::TextRole::Title));
        title.set_foreground_color(theme::theme().ink_primary);
        let retry = Button::builder(&page)
            .with_label("重新加载登录页")
            .with_kind(ButtonKind::Ghost)
            .build();
        row.add(&title, 1, SizerFlag::AlignCenterVertical, 0);
        row.add(&retry, 0, SizerFlag::AlignCenterVertical, 0);
        col.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            page.from_dip(12),
        );
        let status = StaticText::builder(&page)
            .with_label("正在确认登录状态…")
            .build();
        status.set_foreground_color(theme::theme().ink_secondary);
        status.wrap(page.from_dip(520));
        col.add(
            &status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            page.from_dip(12),
        );
        let host = Panel::builder(&page).build();
        host.set_background_color(theme::theme().bg_card);
        host.set_sizer(BoxSizer::builder(Orientation::Vertical).build(), true);
        col.add(&host, 1, SizerFlag::Expand, 0);
        page.set_sizer(col, true);
        Self {
            page,
            retry,
            status,
            host,
            view: Default::default(),
            session: Default::default(),
            origin,
        }
    }
    pub fn activate(&self) {
        if self.view.borrow().is_some() {
            return;
        }
        let backend = if cfg!(windows) {
            WebViewBackend::Edge
        } else {
            WebViewBackend::WebKit
        };
        let Some(view) = WebView::try_private(&self.host, backend) else {
            self.show_error("登录组件不可用。Windows 请安装或修复 Microsoft Edge WebView2 Runtime，再点击重新加载登录页。");
            return;
        };
        view.enable_access_to_dev_tools(false);
        view.enable_context_menu(false);
        if !view.add_script_message_handler("TianCaiLogin") {
            view.destroy();
            self.show_error(
                "无法建立安全登录通道，请重新加载登录页；仍失败时请修复 WebView2 Runtime。",
            );
            return;
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        let script = session_script(self.origin, &nonce);
        // wxWebViewEdge 3.3.2 rejects AtDocumentEnd. Do not silently continue
        // with a website that can sign in but cannot return the session to Hub.
        if !view.add_user_script(&script, WebViewUserScriptInjectionTime::AtDocumentStart) {
            view.remove_script_message_handler("TianCaiLogin");
            view.destroy();
            self.show_error(
                "无法准备登录回传，请重新加载登录页；仍失败时请修复 WebView2 Runtime。",
            );
            return;
        }
        let (result, origin) = (self.session.clone(), self.origin);
        let accepted = Rc::new(Cell::new(false));
        view.on_script_message_received(move |event| {
            if accepted.get() {
                return;
            }
            if !same_origin(origin, &view.get_current_url()) {
                return;
            }
            let Some(text) = event.get_string().filter(|s| s.len() <= 40 * 1024) else {
                return;
            };
            if let Some(value) = validated_session(origin, &view.get_current_url(), &nonce, &text) {
                accepted.set(true);
                *result.borrow_mut() = Some(value);
            }
        });
        let origin = self.origin;
        view.on_navigating(move |event| {
            if !login_navigation(origin, &event.get_string().unwrap_or_default()) {
                event.event.event.veto();
            }
        });
        view.on_new_window(move |event| {
            event.event.event.veto();
            let raw = event.get_string().unwrap_or_default();
            if login_navigation(origin, &raw) {
                view.load_url(&raw);
            }
        });
        let status = self.status;
        let page = self.page;
        let retry = self.retry;
        view.on_error(move |_| {
            status.set_label("登录页未能加载，请检查网络或代理后重新加载。若一直为空白，请修复 WebView2 Runtime。");
            status.set_foreground_color(theme::theme().error);
            status.wrap(page.from_dip(520));
            retry.enable(true);
            page.layout();
        });
        if let Some(sizer) = self.host.get_sizer() {
            sizer.add(&view, 1, SizerFlag::Expand, 0);
        }
        *self.view.borrow_mut() = Some(view);
        self.status
            .set_foreground_color(theme::theme().ink_secondary);
        self.status
            .set_label("请在下方登录。验证码、二次验证和第三方登录沿用官网流程。");
        self.host.layout();
        self.page.layout();
        view.load_url(&format!("{}/login", self.origin));
    }
    pub fn take_session(&self) -> Option<Value> {
        self.session.borrow_mut().take()
    }
    pub fn submitting(&self) {
        self.status.set_label("正在确认登录…");
        self.retry.enable(false);
        if let Some(view) = *self.view.borrow() {
            view.enable(false);
        }
    }
    pub fn show_error(&self, message: &str) {
        self.status.set_label(message);
        self.status.set_foreground_color(theme::theme().error);
        self.status.wrap(self.page.from_dip(520));
        self.retry.enable(true);
        self.page.layout();
    }
    pub fn clear(&self) {
        if let Some(view) = self.view.borrow_mut().take() {
            view.remove_script_message_handler("TianCaiLogin");
            view.stop();
            view.remove_all_user_scripts();
            view.destroy();
        }
        self.session.borrow_mut().take();
        self.retry.enable(true);
    }
}
fn session_script(origin: &str, nonce: &str) -> String {
    format!(
        r#"(()=>{{if(window.top!==window||location.origin!=={origin})return;let sent=false;let send=()=>{{if(sent)return;try{{let access=localStorage.getItem('auth_token');let bridge=window.TianCaiLogin;if(!access||!bridge||typeof bridge.postMessage!=='function')return;bridge.postMessage(JSON.stringify({{nonce:{nonce},accessToken:access,refreshToken:localStorage.getItem('refresh_token')||'',expiresAt:Number(localStorage.getItem('token_expires_at'))||0}}));sent=true;}}catch(_){{}}}};setInterval(send,500);send();}})();"#,
        origin = serde_json::to_string(origin).unwrap(),
        nonce = serde_json::to_string(nonce).unwrap()
    )
}
pub(super) fn same_origin(origin: &str, raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        u.origin().ascii_serialization() == origin
            && u.username().is_empty()
            && u.password().is_none()
    })
}
pub(super) fn login_navigation(origin: &str, raw: &str) -> bool {
    raw == "about:blank"
        || same_origin(origin, raw)
        || url::Url::parse(raw).is_ok_and(|u| {
            u.scheme() == "https"
                && u.username().is_empty()
                && u.password().is_none()
                && matches!(
                    u.host_str(),
                    Some(
                        "github.com"
                            | "accounts.google.com"
                            | "open.weixin.qq.com"
                            | "login.microsoftonline.com"
                            | "connect.linux.do"
                            | "challenges.cloudflare.com"
                    )
                )
        })
}

fn validated_session(origin: &str, current: &str, nonce: &str, text: &str) -> Option<Value> {
    if !same_origin(origin, current) || text.len() > 40 * 1024 {
        return None;
    }
    let mut value: Value = serde_json::from_str(text).ok()?;
    if value["nonce"] != nonce
        || value["accessToken"]
            .as_str()
            .is_none_or(|token| token.is_empty())
    {
        return None;
    }
    value.as_object_mut()?.remove("nonce");
    Some(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    const ORIGIN: &str = "https://login.example.invalid";
    #[test]
    fn session_capture_requires_origin_nonce_and_nonempty_token() {
        let body =
            r#"{"nonce":"fixture","accessToken":"synthetic","refreshToken":"","expiresAt":0}"#;
        let session = validated_session(
            ORIGIN,
            "https://login.example.invalid/login",
            "fixture",
            body,
        )
        .unwrap();
        assert!(session.get("nonce").is_none());
        assert_eq!(session["accessToken"], "synthetic");
        for page in [
            "https://github.com/login",
            "https://login.example.invalid.evil.invalid/",
            "http://login.example.invalid/",
            "https://user@login.example.invalid/",
            "file:///login",
        ] {
            assert!(validated_session(ORIGIN, page, "fixture", body).is_none());
        }
        assert!(validated_session(ORIGIN, ORIGIN, "wrong", body).is_none());
        assert!(
            validated_session(
                ORIGIN,
                ORIGIN,
                "fixture",
                r#"{"nonce":"fixture","accessToken":""}"#
            )
            .is_none()
        );
        assert!(
            validated_session(
                ORIGIN,
                ORIGIN,
                "fixture",
                r#"{"nonce":"fixture","accessToken":42}"#
            )
            .is_none()
        );
        assert!(validated_session(ORIGIN, ORIGIN, "fixture", &" ".repeat(40 * 1024 + 1)).is_none());
    }
    #[test]
    fn known_login_verifiers_can_navigate_but_cannot_supply_session() {
        for url in [
            "https://github.com/login/oauth",
            "https://accounts.google.com/",
            "https://open.weixin.qq.com/",
            "https://login.microsoftonline.com/",
            "https://connect.linux.do/",
            "https://challenges.cloudflare.com/",
        ] {
            assert!(login_navigation(ORIGIN, url));
            assert!(!same_origin(ORIGIN, url));
        }
        for url in [
            "https://evil.invalid/",
            "https://github.com.evil.invalid/",
            "https://user@github.com/",
            "http://github.com/",
            "javascript:alert(1)",
        ] {
            assert!(!login_navigation(ORIGIN, url));
        }
    }
}
