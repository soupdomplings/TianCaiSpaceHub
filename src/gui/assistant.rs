//! Native assistant controls. The backend owns the temporary conversation and
//! configuration operations; this view never stores keys in chat messages.
use super::{
    api::ApiClient,
    controls::{Button, ButtonKind, TextCtrl},
    dialog_job,
    shell::{PageId, Shell},
    theme, visual,
};
use crate::desktop::clients::ClientId;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer};

#[derive(Clone)]
enum Action {
    Status,
    Settings(bool),
    Prepare,
    Uninstall,
    Send(String),
    Stop,
    Clear,
    Confirm,
    Navigate,
}
type Inbox = Arc<Mutex<Vec<(Action, Result<Value, String>)>>>;

#[derive(Clone)]
pub(super) struct AssistantUi {
    pub page: Panel,
    page_scroll: ScrolledWindow,
    parent: Frame,
    shell: Shell,
    api: ApiClient,
    enabled: CheckBox,
    runtime: StaticText,
    account: StaticText,
    model: StaticText,
    detail: StaticText,
    references: StaticText,
    prepare: Button,
    uninstall: Button,
    open: Button,
    clean: Button,
    chat: Frame,
    chat_status: StaticText,
    transcript: TextCtrl,
    input: TextCtrl,
    send: Button,
    stop: Button,
    clear: Button,
    pending: Panel,
    pending_scroll: ScrolledWindow,
    pending_text: StaticText,
    approve: Button,
    reject: Button,
    credential: Button,
    last_text: Rc<RefCell<String>>,
    state: Rc<RefCell<Value>>,
    inbox: Inbox,
    fetching: Arc<AtomicBool>,
    acting: Arc<AtomicBool>,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    last_poll: Rc<Cell<Instant>>,
    active: Rc<Cell<bool>>,
    positioned: Rc<Cell<bool>>,
    navigated: Rc<RefCell<Option<String>>>,
}

impl AssistantUi {
    pub fn new(parent: &Frame, shell: &Shell, api: ApiClient) -> Self {
        Self::build(parent, shell, api, true)
    }

