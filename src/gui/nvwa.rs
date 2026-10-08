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
    show_error,
    text::{GuiLocale, GuiText},
    theme,
    widgets::{apply_textctrl_theme, text_field_row},
};

#[derive(Default)]
struct Selection {
    config: Value,
    profiles: Vec<Value>,
    selected_id: Option<String>,
    clients: BTreeMap<String, Value>,
    twofactor_session: Option<String>,
    runtime_profiles: Vec<Value>,
    initial_refreshes: u8,
    last_refresh: Option<Instant>,
    snapshot_loaded: bool,
    active_login: bool,
    login_attempt: Option<String>,
}

#[derive(Clone)]
struct ClientRow {
    kind: &'static str,
    path: TextCtrl,
    status: StaticText,
    inspect: Button,
    check: Button,
    apply: Button,
    remove: Button,
    restore: Button,
}

#[derive(Clone)]
pub(super) struct NvwaTab {
    pub(super) page: ScrolledWindow,
    profile: Choice,
    name: TextCtrl,
    product: TextCtrl,
    certification: TextCtrl,
    mcp: TextCtrl,
    mode: Choice,
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
    verify_id: TextCtrl,
    verify_code: TextCtrl,
    factor_code: TextCtrl,
    status: StaticText,
    identity: StaticText,
    tools: TextCtrl,
    reload: Button,
    save: Button,
    delete: Button,
    login: Button,
    cancel: Button,
    logout: Button,
    detect: Button,
    send_factor: Button,
    clients: Vec<ClientRow>,
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
    Save,
    Delete,
    Login,
    Logout,
    Cancel,
    Detect,
    DetectClient(String),
    SendFactor,
    Inspect(String),
    Preview {
        client: String,
        operation: String,
        body: Value,
    },
    Mutate(String),
}

fn tr(text: GuiText, zh: &'static str, en: &'static str) -> &'static str {
    match text.locale {
        GuiLocale::ZhCn => zh,
        GuiLocale::EnUs => en,
    }
}

fn label(parent: &ScrolledWindow, root: &BoxSizer, title: &str) {
    let item = StaticText::builder(parent).with_label(title).build();
    item.set_foreground_color(theme::theme().ink_secondary);
    root.add(&item, 0, SizerFlag::Expand | SizerFlag::All, 10);
}

fn choice_row(
    parent: &ScrolledWindow,
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
    grid.add(&field, 1, SizerFlag::Expand, 0);
    field
}

fn secret_row(parent: &ScrolledWindow, grid: &FlexGridSizer, title: &str) -> TextCtrl {
    let title = StaticText::builder(parent).with_label(title).build();
    grid.add(&title, 0, SizerFlag::AlignCenterVertical, 0);
    let input = TextCtrl::builder(parent)
        .with_style(TextCtrlStyle::Password)
        .build();
    apply_textctrl_theme(&input);
    input.set_min_size(Size::new(420, 30));
    grid.add(&input, 1, SizerFlag::Expand, 0);
    input
}

fn button(parent: &ScrolledWindow, row: &BoxSizer, title: &str) -> Button {
    let control = Button::builder(parent).with_label(title).build();
    row.add(&control, 0, SizerFlag::Right, 8);
    control
}

