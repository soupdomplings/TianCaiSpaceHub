use crate::gui::controls::Button;
use crate::gui::controls::{Choice, TextCtrl};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use reqwest::{blocking::Client, header::HeaderValue};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{
    GuiMessage,
    browser::open_url_in_browser,
    daemon::{app_support_config_path, daemon_config_path},
    text::{GuiLocale, GuiText},
    theme,
    widgets::{
        apply_textctrl_theme, connection_field_theme, connection_text_field_row as text_field_row,
    },
};

#[derive(Default)]
struct Selection {
    config: Value,
    profiles: Vec<Value>,
    selected_id: Option<String>,
    clients: BTreeMap<String, Value>,
    selected_client: String,
    client_paths: BTreeMap<String, String>,
    client_feedback: BTreeMap<String, String>,
    twofactor_session: Option<String>,
    runtime_profiles: Vec<Value>,
    initial_refreshes: u8,
    last_refresh: Option<Instant>,
    snapshot_loaded: bool,
    active_login: bool,
    login_attempt: Option<String>,
    saved_profile_snapshot: Option<Value>,
}

#[derive(Clone)]
struct ClientControls {
    selector: Choice,
    path: TextCtrl,
    status: StaticText,
    check: Button,
    manage: Button,
}

#[derive(Clone)]
pub(super) struct NvwaTab {
    pub(super) page: ScrolledWindow,
    text: GuiText,
    profile: Choice,
    name: TextCtrl,
    product: TextCtrl,
    certification: TextCtrl,
    mcp: TextCtrl,
    mode: Choice,
    application: CheckBox,
    advanced: CollapsiblePane,
    account_row: Panel,
    browser_account_row: Panel,
    browser_username: TextCtrl,
    password_row: Panel,
    password_settings: Panel,
    application_settings: Panel,
    signature_row: Panel,
    connection_hint: StaticText,
    factor_row: Panel,
    username: TextCtrl,
    password: TextCtrl,
    client_id: TextCtrl,
    client_secret: TextCtrl,
    tenant: TextCtrl,
    unit: TextCtrl,
    auth_header: TextCtrl,
    signature: Choice,
    remember_password: CheckBox,
    remember_secret: CheckBox,
    secret_status: StaticText,
    factor_code: TextCtrl,
    status: StaticText,
    loading: super::loading::LoadingIndicator,
    identity: StaticText,
    tools: TextCtrl,
    save: Button,
    login: Button,
    cancel: Button,
    send_factor: Button,
    help: Button,
    client: ClientControls,
    state: Rc<RefCell<Selection>>,
    busy: Arc<AtomicBool>,
    operation: Arc<AtomicU64>,
}

pub(super) struct NvwaActionResult {
    operation: u64,
    profile_id: Option<String>,
    action: Action,
    outcome: Result<Value, String>,
}

#[derive(Clone)]
enum Action {
    Refresh,
    RefreshAfterLogin,
    RefreshSaved,
    LoadCredential {
        fingerprint: String,
    },
    Save,
    Delete,
    Login,
    Cancel,
    ResetAuthentication,
    DetectClient(String),
    PrepareClient {
        client: String,
        body: Value,
    },
    SendFactor,
    Preview {
        client: String,
        operation: String,
        body: Value,
        check_after: bool,
    },
    Mutate {
        client: String,
        check_after: Option<Value>,
    },
}

impl Action {
    fn client_kind(&self) -> Option<&str> {
        match self {
            Self::DetectClient(kind) => Some(kind),
            Self::Preview { client, .. }
            | Self::PrepareClient { client, .. }
            | Self::Mutate { client, .. } => Some(client),
            _ => None,
        }
    }
}

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

fn display_message(text: GuiText, message: &str) -> String {
    let (zh, en) = match message {
        "NVWA login required" | "Login required" => (
            "还没有登录成功。请先点击“登录 / 授权”，成功后再检测 MCP 或接入客户端。",
            "You are not signed in. Sign in successfully before checking MCP or connecting a client.",
        ),
        "NVWA MCP token expired; login required"
        | "NVWA browser authorization expired; authorize again" => (
            "登录已过期。请重新点击“登录 / 授权”，成功后再继续。",
            "Your sign-in has expired. Sign in again before continuing.",
        ),
        "NVWA token expired; explicit login required" => (
            "登录已过期，尚未保存自动登录所需的资料。请重新登录；需要自动重新认证时，勾选“记住密码”或“保存应用密钥”。",
            "Sign-in expired and credentials for automatic renewal were not saved. Sign in again; enable Remember password or Save application secret for automatic renewal.",
        ),
        "NVWA automatic renewal failed recently; retry after one minute or sign in manually"
        | "Automatic renewal failed; retry after one minute or sign in manually" => (
            "自动重新认证没有成功。请核对密码或应用资料；一分钟后再次使用会重试，也可现在点击“登录 / 授权”。",
            "Automatic renewal failed. Check the password or application settings. Using the connection after one minute retries; you can also sign in manually now.",
        ),
        "Authentication verified; existing client access retained"
        | "Authenticated; existing client access retained"
        | "Verified login; existing client connections retained" => (
            "登录有效，已有客户端接入已保留。可以直接连接并检测或继续使用，无需重复接入。",
            "Sign-in verified and existing client connections retained. Check the local bridge or continue using them; reconnecting is unnecessary.",
        ),
        "NVWA profile changed; explicit login required" => (
            "环境设置已变化。请保存环境并重新登录，再接入客户端。",
            "Environment settings changed. Save the environment and sign in again.",
        ),
        "Saved NVWA credential unavailable; login required" => (
            "无法读取之前保存的登录资料。请重新填写并登录。",
            "Saved credentials could not be read. Enter them and sign in again.",
        ),
        "Restored NVWA identity could not be verified; login again" => (
            "无法确认之前保存的登录仍然有效。请重新登录。",
            "The saved sign-in could not be verified. Please sign in again.",
        ),
        "Restored client access suspended; explicit login required" => (
            "之前的客户端接入已保留，正在等待登录核验。请用原账号和租户登录；确认相同身份后原连接可继续使用，无需重新接入。",
            "Previous client connections were retained and await sign-in verification. Sign in with the original account and tenant; the same identity restores them without reconnecting.",
        ),
        "Protected client access could not be restored; sign in and preview connection once" => (
            "已保存的客户端接入信息无法安全恢复。请重新登录，并预览确认接入一次；这不是普通认证到期。",
            "Saved client access could not be safely restored. Sign in and preview the connection once. This is not normal authentication expiry.",
        ),
        "NVWA identity changed; explicit login required"
        | "NVWA identity changed; explicit login and client access required" => (
            "服务返回的账号或租户已变化。请重新登录，确认身份后再接入客户端。",
            "The returned account or tenant changed. Sign in, verify your identity, and reconnect the client.",
        ),
        "NVWA requires interactive login verification" => (
            "登录还需要验证码或其他确认。请点击“登录 / 授权”完成验证。",
            "Sign-in needs a verification code or another confirmation. Select Sign in to continue.",
        ),
        "Login failed; previous client access remains revoked" => (
            "登录没有成功。请处理登录错误后重试，再重新接入客户端。",
            "Sign-in failed. Resolve the sign-in error and try again, then reconnect the client.",
        ),
        "Login failed; existing client access remains suspended" => (
            "登录没有成功，已有客户端接入暂时停用。请处理登录错误后重试；同账号和租户登录成功后，原连接可继续使用。",
            "Sign-in failed and existing client access is temporarily suspended. Resolve the error and retry; signing in with the same account and tenant restores the connections.",
        ),
        "Waiting for login; existing client access suspended" => (
            "正在等待登录，已有客户端接入暂时停用。请完成原账号和租户的登录；成功后原连接可继续使用，无需重新接入。",
            "Waiting for sign-in; existing client access is temporarily suspended. Complete sign-in with the original account and tenant to restore the connections without reconnecting.",
        ),
        "Sign-in cancelled; existing client access suspended" => (
            "已取消本次登录，已有客户端接入暂时停用。用原账号和租户登录成功后恢复连接，无需重复接入。",
            "Sign-in was cancelled and existing access is suspended. Sign in with the same account and tenant to restore the connections without another preview.",
        ),
        "Waiting for login" => (
            "尚未完成登录。请查看登录结果或完成浏览器中的授权。",
            "Sign-in is not complete. Check the result or finish authorization in your browser.",
        ),
        "Authenticated; client access must be explicitly applied"
        | "Verified login; explicitly apply each client to grant access to this identity"
        | "Verified login; connect each client once to use this identity" => (
            "登录成功。选择下方客户端，点击“连接并检测”即可。",
            "Signed in. Choose a client below, then select Connect and check.",
        ),
        "Logged out; previous client access revoked" => (
            "已退出登录。客户端中的旧连接暂时不能使用；需要时重新登录并接入。",
            "Signed out. Previous client connections are inactive; sign in and reconnect when needed.",
        ),
        "NVWA authorization expired" | "NVWA rejected authorization; request was not replayed" => (
            "服务已拒绝当前认证。已保存密码或共享应用密钥时，下次使用会尝试自动重新认证；其他情况请重新登录。刚才的操作没有自动重试。",
            "The service rejected the current authentication. Saved password or shared application credentials allow renewal on next use; otherwise sign in again. The previous operation was not retried.",
        ),
        "MCP initialize and paginated tools/list verified; no tool was invoked" => (
            "已连接到 MCP 并读取工具清单。可以继续接入客户端；工具是否能执行，需要在客户端中确认。",
            "MCP connected and its tool list was read. Connect a client next; verify tool execution there.",
        ),
        "MCP connected; tool catalog exceeds read limit" => (
            "MCP 已连通；工具目录较大，未完整读取，工具总数未知。",
            "MCP connected; the tool catalog is large and incomplete, so the total is unknown.",
        ),
        "Waiting for one-time browser authorization"
        | "Complete authorization in NVWA; callback must match this one-time transaction" => (
            "请在打开的 NVWA 页面完成登录和授权，然后回到 Hub 查看结果。",
            "Finish sign-in and authorization on the opened NVWA page, then return to Hub.",
        ),
        "Browser authorization failed or superseded; start again" => (
            "浏览器授权未完成或已经失效。请重新点击“登录 / 授权”。",
            "Browser authorization failed or expired. Start sign-in again.",
        ),
        "Two factor verification required" | "Enter the verification code to continue" => (
            "还需要验证码才能登录。请发送验证码，收到后填写并再次点击“登录 / 授权”。",
            "A verification code is needed. Request it, enter it, and select Sign in again.",
        ),
        "Verification code requested" => (
            "已请求发送验证码。收到后填写并再次点击“登录 / 授权”。",
            "Verification code requested. Enter it and select Sign in again.",
        ),
        "Password change required in NVWA" => (
            "请先到 NVWA 页面修改密码，再回到 Hub 登录。",
            "Change your password on the NVWA page, then sign in from Hub.",
        ),
        "Client authorization required" | "Apply this client before checking its local bridge" => (
            "此客户端需要接入。点击“连接并检测”会直接显示接入确认，保存后自动继续检测。",
            "This client needs a connection. Connect and check opens setup and continues detection after you confirm.",
        ),
        "Client authorization invalid" => (
            "此连接的接入凭据需要更新。点击“连接并检测”，确认后自动继续检测。",
            "This connection's access key needs updating. Select Connect and check; detection continues after confirmation.",
        ),
        "Client connection needs repair; use Connect or repair" => (
            "客户端设置已变化。点击“连接并检测”，确认后会备份并修复这条连接，再自动检测。",
            "Client settings changed. Select Connect and check to back up, repair and check this connection.",
        ),
        "连接设置有变化，可以备份并修复。点击“连接并检测”即可继续；超时、工具选择等其他设置会保留。" => {
            (
                "连接设置有变化，可以备份并修复。点击“连接并检测”即可继续；超时、工具选择等其他设置会保留。",
                "Connection settings changed. Select Connect and check to back up and repair. Timeouts, tool selections and other compatible settings are preserved.",
            )
        }
        "将先备份此连接，再修复 Hub 本机地址、接入凭据和启用状态；保留超时、工具选择及其他可兼容设置。改用命令启动或其他认证方式的字段会移除。" => {
            (
                "将先备份此连接，再修复 Hub 本机地址、接入凭据和启用状态；保留超时、工具选择及其他可兼容设置。改用命令启动或其他认证方式的字段会移除。",
                "Back up this connection, repair its local Hub address, access key and enabled state, and keep timeouts, tool selections and other compatible settings. Conflicting command-launch and authentication fields are removed.",
            )
        }
        "此前的接入记录仍在，但客户端中的连接已不存在。点击“连接并检测”并确认，可重新添加连接。" => {
            (
                "此前的接入记录仍在，但客户端中的连接已不存在。点击“连接并检测”并确认，可重新添加连接。",
                "Hub retains the previous connection record, but the entry is missing from the client. Preview and confirm the connection to add it again.",
            )
        }
        "Login was cancelled or superseded"
        | "Login was superseded"
        | "Login changed during detection"
        | "Login changed during renewal" => (
            "登录已取消或变化。请确认当前环境并重新登录，再继续操作。",
            "Sign-in was cancelled or changed. Check the environment and sign in again.",
        ),
        "NVWA operation failed; check configuration or login again" => (
            "操作未完成。请检查环境地址和登录资料，保存后重新登录。配置方法可查看右上角“?”。",
            "The operation failed. Check the environment URL and credentials, save, and sign in again. See '?' for setup help.",
        ),
        _ => return message.to_string(),
    };
    tr(text, zh, en).to_string()
}

fn client_title(kind: &str) -> &'static str {
    match kind {
        "codex" => "ChatGPT(Codex)",
        "workbuddy" => "WorkBuddy",
        "tiangong" => "天工 Work",
        _ => "Dumpling-MCP",
    }
}

fn selected_client_kind(tab: &NvwaTab) -> &'static str {
    match tab.client.selector.get_selection().unwrap_or(0) {
        1 => "workbuddy",
        2 => "tiangong",
        _ => "codex",
    }
}

fn runtime_client_access(profile: &Value, kind: &str) -> (bool, bool) {
    let includes_client = |field: &str| {
        profile[field]
            .as_array()
            .is_some_and(|clients| clients.iter().any(|client| client.as_str() == Some(kind)))
    };
    (
        includes_client("clientAccess"),
        includes_client("suspendedClientAccess"),
    )
}