    fn build(parent: &Frame, shell: &Shell, api: ApiClient, monitor: bool) -> Self {
        let t = theme::theme();
        let page = Panel::builder(&shell.book).build();
        page.set_background_color(t.bg_app);
        let outer = BoxSizer::builder(Orientation::Vertical).build();
        let scroll = ScrolledWindow::builder(&page)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        scroll.set_scroll_rate(0, scroll.from_dip(10));
        scroll.set_background_color(t.bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let title = label(&scroll, "让天才搭档帮你配置 Hub", theme::TextRole::Title);
        column.add(&title, 0, SizerFlag::Bottom, page.from_dip(16));
        let enabled = CheckBox::builder(&scroll)
            .with_label("启用天才搭档")
            .build();
        enabled.set_foreground_color(t.ink_primary);
        enabled.set_background_color(t.bg_app);
        enabled.enable(false);
        column.add(&enabled, 0, SizerFlag::Bottom, page.from_dip(20));
        let runtime = label(&scroll, "天才搭档 Server · 正在检查", theme::TextRole::Body);
        let account = label(&scroll, "小分队账户 · 正在读取", theme::TextRole::Body);
        let model = label(&scroll, "模型服务 · 正在读取", theme::TextRole::Body);
        for item in [runtime, account, model] {
            column.add(
                &item,
                0,
                SizerFlag::Expand | SizerFlag::Bottom,
                page.from_dip(12),
            );
        }
        let actions = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let prepare = Button::builder(&scroll)
            .with_label("准备天才搭档 Server")
            .with_kind(ButtonKind::Secondary)
            .build();
        prepare.set_tooltip("启用后检查现有运行端；由你确认后才准备或从官方下载安装");
        let uninstall = Button::builder(&scroll)
            .with_label("移除天才搭档 Server")
            .with_kind(ButtonKind::Ghost)
            .build();
        uninstall
            .set_tooltip("天才搭档和天工协助都停用后，可手动移除 Hub 自管运行包；系统原安装保留");
        let login = Button::builder(&scroll)
            .with_label("前往小分队账户")
            .with_kind(ButtonKind::Ghost)
            .build();
        let open = Button::builder(&scroll)
            .with_label("打开对话")
            .with_icon(visual::Icon::Chat)
            .build();
        let clean = Button::builder(&scroll)
            .with_label("清理搭档临时内容")
            .with_kind(ButtonKind::Ghost)
            .build();
        for button in [prepare, uninstall, login, open, clean] {
            actions.add(
                &button,
                0,
                SizerFlag::Right | SizerFlag::Bottom,
                page.from_dip(8),
            );
        }
        column.add_sizer(
            &actions,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(6),
        );
        let detail = label(&scroll, "正在读取状态…", theme::TextRole::Body);
        detail.set_foreground_color(t.ink_secondary);
        column.add(
            &detail,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(16),
        );
        let note = label(
            &scroll,
            "可读取及调整模型、渠道和客户端配置，也可询问 Hub 与小分队的使用方法。删除配置会集中确认一次，密钥通过本机表单输入。\n\n收起可继续当前对话；停止会中断执行。真正退出 Hub 后清理本次临时内容，已保存的模型配置和主动另存的成果保留。",
            theme::TextRole::Body,
        );
        note.set_foreground_color(t.ink_secondary);
        column.add(
            &note,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(20),
        );
        let references = label(&scroll, "", theme::TextRole::Caption);
        references.set_foreground_color(t.ink_secondary);
        column.add(
            &references,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(16),
        );
        scroll.set_sizer(column, true);
        outer.add(&scroll, 1, SizerFlag::Expand, 0);
        page.set_sizer(outer, true);
        shell.register(PageId::Assistant, &page);

        let size = super::ai_gw_channel_dialog_size(parent, Size::new(400, 560));
        let chat = Frame::builder()
            .with_parent(parent)
            .with_title("天才搭档")
            .with_size(size)
            .with_style(
                FrameStyle::Caption
                    | FrameStyle::ResizeBorder
                    | FrameStyle::SystemMenu
                    | FrameStyle::CloseBox
                    | FrameStyle::ToolWindow
                    | FrameStyle::NoTaskbar,
            )
            .build();
        chat.set_background_color(t.bg_card);
        chat.set_font(&theme::font(theme::TextRole::Body));
        chat.set_min_size(visual::dip_size(&chat, 360, 440));
        let root = Panel::builder(&chat).build();
        root.set_background_color(t.bg_card);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let bar = BoxSizer::builder(Orientation::Horizontal).build();
        let heading = label(&root, "天才搭档", theme::TextRole::Title);
        let collapse = Button::builder(&root)
            .with_label("收起")
            .with_kind(ButtonKind::Ghost)
            .build();
        collapse.set_tooltip("收起窗口，保留当前对话");
        bar.add(&heading, 1, SizerFlag::AlignCenterVertical, 0);
        bar.add(&collapse, 0, SizerFlag::AlignCenterVertical, 0);
        col.add_sizer(
            &bar,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            root.from_dip(16),
        );
        let chat_status = label(&root, "正在读取状态…", theme::TextRole::Caption);
        chat_status.set_foreground_color(t.ink_secondary);
        col.add(
            &chat_status,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            root.from_dip(16),
        );
        let transcript = TextCtrl::builder(&root)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
            .build();
        transcript.set_name("assistant-conversation");
        transcript.set_background_color(t.bg_card);
        transcript.set_foreground_color(t.ink_primary);
        col.add(
            &transcript,
            1,
            SizerFlag::Expand | SizerFlag::All,
            root.from_dip(12),
        );
        let pending = Panel::builder(&root).build();
        pending.set_background_color(t.bg_card);
        let pending_col = BoxSizer::builder(Orientation::Vertical).build();
        let pending_scroll = ScrolledWindow::builder(&pending)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        pending_scroll.set_scroll_rate(0, pending_scroll.from_dip(8));
        pending_scroll.set_background_color(t.bg_card);
        pending_scroll.set_min_size(visual::dip_size(&pending_scroll, -1, 72));
        pending_scroll.set_max_size(visual::dip_size(&pending_scroll, -1, 100));
        let pending_details = BoxSizer::builder(Orientation::Vertical).build();
        let pending_text = label(&pending_scroll, "", theme::TextRole::Body);
        pending_details.add(&pending_text, 0, SizerFlag::Expand, 0);
        pending_scroll.set_sizer(pending_details, false);
        pending_col.add(
            &pending_scroll,
            1,
            SizerFlag::Expand | SizerFlag::Bottom,
            root.from_dip(8),
        );
        let pending_row = BoxSizer::builder(Orientation::Horizontal).build();
        let approve = Button::builder(&pending).with_label("确认删除").build();
        let reject = Button::builder(&pending)
            .with_label("取消删除")
            .with_kind(ButtonKind::Ghost)
            .build();
        let credential = Button::builder(&pending).with_label("输入凭据").build();
        for button in [approve, reject, credential] {
            pending_row.add(&button, 0, SizerFlag::Right, root.from_dip(8));
        }
        pending_col.add_sizer(&pending_row, 0, SizerFlag::Expand, 0);
        pending.set_sizer(pending_col, false);
        pending.hide();
        col.add(
            &pending,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            root.from_dip(16),
        );
        let input_label = label(&root, "告诉搭档要做什么", theme::TextRole::Caption);
        input_label.set_foreground_color(t.ink_secondary);
        col.add(
            &input_label,
            0,
            SizerFlag::Left | SizerFlag::Right,
            root.from_dip(16),
        );
        let input = TextCtrl::builder(&root)
            .with_style(TextCtrlStyle::MultiLine)
            .with_size(Size::new(-1, root.from_dip(76)))
            .build();
        input.set_name("assistant-message-input");
        input.set_hint("例如：帮我把这个模型配置到天工。Ctrl+Enter 发送");
        col.add(
            &input,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            root.from_dip(12),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let clear = Button::builder(&root)
            .with_label("清空")
            .with_kind(ButtonKind::Ghost)
            .build();
        let stop = Button::builder(&root)
            .with_label("停止")
            .with_kind(ButtonKind::Secondary)
            .build();
        let send = Button::builder(&root)
            .with_label("发送")
            .with_kind(ButtonKind::Primary)
            .build();
        row.add(&clear, 0, SizerFlag::Right, root.from_dip(8));
        row.add_stretch_spacer(1);
        row.add(&stop, 0, SizerFlag::Right, root.from_dip(8));
        row.add(&send, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            root.from_dip(12),
        );
        root.set_sizer(col, false);
        let frame_col = BoxSizer::builder(Orientation::Vertical).build();
        frame_col.add(&root, 1, SizerFlag::Expand, 0);
        chat.set_sizer(frame_col, false);

        let ui = Self {
            page,
            page_scroll: scroll,
            parent: *parent,
            shell: shell.clone(),
            api,
            enabled,
            runtime,
            account,
            model,
            detail,
            references,
            prepare,
            uninstall,
            open,
            clean,
            chat,
            chat_status,
            transcript,
            input,
            send,
            stop,
            clear,
            pending,
            pending_scroll,
            pending_text,
            approve,
            reject,
            credential,
            last_text: Default::default(),
            state: Default::default(),
            inbox: Default::default(),
            fetching: Default::default(),
            acting: Default::default(),
            timer: Default::default(),
            last_poll: Rc::new(Cell::new(Instant::now() - Duration::from_secs(30))),
            active: Default::default(),
            positioned: Default::default(),
            navigated: Default::default(),
        };
        let u = ui.clone();
        enabled.on_toggled(move |_| {
            u.post(
                "settings",
                json!({"enabled":enabled.is_checked()}),
                Action::Settings(enabled.is_checked()),
            )
        });
        let nav = shell.clone();
        login.on_click(move |_| nav.navigate(PageId::Squad));
        for button in [shell.assistant_launcher, open] {
            let u = ui.clone();
            button.on_click(move |_| u.show_chat());
        }
        let u = ui.clone();
        prepare.on_click(move |_| u.confirm_install());
        let u = ui.clone();
        uninstall.on_click(move |_| u.confirm_uninstall());
        let u = ui.clone();
        send.on_click(move |_| u.send_message());
        let u = ui.clone();
        input.on_key_down(move |event| {
            if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(13) && key.cmd_down()) {
                u.send_message();
            } else if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) {
                u.collapse();
            } else {
                event.skip(true);
            }
        });
        let u = ui.clone();
        stop.on_click(move |_| u.post("stop", json!({}), Action::Stop));
        for button in [clear, clean] {
            let u = ui.clone();
            button.on_click(move |_| u.post("clear", json!({}), Action::Clear));
        }
        for (button, approved) in [(approve, true), (reject, false)] {
            let u = ui.clone();
            button.on_click(move |_| {
                let nonce = u.state.borrow()["pendingDeletion"]["nonce"].clone();
                if !nonce.is_null() {
                    u.post(
                        "confirm",
                        json!({"nonce":nonce,"approved":approved}),
                        Action::Confirm,
                    );
                }
            });
        }
        let u = ui.clone();
        credential.on_click(move |_| u.enter_credential());
        let u = ui.clone();
        collapse.on_click(move |_| u.collapse());
        let u = ui.clone();
        chat.on_close(move |event| {
            if let WindowEventData::General(event) = event {
                event.veto();
            }
            u.collapse();
        });
        let u = ui.clone();
        page.on_size(move |event| {
            let width =
                (u.page.get_client_size().width - u.page.from_dip(30)).max(u.page.from_dip(240));
            for item in [u.runtime, u.account, u.model, u.detail, u.references, note] {
                item.wrap(width);
            }
            scroll.layout();
            scroll.fit_inside();
            event.skip(true);
        });
        let u = ui.clone();
        chat.on_size(move |event| {
            let width = (u.chat.get_client_size().width - u.chat.from_dip(32)).max(120);
            u.chat_status.wrap(width);
            u.pending_text.wrap(width);
            u.pending_scroll.layout();
            u.pending_scroll.fit_inside();
            root.layout();
            event.skip(true);
        });
        if monitor {
            let timer = Timer::new(&page);
            let u = ui.clone();
            timer.on_tick(move |_| u.poll());
            timer.start(150, false);
            ui.timer.borrow_mut().replace(timer);
            ui.fetch();
        }
        ui
    }