pub(super) fn create(parent: &Notebook, text: GuiText) -> NvwaTab {
    let page = ScrolledWindow::builder(parent)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    page.set_background_color(theme::theme().bg_card_alt);
    let root = BoxSizer::builder(Orientation::Vertical).build();
    label(&page, &root, tr(text, "NVWA MCP", "NVWA MCP"));
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
    let product = text_field_row(&page, &grid, tr(text, "产品地址", "Product URL"), "");
    let certification = text_field_row(
        &page,
        &grid,
        tr(text, "认证服务地址", "Certification URL"),
        "",
    );
    let mcp = text_field_row(&page, &grid, tr(text, "MCP 地址", "MCP URL"), "");
    let mode = choice_row(
        &page,
        &grid,
        tr(text, "认证方式", "Authentication"),
        &[
            tr(text, "账号密码", "Account and password"),
            tr(text, "浏览器个人授权", "Browser authorization"),
            tr(text, "应用代表用户", "Application delegation"),
        ],
    );
    let username = text_field_row(&page, &grid, tr(text, "账号", "Username"), "");
    let password = secret_row(&page, &grid, tr(text, "密码", "Password"));
    let client_id = text_field_row(&page, &grid, tr(text, "应用 ID", "Application ID"), "");
    let client_secret = secret_row(&page, &grid, tr(text, "应用密钥", "Application secret"));
    let tenant = text_field_row(&page, &grid, tr(text, "租户", "Tenant"), "");
    let unit = text_field_row(&page, &grid, tr(text, "登录机构", "Login organization"), "");
    let auth_header = text_field_row(
        &page,
        &grid,
        tr(text, "MCP 认证头", "MCP authentication header"),
        "Authorization",
    );
    let signature = choice_row(
        &page,
        &grid,
        tr(text, "应用摘要算法", "Application digest"),
        &["SHA-256", "SM3", "MD5"],
    );
    let verify_id = text_field_row(&page, &grid, tr(text, "图形验证码 ID", "Captcha ID"), "");
    let verify_code = text_field_row(&page, &grid, tr(text, "图形验证码", "Captcha"), "");
    let factor_code = text_field_row(
        &page,
        &grid,
        tr(text, "双因子验证码", "Verification code"),
        "",
    );
    root.add_sizer(
        &grid,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let options = BoxSizer::builder(Orientation::Horizontal).build();
    let remember_password = CheckBox::builder(&page)
        .with_label(tr(text, "记住密码", "Remember password"))
        .build();
    let remember_secret = CheckBox::builder(&page)
        .with_label(tr(text, "保存应用密钥", "Save application secret"))
        .build();
    options.add(&remember_password, 0, SizerFlag::Right, 18);
    options.add(&remember_secret, 0, SizerFlag::Right, 18);
    let send_factor = button(
        &page,
        &options,
        tr(text, "发送双因子验证码", "Send verification code"),
    );
    root.add_sizer(&options, 0, SizerFlag::All, 16);
    let commands = BoxSizer::builder(Orientation::Horizontal).build();
    let reload = button(&page, &commands, tr(text, "刷新", "Refresh"));
    let save = button(&page, &commands, tr(text, "保存环境", "Save environment"));
    let delete = button(&page, &commands, tr(text, "删除环境", "Delete environment"));
    let login = button(&page, &commands, tr(text, "登录 / 授权", "Sign in"));
    let cancel = button(
        &page,
        &commands,
        tr(text, "取消授权", "Cancel authorization"),
    );
    let logout = button(&page, &commands, tr(text, "退出登录", "Sign out"));
    let detect = button(&page, &commands, tr(text, "检测 MCP", "Check MCP"));
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
    let mut clients = Vec::new();
    for (kind, title) in [
        ("codex", "Codex"),
        ("workbuddy", "WorkBuddy"),
        ("tiangong", "天工 Claw"),
    ] {
        let row_grid = FlexGridSizer::builder(0, 2)
            .with_vgap(6)
            .with_hgap(14)
            .build();
        row_grid.add_growable_col(1, 1);
        let path = text_field_row(&page, &row_grid, title, "");
        if kind == "tiangong" {
            path.set_editable(false);
        }
        root.add_sizer(
            &row_grid,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            16,
        );
        let client_commands = BoxSizer::builder(Orientation::Horizontal).build();
        let inspect = button(&page, &client_commands, tr(text, "检查", "Inspect"));
        let check = button(
            &page,
            &client_commands,
            tr(text, "检测本机桥", "Check local bridge"),
        );
        let apply = button(
            &page,
            &client_commands,
            tr(text, "预览接入", "Preview connection"),
        );
        let remove = button(
            &page,
            &client_commands,
            tr(text, "预览移除", "Preview removal"),
        );
        let restore = button(
            &page,
            &client_commands,
            tr(text, "预览恢复", "Preview restore"),
        );
        root.add_sizer(
            &client_commands,
            0,
            SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            16,
        );
        let status = StaticText::builder(&page)
            .with_label(tr(text, "尚未检查", "Not inspected"))
            .build();
        status.wrap(920);
        root.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 16);
        clients.push(ClientRow {
            kind,
            path,
            status,
            inspect,
            check,
            apply,
            remove,
            restore,
        });
    }
    label(&page, &root, tr(text, "当前能力", "Current capabilities"));
    let tools = TextCtrl::builder(&page)
        .with_style(TextCtrlStyle::ReadOnly | TextCtrlStyle::MultiLine)
        .with_size(Size::new(-1, 125))
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
        profile,
        name,
        product,
        certification,
        mcp,
        mode,
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
        verify_id,
        verify_code,
        factor_code,
        status,
        identity,
        tools,
        reload,
        save,
        delete,
        login,
        cancel,
        logout,
        detect,
        send_factor,
        clients,
        state: Rc::new(RefCell::new(Selection::default())),
        busy: Arc::new(AtomicBool::new(false)),
        operation: Arc::new(AtomicU64::new(0)),
    };
    update_controls(&tab);
    tab
}