fn default_client_feedback(tab: &NvwaTab, kind: &str) -> String {
    let state = tab.state.borrow();
    let runtime = state
        .runtime_profiles
        .iter()
        .find(|profile| profile["profileId"].as_str() == state.selected_id.as_deref());
    let (has_access, suspended_access) = runtime
        .map(|profile| runtime_client_access(profile, kind))
        .unwrap_or_default();
    if runtime.is_some_and(|profile| profile["clientAccessRecoveryFailed"].as_bool() == Some(true))
        && !has_access
        && !suspended_access
    {
        return tr(
            tab.text,
            "之前的本机接入授权未能安全恢复。请重新登录，再“连接并检测”并确认一次；这与普通认证到期不同。",
            "The previous local access grant could not be safely restored. Sign in, then preview and confirm the connection once. This differs from normal authentication expiry.",
        )
        .to_string();
    }
    let state_name = runtime.and_then(|profile| profile["state"].as_str());
    let message = match (
        state_name,
        if state_name == Some("login_pending") {
            suspended_access
        } else {
            has_access
        },
    ) {
        (Some("authenticated"), true) => tr(
            tab.text,
            "Hub 已保留此客户端接入，点击“连接并检测”会自动核对设置并检测连接。",
            "Hub retained this client's access. Connect and check verifies its settings and connection automatically.",
        ),
        (Some("expired" | "personal_expired" | "identity_verification_pending"), true) => tr(
            tab.text,
            "Hub 已保留此客户端接入，无需重复预览。连接并检测时会核验或续认证；未保存自动登录资料时，请先用原身份重新登录。",
            "Hub retained this client's access; another preview is unnecessary. A bridge check verifies or renews authentication. If automatic sign-in credentials were not saved, sign in with the original identity first.",
        ),
        (Some("login_pending"), true) => tr(
            tab.text,
            "已有客户端接入暂时停用。请完成同账号和租户的登录，成功后原连接可继续使用，无需重新接入。",
            "Existing client access is temporarily suspended. Complete sign-in with the same account and tenant to restore the connection without reconnecting.",
        ),
        (Some("authenticated"), false) => tr(
            tab.text,
            "点击“连接并检测”即可接入并检测；需要保存连接时会直接显示确认。",
            "Select Connect and check to connect and check. Setup confirmation opens automatically when needed.",
        ),
        _ => tr(
            tab.text,
            "登录成功后，选择客户端并点击“连接并检测”即可。",
            "Sign in, choose a client, then select Connect and check.",
        ),
    };
    message.to_string()
}

fn sync_runtime_profile(tab: &NvwaTab, response: &Value) {
    let Some(runtime) = response.get("runtimeProfile") else {
        return;
    };
    let Some(profile_id) = runtime["profileId"].as_str() else {
        return;
    };
    let selected_kind = selected_client_kind(tab);
    let selected_changed = {
        let mut state = tab.state.borrow_mut();
        if state.selected_id.as_deref() != Some(profile_id) {
            return;
        }
        let previous = state
            .runtime_profiles
            .iter()
            .find(|profile| profile["profileId"].as_str() == Some(profile_id))
            .cloned()
            .unwrap_or(Value::Null);
        state
            .runtime_profiles
            .retain(|profile| profile["profileId"].as_str() != Some(profile_id));
        state.runtime_profiles.push(runtime.clone());
        let authentication_changed = previous["state"] != runtime["state"]
            || previous["identity"] != runtime["identity"]
            || previous["clientAccessRecoveryFailed"] != runtime["clientAccessRecoveryFailed"];
        let mut selected_changed = false;
        for kind in ["codex", "workbuddy", "tiangong"] {
            let (has_access, suspended) = runtime_client_access(runtime, kind);
            let changed = authentication_changed
                || runtime_client_access(&previous, kind) != (has_access, suspended)
                || state.clients.get(kind).is_some_and(|status| {
                    status["requiresClientAccess"].as_bool() != Some(!has_access)
                        || status["clientAccessSuspended"].as_bool() != Some(suspended)
                });
            if let Some(status) = state.clients.get_mut(kind) {
                status["requiresClientAccess"] = json!(!has_access);
                status["clientAccessSuspended"] = json!(suspended);
                status["clientAccessRecoveryFailed"] =
                    runtime["clientAccessRecoveryFailed"].clone();
                if !has_access || suspended {
                    status["bridgeState"] = json!("unknown");
                    status["bridgeCheckedAtMs"] = Value::Null;
                }
            }
            if changed {
                state.client_feedback.remove(kind);
                selected_changed |= kind == selected_kind;
            }
        }
        selected_changed
    };
    // Keep form inputs and per-client paths intact. A full status refresh
    // reloads the environment and would overwrite unsaved edits.
    render_runtime_status(tab, false);
    if selected_changed {
        let status = tab.state.borrow().clients.get(selected_kind).cloned();
        if let Some(status) = status {
            render_client_status(tab, selected_kind, status, false);
        } else {
            set_client_feedback(
                tab,
                selected_kind,
                &default_client_feedback(tab, selected_kind),
            );
        }
    }
}

fn set_client_feedback(tab: &NvwaTab, kind: &str, message: &str) {
    tab.state
        .borrow_mut()
        .client_feedback
        .insert(kind.to_string(), message.to_string());
    if selected_client_kind(tab) == kind {
        tab.client.status.set_label(message);
        tab.client.status.wrap(920);
    }
}

fn clear_client_results(tab: &NvwaTab) {
    let mut state = tab.state.borrow_mut();
    state.clients.clear();
    state.client_paths.clear();
    state.client_feedback.clear();
    state.selected_client = selected_client_kind(tab).to_string();
    drop(state);
    tab.client.path.set_value("");
    tab.client.status.set_label("");
}

fn select_client(tab: &NvwaTab) {
    let kind = selected_client_kind(tab);
    let (path, feedback, status) = {
        let mut state = tab.state.borrow_mut();
        let previous = state.selected_client.clone();
        if !previous.is_empty() {
            state
                .client_paths
                .insert(previous, tab.client.path.get_value());
        }
        state.selected_client = kind.to_string();
        (
            state.client_paths.get(kind).cloned().unwrap_or_default(),
            state.client_feedback.get(kind).cloned(),
            state.clients.get(kind).cloned(),
        )
    };
    tab.client.path.set_value(&path);
    if let Some(feedback) = feedback {
        tab.client.status.set_label(&feedback);
        tab.client.status.wrap(920);
    } else if let Some(status) = status {
        render_client_status(tab, kind, status, false);
    } else {
        set_client_feedback(tab, kind, &default_client_feedback(tab, kind));
    }
    update_controls(tab);
}

fn tool_catalog_text(text: GuiText, detection: &Value) -> String {
    let fallback;
    let summary = if let Some(summary) = detection.get("catalogSummary") {
        summary
    } else {
        fallback = crate::nvwa::discovery::summarize_tools(
            detection["tools"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        &fallback
    };
    let complete = detection["catalogComplete"].as_bool().unwrap_or(true);
    let mut lines = vec![catalog_connection_text(text, detection, false)];
    if let Some(count) = summary["groupCount"].as_u64().filter(|count| *count > 0) {
        let kind = if summary["moduleCount"].as_u64() == Some(count) {
            tr(text, "模块", "modules")
        } else {
            tr(text, "内容分组", "content groups")
        };
        lines.push(match (text.locale, complete) {
            (GuiLocale::ZhCn, true) => format!("共 {count} 个{kind}。"),
            (GuiLocale::ZhCn, false) => format!("已读取内容包含 {count} 个{kind}。"),
            (GuiLocale::EnUs, true) => format!("{count} {kind}."),
            (GuiLocale::EnUs, false) => format!("The content read so far contains {count} {kind}."),
        });
    }
    let groups = summary["groups"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if groups.is_empty() {
        lines.push(
            tr(
                text,
                if complete {
                    "服务未返回可用工具。"
                } else {
                    "目录未完整读取，暂时没有可展示的模块和用途。"
                },
                if complete {
                    "The service returned no available tools."
                } else {
                    "The catalog is incomplete; no module or purpose details are available yet."
                },
            )
            .to_string(),
        );
    }
    for group in groups {
        let name = group["name"].as_str().unwrap_or("");
        let count = group["toolCount"].as_u64().unwrap_or(0);
        let kind = match group["source"].as_str() {
            Some("module") => tr(text, "模块", "Module"),
            _ => tr(text, "工具分组", "Tool group"),
        };
        lines.push(match text.locale {
            GuiLocale::ZhCn => format!("\n{kind} {name}：{count} 个工具"),
            GuiLocale::EnUs => format!("\n{kind} {name}: {count} tools"),
        });
        for example in group["examples"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let code = example["name"].as_str().unwrap_or("");
            let title = example["title"]
                .as_str()
                .filter(|value| !value.is_empty())
                .unwrap_or(code);
            let heading = if title == code {
                title.to_string()
            } else {
                format!("{title} ({code})")
            };
            let description = example["description"]
                .as_str()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    tr(
                        text,
                        "服务未提供用途说明",
                        "The service did not describe its purpose",
                    )
                });
            lines.push(format!("  • {heading}: {description}"));
        }
        let omitted = group["omittedToolCount"].as_u64().unwrap_or(0);
        if omitted > 0 {
            lines.push(match text.locale {
                GuiLocale::ZhCn => format!("  另有 {omitted} 个工具未展开。"),
                GuiLocale::EnUs => format!("  {omitted} more tools are not expanded."),
            });
        }
    }
    let omitted_groups = summary["omittedGroupCount"].as_u64().unwrap_or(0);
    if omitted_groups > 0 {
        lines.push(match text.locale {
            GuiLocale::ZhCn => format!("\n另有 {omitted_groups} 个分组未展开。"),
            GuiLocale::EnUs => format!("\n{omitted_groups} more groups are not expanded."),
        });
    }
    lines.join("\n")
}

fn catalog_connection_text(text: GuiText, detection: &Value, local: bool) -> String {
    if detection["catalogComplete"] == false {
        return tr(text,
            if local { "本机桥已连通；工具目录较大，未完整读取，工具总数未知。" }
                else { "MCP 已连通；工具目录较大，未完整读取，工具总数未知。" },
            if local { "Local bridge connected; the tool catalog is large and incomplete, so the total is unknown." }
                else { "MCP connected; the tool catalog is large and incomplete, so the total is unknown." },
        ).to_string();
    }
    let count = detection["toolCount"].as_u64().or_else(|| {
        detection["tools"]
            .as_array()
            .map(|tools| tools.len() as u64)
    });
    match (text.locale, local, count) {
        (GuiLocale::ZhCn, false, Some(count)) => format!("MCP 连接成功，共 {count} 个可用工具。"),
        (GuiLocale::ZhCn, true, Some(count)) => format!("本机桥连接成功，共 {count} 个可用工具。"),
        (GuiLocale::EnUs, false, Some(count)) => format!("MCP connected; {count} tools available."),
        (GuiLocale::EnUs, true, Some(count)) => {
            format!("Local bridge connected; {count} tools available.")
        }
        _ => tr(
            text,
            "连接成功，服务未提供工具总数。",
            "Connected; the service did not provide a tool total.",
        )
        .to_string(),
    }
}

#[cfg(test)]
mod detection_summary_tests {
    use super::*;

    #[test]
    fn cancellation_stays_available_during_browser_wait_but_hides_after_current_profile_signs_in() {
        let mut state = Selection {
            selected_id: Some("current".into()),
            ..Selection::default()
        };
        assert!(!login_in_progress(&state));
        state.active_login = true;
        assert!(login_in_progress(&state));
        state.active_login = false;
        state.login_attempt = Some("fixture-attempt".into());
        assert!(login_in_progress(&state));
        state.runtime_profiles = vec![json!({"profileId":"other","state":"authenticated"})];
        assert!(login_in_progress(&state));
        state
            .runtime_profiles
            .push(json!({"profileId":"current","state":"authenticated"}));
        assert!(!login_in_progress(&state));
    }

    #[test]
    fn connection_previews_allow_repair_without_switching_to_destructive_actions() {
        for operation in ["apply", "repair", "remove", "restore", "unknown"] {
            assert_eq!(
                preview_operation_allowed("apply", operation),
                matches!(operation, "apply" | "repair")
            );
            assert_eq!(
                preview_operation_allowed("remove", operation),
                operation == "remove"
            );
            assert_eq!(
                preview_operation_allowed("restore", operation),
                operation == "restore"
            );
        }
        assert!(!preview_operation_allowed("unknown", "apply"));
    }

    #[test]
    fn abbreviated_examples_keep_the_exact_total_and_show_the_omission() {
        let detection = crate::nvwa::discovery::compact_detection(
            crate::nvwa::discovery::catalog_result(
                "fixture", None, "fixture-generation", false,
                (0..156).map(|index| json!({"name":format!("fixture.tool_{index}"),
                    "description":"只读用途", "_meta":{"com.jiuqi.nvwa":{"moduleName":"fixture-module"}}})).collect(), None,
            ),
        );
        let rendered = tool_catalog_text(
            GuiText {
                locale: GuiLocale::ZhCn,
            },
            &detection,
        );
        assert!(rendered.contains("156 个可用工具"));
        assert!(rendered.contains("共 1 个模块"));
        assert!(rendered.contains("153 个工具未展开"));
        assert!(rendered.contains("只读用途"));
        assert!(!rendered.contains("fixture.tool_3:"));
    }

    #[test]
    fn incomplete_catalog_does_not_render_a_partial_or_zero_count_as_the_total() {
        let partial = crate::nvwa::discovery::catalog_result(
            "fixture",
            None,
            "fixture-generation",
            true,
            vec![json!({"name":"fixture.tool"})],
            Some("response_size"),
        );
        let chinese = catalog_connection_text(
            GuiText {
                locale: GuiLocale::ZhCn,
            },
            &partial,
            true,
        );
        assert!(chinese.contains("本机桥已连通"));
        assert!(chinese.contains("工具总数未知"));
        assert!(!chinese.contains("1 个"));
        let english = catalog_connection_text(
            GuiText {
                locale: GuiLocale::EnUs,
            },
            &partial,
            false,
        );
        assert!(english.contains("total is unknown"));
        assert!(!english.contains("0 tools"));
        assert!(!english.contains("1 tools"));
    }
}

fn show_error(parent: &dyn WxWidget, message: &str) {
    super::feedback::error(parent, message);
}

fn label<W: WxWidget>(parent: &W, root: &BoxSizer, title: &str) {
    let item = StaticText::builder(parent).with_label(title).build();
    item.set_foreground_color(theme::theme().ink_secondary);
    root.add(&item, 0, SizerFlag::Expand | SizerFlag::All, 10);
}

fn choice_row<W: WxWidget>(
    parent: &W,
    grid: &FlexGridSizer,
    title: &str,
    values: &[&str],
) -> Choice {
    let title = StaticText::builder(parent).with_label(title).build();
    grid.add(&title, 0, SizerFlag::AlignCenterVertical, 0);
    let field = Choice::builder(parent).build();
    for value in values {
        field.append(value);
    }
    field.set_selection(0);
    connection_field_theme(&field, true);
    grid.add(&field, 1, SizerFlag::Expand, 0);
    field
}

fn button<W: WxWidget>(parent: &W, row: &BoxSizer, title: &str) -> Button {
    let control = Button::builder(parent).with_label(title).build();
    row.add(
        &control,
        0,
        SizerFlag::AlignCenterVertical | SizerFlag::Right,
        8,
    );
    control
}

fn section<W: WxWidget>(parent: &W, root: &BoxSizer) -> (Panel, FlexGridSizer) {
    let panel = Panel::builder(parent).build();
    panel.set_background_color(theme::theme().bg_card_alt);
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(8)
        .with_hgap(14)
        .build();
    grid.add_growable_col(1, 1);
    root.add(&panel, 0, SizerFlag::Expand | SizerFlag::Top, 8);
    (panel, grid)
}

fn auth_mode(tab: &NvwaTab) -> &'static str {
    if tab.mode.get_selection() != Some(1) {
        "password"
    } else if tab.application.is_checked() {
        "application"
    } else {
        "browser"
    }
}