    pub fn enter(&self, page: PageId) {
        self.active.set(page == PageId::Assistant);
        if page == PageId::Assistant {
            self.fetch();
        }
    }

    fn show_chat(&self) {
        let state = self.state.borrow();
        if state["enabled"] != true || state["runtimeReady"] != true {
            return;
        }
        drop(state);
        if !self.positioned.replace(true) {
            let parent = self.parent.get_client_size();
            let size = self.chat.get_size();
            let p = self.parent.client_to_screen(Point::new(
                (parent.width - size.width - self.parent.from_dip(20)).max(0),
                (parent.height - size.height - self.parent.from_dip(16)).max(0),
            ));
            self.chat.move_window(p.x, p.y);
        }
        self.chat.show(true);
        self.chat.raise();
        self.input.set_focus();
        self.fetch();
    }

    fn collapse(&self) {
        self.chat.show(false);
        if self.parent.is_shown() && self.shell.assistant_launcher.is_shown() {
            self.shell.assistant_launcher.set_focus();
        }
    }

    fn fetch(&self) {
        if self.fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        self.last_poll.set(Instant::now());
        let (api, inbox, flag) = (self.api.clone(), self.inbox.clone(), self.fetching.clone());
        std::thread::spawn(move || {
            let result = api
                .request_text(
                    api.http
                        .get(format!("{}/api/desktop/assistant/status", api.base_url))
                        .header("x-tiancai-desktop", "1")
                        .timeout(Duration::from_secs(15)),
                )
                .and_then(|body| {
                    serde_json::from_str(&body)
                        .map_err(|_| "天才搭档状态无效，请重新读取".to_string())
                });
            if let Ok(mut slot) = inbox.lock() {
                slot.push((Action::Status, result));
            }
            flag.store(false, Ordering::SeqCst);
            wxdragon::wake_up_idle();
        });
    }