fn config_path() -> PathBuf {
    daemon_config_path().unwrap_or_else(app_support_config_path)
}

fn management_request(path: &str, body: Option<Value>) -> Result<Value, String> {
    let info = crate::nvwa::NvwaService::load_management_info(&config_path())
        .map_err(|_| "无法读取 NVWA 后台安全连接".to_string())?
        .ok_or_else(|| "NVWA 后台尚未就绪，请刷新".to_string())?;
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
        .is_some_and(|size| size > 2 * 1024 * 1024)
    {
        return Err("NVWA 后台响应超过限制".to_string());
    }
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 NVWA 后台响应".to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("NVWA 后台响应超过限制".to_string());
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
    if matches!(action, Action::Cancel) {
        tab.busy.store(true, Ordering::SeqCst);
    } else if tab.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    {
        let mut state = tab.state.borrow_mut();
        state.active_login = matches!(action, Action::Login);
        if matches!(action, Action::Login) {
            let attempt = uuid::Uuid::new_v4().to_string();
            if let Some(body) = body.as_mut() {
                body["loginAttemptId"] = json!(attempt);
            }
            state.login_attempt = Some(attempt);
        } else if matches!(action, Action::Cancel) {
            if let (Some(body), Some(attempt)) = (body.as_mut(), state.login_attempt.as_ref()) {
                body["loginAttemptId"] = json!(attempt);
            }
        }
    }
    let operation = tab.operation.fetch_add(1, Ordering::SeqCst) + 1;
    let profile_id = tab.state.borrow().selected_id.clone();
    tab.status.set_label("处理中");
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
    let t = tab.clone();
    tab.profile.on_selection_changed(move |_| {
        select_profile(&t);
    });
    let t = tab.clone();
    tab.mode.on_selection_changed(move |_| {
        t.auth_header
            .set_value(if t.mode.get_selection() == Some(0) {
                "Authorization"
            } else {
                "authorization-ticket-token"
            });
        update_controls(&t);
    });
    let t = tab.clone();
    let sender = tx.clone();
    tab.reload
        .on_click(move |_| start(&t, &sender, Action::Refresh, "/manage/status", None));
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
            profile[key] = json!(value);
        }
        let revision = config_revision(&state.config).to_string();
        drop(state);
        if profile["name"].as_str().unwrap_or("").is_empty() {
            show_error(&f, "请输入环境名称");
            return;
        }
        start(
            &t,
            &sender,
            Action::Save,
            "/manage/profile/save",
            Some(json!({"profile":profile,"expectedRevision":revision})),
        );
    });
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.delete.on_click(move |_| {
        if !confirm(&f, tr(text,"删除此环境及其本地授权？客户端受管条目应先移除。","Delete this environment and its local authorization? Remove managed client entries first.")) { return; }
        let state = t.state.borrow();
        let body = json!({"profileId":state.selected_id,"expectedRevision":config_revision(&state.config)});
        drop(state);
        start(&t, &sender, Action::Delete, "/manage/profile/delete", Some(body));
    });
    let t = tab.clone();
    let sender = tx.clone();
    let f = *frame;
    tab.login.on_click(move |_| {
        if !require_saved_form(&t, &f, text) { return; }
        let state = t.state.borrow();
        let mut ext = serde_json::Map::new();
        for (key, value) in [("verifyId",t.verify_id.get_value()),("verifyCode",t.verify_code.get_value()),("validCode",t.factor_code.get_value())] {
            if !value.is_empty() { ext.insert(key.to_string(),json!(value)); }
        }
        if let Some(session) = &state.twofactor_session { ext.insert("twofactorSessionId".to_string(),json!(session)); }
        let body = json!({"profileId":state.selected_id,"password":t.password.get_value(),
            "clientSecret":t.client_secret.get_value(),"rememberPassword":t.remember_password.is_checked(),
            "rememberSecret":t.remember_secret.is_checked(),"extInfo":ext});
        drop(state);
        let path = match t.mode.get_selection() { Some(1) => "/manage/login/browser", Some(2) => "/manage/login/application", _ => "/manage/login/password" };
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
    for (control, path, action) in [
        (tab.logout, "/manage/logout", Action::Logout),
        (tab.cancel, "/manage/login/cancel", Action::Cancel),
        (tab.detect, "/manage/tools/detect", Action::Detect),
    ] {
        let t = tab.clone();
        let sender = tx.clone();
        let f = *frame;
        control.on_click(move |_| {
            if matches!(action, Action::Detect) && !require_saved_form(&t, &f, text) {
                return;
            }
            let body = json!({"profileId":t.state.borrow().selected_id});
            start(&t, &sender, action.clone(), path, Some(body));
        });
    }
    for row in &tab.clients {
        let t = tab.clone();
        let sender = tx.clone();
        let r = row.clone();
        row.inspect.on_click(move |_| {
            start(
                &t,
                &sender,
                Action::Inspect(r.kind.to_string()),
                "/manage/client/inspect",
                Some(client_body(&t, &r)),
            )
        });
        let t = tab.clone();
        let sender = tx.clone();
        let r = row.clone();
        let f = *frame;
        row.check.on_click(move |_| {
            if !require_saved_form(&t, &f, text) {
                return;
            }
            start(
                &t,
                &sender,
                Action::DetectClient(r.kind.to_string()),
                "/manage/tools/detect",
                Some(client_body(&t, &r)),
            );
        });
        for (control, operation) in [
            (row.apply, "apply"),
            (row.remove, "remove"),
            (row.restore, "restore"),
        ] {
            let t = tab.clone();
            let sender = tx.clone();
            let r = row.clone();
            let f = *frame;
            control.on_click(move |_| {
                if !require_saved_form(&t, &f, text) {
                    return;
                }
                let mut body = client_body(&t, &r);
                body["operation"] = json!(operation);
                if let Some(backup) = t
                    .state
                    .borrow()
                    .clients
                    .get(r.kind)
                    .and_then(|v| v.get("backupRef"))
                {
                    body["backupRef"] = backup.clone();
                }
                start(
                    &t,
                    &sender,
                    Action::Preview {
                        client: r.kind.to_string(),
                        operation: operation.to_string(),
                        body: body.clone(),
                    },
                    "/manage/client/preview",
                    Some(body),
                );
            });
        }
    }
}

fn config_revision(config: &Value) -> &str {
    config
        .get("revision")
        .or_else(|| config.get("_revision"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn form_fields(tab: &NvwaTab) -> Vec<(&'static str, String)> {
    let mut fields: Vec<_> = [
        ("name", tab.name),
        ("productBaseUrl", tab.product),
        ("certificationBaseUrl", tab.certification),
        ("mcpUrl", tab.mcp),
        ("username", tab.username),
        ("clientId", tab.client_id),
        ("tenant", tab.tenant),
        ("loginUnit", tab.unit),
        ("mcpAuthHeader", tab.auth_header),
    ]
    .into_iter()
    .map(|(key, field)| (key, field.get_value().trim().to_string()))
    .collect();
    fields.push((
        "authMode",
        match tab.mode.get_selection() {
            Some(1) => "browser",
            Some(2) => "application",
            _ => "password",
        }
        .to_string(),
    ));
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
    let matches = saved.is_some_and(|profile| {
        form_fields(tab).iter().all(|(key, value)| {
            let default = if *key == "mcpAuthHeader" {
                if profile["authMode"].as_str() == Some("password") {
                    "Authorization"
                } else {
                    "authorization-ticket-token"
                }
            } else {
                ""
            };
            profile[*key].as_str().unwrap_or(default) == value
        })
    });
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

pub(super) fn refresh_if_needed(
    tab: &NvwaTab,
    tx: &tokio::sync::mpsc::UnboundedSender<GuiMessage>,
) {
    if tab.busy.load(Ordering::SeqCst) {
        return;
    }
    let mut state = tab.state.borrow_mut();
    if state.snapshot_loaded
        || state.initial_refreshes >= 3
        || state
            .last_refresh
            .is_some_and(|last| last.elapsed() < Duration::from_secs(5))
    {
        return;
    }
    state.initial_refreshes += 1;
    state.last_refresh = Some(Instant::now());
    drop(state);
    start(tab, tx, Action::Refresh, "/manage/status", None);
}

fn client_body(tab: &NvwaTab, row: &ClientRow) -> Value {
    let mut result = json!({"profileId":tab.state.borrow().selected_id,"clientKind":row.kind});
    let path = row.path.get_value();
    if !path.trim().is_empty() && row.kind != "tiangong" {
        result["overridePath"] = json!(path.trim());
    }
    result
}

fn confirm(frame: &Frame, message: &str) -> bool {
    MessageDialog::builder(frame, message, "NVWA MCP")
        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
        .build()
        .show_modal()
        == ID_YES
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
    tab.state.borrow_mut().active_login = false;
    if !matches!(
        result.action,
        Action::Refresh | Action::Save | Action::Delete
    ) && result.profile_id != tab.state.borrow().selected_id
    {
        update_controls(tab);
        return;
    }
    let value = match result.outcome {
        Ok(value) => value,
        Err(error) => {
            tab.status.set_label(&error);
            update_controls(tab);
            return;
        }
    };
    match result.action {
        Action::Refresh => apply_snapshot(tab, text, value),
        Action::Save => {
            if let Some(id) = value
                .pointer("/profile/id")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
            {
                tab.state.borrow_mut().selected_id = Some(id.to_string());
            }
            tab.status
                .set_label(tr(text, "环境已保存", "Environment saved"));
            start(tab, tx, Action::Refresh, "/manage/status", None);
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
            let detail = value
                .get("detail")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("认证步骤已返回");
            tab.status.set_label(detail);
            if let Some(url) = value.get("authorizeUrl").and_then(Value::as_str) {
                if let Err(error) = open_url_in_browser(text, url) {
                    show_error(frame, &error);
                }
            } else if value.get("state").and_then(Value::as_str) == Some("authenticated") {
                tab.password.set_value("");
                tab.client_secret.set_value("");
                start(tab, tx, Action::Refresh, "/manage/status", None);
            }
        }
        Action::Logout | Action::Cancel => {
            tab.password.set_value("");
            tab.client_secret.set_value("");
            tab.tools.set_value("");
            tab.state.borrow_mut().twofactor_session = None;
            tab.state.borrow_mut().login_attempt = None;
            start(tab, tx, Action::Refresh, "/manage/status", None);
        }
        Action::Detect => {
            let tools = value
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let names: Vec<_> = tools
                .iter()
                .filter_map(|tool| {
                    tool.get("name")
                        .and_then(Value::as_str)
                        .or_else(|| tool.as_str())
                })
                .collect();
            tab.tools.set_value(&names.join("\n"));
            tab.status.set_label(&format!(
                "MCP 协议检测通过；当前 {} 项能力，客户端实际调用待验证",
                tools.len()
            ));
        }
        Action::DetectClient(kind) => {
            let tools = value["tools"].as_array().cloned().unwrap_or_default();
            tab.tools.set_value(
                &tools
                    .iter()
                    .filter_map(|tool| tool["name"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            let native = if kind == "tiangong" {
                if value["tiangongDirectoryUpdated"].as_bool() == Some(true) {
                    "；天工能力目录已更新，真实调用待验收"
                } else {
                    "；天工目录未更新，请检查受管项与原生状态"
                }
            } else {
                "；客户端加载、信任与真实调用待验收"
            };
            if let Some(row) = tab.clients.iter().find(|row| row.kind == kind) {
                row.status.set_label(&format!(
                    "本机桥协议检测通过；{} 项能力{native}",
                    tools.len()
                ));
                row.status.wrap(920);
            }
            let mut status = tab
                .state
                .borrow()
                .clients
                .get(&kind)
                .cloned()
                .unwrap_or(Value::Null);
            status["bridgeState"] = json!("checked");
            tab.state.borrow_mut().clients.insert(kind, status);
            tab.status
                .set_label("本机桥检测通过；客户端实际调用仍需验证");
        }
        Action::SendFactor => {
            tab.status
                .set_label(value["detail"].as_str().unwrap_or("已请求发送双因子验证码"));
        }
        Action::Inspect(kind) | Action::Mutate(kind) => {
            let value = value.get("status").cloned().unwrap_or(value);
            apply_client(tab, &kind, value);
        }
        Action::Preview {
            client,
            operation,
            mut body,
        } => {
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
                tab.status.set_label(detail);
            } else {
                let target_path = value
                    .pointer("/status/targetPath")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let server_name = value
                    .pointer("/status/serverName")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let warnings = value["warnings"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let operation_label = match operation.as_str() {
                    "apply" => "写入接入配置",
                    "remove" => "移除受管配置",
                    "restore" => "恢复目标备份",
                    _ => "修改配置",
                };
                let message = format!(
                    "{client} · {operation_label}\n{target_path}\n{server_name}\n\n{detail}\n{warnings}"
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
                        Action::Mutate(client),
                        &format!("/manage/client/{operation}"),
                        Some(body),
                    );
                } else {
                    tab.status.set_label("已取消配置修改");
                }
            }
        }
    }
    update_controls(tab);
    tab.status.wrap(920);
    tab.identity.wrap(920);
    tab.page.layout();
    tab.page.fit_inside();
}

fn apply_client(tab: &NvwaTab, kind: &str, value: Value) {
    if let Some(row) = tab.clients.iter().find(|row| row.kind == kind) {
        let detail = value
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("状态已读取");
        let path = value
            .get("targetPath")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !path.is_empty() {
            row.path.set_value(path);
        }
        let load = value.get("loadState").and_then(Value::as_str).unwrap_or("");
        let connection = value
            .get("connectionState")
            .and_then(Value::as_str)
            .unwrap_or("");
        let bridge = value
            .get("bridgeState")
            .and_then(Value::as_str)
            .unwrap_or("");
        row.status
            .set_label(&format!("{detail}\n{load}  {connection}  {bridge}"));
        row.status.wrap(920);
    }
    tab.state
        .borrow_mut()
        .clients
        .insert(kind.to_string(), value);
}

fn apply_snapshot(tab: &NvwaTab, text: GuiText, value: Value) {
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
    select_profile(tab);
    if tab.state.borrow().selected_id.is_none() {
        tab.status
            .set_label(tr(text, "后台已就绪", "Backend ready"));
    }
}

fn expiry_label(expiry: Option<u64>) -> String {
    let Some(expiry) = expiry else {
        return "未知（服务未返回期限）".to_string();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if expiry <= now {
        return "已过期".to_string();
    }
    let minutes = (expiry - now).div_ceil(60_000);
    if minutes >= 1440 {
        format!("约 {} 天 {} 小时", minutes / 1440, minutes % 1440 / 60)
    } else if minutes >= 60 {
        format!("约 {} 小时 {} 分钟", minutes / 60, minutes % 60)
    } else {
        format!("约 {minutes} 分钟")
    }
}

fn show_runtime_status(tab: &NvwaTab) {
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
    let state_label = match state["state"].as_str() {
        Some("authenticated") => "已登录",
        Some("expired") => "MCP 凭据已过期",
        Some("personal_expired") => "个人会话已过期",
        Some("identity_verification_pending") => "待重新核验身份",
        _ => "未登录",
    };
    let personal =
        if state["personalTokenPresent"].as_bool() == Some(false) || state["identity"].is_null() {
            "无个人会话".to_string()
        } else {
            expiry_label(state["personalExpiresAtMs"].as_u64())
        };
    let mcp = if state["identity"].is_null() {
        "未取得".to_string()
    } else {
        expiry_label(state["mcpExpiresAtMs"].as_u64())
    };
    tab.identity.set_label(&format!(
        "{state_label}  {person}  租户 {tenant}\n个人会话：{personal}；MCP 凭据：{mcp}"
    ));
    tab.status
        .set_label(state["detail"].as_str().unwrap_or("后台已连接"));
}

fn select_profile(tab: &NvwaTab) {
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
        state.clients.clear();
        state.twofactor_session = None;
        state.login_attempt = None;
    }
    let profile = profile.unwrap_or(Value::Null);
    for (field, key) in [
        (tab.name, "name"),
        (tab.product, "productBaseUrl"),
        (tab.certification, "certificationBaseUrl"),
        (tab.mcp, "mcpUrl"),
        (tab.username, "username"),
        (tab.client_id, "clientId"),
        (tab.tenant, "tenant"),
        (tab.unit, "loginUnit"),
        (tab.auth_header, "mcpAuthHeader"),
    ] {
        field.set_value(profile[key].as_str().unwrap_or(""));
    }
    let mode = match profile["authMode"].as_str() {
        Some("browser") => 1,
        Some("application") => 2,
        _ => 0,
    };
    tab.mode.set_selection(mode);
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
    tab.password.set_value("");
    tab.client_secret.set_value("");
    tab.verify_id.set_value("");
    tab.verify_code.set_value("");
    tab.factor_code.set_value("");
    tab.remember_password.set_value(false);
    tab.remember_secret.set_value(false);
    tab.tools.set_value("");
    tab.identity.set_label("未读取当前授权状态");
    for client in &tab.clients {
        client.path.set_value("");
        client.status.set_label("尚未检查");
    }
    show_runtime_status(tab);
    update_controls(tab);
}

fn update_controls(tab: &NvwaTab) {
    let busy = tab.busy.load(Ordering::SeqCst);
    let saved = tab.state.borrow().selected_id.is_some();
    for field in [
        tab.name,
        tab.product,
        tab.certification,
        tab.mcp,
        tab.username,
        tab.client_id,
        tab.tenant,
        tab.unit,
        tab.auth_header,
    ] {
        field.enable(!busy);
    }
    tab.profile.enable(!busy);
    tab.mode.enable(!busy);
    tab.signature.enable(!busy);
    tab.password
        .enable(!busy && tab.mode.get_selection() == Some(0));
    tab.client_secret
        .enable(!busy && tab.mode.get_selection() != Some(0));
    tab.remember_password
        .enable(!busy && tab.mode.get_selection() == Some(0));
    tab.remember_secret
        .enable(!busy && tab.mode.get_selection() != Some(0));
    tab.send_factor.enable(
        !busy
            && tab.mode.get_selection() == Some(0)
            && tab.state.borrow().twofactor_session.is_some(),
    );
    for field in [tab.verify_id, tab.verify_code, tab.factor_code] {
        field.enable(!busy && tab.mode.get_selection() == Some(0));
    }
    tab.reload.enable(!busy);
    tab.save.enable(!busy);
    tab.delete.enable(!busy && saved);
    for control in [tab.login, tab.logout, tab.detect] {
        control.enable(!busy && saved);
    }
    tab.cancel
        .enable(saved && (!busy || tab.state.borrow().active_login));
    for row in &tab.clients {
        for control in [row.inspect, row.check, row.apply, row.remove] {
            control.enable(!busy && saved);
        }
        row.restore.enable(
            !busy
                && saved
                && tab
                    .state
                    .borrow()
                    .clients
                    .get(row.kind)
                    .and_then(|v| v.get("backupRef"))
                    .and_then(Value::as_str)
                    .is_some(),
        );
        row.path.enable(!busy && row.kind != "tiangong");
    }
}