pub(super) fn create(parent: &dyn WxWidget, text: GuiText) -> NvwaTab {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_card_alt);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    let heading = BoxSizer::builder(Orientation::Horizontal).build();
    let title = StaticText::builder(&page)
        .with_label("Dumpling-MCP")
        .build();
    title.set_foreground_color(theme::theme().ink_secondary);
    heading.add(&title, 0, SizerFlag::AlignCenterVertical, 0);
    heading.add_stretch_spacer(1);
    let help = Button::builder(&page)
        .with_label("?")
        .with_size(Size::new(32, 30))
        .build();
    help.set_tooltip(tr(
        text,
        "如何配置 Dumpling-MCP",
        "How to configure Dumpling-MCP",
    ));
    heading.add(&help, 0, SizerFlag::AlignCenterVertical, 0);
    root.add_sizer(&heading, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let grid = FlexGridSizer::builder(0, 2)
        .with_vgap(8)
        .with_hgap(14)
        .build();
    grid.add_growable_col(1, 1);
    let profile = choice_row(
        &page,
        &grid,
        tr(text, "环境", "Environment"),
        &[tr(text, "新增环境", "New environment")],
    );
    let name = text_field_row(&page, &grid, tr(text, "环境名称", "Environment name"), "");
    name.set_tooltip(tr(
        text,
        "Codex、WorkBuddy、天工接入的 MCP 都使用此名称；请使用 1～64 位英文字母、数字、- 或 _，例如 MCP196。",
        "Codex, WorkBuddy and TianGong use this name for MCP: 1–64 letters, numbers, - or _, for example MCP196.",
    ));
    let product = text_field_row(
        &page,
        &grid,
        tr(text, "NVWA 服务地址", "NVWA service URL"),
        "",
    );
    let mode = choice_row(
        &page,
        &grid,
        tr(text, "认证方式", "Authentication"),
        &[
            tr(text, "账号密码", "Account and password"),
            tr(text, "认证服务连接", "Authentication service connection"),
        ],
    );
    root.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let account_root = BoxSizer::builder(Orientation::Vertical).build();
    let (account_row, account_grid) = section(&page, &account_root);
    let username = text_field_row(
        &account_row,
        &account_grid,
        tr(text, "账号", "Username"),
        "",
    );
    account_row.set_sizer(account_grid, true);
    let (password_row, password_grid) = section(&page, &account_root);
    let password = text_field_row(
        &password_row,
        &password_grid,
        tr(text, "密码", "Password"),
        "",
    );
    let remember_password = CheckBox::builder(&password_row)
        .with_label(tr(text, "记住密码", "Remember password"))
        .build();
    password_grid.add_spacer(1);
    password_grid.add(&remember_password, 0, SizerFlag::Top, 4);
    password_row.set_sizer(password_grid, true);
    root.add_sizer(
        &account_root,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let connection_hint = StaticText::builder(&page)
        .with_label(tr(text, "在认证服务管理添加应用服务，获取ClientID和ClientSecret", "Add an application service in authentication service management to obtain ClientID and ClientSecret."))
        .build();
    connection_hint.wrap(920);
    root.add(&connection_hint, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let application = CheckBox::builder(&page)
        .with_label(tr(
            text,
            "使用共享应用代表指定用户",
            "Use a shared application for a specified user",
        ))
        .with_value(true)
        .build();
    root.add(
        &application,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        16,
    );
    let application_root = BoxSizer::builder(Orientation::Vertical).build();
    let (application_settings, application_grid) = section(&page, &application_root);
    let client_id = text_field_row(&application_settings, &application_grid, "ClientID", "");
    let client_secret =
        text_field_row(&application_settings, &application_grid, "ClientSecret", "");
    let remember_secret = CheckBox::builder(&application_settings)
        .with_label(tr(text, "保存应用密钥", "Save application secret"))
        .build();
    application_grid.add_spacer(1);
    application_grid.add(&remember_secret, 0, SizerFlag::Top, 4);
    let secret_status = StaticText::builder(&application_settings)
        .with_label("")
        .build();
    application_grid.add_spacer(1);
    application_grid.add(&secret_status, 0, SizerFlag::Expand | SizerFlag::Top, 4);
    application_settings.set_sizer(application_grid, true);
    root.add_sizer(
        &application_root,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        16,
    );

    let advanced = CollapsiblePane::builder(&page)
        .with_label(tr(
            text,
            "高级设置（特殊部署 / 管理员）",
            "Advanced settings (deployment / administrator)",
        ))
        .with_style(CollapsiblePaneStyle::NoTlwResize)
        .build();
    let content = advanced.get_pane().expect("NVWA advanced pane");
    let advanced_root = BoxSizer::builder(Orientation::Vertical).build();
    let (addresses, addresses_grid) = section(&content, &advanced_root);
    let certification = text_field_row(
        &addresses,
        &addresses_grid,
        tr(
            text,
            "独立认证地址（可选）",
            "Separate authentication URL (optional)",
        ),
        "",
    );
    let mcp = text_field_row(
        &addresses,
        &addresses_grid,
        tr(
            text,
            "MCP 路径或地址（默认 /mcp，可修改）",
            "MCP path or URL (default /mcp, editable)",
        ),
        "/mcp",
    );
    let auth_header = text_field_row(
        &addresses,
        &addresses_grid,
        tr(
            text,
            "认证头覆盖（自动选择）",
            "Authentication header override (automatic)",
        ),
        "Authorization",
    );
    let tenant = text_field_row(
        &addresses,
        &addresses_grid,
        tr(text, "指定租户（可选）", "Specific tenant (optional)"),
        "",
    );
    let client_path = text_field_row(
        &addresses,
        &addresses_grid,
        tr(
            text,
            "客户端配置位置（自动查找）",
            "Client settings location (automatic)",
        ),
        "",
    );
    client_path.set_tooltip(tr(
        text,
        "Hub 自动查找并核对所选客户端的配置。仅自定义安装位置需要手动填写；天工使用官方本机连接设置。",
        "Hub locates and checks the selected client's settings automatically. Enter a path only for a custom location; TianGong uses its official local connection settings.",
    ));
    addresses.set_sizer(addresses_grid, true);
    let (password_settings, password_settings_grid) = section(&content, &advanced_root);
    let unit = text_field_row(
        &password_settings,
        &password_settings_grid,
        tr(text, "登录机构（可选）", "Login organization (optional)"),
        "",
    );
    password_settings.set_sizer(password_settings_grid, true);
    let (browser_account_row, browser_account_grid) = section(&content, &advanced_root);
    let browser_username = text_field_row(
        &browser_account_row,
        &browser_account_grid,
        tr(
            text,
            "限定授权账号（可选）",
            "Restrict authorized username (optional)",
        ),
        "",
    );
    browser_account_row.set_sizer(browser_account_grid, true);

    let (signature_row, signature_grid) = section(&content, &advanced_root);
    let signature = choice_row(
        &signature_row,
        &signature_grid,
        tr(
            text,
            "应用签名算法（默认 SHA-256）",
            "Application signature (default SHA-256)",
        ),
        &["SHA-256", "SM3", "MD5"],
    );
    signature_row.set_sizer(signature_grid, true);
    content.set_sizer(advanced_root, true);
    advanced.collapse(true);
    root.add(&advanced, 0, SizerFlag::Expand | SizerFlag::All, 16);
    advanced.on_changed(move |_| {
        page.layout();
        page.fit_inside();
    });

    let factor_root = BoxSizer::builder(Orientation::Vertical).build();
    let (factor_row, factor_grid) = section(&page, &factor_root);
    let factor_code = text_field_row(
        &factor_row,
        &factor_grid,
        tr(text, "双因子验证码", "Verification code"),
        "",
    );
    let options = BoxSizer::builder(Orientation::Horizontal).build();
    let send_factor = button(
        &factor_row,
        &options,
        tr(text, "发送双因子验证码", "Send verification code"),
    );
    factor_grid.add_spacer(1);
    factor_grid.add_sizer(&options, 0, SizerFlag::Top, 4);
    factor_row.set_sizer(factor_grid, true);
    root.add_sizer(
        &factor_root,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        16,
    );
    let commands = BoxSizer::builder(Orientation::Horizontal).build();
    let save = button(&page, &commands, tr(text, "保存环境", "Save environment"));
    let login = button(&page, &commands, tr(text, "登录 / 授权", "Sign in"));
    let cancel = button(&page, &commands, tr(text, "取消登录", "Cancel sign-in"));
    cancel.set_tooltip(tr(
        text,
        "取消本次登录并暂时停用已有接入；原账号和租户重新登录成功后恢复连接。",
        "Cancel sign-in and suspend existing access until the same account and tenant sign in again.",
    ));
    root.add_sizer(
        &commands,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        16,
    );
    let status = StaticText::builder(&page)
        .with_label(tr(text, "后台状态未读取", "Backend status unavailable"))
        .build();
    status.wrap(920);
    root.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let identity = StaticText::builder(&page)
        .with_label(tr(text, "未登录", "Signed out"))
        .build();
    identity.wrap(920);
    root.add(
        &identity,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        16,
    );
    label(&page, &root, tr(text, "客户端接入", "Client connections"));
    let client = {
        let selector_row = BoxSizer::builder(Orientation::Horizontal).build();
        let selector_label = StaticText::builder(&page)
            .with_label(tr(text, "客户端", "Client"))
            .build();
        selector_row.add(
            &selector_label,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            14,
        );
        let selector = Choice::builder(&page).with_size(Size::new(210, -1)).build();
        for title in ["ChatGPT(Codex)", "WorkBuddy", "天工 Work"] {
            selector.append(title);
        }
        selector.set_selection(0);
        selector.set_tooltip(tr(
            text,
            "选择要连接的客户端，然后点击右侧“连接并检测”。",
            "Choose a client, then select Connect and check.",
        ));
        selector_row.add(
            &selector,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            8,
        );
        let check = button(
            &page,
            &selector_row,
            tr(text, "连接并检测", "Connect and check"),
        );
        check.set_tooltip(tr(
            text,
            "自动核对接入，必要时确认保存或修复，然后检测连接并显示工具概览。",
            "Check settings automatically, confirm setup or repair when needed, then verify the connection and show the tool overview.",
        ));
        let manage = Button::builder(&page)
            .with_label("⋯")
            .with_style(ButtonStyle::ExactFit)
            .with_size(Size::new(34, 30))
            .build();
        manage.set_tooltip(tr(
            text,
            "连接管理：断开当前客户端、撤销上次连接更改或删除当前环境。",
            "Manage connections: disconnect this client, undo the last connection change, or delete this environment.",
        ));
        selector_row.add(&manage, 0, SizerFlag::AlignCenterVertical, 0);
        root.add_sizer(
            &selector_row,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            16,
        );
        let status = StaticText::builder(&page)
            .with_label(tr(
                text,
                "选择客户端后，点击“连接并检测”即可。",
                "Choose a client, then select Connect and check.",
            ))
            .build();
        status.wrap(920);
        root.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 16);
        ClientControls {
            selector,
            path: client_path,
            status,
            check,
            manage,
        }
    };
    label(
        &page,
        &root,
        tr(text, "连接内容概览", "Connection overview"),
    );
    label(
        &page,
        &root,
        tr(
            text,
            "显示可用工具数量、模块或分组及少量用途示例；较多内容会省略展示。",
            "Shows the available tool count, modules or groups, and a few purpose examples; larger displays are abbreviated.",
        ),
    );
    let tools = TextCtrl::builder(&page)
        .with_style(TextCtrlStyle::ReadOnly | TextCtrlStyle::MultiLine)
        .with_size(Size::new(-1, 220))
        .build();
    apply_textctrl_theme(&tools);
    root.add(
        &tools,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        16,
    );
    page.set_sizer(root, true);
    page.set_scroll_rate(0, 12);
    page.layout();
    page.fit_inside();
    let tab = NvwaTab {
        page,
        text,
        profile,
        name,
        product,
        certification,
        mcp,
        mode,
        application,
        advanced,
        account_row,
        browser_account_row,
        browser_username,
        password_row,
        password_settings,
        application_settings,
        signature_row,
        connection_hint,
        factor_row,
        username,
        password,
        client_id,
        client_secret,
        tenant,
        unit,
        auth_header,
        signature,
        remember_password,
        remember_secret,
        secret_status,
        factor_code,
        status,
        identity,
        loading: super::loading::LoadingIndicator::new(&page, status),
        tools,
        save,
        login,
        cancel,
        send_factor,
        help,
        client,
        state: Rc::new(RefCell::new(Selection {
            selected_client: "codex".to_string(),
            ..Selection::default()
        })),
        busy: Arc::new(AtomicBool::new(false)),
        operation: Arc::new(AtomicU64::new(0)),
    };
    update_controls(&tab);
    tab
}

fn config_path() -> PathBuf {
    daemon_config_path().unwrap_or_else(app_support_config_path)
}

const MAX_MANAGEMENT_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

fn management_size_message(path: &str) -> String {
    if path == "/manage/tools/detect" {
        "Hub 界面未能完整读取较大的检测结果；这不代表 MCP 连接失败。请再次点击“连接并检测”。".into()
    } else {
        "Hub 后台返回内容较大，界面未能完整读取结果。".into()
    }
}

fn management_request(path: &str, body: Option<Value>) -> Result<Value, String> {
    let info = crate::nvwa::NvwaService::load_management_info(&config_path())
        .map_err(|_| "无法读取 NVWA 后台安全连接".to_string())?
        .ok_or_else(|| "NVWA 后台尚未就绪，请稍后重试".to_string())?;
    let url = url::Url::parse(&info.base_url).map_err(|_| "NVWA 后台地址无效".to_string())?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.path() != "/"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("NVWA 后台地址不符合本机连接要求".to_string());
    }
    let mut authorization = HeaderValue::from_str(&format!("Bearer {}", info.capability))
        .map_err(|_| "NVWA 后台授权无效".to_string())?;
    authorization.set_sensitive(true);
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(95))
        .build()
        .map_err(|_| "无法准备 NVWA 后台连接".to_string())?;
    let endpoint = format!("{}{}", info.base_url.trim_end_matches('/'), path);
    let request = if let Some(body) = body {
        client.post(endpoint).json(&body)
    } else {
        client.get(endpoint)
    };
    let response = request
        .header("Authorization", authorization)
        .send()
        .map_err(|_| "NVWA 后台连接失败或等待超时；操作不会自动重试".to_string())?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_MANAGEMENT_RESPONSE_BYTES)
    {
        return Err(management_size_message(path));
    }
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(MAX_MANAGEMENT_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 NVWA 后台响应".to_string())?;
    if bytes.len() as u64 > MAX_MANAGEMENT_RESPONSE_BYTES {
        return Err(management_size_message(path));
    }
    let payload: Value =
        serde_json::from_slice(&bytes).map_err(|_| "NVWA 后台响应格式无效".to_string())?;
    if !status.is_success() || payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("NVWA 操作未完成")
            .chars()
            .take(700)
            .collect());
    }
    Ok(payload.get("data").cloned().unwrap_or(Value::Null))
}