    fn post(&self, operation: &str, body: Value, action: Action) {
        self.post_path(&format!("/api/desktop/assistant/{operation}"), body, action);
    }

    fn post_path(&self, path: &str, body: Value, action: Action) {
        if self.acting.swap(true, Ordering::SeqCst) {
            return;
        }
        self.update_buttons();
        let (api, inbox, flag, path) = (
            self.api.clone(),
            self.inbox.clone(),
            self.acting.clone(),
            path.to_string(),
        );
        std::thread::spawn(move || {
            let result = api.desktop_post(&path, &body);
            if let Ok(mut slot) = inbox.lock() {
                slot.push((action, result));
            }
            flag.store(false, Ordering::SeqCst);
            wxdragon::wake_up_idle();
        });
    }

    fn send_message(&self) {
        let state = self.state.borrow();
        if state["running"] == true
            || state["enabled"] != true
            || state["runtimeReady"] != true
            || state["loggedIn"] != true
            || state["modelServiceReady"] != true
            || state["cleanupPending"] == true
        {
            return;
        }
        drop(state);
        let text = self.input.get_value();
        if text.trim().is_empty() {
            self.input.set_focus();
            return;
        }
        self.post("send", json!({"text":text}), Action::Send(text));
    }

    fn confirm_install(&self) {
        if self.state.borrow()["enabled"] != true || self.acting.load(Ordering::SeqCst) {
            return;
        }
        let prompt = MessageDialog::builder(&self.parent,
            "将从已有兼容程序准备共享的天才搭档 Server；本机没有可用来源时，从官方下载安装。\n\n天才搭档和天工协助共用这一份运行端，安装后长期保留。现在准备？",
            "安装天才搭档 Server")
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion).build();
        let accepted = prompt.show_modal() == ID_YES;
        prompt.destroy();
        if accepted {
            self.detail.set_label("正在准备天才搭档 Server…");
            self.post_path(
                "/api/desktop/assistance/prepare",
                json!({"confirmed":true}),
                Action::Prepare,
            );
        }
    }

    fn confirm_uninstall(&self) {
        let state = self.state.borrow();
        if state["runtime"]["canUninstall"] != true && state["canUninstall"] != true {
            return;
        }
        drop(state);
        let prompt = MessageDialog::builder(&self.parent,
            "移除 Hub 自管的天才搭档 Server？\n\n系统中原有的运行程序、已保存模型配置和用户文件都会保留。以后再次启用时，需要由你确认重新准备。",
            "移除天才搭档 Server")
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion).build();
        let accepted = prompt.show_modal() == ID_YES;
        prompt.destroy();
        if accepted {
            self.post_path(
                "/api/desktop/assistance/uninstall",
                json!({"confirmed":true}),
                Action::Uninstall,
            );
        }
    }

    fn poll(&self) {
        let results = self
            .inbox
            .lock()
            .map(|mut slot| std::mem::take(&mut *slot))
            .unwrap_or_default();
        for (action, result) in results {
            match result {
                Ok(value) => {
                    if let Action::Send(text) = &action {
                        if self.input.get_value() == *text {
                            self.input.clear();
                        }
                    }
                    if matches!(action, Action::Clear) {
                        self.input.clear();
                    }
                    if value.get("enabled").is_some() {
                        self.apply(value);
                    } else {
                        self.last_poll.set(Instant::now() - Duration::from_secs(30));
                    }
                    if matches!(action, Action::Settings(true))
                        && self.state.borrow()["runtimeNeedsPreparation"] == true
                    {
                        self.confirm_install();
                    }
                }
                Err(error) => {
                    self.detail.set_label(&error);
                    self.detail.set_foreground_color(theme::theme().error);
                    self.chat_status.set_label(&error);
                    self.chat_status.set_foreground_color(theme::theme().error);
                    // Restore the saved switch after a failed settings request.
                    self.enabled
                        .set_value(self.state.borrow()["enabled"] == true);
                }
            }
        }
        self.update_buttons();
        if self.chat.is_shown() && (!self.parent.is_shown() || self.parent.is_iconized()) {
            self.chat.show(false);
        }
        let seconds = if self.active.get()
            || self.chat.is_shown()
            || self.state.borrow()["running"] == true
        {
            1
        } else {
            5
        };
        if self.last_poll.get().elapsed() >= Duration::from_secs(seconds) {
            self.fetch();
        }
    }

    fn apply(&self, value: Value) {
        let enabled = value["enabled"] == true;
        let ready = value["runtimeReady"] == true;
        let logged_in = value["loggedIn"] == true;
        let old = self.state.borrow();
        if (old["loggedIn"] == true && !logged_in)
            || (old["conversationId"].is_string()
                && old["conversationId"] != value["conversationId"])
        {
            self.input.clear();
        }
        drop(old);
        self.enabled.set_value(enabled);
        self.enabled.enable(true);
        self.runtime.set_label(if ready {
            "天才搭档 Server · 已就绪"
        } else if enabled {
            "天才搭档 Server · 尚未就绪"
        } else {
            "天才搭档 Server · 未启用"
        });
        self.account.set_label(if logged_in {
            "小分队账户 · 已登录"
        } else {
            "小分队账户 · 请先登录"
        });
        self.model
            .set_label(&if value["modelServiceReady"] == true {
                format!(
                    "模型服务 · {}\n费用由管理员承担",
                    value["model"]["label"]
                        .as_str()
                        .or(value["model"]["id"].as_str())
                        .unwrap_or("管理员默认模型")
                )
            } else if !logged_in {
                "模型服务 · 登录后读取".into()
            } else {
                "模型服务 · 尚未配置或管理员未启用".into()
            });
        let detail = value["detail"]
            .as_str()
            .unwrap_or("请启用天才搭档后准备运行端");
        self.detail.set_label(detail);
        self.detail
            .set_foreground_color(theme::theme().ink_secondary);
        self.chat_status.set_label(if value["running"] == true {
            "正在处理… · 收起后继续运行"
        } else {
            detail.lines().next().unwrap_or(detail)
        });
        self.chat_status.set_tooltip(detail);
        self.chat_status
            .set_foreground_color(theme::theme().ink_secondary);
        let mut text = String::new();
        if let Some(messages) = value["messages"].as_array() {
            for message in messages {
                let role = match message["role"].as_str() {
                    Some("user") => "你",
                    Some("tool") => "配置操作",
                    Some("system") => "提示",
                    _ => "天才搭档",
                };
                let content = message["text"].as_str().unwrap_or("");
                if !content.is_empty() {
                    text.push_str(&format!("{role}\n{content}\n\n"));
                }
            }
        }
        if text.is_empty() {
            text = "你好，我是天才搭档。可以帮你配置模型、渠道及客户端，也可以介绍 Hub 和天才小分队的用法。\n\n".into();
        }
        if *self.last_text.borrow() != text {
            let (start, end) = self.transcript.get_selection();
            let at_end = end.max(start) as usize >= self.last_text.borrow().chars().count();
            self.transcript.change_value(&text);
            if at_end {
                self.transcript.set_insertion_point_end();
            } else {
                self.transcript.set_selection(start, end);
            }
            *self.last_text.borrow_mut() = text;
        }
        let deletion = &value["pendingDeletion"];
        let credential = &value["pendingCredential"];
        let has_deletion = deletion.is_object();
        let has_credential = credential.is_object();
        self.approve.show(has_deletion);
        self.reject.show(has_deletion);
        self.credential.show(has_credential);
        self.pending.show(has_deletion || has_credential);
        if has_deletion {
            let mut summary = deletion["summary"]
                .as_str()
                .unwrap_or("删除以下配置？")
                .to_string();
            if deletion["summary"].as_str().is_none() {
                if let Some(items) = deletion["items"].as_array() {
                    for item in items {
                        let item = item
                            .as_str()
                            .or_else(|| item["label"].as_str())
                            .or_else(|| item["name"].as_str());
                        if let Some(item) = item {
                            summary.push_str(&format!("\n• {item}"));
                        }
                    }
                }
            }
            self.pending_text.set_label(&summary);
        } else if has_credential {
            self.pending_text.set_label(&format!(
                "需要输入{}。凭据只交给本机配置操作，对话中使用引用和掩码。",
                credential["label"].as_str().unwrap_or("凭据")
            ));
        }
        self.references.set_label(
            &value["credentialReferences"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            format!(
                                "{} · {}",
                                item["label"].as_str().unwrap_or("凭据"),
                                item["mask"].as_str().unwrap_or("已保存引用")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
        );
        self.shell.set_assistant_ready(enabled && ready);
        if !enabled {
            self.chat.show(false);
        }
        *self.state.borrow_mut() = value;
        let width = (self.chat.get_client_size().width - self.chat.from_dip(32)).max(120);
        self.chat_status.wrap(width);
        self.pending_text.wrap(width);
        self.pending_scroll.layout();
        self.pending_scroll.fit_inside();
        let page_width = (self.page.get_client_size().width - self.page.from_dip(30))
            .max(self.page.from_dip(240));
        for text in [
            self.runtime,
            self.account,
            self.model,
            self.detail,
            self.references,
        ] {
            text.wrap(page_width);
        }
        self.page_scroll.layout();
        self.page_scroll.fit_inside();
        self.page.layout();
        self.chat.layout();
        self.update_buttons();
        self.apply_navigation();
    }

    fn update_buttons(&self) {
        let value = self.state.borrow();
        let busy = self.acting.load(Ordering::SeqCst);
        let running = value["running"] == true;
        let ready = value["enabled"] == true && value["runtimeReady"] == true;
        self.prepare
            .enable(value["enabled"] == true && !busy && value["runtimeReady"] != true);
        self.prepare.show(value["runtimeReady"] != true);
        self.uninstall
            .show(value["runtime"]["installed"] == true || value["installed"] == true);
        self.uninstall.enable(
            !busy && (value["runtime"]["canUninstall"] == true || value["canUninstall"] == true),
        );
        self.open.enable(ready);
        self.clean.enable(!busy);
        self.send.enable(
            ready
                && value["loggedIn"] == true
                && value["modelServiceReady"] == true
                && value["cleanupPending"] != true
                && !running
                && !busy,
        );
        self.stop.enable(running && !busy);
        self.clear.enable(!busy);
        self.approve.enable(!busy);
        self.reject.enable(!busy);
        self.credential.enable(!busy);
        self.enabled.enable(value.get("enabled").is_some() && !busy);
    }

    fn apply_navigation(&self) {
        let navigation = self.state.borrow()["navigation"].clone();
        let Some(id) = navigation["id"].as_str() else {
            return;
        };
        if self.navigated.borrow().as_deref() == Some(id) {
            return;
        }
        let page = match navigation["page"].as_str() {
            Some("overview") => Some(PageId::Overview),
            Some("squad") => Some(PageId::Squad),
            Some("sources") => Some(PageId::Sources),
            Some("settings") => Some(PageId::Settings),
            Some("help") => Some(PageId::Help),
            Some("assistant") => Some(PageId::Assistant),
            Some("client") => navigation["client"]
                .as_str()
                .and_then(|key| ClientId::ALL.into_iter().find(|id| id.key() == key))
                .map(PageId::Client),
            _ => None,
        };
        if let Some(page) = page {
            if self.acting.load(Ordering::SeqCst) {
                return;
            }
            *self.navigated.borrow_mut() = Some(id.to_string());
            self.collapse();
            self.shell.navigate(page);
            self.post("navigation/ack", json!({"id":id}), Action::Navigate);
        }
    }

    fn enter_credential(&self) {
        let request = self.state.borrow()["pendingCredential"].clone();
        let Some(nonce) = request["nonce"].as_str().map(str::to_string) else {
            return;
        };
        let name = request["label"].as_str().unwrap_or("API 凭据").to_string();
        let dialog = Dialog::builder(&self.chat, "输入凭据")
            .with_size(self.chat.from_dip(420), self.chat.from_dip(270))
            .build();
        dialog.set_background_color(theme::theme().bg_card);
        dialog.set_font(&theme::font(theme::TextRole::Body));
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let caption = label(&dialog, &name, theme::TextRole::Body);
        col.add(
            &caption,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            dialog.from_dip(20),
        );
        let secret = TextCtrl::builder(&dialog)
            .with_style(TextCtrlStyle::Password)
            .build();
        secret.set_hint("在本机填写，不会发送到聊天对话");
        col.add(
            &secret,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dialog.from_dip(20),
        );
        let status = label(
            &dialog,
            "对话中只提供凭据引用与掩码。",
            theme::TextRole::Caption,
        );
        status.set_foreground_color(theme::theme().ink_secondary);
        col.add(
            &status,
            1,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
            dialog.from_dip(20),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add_stretch_spacer(1);
        let cancel = Button::builder(&dialog)
            .with_id(ID_CANCEL)
            .with_label("取消")
            .with_kind(ButtonKind::Ghost)
            .build();
        let save = Button::builder(&dialog).with_label("使用此凭据").build();
        row.add(&cancel, 0, SizerFlag::Right, dialog.from_dip(8));
        row.add(&save, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            dialog.from_dip(20),
        );
        dialog.set_sizer(col, false);
        let submit_nonce = nonce.clone();
        let job = dialog_job::bind(dialog, save, status, self.api.clone(), move || {
            let text = secret.get_value();
            if text.trim().is_empty() {
                return Err("请填写凭据".into());
            }
            Ok((
                "/api/desktop/assistant/credential".into(),
                json!({"nonce":submit_nonce,"label":name,"secret":text}),
            ))
        });
        dialog.centre();
        secret.set_focus();
        let saved = dialog.show_modal() == ID_OK;
        secret.clear();
        drop(job);
        dialog.destroy();
        if saved {
            self.fetch();
        } else {
            self.post(
                "credential",
                json!({"nonce":nonce,"cancelled":true}),
                Action::Status,
            );
        }
    }
}

fn label(parent: &impl WxWidget, text: &str, role: theme::TextRole) -> StaticText {
    let label = StaticText::builder(parent).with_label(text).build();
    label.set_font(&theme::font(role));
    label.set_foreground_color(theme::theme().ink_primary);
    label
}

#[cfg(all(test, windows))]
#[test]
fn assistant_readiness_account_switch_and_pending_actions_fit_the_native_small_window() {
    wxdragon::app::render_offscreen(|| {
        for (mode,suffix) in [(theme::ThemeMode::Light,"light"),(theme::ThemeMode::Dark,"dark")] {
            theme::init(mode);
            let parent = Frame::builder().with_title("hidden synthetic assistant").build();
            parent.set_client_size(visual::dip_size(&parent,960,640));
            let host = Panel::builder(&parent).build();
            let shell = Shell::new(&host,&parent);
            let api = ApiClient::new("http://127.0.0.1:1".into(),super::text::GuiText::new(super::text::GuiLocale::ZhCn));
            // No timer, HTTP request, product process, model or login is started.
            let ui = AssistantUi::build(&parent,&shell,api,false);
            let column=BoxSizer::builder(Orientation::Vertical).build();
            column.add(&shell.panel,1,SizerFlag::Expand,0);
            host.set_sizer(column,false);
            let outer=BoxSizer::builder(Orientation::Vertical).build();
            outer.add(&host,1,SizerFlag::Expand,0);
            parent.set_sizer(outer,false);
            shell.navigate(PageId::Assistant);
            let mut state=json!({"conversationId":"fixture-a","enabled":false,"runtimeReady":true,"loggedIn":true,"modelServiceReady":true,
                "runtime":{"installed":true,"canUninstall":true},"model":{"id":"fixture-model","label":"管理员模型（合成）"},"messages":[],"running":false,"detail":"合成状态，无真实调用"});
            ui.apply(state.clone());
            assert!(!shell.assistant_launcher.is_shown());
            assert!(ui.uninstall.is_enabled());
            state["enabled"]=true.into();state["runtime"]["canUninstall"]=false.into();state["running"]=true.into();
            state["messages"]=json!([{"id":"u1","role":"user","text":"帮我调整模型配置"},{"id":"a1","role":"assistant","text":"请确认以下删除操作"}]);
            state["pendingDeletion"]=json!({"nonce":"fixture-delete","summary":"确认删除以下合成配置？","items":(0..20).map(|i|format!("合成渠道 {i}")).collect::<Vec<_>>()});
            ui.apply(state.clone());
            assert!(shell.assistant_launcher.is_shown());
            assert!(ui.stop.is_enabled());assert!(!ui.send.is_enabled());
            assert!(!ui.uninstall.is_enabled());assert!(ui.approve.is_shown());
            ui.chat.layout();
            let size=ui.chat.get_client_size();
            for control in [&ui.input as &dyn WxWidget,&ui.send,&ui.stop,&ui.clear] {
                let screen=control.client_to_screen(Point::new(0,0));
                let p=ui.chat.screen_to_client(screen);let control_size=control.get_size();
                assert!(p.x>=0 && p.y>=0 && p.x+control_size.width<=size.width+1 && p.y+control_size.height<=size.height+1,"assistant action clipped: {p:?} {control_size:?} in {size:?}");
            }
            assert!(ui.transcript.get_value().contains("帮我调整模型配置"));
            ui.input.set_value("旧账户尚未发送的草稿");
            state["conversationId"]="fixture-b".into();state["messages"]=json!([]);state["pendingDeletion"]=Value::Null;state["running"]=false.into();
            ui.apply(state);
            assert!(ui.input.get_value().is_empty());
            assert!(!ui.transcript.get_value().contains("帮我调整模型配置"));
            assert!(!ui.chat.is_shown() && !parent.is_shown());
            super::native_model_review::capture(&ui.chat,&format!("assistant-chat-{suffix}"));
            ui.chat.destroy();parent.destroy();
        }
    }).unwrap();
}