fn start(
    tab: &NvwaTab,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    action: Action,
    path: &str,
    mut body: Option<Value>,
) {
    if matches!(&action, Action::DetectClient(_))
        && let Some(body) = body.as_mut()
    {
        body["summaryOnly"] = json!(true);
    }
    if matches!(action, Action::Cancel | Action::ResetAuthentication) {
        tab.busy.store(true, Ordering::SeqCst);
    } else if tab.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    tab.loading.start(tr(tab.text, "处理中…", "Working…"));
    {
        let mut state = tab.state.borrow_mut();
        state.active_login = matches!(action, Action::Login);
        if matches!(action, Action::Login) {
            let attempt = uuid::Uuid::new_v4().to_string();
            if let Some(body) = body.as_mut() {
                body["loginAttemptId"] = json!(attempt);
            }
            state.login_attempt = Some(attempt);
            let selected_id = state.selected_id.clone();
            state
                .runtime_profiles
                .retain(|profile| profile["profileId"].as_str() != selected_id.as_deref());
            for client in state.clients.values_mut() {
                client["bridgeState"] = json!("unknown");
                client["bridgeCheckedAtMs"] = Value::Null;
            }
            state.client_feedback.clear();
        } else if matches!(action, Action::Cancel | Action::ResetAuthentication) {
            if let (Some(body), Some(attempt)) = (body.as_mut(), state.login_attempt.as_ref()) {
                body["loginAttemptId"] = json!(attempt);
            }
        }
    }
    if matches!(action, Action::Login) {
        tab.identity.set_label(tr(
            tab.text,
            "登录尚未完成。成功后这里会显示服务返回的账号和租户。",
            "Sign-in is pending. The returned account and tenant will appear here after success.",
        ));
        tab.tools.set_value("");
        set_client_feedback(
            tab,
            selected_client_kind(tab),
            tr(
                tab.text,
                "正在登录。成功后选择客户端，点击“连接并检测”即可。",
                "Signing in. After success, choose a client and select Connect and check.",
            ),
        );
    } else if matches!(action, Action::Cancel) {
        clear_client_results(tab);
        tab.tools.set_value("");
        tab.identity.set_label("");
    }
    let operation = tab.operation.fetch_add(1, Ordering::SeqCst) + 1;
    let profile_id = tab.state.borrow().selected_id.clone();
    if let Some(kind) = action.client_kind() {
        set_client_feedback(tab, kind, tr(tab.text, "处理中…", "Working…"));
    }
    update_controls(tab);
    let path = path.to_string();
    let tx = tx.clone();
    thread::spawn(move || {
        let outcome = management_request(&path, body);
        let _ = tx.send(GuiMessage::Nvwa(NvwaActionResult {
            operation,
            profile_id,
            action,
            outcome,
        }));
        wxdragon::wake_up_idle();
    });
}

pub(super) fn bind_actions(
    tab: &NvwaTab,
    frame: &Frame,
    text: GuiText,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
) {
    let f = *frame;
    tab.help.on_click(move |_| {
        super::feedback::information(
            &f,
            tr(text, "Dumpling-MCP 配置说明", "Dumpling-MCP configuration"),
            configuration_help(text),
        );
    });
    let t = tab.clone();
    let sender = tx.clone();
    tab.profile.on_selection_changed(move |_| {
        select_profile(&t, &sender);
    });
    let t = tab.clone();
    tab.client
        .selector
        .on_selection_changed(move |_| select_client(&t));
    let t = tab.clone();
    let sender = tx.clone();
    tab.mode.on_selection_changed(move |_| {
        t.application.set_value(true);
        reset_authentication(&t, &sender, false);
    });
    let t = tab.clone();
    let sender = tx.clone();
    tab.application
        .on_toggled(move |_| reset_authentication(&t, &sender, true));
    let t = tab.clone();
    tab.remember_secret
        .on_toggled(move |_| update_secret_status(&t));
    for field in [
        tab.name,
        tab.product,
        tab.certification,
        tab.mcp,
        tab.username,
        tab.browser_username,
        tab.client_id,
        tab.client_secret,
        tab.tenant,
        tab.unit,
        tab.auth_header,
    ] {
        let t = tab.clone();
        field.on_text_updated(move |_| update_secret_status(&t));
    }
    let t = tab.clone();
    tab.signature
        .on_selection_changed(move |_| update_secret_status(&t));
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.save.on_click(move |_| {
        let state = t.state.borrow();
        let mut profile = state
            .profiles
            .iter()
            .find(|p| p["id"].as_str() == state.selected_id.as_deref())
            .cloned()
            .unwrap_or_else(|| json!({"id":uuid::Uuid::new_v4().to_string()}));
        for (key, value) in form_fields(&t) {
            // Equivalent defaults must not revoke an existing saved credential.
            if key == "mcpUrl" && value.is_empty() && profile[key].as_str() == Some("/mcp") {
                continue;
            }
            if key == "loginUnit" && value.is_empty() && profile[key].is_null() {
                continue;
            }
            if key == "mcpAuthHeader" && profile[key].is_null() {
                let default = if auth_mode(&t) == "password" {
                    "Authorization"
                } else {
                    "authorization-ticket-token"
                };
                if value == default {
                    continue;
                }
            }
            profile[key] = json!(value);
        }
        if let Some(profile) = profile.as_object_mut() {
            profile.remove("credentialSecretRef");
        }
        let revision = config_revision(&state.config).to_string();
        drop(state);
        if profile["name"].as_str().unwrap_or("").is_empty() {
            show_error(&f, "请输入环境名称");
            return;
        }
        let mut body = json!({"profile":profile,"expectedRevision":revision});
        if auth_mode(&t) != "password" {
            body["rememberSecret"] = json!(t.remember_secret.is_checked());
            if t.remember_secret.is_checked() {
                body["clientSecret"] = json!(t.client_secret.get_value());
            }
        }
        start(
            &t,
            &sender,
            Action::Save,
            "/manage/profile/save",
            Some(body),
        );
    });
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.login.on_click(move |_| {
        if !require_saved_form(&t, &f, text) {
            return;
        }
        let state = t.state.borrow();
        let mode = auth_mode(&t);
        if mode != "password" && t.client_id.get_value().trim().is_empty() {
            drop(state);
            t.advanced.collapse(false);
            t.page.layout();
            t.page.fit_inside();
            show_error(
                &f,
                tr(
                    text,
                    "请填写 ClientID 和 ClientSecret。获取方法见页面右上角“?”。",
                    "Enter ClientID and ClientSecret. See the '?' button for setup instructions.",
                ),
            );
            return;
        }
        let mut body = json!({"profileId":state.selected_id});
        if mode == "password" {
            let mut ext = serde_json::Map::new();
            if let Some(session) = &state.twofactor_session {
                ext.insert("twofactorSessionId".to_string(), json!(session));
                let code = t.factor_code.get_value();
                if !code.is_empty() {
                    ext.insert("validCode".to_string(), json!(code));
                }
            }
            body["password"] = json!(t.password.get_value());
            body["rememberPassword"] = json!(t.remember_password.is_checked());
            body["extInfo"] = json!(ext);
        } else {
            body["clientSecret"] = json!(t.client_secret.get_value());
            body["rememberSecret"] = json!(t.remember_secret.is_checked());
        }
        drop(state);
        let path = match mode {
            "browser" => "/manage/login/browser",
            "application" => "/manage/login/application",
            _ => "/manage/login/password",
        };
        start(&t, &sender, Action::Login, path, Some(body));
    });
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.send_factor.on_click(move |_| {
        if !require_saved_form(&t, &f, text) {
            return;
        }
        let session = t.state.borrow().twofactor_session.clone();
        if let Some(session) = session {
            let body =
                json!({"profileId":t.state.borrow().selected_id,"twofactorSessionId":session});
            start(
                &t,
                &sender,
                Action::SendFactor,
                "/manage/login/twofactor/send",
                Some(body),
            );
        }
    });
    let t = tab.clone();
    let sender = tx.clone();
    tab.cancel.on_click(move |_| {
        let body = json!({"profileId":t.state.borrow().selected_id});
        start(
            &t,
            &sender,
            Action::Cancel,
            "/manage/login/cancel",
            Some(body),
        );
    });
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.client.check.on_click(move |_| {
        if !require_saved_form(&t, &f, text) {
            return;
        }
        let kind = selected_client_kind(&t);
        start(
            &t,
            &sender,
            Action::PrepareClient {
                client: kind.to_string(),
                body: client_body(&t, kind),
            },
            "/manage/client/prepare",
            Some(client_body(&t, kind)),
        );
    });
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.client
        .manage
        .on_click(move |_| show_connection_menu(&t, &f, text, &sender));
}

fn config_revision(config: &Value) -> &str {
    config
        .get("revision")
        .or_else(|| config.get("_revision"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn clear_login_material(tab: &NvwaTab) {
    tab.password.set_value("");
    tab.client_secret.set_value("");
    tab.factor_code.set_value("");
    tab.remember_password.set_value(false);
    tab.remember_secret.set_value(false);
    tab.state.borrow_mut().twofactor_session = None;
}

fn reset_authentication(
    tab: &NvwaTab,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    keep_application_credentials: bool,
) {
    let application_credentials = keep_application_credentials.then(|| {
        (
            tab.client_id.get_value(),
            tab.client_secret.get_value(),
            tab.remember_secret.is_checked(),
        )
    });
    clear_login_material(tab);
    for field in [
        tab.username,
        tab.browser_username,
        tab.client_id,
        tab.tenant,
        tab.unit,
    ] {
        field.set_value("");
    }
    if let Some((client_id, client_secret, remember_secret)) = application_credentials {
        tab.client_id.set_value(&client_id);
        tab.client_secret.set_value(&client_secret);
        tab.remember_secret.set_value(remember_secret);
    }
    tab.signature.set_selection(0);
    tab.auth_header.set_value(if auth_mode(tab) == "password" {
        "Authorization"
    } else {
        "authorization-ticket-token"
    });
    tab.tools.set_value("");
    tab.identity.set_label("");
    tab.status.set_label("");
    {
        let mut state = tab.state.borrow_mut();
        let selected_id = state.selected_id.clone();
        state
            .runtime_profiles
            .retain(|profile| profile["profileId"].as_str() != selected_id.as_deref());
    }
    clear_client_results(tab);
    let selected_id = tab.state.borrow().selected_id.clone();
    if selected_id.is_some() {
        start(
            tab,
            tx,
            Action::ResetAuthentication,
            "/manage/login/cancel",
            Some(json!({"profileId":selected_id})),
        );
        tab.state.borrow_mut().login_attempt = None;
    }
    tab.status.set_label("");
    update_controls(tab);
}

fn form_fields(tab: &NvwaTab) -> Vec<(&'static str, String)> {
    let mut fields: Vec<_> = [
        ("name", tab.name),
        ("productBaseUrl", tab.product),
        ("certificationBaseUrl", tab.certification),
        ("clientId", tab.client_id),
        ("tenant", tab.tenant),
        ("loginUnit", tab.unit),
        ("mcpAuthHeader", tab.auth_header),
    ]
    .into_iter()
    .map(|(key, field)| (key, field.get_value().trim().to_string()))
    .collect();
    let mcp = tab.mcp.get_value();
    let mcp = mcp.trim();
    fields.push((
        "mcpUrl",
        if mcp == "/mcp" {
            String::new()
        } else {
            mcp.to_string()
        },
    ));
    let username = if auth_mode(tab) == "browser" {
        tab.browser_username
    } else {
        tab.username
    };
    fields.push(("username", username.get_value().trim().to_string()));
    fields.push(("authMode", auth_mode(tab).to_string()));
    fields.push((
        "signatureAlgorithm",
        match tab.signature.get_selection() {
            Some(1) => "sm3",
            Some(2) => "md5",
            _ => "sha256",
        }
        .to_string(),
    ));
    fields
}

fn require_saved_form(tab: &NvwaTab, frame: &Frame, text: GuiText) -> bool {
    let state = tab.state.borrow();
    let saved = state
        .profiles
        .iter()
        .find(|profile| profile["id"].as_str() == state.selected_id.as_deref());
    let matches = saved.is_some_and(|profile| form_matches_profile(&form_fields(tab), profile));
    drop(state);
    if !matches {
        show_error(
            frame,
            tr(
                text,
                "环境或账号已修改，请先保存环境，再执行此操作。",
                "Save the changed environment or account before continuing.",
            ),
        );
    }
    matches
}

fn form_matches_profile(fields: &[(&str, String)], profile: &Value) -> bool {
    fields.iter().all(|(key, value)| {
        let default = match *key {
            "mcpAuthHeader" if profile["authMode"].as_str() == Some("password") => "Authorization",
            "mcpAuthHeader" => "authorization-ticket-token",
            "signatureAlgorithm" => "sha256",
            "authMode" => "password",
            _ => "",
        };
        let saved = profile[*key].as_str().unwrap_or(default);
        let saved = if *key == "mcpUrl" && saved == "/mcp" {
            ""
        } else {
            saved
        };
        saved == value
    })
}

pub(super) fn refresh_if_needed(
    tab: &NvwaTab,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
) {
    if tab.busy.load(Ordering::SeqCst) {
        return;
    }
    let mut state = tab.state.borrow_mut();
    let retry_interval = if state.initial_refreshes < 3 {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(30)
    };
    if state.snapshot_loaded
        || state
            .last_refresh
            .is_some_and(|last| last.elapsed() < retry_interval)
    {
        return;
    }
    state.initial_refreshes = state.initial_refreshes.saturating_add(1);
    state.last_refresh = Some(Instant::now());
    drop(state);
    start(tab, tx, Action::Refresh, "/manage/status", None);
}

fn client_body(tab: &NvwaTab, kind: &str) -> Value {
    let mut result = json!({"profileId":tab.state.borrow().selected_id,"clientKind":kind});
    let path = tab.client.path.get_value();
    if !path.trim().is_empty() && kind != "tiangong" {
        result["overridePath"] = json!(path.trim());
    }
    result
}

fn show_connection_menu(
    tab: &NvwaTab,
    frame: &Frame,
    text: GuiText,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
) {
    if tab.busy.load(Ordering::SeqCst) || tab.state.borrow().selected_id.is_none() {
        return;
    }
    const DISCONNECT: i32 = 15_201;
    const RESTORE: i32 = 15_202;
    const DELETE_ENVIRONMENT: i32 = 15_203;
    let mut menu = Menu::builder()
        .append_item(
            DISCONNECT,
            tr(text, "断开当前客户端…", "Disconnect this client…"),
            tr(
                text,
                "移除当前环境在所选客户端中的受管连接，保留环境和其他客户端。",
                "Remove this environment's managed entry from the selected client; keep the environment and other clients.",
            ),
        )
        .append_item(
            RESTORE,
            tr(text, "撤销上次连接更改…", "Undo last connection change…"),
            tr(
                text,
                "预览并恢复所选客户端最近一次连接更改前的备份。",
                "Preview and restore the selected client's backup from before the last connection change.",
            ),
        )
        .append_separator()
        .append_item(
            DELETE_ENVIRONMENT,
            tr(text, "删除当前环境…", "Delete this environment…"),
            tr(
                text,
                "删除 Hub 中当前环境的地址、已保存登录资料和授权；须先断开各客户端。",
                "Delete this environment's URLs, saved credentials and access from Hub; disconnect its clients first.",
            ),
        )
        .build();
    let can_restore = tab
        .state
        .borrow()
        .clients
        .get(selected_client_kind(tab))
        .and_then(|value| value["backupRef"].as_str())
        .is_some();
    menu.enable_item(RESTORE, can_restore);
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    menu.on_selected(move |event| {
        if t.busy.load(Ordering::SeqCst) || t.state.borrow().selected_id.is_none() {
            return;
        }
        let operation = match event.get_id() {
            DISCONNECT => "remove",
            RESTORE => "restore",
            DELETE_ENVIRONMENT => {
                let state = t.state.borrow();
                let name = state
                    .profiles
                    .iter()
                    .find(|profile| profile["id"].as_str() == state.selected_id.as_deref())
                    .and_then(|profile| profile["name"].as_str())
                    .unwrap_or("NVWA")
                    .to_string();
                drop(state);
                let message = match text.locale {
                    GuiLocale::ZhCn => format!(
                        "删除环境“{name}”？\n\n这会清理 Hub 中该环境的地址、已保存登录资料和全部接入授权。\n\n请先切换到各已接入客户端，通过“⋯ → 断开当前客户端”移除受管连接。"
                    ),
                    GuiLocale::EnUs => format!(
                        "Delete environment “{name}”?\n\nThis removes its URLs, saved credentials and all local access grants from Hub.\n\nFirst select each connected client and use ⋯ → Disconnect this client to remove its managed entry."
                    ),
                };
                if confirm(&f, &message) {
                    let state = t.state.borrow();
                    let body = json!({
                        "profileId": state.selected_id,
                        "expectedRevision": config_revision(&state.config),
                    });
                    drop(state);
                    start(&t, &sender, Action::Delete, "/manage/profile/delete", Some(body));
                }
                return;
            }
            _ => return,
        };
        if !require_saved_form(&t, &f, text) {
            return;
        }
        let kind = selected_client_kind(&t);
        let mut body = client_body(&t, kind);
        body["operation"] = json!(operation);
        if let Some(backup) = t
            .state
            .borrow()
            .clients
            .get(kind)
            .and_then(|value| value.get("backupRef"))
        {
            body["backupRef"] = backup.clone();
        }
        start(
            &t,
            &sender,
            Action::Preview {
                client: kind.to_string(),
                operation: operation.to_string(),
                body: body.clone(),
                check_after: false,
            },
            "/manage/client/preview",
            Some(body),
        );
    });
    let position = tab
        .client
        .manage
        .client_to_screen(Point::new(0, tab.client.manage.get_size().height));
    tab.client.manage.popup_menu(&mut menu, Some(position));
}

fn confirm(frame: &Frame, message: &str) -> bool {
    super::feedback::confirm(frame, "Dumpling-MCP", message, "继续")
}

fn configuration_help(text: GuiText) -> &'static str {
    tr(
        text,
        concat!(
            "1. 填写环境\n填写环境名称和 NVWA 服务地址。Codex、WorkBuddy、天工中的 MCP 都使用已保存的环境名称，支持 1～64 位英文字母、数字、- 或 _，例如 MCP196；各客户端的旧名称可通过“连接并检测”并确认更新。MCP 默认 /mcp，可在高级设置改路径或完整地址；路径跟随服务地址的部署前缀。认证服务单独部署时，再填写独立认证地址。\n\n",
            "2. 选择登录方式\n账号密码：填写账号和密码，收到验证码要求后再填写验证码；改密或图形验证码按服务提示处理。密码在页面直接显示。\n",
            "认证服务连接：在认证服务管理添加应用服务，获取ClientID和ClientSecret。填写这两个值，ClientSecret 在页面直接显示。默认勾选共享应用，填写要代表的账号；应用为该账号申请连接，不验证个人密码。没有应用管理权限时，请向管理员获取应用资料。\n\n",
            "3. 浏览器授权（可选）\n取消共享应用勾选后，用同一组 ClientID/ClientSecret，在打开的产品页面完成个人登录和授权。应用还须允许 Hub 的本机回调地址。高级“限定授权账号”可留空，填写时须与实际授权账号一致。旧浏览器环境保持此方式。\n\n",
            "4. 登录后确认账号与租户\n先保存环境，再点击“登录 / 授权”。成功后显示服务实际返回的账号和租户；__default_tenant__ 表示默认租户。高级指定租户通常可留空。登录未成功时，先处理登录提示，再检测 MCP。\n\n",
            "5. 连接客户端\n选择 ChatGPT(Codex)、WorkBuddy 或天工 Work，点击“连接并检测”。Hub 自动查找并核对配置，需要接入或修复时显示一次确认，保存后继续检测并显示工具数量与用途概览。已有连接正常时直接检测。右侧“⋯”仅保留连接管理：断开当前客户端只移除该端连接，保留环境及其他客户端；撤销上次连接更改按备份恢复；删除当前环境会清理 Hub 保存的整套环境资料，须先断开各客户端。自定义配置位置在高级设置中，通常无需填写。保存后刷新客户端或重开会话，并按客户端提示确认使用；天工从下一轮对话使用。工具是否能执行，在客户端中确认。\n\n",
            "共享应用勾选切换会保留 ClientID、ClientSecret 和保存密钥选项，取消当前登录并暂时停用旧连接；保存后重新登录，认证目标和真实身份一致时恢复原连接。主认证下拉切换会清空旧方式的输入。\n",
            "“保存应用密钥”在新环境默认关闭；勾选后点击“保存环境”就保存 ClientSecret，无需先登录成功。已保存环境会恢复勾选，并在选择、刷新或重新打开时将 ClientSecret 明文显示在输入框中；填写新值并保存可替换。环境设置改变时需要重新填写。取消勾选并保存环境会删除保存的应用密钥。密钥仍由系统保护存储，不写入普通配置。\n",
            "“记住密码”仍只在登录成功后保存密码，之后选择、刷新或重新打开环境会明文回显。登录成功后保留本次输入的密码或 ClientSecret；未勾选保存的值只保留在本次页面中。HTTP 401 表示服务拒绝了该步请求；请按提示中的失败步骤核对，保存密钥不代表登录成功。\n\n",
            "6. 有效期与自动重新认证\nClientID + ClientSecret 取票换得的凭证默认固定 24 小时，调用不会延长；认证服务应用的 tokenValidTime 可覆盖。服务返回准确时间时显示到期时间（UTC）和剩余时间，没有返回时只说明默认规则。账号密码登录默认闲置 30 分钟超时，正常认证访问可延长；连续使用不会仅因登录已过 30 分钟被 Hub 判定到期，实际是否有效由服务判断。\n",
            "账号密码需勾选“记住密码”并成功登录；共享应用需勾选“保存应用密钥”并保存环境，再成功登录。认证过期后，Hub 会在下次检测、接入或客户端请求前使用已保存资料获取新认证，保持原账号与租户及已有客户端连接。无需一直打开此页，也不按固定周期提前登录。\n",
            "客户端连接的是 Hub，保存的本机接入凭据与上游会过期的登录 token 分开。手动重新登录或取消登录时已有接入暂时停用；服务确认同一账号身份和租户后自动恢复。Hub 正常重启后由客户端重新建立连接会话。首次接入、断开或撤销后重新接入、认证目标或真实身份变化、接入凭据无法安全恢复时，“连接并检测”会显示确认；只改环境名称或更新同一认证目标的保存密钥不应撤销原接入。\n",
            "已经发送的操作如果收到认证拒绝，不会自动重新执行；下次请求才使用新认证。重新认证失败后至少间隔一分钟才自动重试，手动登录可立即进行。验证码、双因子、改密或个人浏览器授权仍需手动完成。"
        ),
        concat!(
            "1. Environment\nEnter a name and the NVWA service URL. Codex, WorkBuddy and TianGong use the saved environment name for MCP: 1–64 letters, numbers, - or _, for example MCP196. Select Connect and check and confirm in each client to update a previous name. MCP defaults to /mcp; edit its path or full URL in advanced settings. Paths preserve the service deployment prefix. Set a separate authentication URL only if deployed separately.\n\n",
            "2. Sign-in method\nAccount and password: enter both, then complete any requested verification. Follow service instructions for password changes or captcha. Password input is visible.\n",
            "Authentication service connection: add an application service in authentication service management to obtain ClientID and ClientSecret. Enter both; ClientSecret input is visible. Shared application delegation is checked by default; enter the account to represent. This requests access for that account without validating its password. Obtain application settings from your administrator if needed.\n\n",
            "3. Browser authorization (optional)\nUncheck shared delegation and use the same ClientID/ClientSecret to sign in and authorize on the product page. The application must allow Hub's local callback. A restricted username in advanced settings is optional and must match the authorized account if set. Existing browser profiles keep this method.\n\n",
            "4. Check account and tenant\nSave, then select Sign in. After success, Hub shows the returned account and tenant. __default_tenant__ is the default tenant. Specific tenant can usually be left blank. Resolve sign-in errors before checking MCP.\n\n",
            "5. Connect a client\nChoose ChatGPT(Codex), WorkBuddy or TianGong Work, then select Connect and check. Hub locates and checks settings automatically and opens one confirmation if setup or repair is needed. After saving it checks the connection and shows tool counts and purposes. Working connections are checked directly. The ⋯ menu contains maintenance only: disconnect this client keeps the environment and other clients; undo restores the last connection backup; delete this environment removes its saved details from Hub after its clients are disconnected. Custom settings paths are in advanced settings and can usually be left blank. Refresh or reopen the client session and follow its confirmation prompts; TianGong uses the connection from the next turn. Verify actual tool use in the client.\n\n",
            "Toggling shared delegation keeps ClientID, ClientSecret and Save application secret, and cancels sign-in while suspending existing access. Save and sign in again; the same authentication target and verified identity restore the connection. Switching the main sign-in dropdown clears the previous inputs.\n",
            "Save application secret defaults to off for new environments. Check it and select Save environment to store ClientSecret immediately, even before successful sign-in. Selecting, refreshing or reopening a saved environment restores the checkbox and shows its saved ClientSecret as plain text in the input. Enter a new value and save to replace it. Changed environment settings require entering it again. Uncheck and save to remove the stored application secret. System-protected storage is still used; the secret is not written to ordinary configuration.\n",
            "Remember password still stores the password only after successful sign-in. Selecting, refreshing or reopening the environment then shows the saved password as plain text. Successful sign-in keeps the current password or ClientSecret in the input; unsaved values remain only in the current form. HTTP 401 means the service rejected that request step; check the named step. Saving a secret does not sign you in.\n\n",
            "6. Validity and automatic renewal\nClientID + ClientSecret ticket tokens default to a fixed 24 hours; calls do not extend them. The application's tokenValidTime can override this. Hub shows the expiry time (UTC) and remaining time when supplied by the service; otherwise it explains the default rule. Password sign-in defaults to a 30-minute idle timeout, extended by normal authenticated access. Hub does not expire an actively used session merely because 30 minutes have passed since sign-in; the service decides whether it is valid.\n",
            "For password sign-in, enable Remember password and sign in successfully. For a shared application, enable Save application secret, save the environment and sign in successfully. After expiry, Hub uses the saved credentials before the next check, connection or client request, preserving the account, tenant and existing client connections. This page need not stay open; Hub does not sign in early on a timer.\n",
            "Clients connect to Hub using a local access key, separate from the expiring upstream login token. Manual sign-in or cancellation suspends existing access; it restores when the service verifies the same account identity and tenant. After a normal Hub restart, clients rebuild their sessions. Connect and check asks for confirmation at first setup, reconnection after disconnecting or undoing a change, authentication target or identity changes, or unrecoverable local credentials. Renaming an environment or updating saved credentials for the same authentication target should not revoke access.\n",
            "An operation already sent when authentication is rejected is not replayed; the next request can use renewed credentials. Failed automatic renewal waits at least one minute before another attempt; manual sign-in is available immediately. Captcha, two-factor verification, password changes and personal browser authorization require manual completion."
        ),
    )
}

pub(super) fn apply_result(
    tab: &NvwaTab,
    frame: &Frame,
    text: GuiText,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    result: NvwaActionResult,
) {
    if result.operation != tab.operation.load(Ordering::SeqCst) {
        return;
    }
    tab.busy.store(false, Ordering::SeqCst);
    tab.loading.finish();
    tab.state.borrow_mut().active_login = false;
    if !matches!(
        result.action,
        Action::Refresh
            | Action::RefreshAfterLogin
            | Action::RefreshSaved
            | Action::Save
            | Action::Delete
    ) && result.profile_id != tab.state.borrow().selected_id
    {
        update_controls(tab);
        return;
    }
    if result
        .action
        .client_kind()
        .is_some_and(|kind| kind != selected_client_kind(tab))
    {
        update_controls(tab);
        return;
    }
    let value = match result.outcome {
        Ok(value) => value,
        Err(error) => {
            let message = display_message(text, &error);
            if result.action.client_kind().is_none() {
                let status = if matches!(result.action, Action::Refresh)
                    && !tab.state.borrow().snapshot_loaded
                {
                    format!(
                        "{message}\n{}",
                        tr(
                            text,
                            "Hub 会自动重试读取状态。",
                            "Hub will retry loading the status automatically.",
                        )
                    )
                } else {
                    message.clone()
                };
                tab.status.set_label(&status);
                tab.status.wrap(920);
            }
            match &result.action {
                Action::DetectClient(kind) | Action::PrepareClient { client: kind, .. } => {
                    show_client_failure(
                        tab,
                        kind,
                        tr(text, "连接未完成", "Connection incomplete"),
                        &message,
                    )
                }
                Action::Preview { client, .. } => show_client_failure(
                    tab,
                    client,
                    tr(
                        text,
                        "客户端操作预览未完成",
                        "Client operation preview did not complete",
                    ),
                    &message,
                ),
                Action::Mutate { client: kind, .. } => show_client_failure(
                    tab,
                    kind,
                    tr(
                        text,
                        "客户端连接修改未完成",
                        "Client connection change did not complete",
                    ),
                    &message,
                ),
                _ => {}
            }
            if matches!(result.action, Action::Login) {
                tab.identity.set_label(tr(
                    text,
                    "登录未完成。成功后这里会显示服务返回的账号和租户。",
                    "Sign-in is incomplete. The returned account and tenant will appear here after success.",
                ));
                set_client_feedback(
                    tab,
                    selected_client_kind(tab),
                    tr(
                        text,
                        "登录未完成，请先处理上方登录提示。已保留的接入会在同身份登录成功后恢复；首次接入仍需预览确认一次。",
                        "Sign-in is incomplete. Resolve the sign-in message above first. Retained access restores after successful sign-in with the same identity; first setup still requires one connection preview.",
                    ),
                );
            }
            update_controls(tab);
            return;
        }
    };
    sync_runtime_profile(tab, &value);
    match result.action {
        Action::Refresh => apply_snapshot(tab, text, value, tx),
        Action::RefreshAfterLogin => {
            let fields = form_fields(tab);
            let profile_id = tab.state.borrow().selected_id.clone();
            let unchanged = value
                .pointer("/config/profiles")
                .and_then(Value::as_array)
                .and_then(|profiles| {
                    profiles
                        .iter()
                        .find(|profile| profile["id"].as_str() == profile_id.as_deref())
                })
                .is_some_and(|profile| form_matches_profile(&fields, profile));
            let password = unchanged.then(|| tab.password.get_value());
            let secret = unchanged.then(|| tab.client_secret.get_value());
            apply_snapshot(tab, text, value, tx);
            if let Some(password) = password.filter(|value| !value.is_empty()) {
                tab.password.set_value(&password);
            }
            if let Some(secret) = secret.filter(|value| !value.is_empty()) {
                tab.client_secret.set_value(&secret);
            }
            let kind = selected_client_kind(tab);
            set_client_feedback(tab, kind, &default_client_feedback(tab, kind));
        }
        Action::RefreshSaved => {
            let fields = form_fields(tab);
            let selected_id = tab.state.borrow().selected_id.clone();
            let expected_profile = tab.state.borrow_mut().saved_profile_snapshot.take();
            let unchanged = value
                .pointer("/config/profiles")
                .and_then(Value::as_array)
                .and_then(|profiles| {
                    profiles
                        .iter()
                        .find(|profile| profile["id"].as_str() == selected_id.as_deref())
                })
                .is_some_and(|profile| {
                    expected_profile.as_ref() == Some(profile)
                        && form_matches_profile(&fields, profile)
                });
            let password = unchanged.then(|| tab.password.get_value());
            let secret = unchanged.then(|| tab.client_secret.get_value());
            let remember_password = unchanged && tab.remember_password.is_checked();
            let remember_secret = unchanged.then(|| tab.remember_secret.is_checked());
            apply_snapshot(tab, text, value, tx);
            if let Some(password) = password {
                tab.password.set_value(&password);
            }
            if let Some(secret) = secret {
                tab.client_secret.set_value(&secret);
            }
            tab.remember_password.set_value(remember_password);
            if let Some(remember_secret) = remember_secret {
                tab.remember_secret.set_value(remember_secret);
            }
        }
        Action::LoadCredential { fingerprint } => {
            let profile = {
                let state = tab.state.borrow();
                state
                    .profiles
                    .iter()
                    .find(|profile| profile["id"].as_str() == state.selected_id.as_deref())
                    .cloned()
            };
            if let Some(profile) = profile
                && saved_profile_fingerprint(&profile).as_deref() == Some(fingerprint.as_str())
                && value["profileFingerprint"].as_str() == Some(fingerprint.as_str())
                && value["profileId"].as_str() == profile["id"].as_str()
                && form_matches_profile(&form_fields(tab), &profile)
                && let Some(credential) = value["credentialValue"].as_str()
            {
                let field = if matches!(profile["authMode"].as_str(), None | Some("password")) {
                    ("password", tab.password)
                } else {
                    ("clientSecret", tab.client_secret)
                };
                if value["credentialKind"].as_str() == Some(field.0)
                    && field.1.get_value().is_empty()
                {
                    field.1.set_value(credential);
                }
            }
        }
        Action::Save => {
            tab.state.borrow_mut().saved_profile_snapshot = value.get("profile").cloned();
            if let Some(id) = value
                .pointer("/profile/id")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
            {
                tab.state.borrow_mut().selected_id = Some(id.to_string());
            }
            tab.status
                .set_label(tr(text, "环境已保存", "Environment saved"));
            start(tab, tx, Action::RefreshSaved, "/manage/status", None);
        }
        Action::Delete => {
            tab.state.borrow_mut().selected_id = None;
            start(tab, tx, Action::Refresh, "/manage/status", None);
        }
        Action::Login => {
            tab.state.borrow_mut().twofactor_session = value
                .get("twofactorSessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            tab.factor_code.set_value("");
            let detail = value
                .get("detail")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("认证步骤已返回");
            tab.status.set_label(&display_message(text, detail));
            if let Some(url) = value.get("authorizeUrl").and_then(Value::as_str) {
                if let Err(error) = open_url_in_browser(text, url) {
                    show_error(frame, &error);
                }
            } else if value.get("state").and_then(Value::as_str) == Some("authenticated") {
                tab.factor_code.set_value("");
                tab.state.borrow_mut().twofactor_session = None;
                start(tab, tx, Action::RefreshAfterLogin, "/manage/status", None);
            }
        }
        Action::Cancel => {
            clear_login_material(tab);
            tab.tools.set_value("");
            tab.state.borrow_mut().twofactor_session = None;
            tab.state.borrow_mut().login_attempt = None;
            start(tab, tx, Action::Refresh, "/manage/status", None);
        }
        Action::ResetAuthentication => {
            tab.status.set_label("");
        }
        Action::DetectClient(kind) => {
            tab.tools.set_value(&tool_catalog_text(text, &value));
            let next_step = if kind == "tiangong" {
                if value["catalogComplete"] == false {
                    tr(
                        text,
                        "目录未完整读取，本次未更新天工工具清单。",
                        "The catalog is incomplete; TianGong's tool list was not updated.",
                    )
                } else if value["tiangongDirectoryUpdated"].as_bool() == Some(true) {
                    tr(
                        text,
                        "工具清单已更新到天工，可在下一轮对话中使用。",
                        "The tool list was updated in TianGong; use it in your next conversation turn.",
                    )
                } else {
                    tr(
                        text,
                        "工具清单尚未更新到天工，请检查天工是否已连接并重新检测。",
                        "The tool list was not updated in TianGong. Check its connection and try again.",
                    )
                }
            } else {
                tr(
                    text,
                    "请到客户端刷新或重开会话，并按提示确认使用这些工具。",
                    "Refresh or reopen a client session and follow its prompts to enable these tools.",
                )
            };
            set_client_feedback(
                tab,
                &kind,
                &format!(
                    "{} {next_step}",
                    catalog_connection_text(text, &value, true)
                ),
            );
            let mut status = tab
                .state
                .borrow()
                .clients
                .get(&kind)
                .cloned()
                .unwrap_or(Value::Null);
            let (has_access, suspended) = runtime_client_access(&value["runtimeProfile"], &kind);
            status["requiresClientAccess"] = json!(!has_access);
            status["clientAccessSuspended"] = json!(suspended);
            status["clientAccessRecoveryFailed"] =
                value["runtimeProfile"]["clientAccessRecoveryFailed"].clone();
            status["bridgeState"] = json!(if has_access && !suspended {
                if value["catalogComplete"] == false {
                    "connected_catalog_limited"
                } else {
                    "checked"
                }
            } else {
                "unknown"
            });
            if !has_access || suspended {
                set_client_feedback(tab, &kind, &default_client_feedback(tab, &kind));
            }
            tab.state.borrow_mut().clients.insert(kind, status);
        }
        Action::SendFactor => {
            tab.status.set_label(&display_message(
                text,
                value["detail"]
                    .as_str()
                    .unwrap_or("Verification code requested"),
            ));
        }
        Action::PrepareClient { client, body } => {
            if value["readyForDetection"] == true {
                if let Some(status) = value.get("status") {
                    apply_client(tab, &client, status.clone());
                }
                start(
                    tab,
                    tx,
                    Action::DetectClient(client),
                    "/manage/tools/detect",
                    Some(body),
                );
            } else {
                show_client_preview(
                    tab,
                    frame,
                    text,
                    tx,
                    client,
                    "apply".into(),
                    body,
                    &value["preview"],
                    true,
                );
            }
        }
        Action::Mutate {
            client: kind,
            check_after,
        } => {
            let value = value.get("status").cloned().unwrap_or(value);
            apply_client(tab, &kind, value);
            if let Some(body) = check_after {
                set_client_feedback(
                    tab,
                    &kind,
                    tr(
                        text,
                        "连接已保存，正在连接并检测…",
                        "Connection saved; checking the local bridge…",
                    ),
                );
                start(
                    tab,
                    tx,
                    Action::DetectClient(kind),
                    "/manage/tools/detect",
                    Some(body),
                );
            }
        }
        Action::Preview {
            client,
            operation,
            body,
            check_after,
        } => {
            show_client_preview(
                tab,
                frame,
                text,
                tx,
                client,
                operation,
                body,
                &value,
                check_after,
            );
        }
    }
    update_controls(tab);
    tab.status.wrap(920);
    tab.identity.wrap(920);
    tab.page.layout();
    tab.page.fit_inside();
}

fn preview_operation_allowed(requested: &str, returned: &str) -> bool {
    match requested {
        "apply" => matches!(returned, "apply" | "repair"),
        "remove" => returned == "remove",
        "restore" => returned == "restore",
        _ => false,
    }
}

fn show_client_preview(
    tab: &NvwaTab,
    frame: &Frame,
    text: GuiText,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
    client: String,
    requested: String,
    mut body: Value,
    value: &Value,
    check_after: bool,
) {
    let operation = value["operation"]
        .as_str()
        .unwrap_or(&requested)
        .to_string();
    if !preview_operation_allowed(&requested, &operation) {
        show_client_failure(
            tab,
            &client,
            tr(text, "接入未完成", "Connection incomplete"),
            tr(
                text,
                "预览结果不属于本次操作，请重试。",
                "The preview does not match this operation. Try again.",
            ),
        );
        return;
    }
    let fingerprint = value
        .get("targetFingerprint")
        .or_else(|| value.get("target_fingerprint"))
        .or_else(|| value.pointer("/status/targetFingerprint"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let allowed = value
        .get("canApply")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let detail = value
        .pointer("/status/detail")
        .or_else(|| value.get("detail"))
        .and_then(Value::as_str)
        .unwrap_or("无法取得客户端预览详情");
    if !allowed || fingerprint.is_empty() {
        let message = if allowed {
            tr(
                text,
                "未取得最新客户端配置，请再次点击“连接并检测”。",
                "The latest client settings were not returned. Select Connect and check again.",
            )
            .to_string()
        } else {
            display_message(text, detail)
        };
        if let Some(status) = value.get("status") {
            apply_client(tab, &client, status.clone());
        }
        show_client_failure(
            tab,
            &client,
            tr(text, "连接需要处理", "Connection needs attention"),
            &message,
        );
    } else {
        let target_path = value
            .pointer("/status/targetPath")
            .and_then(Value::as_str)
            .unwrap_or("");
        let warnings = value["warnings"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|message| display_message(text, message))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let client_name = client_title(&client);
        let environment = tab.name.get_value();
        let mcp_name = if matches!(operation.as_str(), "apply" | "repair") {
            value
                .pointer("/status/desiredServerName")
                .or_else(|| value.get("desiredServerName"))
                .and_then(Value::as_str)
        } else {
            None
        }
        .or_else(|| {
            value
                .pointer("/status/serverName")
                .or_else(|| value.get("serverName"))
                .and_then(Value::as_str)
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(tr(text, "服务未返回名称", "Name was not returned"));
        let name_change = if matches!(operation.as_str(), "apply" | "repair") {
            value
                .pointer("/status/previousServerName")
                .or_else(|| value.get("previousServerName"))
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty() && *name != mcp_name)
                .map(|previous| match (text.locale, value.pointer("/status/present").and_then(Value::as_bool)) {
                    (GuiLocale::ZhCn, Some(false)) => format!(
                        "\n历史名称：{previous}（客户端中已缺失）；确认后按上方 MCP 名称重新添加。"
                    ),
                    (GuiLocale::EnUs, Some(false)) => format!(
                        "\nPrevious name: {previous} (missing from the client); confirmation adds the MCP name above."
                    ),
                    (GuiLocale::ZhCn, _) => format!(
                        "\n当前名称：{previous}；确认后更新为上方 MCP 名称。"
                    ),
                    (GuiLocale::EnUs, _) => format!(
                        "\nCurrent name: {previous}; confirmation updates it to the MCP name above."
                    ),
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let (introduction, decision) = match operation.as_str() {
            "repair" => (
                match text.locale {
                    GuiLocale::ZhCn => {
                        format!("将备份并修复 {client_name} 中“{environment}”的工具连接。")
                    }
                    GuiLocale::EnUs => format!(
                        "Back up and repair the tool connection for '{environment}' in {client_name}."
                    ),
                },
                tr(
                    text,
                    "点击“是”修复并自动检测；点击“否”取消。",
                    "Select Yes to repair and check automatically, or No to cancel.",
                ),
            ),
            "apply" => (
                match text.locale {
                    GuiLocale::ZhCn => {
                        format!("将把“{environment}”的 NVWA 工具连接添加到 {client_name}。")
                    }
                    GuiLocale::EnUs => {
                        format!("Add the NVWA tools for '{environment}' to {client_name}.")
                    }
                },
                tr(
                    text,
                    "点击“是”保存并自动检测；点击“否”取消。",
                    "Select Yes to save and check automatically, or No to cancel.",
                ),
            ),
            "remove" => (
                match text.locale {
                    GuiLocale::ZhCn => format!(
                        "将断开 {client_name} 中“{environment}”的 NVWA 工具连接，并撤销该端接入授权。\n环境资料和其他客户端连接保留。"
                    ),
                    GuiLocale::EnUs => format!(
                        "Disconnect the NVWA connection for '{environment}' in {client_name} and revoke that client's access.\nKeep the environment and other client connections."
                    ),
                },
                tr(
                    text,
                    "点击“是”断开连接；点击“否”取消。",
                    "Select Yes to disconnect, or No to cancel.",
                ),
            ),
            "restore" => (
                match text.locale {
                    GuiLocale::ZhCn => format!(
                        "将把 {client_name} 中“{environment}”的 NVWA 连接设置恢复到 Hub 修改前的备份，恢复后需要重新接入。"
                    ),
                    GuiLocale::EnUs => format!(
                        "Restore the backed-up settings from before Hub changed the NVWA connection for '{environment}' in {client_name}. Reconnect afterwards."
                    ),
                },
                tr(
                    text,
                    "点击“是”恢复备份；点击“否”取消。",
                    "Select Yes to restore the backup, or No to cancel.",
                ),
            ),
            _ => return,
        };
        let target = if target_path.is_empty() {
            tr(
                text,
                "保存位置：天工中的 MCP 连接设置",
                "Save location: TianGong MCP connection settings",
            )
            .to_string()
        } else {
            format!(
                "{}\n{target_path}",
                tr(text, "配置文件：", "Configuration file:")
            )
        };
        let detail = display_message(text, detail);
        let message = format!(
            "{introduction}\n\n{}{mcp_name}{name_change}\n{target}\n\n{detail}\n{warnings}\n\n{decision}",
            tr(text, "MCP 名称：", "MCP name: "),
        );
        if confirm(frame, &message) {
            body["expectedFingerprint"] = json!(fingerprint);
            if let Some(backup) = value
                .pointer("/status/backupRef")
                .or_else(|| value.get("backupRef"))
            {
                body["backupRef"] = backup.clone();
            }
            start(
                tab,
                tx,
                Action::Mutate {
                    client,
                    check_after: check_after.then(|| body.clone()),
                },
                &format!("/manage/client/{operation}"),
                Some(body),
            );
        } else {
            set_client_feedback(
                tab,
                &client,
                tr(text, "已取消配置修改。", "Settings change cancelled."),
            );
        }
    }
}

fn show_client_failure(tab: &NvwaTab, kind: &str, title: &str, message: &str) {
    set_client_feedback(tab, kind, &format!("{title}\n{message}"));
    let mut state = tab.state.borrow_mut();
    if let Some(status) = state.clients.get_mut(kind) {
        status["bridgeState"] = json!("unknown");
        status["bridgeCheckedAtMs"] = Value::Null;
    }
}

fn apply_client(tab: &NvwaTab, kind: &str, value: Value) {
    render_client_status(tab, kind, value, true);
}

fn render_client_status(tab: &NvwaTab, kind: &str, value: Value, update_path: bool) {
    let detail = value
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("状态已读取");
    let path = value
        .get("targetPath")
        .and_then(Value::as_str)
        .unwrap_or("");
    if update_path && !path.is_empty() {
        tab.state
            .borrow_mut()
            .client_paths
            .insert(kind.to_string(), path.to_string());
        if selected_client_kind(tab) == kind {
            tab.client.path.set_value(path);
        }
    }
    let load = value.get("loadState").and_then(Value::as_str).unwrap_or("");
    let bridge = value
        .get("bridgeState")
        .and_then(Value::as_str)
        .unwrap_or("");
    let requires_access = value["requiresClientAccess"].as_bool() == Some(true)
        && value["owned"].as_bool() == Some(true);
    let access_suspended = value["clientAccessSuspended"].as_bool() == Some(true);
    let recovery_failed = value["clientAccessRecoveryFailed"].as_bool() == Some(true);
    let missing_managed = value["available"].as_bool() == Some(true)
        && value["present"].as_bool() == Some(false)
        && value["owned"].as_bool() == Some(true)
        && value["modified"].as_bool() == Some(false);
    let load = if value["available"].as_bool() == Some(false) {
        tr(
            tab.text,
            "下一步：先打开并连接客户端，再点击“连接并检测”。",
            "Next: open and connect the client, then select Connect and check.",
        )
    } else if value["repairable"].as_bool() == Some(true)
        && value["modified"].as_bool() == Some(true)
    {
        tr(
            tab.text,
            "下一步：点击“连接并检测”，确认后备份并修复，再自动检测。",
            "Next: select Connect and check to back up, repair and check after confirmation.",
        )
    } else if value["modified"].as_bool() == Some(true)
        || value["nameConflict"].as_bool() == Some(true)
    {
        tr(
            tab.text,
            "下一步：按上方提示核对连接，再点击“连接并检测”自动复查。",
            "Next: review the connection as described above, then select Connect and check to verify it again.",
        )
    } else if access_suspended {
        tr(
            tab.text,
            "下一步：完成原账号和租户的登录，原接入授权会恢复；再点击“连接并检测”即可自动核对和补齐连接。",
            "Next: complete sign-in with the original account and tenant to restore access, then select Connect and check to verify or repair the connection.",
        )
    } else if recovery_failed && requires_access {
        tr(
            tab.text,
            "下一步：之前的本机接入授权未能安全恢复。请重新登录，再“连接并检测”并确认一次；普通认证到期不会要求重复接入。",
            "Next: the previous local access grant could not be safely restored. Sign in, then preview and confirm once. Normal authentication expiry does not require reconnecting.",
        )
    } else if missing_managed {
        tr(
            tab.text,
            "下一步：登录成功后点击“连接并检测”，确认重新添加后自动继续检测。",
            "Next: sign in and select Connect and check. Confirm adding the missing connection to continue automatically.",
        )
    } else if requires_access {
        tr(
            tab.text,
            "下一步：点击“连接并检测”，会直接显示接入确认并在保存后自动继续检测。",
            "Next: select Connect and check. Setup opens automatically and detection continues after you confirm.",
        )
    } else {
        match load {
            "pending_client_refresh" => tr(
                tab.text,
                "下一步：刷新客户端或重开会话，并按提示确认使用。",
                "Next: refresh the client or reopen a session and follow its confirmation prompts.",
            ),
            "next_turn" => tr(
                tab.text,
                "下一步：在天工下一轮对话中查看工具。",
                "Next: check the tools in the next TianGong conversation turn.",
            ),
            "not_configured" => tr(
                tab.text,
                "下一步：登录成功后点击“连接并检测”。",
                "Next: sign in, then select Connect and check.",
            ),
            "unavailable" => tr(
                tab.text,
                "下一步：先打开并连接客户端，再点击“连接并检测”。",
                "Next: open and connect the client, then select Connect and check.",
            ),
            _ => "",
        }
    };
    let bridge = match bridge {
        "connected_catalog_limited" => tr(
            tab.text,
            "本机桥已连通；目录未完整读取，工具总数未知。",
            "Local bridge connected; the catalog is incomplete and the tool total is unknown.",
        ),
        "checked" | "tools_discovered" => tr(
            tab.text,
            "Hub 已检测这条工具连接；客户端是否能使用，请在客户端确认。",
            "Hub checked this tool connection; confirm it is usable in the client.",
        ),
        _ => tr(
            tab.text,
            "点击“连接并检测”，自动核对设置并显示工具概览。",
            "Connect and check verifies settings automatically and shows the tool overview.",
        ),
    };
    let detail = display_message(tab.text, detail);
    let current_name = value
        .get("previousServerName")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            value
                .get("serverName")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
        });
    let desired_name = value
        .get("desiredServerName")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty());
    let mcp_name = match (current_name, desired_name) {
        (Some(current), Some(desired))
            if current != desired && value["previousServerName"].as_str().is_some() =>
        {
            format!(
                "{}{current}\n{}{desired}\n{}\n",
                if missing_managed {
                    tr(
                        tab.text,
                        "历史 MCP 名称（客户端中已缺失）：",
                        "Previous MCP name (missing from the client): ",
                    )
                } else {
                    tr(tab.text, "当前 MCP 名称：", "Current MCP name: ")
                },
                if missing_managed {
                    tr(tab.text, "重新接入 MCP 名称：", "MCP name to add again: ")
                } else {
                    tr(tab.text, "更新后 MCP 名称：", "MCP name after updating: ")
                },
                if missing_managed {
                    tr(
                        tab.text,
                        "点击“连接并检测”并确认，将按环境名称重新添加连接。",
                        "Select Connect and check and confirm to add the missing connection under the environment name.",
                    )
                } else {
                    tr(
                        tab.text,
                        "点击“连接并检测”并确认，才会将旧名称更新为环境名称。",
                        "Select Connect and check and confirm to change the old name to the environment name.",
                    )
                },
            )
        }
        (Some(name), _) => format!(
            "{}{name}\n",
            if missing_managed {
                tr(tab.text, "待重新接入 MCP 名称：", "MCP name to add again: ")
            } else if value["present"].as_bool() == Some(false) {
                tr(tab.text, "待接入 MCP 名称：", "MCP name to add: ")
            } else {
                tr(tab.text, "MCP 名称：", "MCP name: ")
            },
        ),
        _ => String::new(),
    };
    set_client_feedback(tab, kind, &format!("{mcp_name}{detail}\n{load}\n{bridge}"));
    tab.state
        .borrow_mut()
        .clients
        .insert(kind.to_string(), value);
}

fn apply_snapshot(
    tab: &NvwaTab,
    text: GuiText,
    value: Value,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
) {
    let config = value.get("config").cloned().unwrap_or(Value::Null);
    let profiles = config
        .get("profiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let old = tab.state.borrow().selected_id.clone();
    tab.profile.clear();
    tab.profile.append(tr(text, "新增环境", "New environment"));
    for profile in &profiles {
        tab.profile
            .append(profile["name"].as_str().unwrap_or("NVWA"));
    }
    let selected = profiles
        .iter()
        .position(|p| p["id"].as_str() == old.as_deref())
        .map(|i| i + 1)
        .unwrap_or(0);
    {
        let mut state = tab.state.borrow_mut();
        state.config = config;
        state.profiles = profiles;
        state.runtime_profiles = value["profiles"].as_array().cloned().unwrap_or_default();
        state.snapshot_loaded = true;
    }
    tab.profile.set_selection(selected as u32);
    select_profile(tab, tx);
    if tab.state.borrow().selected_id.is_none() {
        tab.status
            .set_label(tr(text, "后台已就绪", "Backend ready"));
    }
}

fn expiry_label(text: GuiText, expiry: Option<u64>) -> String {
    let Some(expiry) = expiry else {
        return tr(text, "服务未说明有效期", "Service did not specify validity").to_string();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if expiry <= now {
        return tr(text, "已过期", "Expired").to_string();
    }
    let minutes = (expiry - now).div_ceil(60_000);
    match text.locale {
        GuiLocale::ZhCn if minutes >= 1440 => {
            format!("约 {} 天 {} 小时", minutes / 1440, minutes % 1440 / 60)
        }
        GuiLocale::ZhCn if minutes >= 60 => {
            format!("约 {} 小时 {} 分钟", minutes / 60, minutes % 60)
        }
        GuiLocale::ZhCn => format!("约 {minutes} 分钟"),
        GuiLocale::EnUs if minutes >= 1440 => format!(
            "About {} days {} hours",
            minutes / 1440,
            minutes % 1440 / 60
        ),
        GuiLocale::EnUs if minutes >= 60 => {
            format!("About {} hours {} minutes", minutes / 60, minutes % 60)
        }
        GuiLocale::EnUs => format!("About {minutes} minutes"),
    }
}

fn token_validity_label(text: GuiText, state: &Value, personal: bool) -> String {
    let (policy, expiry) = if personal {
        (
            state["personalExpiryPolicy"].as_str(),
            state["personalExpiresAtMs"].as_u64(),
        )
    } else {
        (
            state["mcpExpiryPolicy"].as_str(),
            state["mcpExpiresAtMs"].as_u64(),
        )
    };
    if expiry == Some(0) {
        return tr(
            text,
            "服务已拒绝此凭证，请重新登录",
            "The service rejected this credential; sign in again",
        )
        .to_string();
    }
    match policy {
        Some("slidingIdle") => tr(text, "默认闲置 30 分钟超时；正常认证访问可延长，由服务判断", "Default idle timeout: 30 minutes; authenticated access extends the session, as decided by the service").to_string(),
        Some("fixed") if expiry.is_some() => {
            let expiry = expiry.unwrap();
            let seconds = expiry / 1000;
            let deadline = if seconds <= 253_402_300_799 {
                crate::codex_app_config::format_rfc3339_utc(seconds)
                    .replace('T', " ")
                    .replace('Z', " UTC")
            } else {
                tr(text, "时间超出可显示范围", "Time is outside the display range").to_string()
            };
            format!("{} {deadline}（{}）；{}", tr(text, "到期：", "Expires:"), expiry_label(text, Some(expiry)), tr(text, "固定到期，调用不延长", "Fixed expiry; calls do not extend it"))
        },
        Some("fixed") => tr(text, "固定有效期默认 24 小时，应用可覆盖；服务未返回准确到期时间", "Fixed validity defaults to 24 hours, configurable by the application; exact expiry was not returned").to_string(),
        _ => expiry_label(text, expiry),
    }
}

fn show_runtime_status(tab: &NvwaTab) {
    render_runtime_status(tab, true);
}

fn render_runtime_status(tab: &NvwaTab, update_detail: bool) {
    let selection = tab.state.borrow();
    let Some(state) = selection
        .runtime_profiles
        .iter()
        .find(|p| p["profileId"].as_str() == selection.selected_id.as_deref())
    else {
        return;
    };
    let person = state
        .pointer("/identity/username")
        .and_then(Value::as_str)
        .unwrap_or("");
    let tenant = state
        .pointer("/identity/tenantId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let can_renew = selection.profiles.iter().any(|profile| {
        profile["id"].as_str() == selection.selected_id.as_deref()
            && matches!(
                profile["authMode"].as_str(),
                Some("password" | "application")
            )
            && profile["credentialSecretRef"]
                .as_str()
                .is_some_and(|reference| !reference.is_empty())
    });
    let state_label = match state["state"].as_str() {
        Some("authenticated") => tr(tab.text, "已登录", "Signed in"),
        Some("expired" | "personal_expired") if can_renew => tr(
            tab.text,
            "认证已过期，下次使用时尝试自动重新认证",
            "Authentication expired; automatic renewal on next use",
        ),
        Some("expired") => tr(
            tab.text,
            "工具连接授权已过期，请重新登录",
            "Tool authorization expired; sign in again",
        ),
        Some("personal_expired") => tr(
            tab.text,
            "登录已过期，请重新登录",
            "Sign-in expired; sign in again",
        ),
        Some("identity_verification_pending") => tr(
            tab.text,
            "正在确认之前的登录是否有效",
            "Verifying the saved sign-in",
        ),
        Some("login_pending") => tr(
            tab.text,
            "登录尚未完成，已有接入暂时停用",
            "Sign-in pending; existing access temporarily suspended",
        ),
        Some("recovery_failed") => tr(
            tab.text,
            "之前的本机接入授权未能恢复，请重新登录并接入一次",
            "Previous local access could not be restored; sign in and reconnect once",
        ),
        _ => tr(tab.text, "未登录", "Not signed in"),
    };
    let tenant = if tenant == "__default_tenant__" {
        tr(
            tab.text,
            "默认租户（__default_tenant__）",
            "Default tenant (__default_tenant__)",
        )
    } else {
        tenant
    };
    let personal =
        if state["personalTokenPresent"].as_bool() == Some(false) || state["identity"].is_null() {
            "无个人会话".to_string()
        } else {
            token_validity_label(tab.text, state, true)
        };
    let mcp = if state["identity"].is_null() {
        "未取得".to_string()
    } else {
        token_validity_label(tab.text, state, false)
    };
    if state["identity"].is_null() {
        tab.identity.set_label(if state["state"].as_str() == Some("login_pending") {
            tr(
                tab.text,
                "登录尚未完成。成功后这里会显示服务返回的账号和租户；同身份的已有接入会恢复。",
                "Sign-in is pending. The returned account and tenant will appear after success; retained connections are restored for the same identity.",
            )
        } else if state["clientAccessRecoveryFailed"].as_bool() == Some(true) {
            tr(
                tab.text,
                "已保存的客户端接入信息无法安全恢复。请重新登录后预览确认一次；不是普通认证到期。",
                "Saved client access could not be safely restored. Sign in and confirm the connection once; this is not normal authentication expiry.",
            )
        } else {
            tr(
                tab.text,
                "未登录。成功后这里会显示服务返回的账号和租户。",
                "Not signed in. The returned account and tenant will appear here after success.",
            )
        });
    } else {
        let account_label = tr(tab.text, "当前账号", "Account");
        let tenant_label = tr(tab.text, "服务返回的租户", "Returned tenant");
        let personal_label = tr(tab.text, "密码登录有效期", "Password sign-in validity");
        let mcp_label = tr(tab.text, "工具连接有效期", "Tool connection validity");
        let personal = if state["personalTokenPresent"].as_bool() == Some(false) {
            tr(
                tab.text,
                "此方式不使用密码登录",
                "This method does not use password sign-in",
            )
            .to_string()
        } else {
            personal
        };
        let validity = if state["personalTokenPresent"].as_bool() == Some(true)
            && state["mcpExpiryPolicy"].as_str() == Some("slidingIdle")
            && state["personalExpiryPolicy"].as_str() == Some("slidingIdle")
            && personal == mcp
        {
            format!("{personal_label} / {mcp_label}：{personal}")
        } else {
            format!("{personal_label}：{personal}\n{mcp_label}：{mcp}")
        };
        tab.identity.set_label(&format!(
            "{state_label}\n{account_label}：{person}    {tenant_label}：{tenant}\n{validity}"
        ));
        tab.identity.wrap(920);
    }
    if update_detail {
        tab.status.set_label(&display_message(
            tab.text,
            state["detail"].as_str().unwrap_or("Login required"),
        ));
    }
}

fn saved_profile_fingerprint(profile: &Value) -> Option<String> {
    serde_json::from_value::<crate::nvwa::types::NvwaProfile>(profile.clone())
        .ok()
        .map(|profile| crate::nvwa::profile_fingerprint(&profile))
}

fn select_profile(tab: &NvwaTab, tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>) {
    let index = tab.profile.get_selection().unwrap_or(0) as usize;
    let profile = if index > 0 {
        tab.state.borrow().profiles.get(index - 1).cloned()
    } else {
        None
    };
    {
        let mut state = tab.state.borrow_mut();
        state.selected_id = profile
            .as_ref()
            .and_then(|p| p["id"].as_str())
            .map(str::to_string);
        state.twofactor_session = None;
        state.login_attempt = None;
    }
    let profile = profile.unwrap_or(Value::Null);
    for (field, key) in [
        (tab.name, "name"),
        (tab.product, "productBaseUrl"),
        (tab.certification, "certificationBaseUrl"),
        (tab.username, "username"),
        (tab.browser_username, "username"),
        (tab.client_id, "clientId"),
        (tab.tenant, "tenant"),
        (tab.unit, "loginUnit"),
        (tab.auth_header, "mcpAuthHeader"),
    ] {
        field.set_value(profile[key].as_str().unwrap_or(""));
    }
    let mcp = profile["mcpUrl"].as_str().unwrap_or("");
    tab.mcp.set_value(if mcp.is_empty() { "/mcp" } else { mcp });
    let mode = match profile["authMode"].as_str() {
        Some("browser" | "application") => 1,
        _ => 0,
    };
    tab.mode.set_selection(mode);
    tab.application
        .set_value(profile["authMode"].as_str() != Some("browser"));
    // Loading a saved browser profile must not change its authentication mode.
    tab.advanced.collapse(true);
    if tab.auth_header.get_value().is_empty() {
        tab.auth_header.set_value(if mode == 0 {
            "Authorization"
        } else {
            "authorization-ticket-token"
        });
    }
    tab.signature
        .set_selection(match profile["signatureAlgorithm"].as_str() {
            Some("sm3") => 1,
            Some("md5") => 2,
            _ => 0,
        });
    clear_login_material(tab);
    let credential_saved = profile["credentialSecretRef"]
        .as_str()
        .is_some_and(|s| !s.is_empty());
    // Loading/refreshing a saved password profile must preserve the user's
    // remember choice, otherwise the next sign-in silently deletes it and
    // leaves Hub unable to renew authentication on behalf of its clients.
    tab.remember_password
        .set_value(mode == 0 && credential_saved);
    tab.remember_secret.set_value(mode == 1 && credential_saved);
    tab.tools.set_value("");
    tab.identity.set_label("未读取当前授权状态");
    clear_client_results(tab);
    let kind = selected_client_kind(tab);
    set_client_feedback(tab, kind, &default_client_feedback(tab, kind));
    show_runtime_status(tab);
    update_controls(tab);
    if credential_saved && let Some(fingerprint) = saved_profile_fingerprint(&profile) {
        start(
            tab,
            tx,
            Action::LoadCredential {
                fingerprint: fingerprint.clone(),
            },
            "/manage/profile/credential/read",
            Some(json!({"profileId":profile["id"],"expectedProfileFingerprint":fingerprint})),
        );
    }
}

fn update_secret_status(tab: &NvwaTab) {
    let state = tab.state.borrow();
    let saved = state
        .profiles
        .iter()
        .find(|p| p["id"].as_str() == state.selected_id.as_deref());
    let stored = saved.is_some_and(|p| {
        matches!(p["authMode"].as_str(), Some("browser" | "application"))
            && p["credentialSecretRef"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
    });
    let same_environment = saved.is_some_and(|p| form_matches_profile(&form_fields(tab), p));
    let message = if stored && !tab.remember_secret.is_checked() {
        tr(
            tab.text,
            "应用密钥已保存。取消勾选后点击“保存环境”会删除保存的密钥。",
            "Application secret is saved. Uncheck and save the environment to remove it.",
        )
    } else if stored && !same_environment {
        tr(
            tab.text,
            "原环境已保存密钥；设置已改变，请重新填写 ClientSecret 并保存。",
            "The original environment has a saved secret. Settings changed; enter ClientSecret again and save.",
        )
    } else if stored {
        tr(
            tab.text,
            "应用密钥已保存。输入框留空可继续使用；填写新值并保存可替换。",
            "Application secret is saved. Leave the input blank to reuse it, or enter a new value and save to replace it.",
        )
    } else if tab.remember_secret.is_checked() {
        tr(
            tab.text,
            "尚未保存应用密钥。填写 ClientSecret 后点击“保存环境”，无需先登录成功。",
            "Application secret is not saved. Enter ClientSecret and save the environment; sign-in is not required.",
        )
    } else {
        tr(
            tab.text,
            "应用密钥尚未保存。勾选“保存应用密钥”并保存环境，可供下次使用。",
            "Application secret is not saved. Check Save application secret and save the environment to reuse it later.",
        )
    };
    tab.secret_status.set_label(message);
    tab.secret_status.wrap(740);
}

fn login_in_progress(state: &Selection) -> bool {
    state.active_login
        || state.login_attempt.is_some()
            && !state.runtime_profiles.iter().any(|profile| {
                profile["profileId"].as_str() == state.selected_id.as_deref()
                    && profile["state"] == "authenticated"
            })
}

fn update_controls(tab: &NvwaTab) {
    // Initial status retries must not replace a draft entered before the first snapshot.
    let busy = tab.busy.load(Ordering::SeqCst) || !tab.state.borrow().snapshot_loaded;
    let saved = tab.state.borrow().selected_id.is_some();
    let mode = auth_mode(tab);
    let password = mode == "password";
    let application = mode == "application";
    let twofactor = password && tab.state.borrow().twofactor_session.is_some();
    tab.account_row.show(mode != "browser");
    tab.browser_account_row.show(mode == "browser");
    tab.password_row.show(password);
    tab.password_settings.show(password);
    tab.application_settings.show(!password);
    tab.application.show(!password);
    tab.signature_row.show(application);
    tab.connection_hint.show(!password);
    tab.factor_row.show(twofactor);
    for field in [
        tab.name,
        tab.product,
        tab.certification,
        tab.mcp,
        tab.username,
        tab.browser_username,
        tab.client_id,
        tab.tenant,
        tab.unit,
        tab.auth_header,
    ] {
        field.enable(!busy);
    }
    tab.profile.enable(!busy);
    tab.mode.enable(!busy);
    tab.application.enable(!busy && !password);
    tab.signature.enable(!busy);
    tab.password.enable(!busy && password);
    tab.client_secret.enable(!busy && !password);
    tab.remember_password.enable(!busy && password);
    tab.remember_secret.enable(!busy && !password);
    tab.send_factor.enable(!busy && twofactor);
    tab.factor_code.enable(!busy && twofactor);
    tab.save.enable(!busy);
    tab.login.enable(!busy && saved);
    tab.cancel
        .enable(saved && (!busy || tab.state.borrow().active_login));
    tab.cancel.show(login_in_progress(&tab.state.borrow()));
    let kind = selected_client_kind(tab);
    tab.client.selector.enable(!busy);
    for control in [tab.client.check, tab.client.manage] {
        control.enable(!busy && saved);
    }
    tab.client.path.set_editable(kind != "tiangong");
    tab.client.path.enable(!busy && kind != "tiangong");
    for choice in [
        &tab.profile,
        &tab.mode,
        &tab.signature,
        &tab.client.selector,
    ] {
        connection_field_theme(choice, choice.is_enabled());
    }
    for input in [
        &tab.name,
        &tab.product,
        &tab.certification,
        &tab.mcp,
        &tab.browser_username,
        &tab.username,
        &tab.password,
        &tab.client_id,
        &tab.client_secret,
        &tab.tenant,
        &tab.unit,
        &tab.auth_header,
        &tab.factor_code,
        &tab.client.path,
        &tab.tools,
    ] {
        connection_field_theme(input, input.is_editable() && input.is_enabled());
    }
    update_secret_status(tab);
    tab.page.layout();
    tab.page.fit_inside();
}
