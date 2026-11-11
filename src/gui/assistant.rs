//! Native assistant controls. The backend owns the temporary conversation and
//! configuration operations; this view never stores keys in chat messages.
use super::{
    api::ApiClient,
    assistant_markdown::MessageBody,
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
#[cfg(all(test, windows))]
use wxdragon::widgets::textctrl::TextCtrl as MessageText;
use wxdragon::{dc::GraphicsDC, prelude::*, timer::Timer};

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
    Credential,
    Navigate,
}
type Inbox = Arc<Mutex<Vec<(Action, Result<Value, String>)>>>;

/// An explicit enable or module visit may offer installation once. Status
/// polling itself never requests consent and cannot repeat a declined offer.
#[derive(Default)]
struct InstallationOffer {
    pending: bool,
}
impl InstallationOffer {
    fn request(&mut self) {
        self.pending = true;
    }
    fn after_status(&mut self, state: &Value, busy: bool, visible: bool) -> bool {
        if !self.pending || busy || !visible || !state["enabled"].is_boolean() {
            return false;
        }
        self.pending = false;
        state["enabled"] == true && !runtime_ready(state)
    }
}
fn runtime_ready(state: &Value) -> bool {
    state["runtimeReady"]
        .as_bool()
        .unwrap_or_else(|| state["runtime"]["ready"] == true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChatMessage {
    id: String,
    role: String,
    text: String,
}

fn chat_messages(state: &Value) -> Vec<ChatMessage> {
    state["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, item)| {
            let text = item["text"].as_str().unwrap_or_default();
            (!text.is_empty()).then(|| ChatMessage {
                id: item["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| index.to_string()),
                role: item["role"].as_str().unwrap_or("assistant").to_owned(),
                text: text.to_owned(),
            })
        })
        .collect()
}

pub(super) fn scroll_transcript_from(widget: &impl WindowEvents, transcript: ScrolledWindow) {
    widget.on_mouse_wheel(move |event| {
        // wxDragon represents wxEVT_MOUSEWHEEL as MouseButton. Forward the
        // original wheel data instead of replacing native scroll settings.
        let handled = match &event {
            WindowEventData::MouseButton(mouse) => {
                transcript.forward_mouse_wheel(&mouse.event.event)
            }
            WindowEventData::General(wheel) => transcript.forward_mouse_wheel(wheel),
            _ => false,
        };
        event.skip(!handled);
    });
}

/// Each turn retains its native selectable Markdown blocks while streaming.
struct MessageCard {
    message: ChatMessage,
    row: Panel,
    bubble: Panel,
    body: MessageBody,
    caption: Option<StaticText>,
    layout_width: Cell<i32>,
}

impl MessageCard {
    fn new(parent: &ScrolledWindow, message: ChatMessage) -> Self {
        let t = theme::theme();
        let user = message.role == "user";
        let background = if user { t.accent } else { t.bg_card_alt };
        let foreground = if user { t.on_accent } else { t.ink_primary };
        let row = Panel::builder(parent).build();
        row.set_background_color(t.bg_card);
        let bubble = Panel::builder(&row).build();
        paint_chat_surface(bubble, background, None);
        let content = BoxSizer::builder(Orientation::Vertical).build();
        let caption = match message.role.as_str() {
            "tool" => Some("操作结果"),
            "system" => Some("提示"),
            _ => None,
        }
        .map(|text| {
            let caption = label(&bubble, text, theme::TextRole::Caption);
            caption.set_background_color(background);
            caption.set_foreground_color(t.ink_secondary);
            content.add(
                &caption,
                0,
                SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
                bubble.from_dip(12),
            );
            caption
        });
        let body = MessageBody::new(&bubble, &message.text, foreground, background, *parent);
        body.panel
            .set_name(if user { "你的消息" } else { "回复内容" });
        content.add(
            &body.panel,
            0,
            SizerFlag::Expand | SizerFlag::All,
            bubble.from_dip(12),
        );
        bubble.set_sizer(content, false);
        let line = BoxSizer::builder(Orientation::Horizontal).build();
        if user {
            line.add_stretch_spacer(1);
        }
        line.add(&bubble, 0, SizerFlag::default(), 0);
        if !user {
            line.add_stretch_spacer(1);
        }
        row.set_sizer(line, false);
        scroll_transcript_from(&bubble, *parent);
        scroll_transcript_from(&row, *parent);
        if let Some(caption) = caption {
            scroll_transcript_from(&caption, *parent);
        }
        Self {
            message,
            row,
            bubble,
            body,
            caption,
            layout_width: Cell::new(0),
        }
    }

    fn update(&mut self, message: ChatMessage) {
        if self.message.text != message.text {
            self.body.update(&message.text);
            self.layout_width.set(0);
        }
        self.message = message;
    }

    fn layout(&self, width: i32) {
        if self.layout_width.replace(width) == width {
            return;
        }
        let padding = self.row.from_dip(24);
        let max_width = (width - self.row.from_dip(40)).max(1);
        let primary = self.body.primary_text();
        let text_width = self
            .message
            .text
            .lines()
            .map(|line| primary.get_text_extent(line).width)
            .max()
            .unwrap_or(0);
        let bubble_width = (text_width + padding)
            .max(self.row.from_dip(92))
            .min(max_width);
        let body_width = (bubble_width - padding).max(1);
        let height = self.body.layout(body_width);
        let caption_height = self
            .caption
            .map(|caption| {
                caption.wrap(body_width);
                caption.get_best_size().height + self.bubble.from_dip(12)
            })
            .unwrap_or(0);
        let bubble_height = height + padding + caption_height;
        self.bubble
            .set_min_size(Size::new(bubble_width, bubble_height));
        self.row.set_min_size(Size::new(-1, bubble_height));
        self.row.layout();
        self.bubble.layout();
    }
}

fn paint_chat_surface(panel: Panel, fill: Colour, border: Option<Colour>) {
    panel.set_background_color(fill);
    panel.set_background_style(BackgroundStyle::Paint);
    visual::redraw_on_resize(panel);
    panel.on_paint(move |_| {
        let base = AutoBufferedPaintDC::new(&panel);
        let dc = GraphicsDC::new(&base);
        let scale = visual::scale(&panel);
        let c = visual::Canvas::new(&dc, scale);
        let size = panel.get_client_size();
        c.clear(
            panel
                .get_parent()
                .map(|p| p.get_background_color())
                .unwrap_or(theme::theme().bg_app),
        );
        c.rounded(
            Rect::new(
                0,
                0,
                (size.width as f64 / scale) as i32,
                (size.height as f64 / scale) as i32,
            ),
            theme::SURFACE_RADIUS,
            fill,
            border,
        );
    });
}

fn fit_chat(area: Rect, preferred: Size) -> Rect {
    let width = preferred.width.min(area.width).max(1);
    let height = preferred.height.min(area.height).max(1);
    Rect::new(
        area.x + area.width - width,
        area.y + area.height - height,
        width,
        height,
    )
}

const CHAT_MOTION_DURATION: Duration = Duration::from_millis(160);

/// Normalized displacement, separate from message layout. Retargeting samples
/// the current position so rapid open/close actions do not jump to an endpoint.
#[derive(Clone, Copy, Default)]
struct ChatMotion {
    from: f64,
    target: f64,
    started: Option<Instant>,
}

impl ChatMotion {
    fn position(self, now: Instant) -> f64 {
        let Some(started) = self.started else {
            return self.target;
        };
        let elapsed = now.saturating_duration_since(started).as_secs_f64();
        let progress = (elapsed / CHAT_MOTION_DURATION.as_secs_f64()).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.from + (self.target - self.from) * eased
    }

    fn retarget(&mut self, open: bool, animate: bool, now: Instant) {
        let target = if open { 0.0 } else { 1.0 };
        if animate && self.target == target {
            return;
        }
        let position = self.position(now);
        self.from = if animate { position } else { target };
        self.target = target;
        self.started = (animate && self.from != target).then_some(now);
    }

    fn finished(self, now: Instant) -> bool {
        self.started
            .is_none_or(|started| now.saturating_duration_since(started) >= CHAT_MOTION_DURATION)
    }
}

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
    chat: Panel,
    chat_status: StaticText,
    transcript: ScrolledWindow,
    welcome: Panel,
    welcome_text: StaticText,
    cards: Rc<RefCell<Vec<MessageCard>>>,
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
    state: Rc<RefCell<Value>>,
    inbox: Inbox,
    fetching: Arc<AtomicBool>,
    acting: Arc<AtomicBool>,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    motion_timer: Rc<RefCell<Option<Timer<Panel>>>>,
    motion: Rc<Cell<ChatMotion>>,
    last_poll: Rc<Cell<Instant>>,
    active: Rc<Cell<bool>>,
    navigated: Rc<RefCell<Option<String>>>,
    installation_offer: Rc<RefCell<InstallationOffer>>,
    installation_prompt_open: Rc<Cell<bool>>,
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

        // A normal child of the content panel is clipped by Hub and follows
        // its minimize/tray lifecycle. It is deliberately outside the sizer.
        let chat = Panel::builder(&shell.assistant_host).build();
        chat.add_style(WindowStyle::ClipChildren);
        chat.set_name("对话浮层");
        chat.hide();
        paint_chat_surface(chat, t.bg_card, Some(t.divider));
        chat.set_font(&theme::font(theme::TextRole::Body));
        let root = chat;
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let bar = BoxSizer::builder(Orientation::Horizontal).build();
        let chat_status = label(&root, "正在连接…", theme::TextRole::Caption);
        chat_status.set_foreground_color(t.ink_secondary);
        let clear = Button::builder(&root)
            .with_icon(visual::Icon::Trash)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(32, 32))
            .build();
        clear.set_name("清空本次对话");
        clear.set_tooltip("清空本次对话；正在执行的任务会先停止");
        let collapse = Button::builder(&root)
            .with_icon(visual::Icon::WindowMinimize)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(32, 32))
            .build();
        collapse.set_name("收起对话");
        collapse.set_tooltip("收起对话，保留消息和未发送内容（Esc）");
        bar.add(&chat_status, 1, SizerFlag::AlignCenterVertical, 0);
        bar.add(
            &clear,
            0,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            root.from_dip(4),
        );
        bar.add(&collapse, 0, SizerFlag::AlignCenterVertical, 0);
        col.add_sizer(
            &bar,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            root.from_dip(12),
        );
        let transcript = ScrolledWindow::builder(&root)
            .with_style(ScrolledWindowStyle::VScroll)
            .build();
        transcript.set_name("assistant-conversation");
        transcript.set_background_color(t.bg_card);
        transcript.set_scroll_rate(0, root.from_dip(8));
        let welcome = Panel::builder(&transcript).build();
        welcome.set_background_color(t.bg_card);
        let intro = BoxSizer::builder(Orientation::Vertical).build();
        let welcome_title = label(&welcome, "想先处理什么？", theme::TextRole::Title);
        scroll_transcript_from(&welcome, transcript);
        scroll_transcript_from(&welcome_title, transcript);
        intro.add(&welcome_title, 0, SizerFlag::Bottom, root.from_dip(8));
        let welcome_text = label(
            &welcome,
            "可以检查模型、调整配置，也可以了解常用功能。",
            theme::TextRole::Body,
        );
        welcome_text.set_foreground_color(t.ink_secondary);
        scroll_transcript_from(&welcome_text, transcript);
        intro.add(
            &welcome_text,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            root.from_dip(20),
        );
        let mut suggestions = Vec::new();
        for (text, prompt) in [
            ("查看当前模型配置", "帮我查看当前模型和渠道配置。"),
            ("了解 Hub 的常用功能", "介绍一下 Hub 的常用功能和使用方法。"),
        ] {
            let button = Button::builder(&welcome)
                .with_label(text)
                .with_kind(ButtonKind::Secondary)
                .build();
            intro.add(&button, 0, SizerFlag::Bottom, root.from_dip(8));
            suggestions.push((button, prompt));
            scroll_transcript_from(&button, transcript);
        }
        welcome.set_sizer(intro, false);
        let messages = BoxSizer::builder(Orientation::Vertical).build();
        messages.add(
            &welcome,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            root.from_dip(24),
        );
        transcript.set_sizer(messages, false);
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
        let input = TextCtrl::builder(&root)
            .with_style(TextCtrlStyle::MultiLine)
            .with_size(Size::new(-1, 68))
            .build();
        input.set_name("assistant-message-input");
        input.set_tooltip("输入消息，Ctrl+Enter 发送；Esc 收起对话");
        input.set_hint("输入消息…");
        col.add(
            &input,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
            root.from_dip(12),
        );
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        let shortcut = label(&root, "Ctrl+Enter 发送", theme::TextRole::Caption);
        shortcut.set_foreground_color(t.ink_muted);
        let stop = Button::builder(&root)
            .with_label("停止")
            .with_kind(ButtonKind::Secondary)
            .build();
        let send = Button::builder(&root)
            .with_label("发送")
            .with_kind(ButtonKind::Primary)
            .build();
        row.add(&shortcut, 0, SizerFlag::AlignCenterVertical, 0);
        row.add_stretch_spacer(1);
        row.add(&stop, 0, SizerFlag::default(), 0);
        row.add(&send, 0, SizerFlag::default(), 0);
        col.add_sizer(
            &row,
            0,
            SizerFlag::Expand | SizerFlag::All,
            root.from_dip(12),
        );
        root.set_sizer(col, false);

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
            welcome,
            welcome_text,
            cards: Default::default(),
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
            state: Default::default(),
            inbox: Default::default(),
            fetching: Default::default(),
            acting: Default::default(),
            timer: Default::default(),
            motion_timer: Default::default(),
            motion: Default::default(),
            last_poll: Rc::new(Cell::new(Instant::now() - Duration::from_secs(30))),
            active: Default::default(),
            navigated: Default::default(),
            installation_offer: Default::default(),
            installation_prompt_open: Default::default(),
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
        let u = ui.clone();
        shell.assistant_launcher.on_click(move |_| {
            if u.chat.is_shown() && u.motion.get().target == 0.0 {
                u.collapse();
            } else {
                u.show_chat();
            }
        });
        let u = ui.clone();
        open.on_click(move |_| u.show_chat());
        for (button, prompt) in suggestions {
            let u = ui.clone();
            button.on_click(move |_| {
                u.input.set_value(prompt);
                u.input.set_insertion_point_end();
                u.input.set_focus();
            });
            ui.bind_collapse_key(&button);
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
            } else if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(9) && !key.shift_down())
                && !u.send.is_enabled() && !u.stop.is_enabled()
            {
                if u.clear.is_enabled() { u.clear.set_focus(); } else { collapse.set_focus(); }
            } else {
                event.skip(true);
            }
        });
        let u = ui.clone();
        stop.on_click(move |_| u.post("stop", json!({}), Action::Stop));
        for button in [clear, clean] {
            let u = ui.clone();
            button.on_click(move |_| {
                u.finish_chat_motion();
                u.post("clear", json!({}), Action::Clear);
            });
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
        for button in [collapse, clear, stop, send, approve, reject, credential] {
            let u = ui.clone();
            button.on_key_down(move |event| {
                if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) {
                    u.collapse();
                } else if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(9) && key.shift_down())
                    && (button.handle_ptr() == clear.handle_ptr()
                        || (button.handle_ptr() == collapse.handle_ptr() && !u.clear.is_enabled()))
                {
                    if u.stop.is_shown() && u.stop.is_enabled() { u.stop.set_focus(); }
                    else if u.send.is_shown() && u.send.is_enabled() { u.send.set_focus(); }
                    else { u.input.set_focus(); }
                } else if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(9) && !key.shift_down())
                    && (button.handle_ptr() == send.handle_ptr() || button.handle_ptr() == stop.handle_ptr())
                {
                    if u.clear.is_enabled() { u.clear.set_focus(); } else { collapse.set_focus(); }
                } else {
                    event.skip(true);
                }
            });
        }
        ui.bind_collapse_key(&chat);
        ui.bind_collapse_key(&transcript);
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
            u.layout_chat();
            event.skip(true);
        });
        let u = ui.clone();
        shell.assistant_host.on_size(move |event| {
            u.position_chat();
            event.skip(true);
        });
        // Timer callbacks are bound to their owner rather than the timer ID.
        // Keep motion on chat, separate from the page's status polling timer.
        let timer = Timer::new(&chat);
        let u = ui.clone();
        timer.on_tick(move |_| u.tick_chat_motion());
        ui.motion_timer.borrow_mut().replace(timer);
        let timer_slot = ui.motion_timer.clone();
        let motion = ui.motion.clone();
        let chat_ptr = chat.handle_ptr();
        chat.on_destroy(move |event| {
            // wx destroy events also bubble from a removed message child.
            let own_destroy = matches!(&event, WindowEventData::General(raw)
                if raw.get_event_object().is_some_and(|object| object.as_ptr() == chat_ptr));
            if own_destroy {
                timer_slot.borrow_mut().take();
                motion.set(ChatMotion::default());
            }
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
        ui.position_chat();
        ui
    }

    pub fn enter(&self, page: PageId) {
        let entering = page == PageId::Assistant;
        let was_active = self.active.replace(entering);
        if entering {
            if !was_active {
                self.installation_offer.borrow_mut().request();
            }
            self.fetch();
        }
        if self.chat.is_shown() {
            self.position_chat();
        }
    }

    fn show_chat(&self) {
        let state = self.state.borrow();
        if state["enabled"] != true || !runtime_ready(&state) {
            return;
        }
        drop(state);
        self.animate_chat(true);
        self.input.set_focus();
        self.fetch();
    }

    fn collapse(&self) {
        self.animate_chat(false);
        if self.parent.is_shown() && self.shell.assistant_launcher.is_shown() {
            self.shell.assistant_launcher.set_focus();
        }
    }

    fn animate_chat(&self, open: bool) {
        let mut motion = self.motion.get();
        let area = self.shell.assistant_overlay_area();
        let room_to_move = fit_chat(area, visual::dip_size(&self.chat, 420, 560)).y > area.y;
        let animate = theme::motion_enabled()
            && self.parent.is_shown()
            && !self.parent.is_iconized()
            && room_to_move
            && (open || self.chat.is_shown());
        if open && !self.chat.is_shown() {
            motion = ChatMotion {
                from: 1.0,
                target: 1.0,
                started: None,
            };
        }
        motion.retarget(open, animate, Instant::now());
        self.motion.set(motion);
        self.position_chat();
        if open {
            self.chat.show(true);
            self.chat.raise_child_overlay();
        }
        let running = motion.started.is_some()
            && self
                .motion_timer
                .borrow()
                .as_ref()
                .is_some_and(|timer| timer.start(16, false));
        if !running {
            self.finish_chat_motion();
        }
    }

    fn tick_chat_motion(&self) {
        if !self.parent.is_shown()
            || self.parent.is_iconized()
            || !theme::motion_enabled()
            || self.motion.get().finished(Instant::now())
        {
            self.finish_chat_motion();
        } else {
            self.position_chat();
        }
    }

    fn finish_chat_motion(&self) {
        if let Some(timer) = self.motion_timer.borrow().as_ref() {
            timer.stop();
        }
        let close = self.motion.get().target == 1.0;
        self.motion.set(ChatMotion::default());
        if close {
            self.chat.hide();
        }
        self.position_chat();
    }

    fn bind_collapse_key(&self, widget: &impl WindowEvents) {
        let u = self.clone();
        widget.on_key_down(move |event| {
            if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(27)) {
                u.collapse();
            } else {
                event.skip(true);
            }
        });
    }

    fn position_chat(&self) {
        let area = self.shell.assistant_overlay_area();
        let mut bounds = fit_chat(area, visual::dip_size(&self.chat, 420, 560));
        // Move only through unused space above the final bounds. Small Hub
        // windows keep the full message/input area and all chrome reachable.
        let travel = self.chat.from_dip(14).min((bounds.y - area.y).max(0));
        bounds.y -= (travel as f64 * self.motion.get().position(Instant::now())).round() as i32;
        let position = self.chat.get_position();
        let size = self.chat.get_size();
        if size.width != bounds.width || size.height != bounds.height {
            self.chat
                .set_size_with_pos(bounds.x, bounds.y, bounds.width, bounds.height);
        } else if position.x != bounds.x || position.y != bounds.y {
            self.chat.move_window(bounds.x, bounds.y);
        }
        if self.chat.is_shown() {
            self.chat.raise_child_overlay();
        }
    }

    fn at_latest_message(&self) -> bool {
        let scroll = self.transcript.view_start();
        let (_, unit) = self.transcript.get_scroll_pixels_per_unit();
        scroll.y * unit.max(1) + self.transcript.get_client_size().height
            >= self.transcript.get_virtual_size().height - self.transcript.from_dip(20)
    }

    fn layout_chat(&self) {
        self.layout_chat_preserving(self.at_latest_message(), self.transcript.view_start());
    }

    fn layout_chat_preserving(&self, follow: bool, scroll: Point) {
        let width = (self.chat.get_client_size().width - self.chat.from_dip(32)).max(1);
        self.welcome_text.wrap(width);
        self.welcome.layout();
        self.pending_text.wrap(width);
        let pending_height = (self.chat.get_client_size().height / 6)
            .clamp(self.chat.from_dip(48), self.chat.from_dip(96));
        self.pending_scroll
            .set_min_size(Size::new(-1, pending_height));
        self.pending_scroll
            .set_max_size(Size::new(-1, pending_height));
        self.chat.layout();
        self.pending_scroll.layout();
        self.pending_scroll.fit_inside();
        let width = (self.transcript.get_client_size().width - self.transcript.from_dip(8)).max(1);
        for card in self.cards.borrow().iter() {
            card.layout(width);
        }
        self.transcript.layout();
        self.transcript.fit_inside();
        if follow {
            let (_, unit) = self.transcript.get_scroll_pixels_per_unit();
            self.transcript
                .scroll_coords(-1, self.transcript.get_virtual_size().height / unit.max(1));
        } else {
            self.transcript.scroll_point(scroll);
        }
    }

    fn sync_messages(&self, state: &Value) {
        let messages = chat_messages(state);
        if self
            .cards
            .borrow()
            .iter()
            .map(|card| &card.message)
            .eq(messages.iter())
        {
            return;
        }
        let follow = self.at_latest_message();
        let scroll = self.transcript.view_start();
        self.transcript.freeze();
        let mut cards = self.cards.borrow_mut();
        let retained = cards
            .iter()
            .zip(&messages)
            .take_while(|(card, message)| {
                card.message.id == message.id && card.message.role == message.role
            })
            .count();
        let structure_changed = retained != cards.len() || retained != messages.len();
        for card in cards.drain(retained..) {
            card.row.destroy();
        }
        for (index, message) in messages.into_iter().enumerate() {
            if let Some(card) = cards.get_mut(index) {
                card.update(message);
            } else {
                let mut card = MessageCard::new(&self.transcript, message);
                let u = self.clone();
                card.body.on_escape(Rc::new(move || u.collapse()));
                cards.push(card);
            }
        }
        if structure_changed {
            self.welcome.show(cards.is_empty());
            let column = BoxSizer::builder(Orientation::Vertical).build();
            // Releasing the old sizer first detaches retained native children.
            // A window cannot be added to two wxSizers at the same time.
            self.transcript.set_sizer(column, true);
            column.add(
                &self.welcome,
                0,
                SizerFlag::Expand | SizerFlag::Top,
                self.chat.from_dip(24),
            );
            for card in cards.iter() {
                column.add(
                    &card.row,
                    0,
                    SizerFlag::Expand | SizerFlag::Bottom,
                    self.chat.from_dip(12),
                );
            }
            column.add_spacer(self.chat.from_dip(8));
        }
        drop(cards);
        self.layout_chat_preserving(follow, scroll);
        self.transcript.thaw();
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
        let (api, inbox, path) = (self.api.clone(), self.inbox.clone(), path.to_string());
        std::thread::spawn(move || {
            let result = api.desktop_post(&path, &body);
            if let Ok(mut slot) = inbox.lock() {
                slot.push((action, result));
            }
            wxdragon::wake_up_idle();
        });
    }

    fn send_message(&self) {
        let state = self.state.borrow();
        if state["running"] == true
            || state["enabled"] != true
            || !runtime_ready(&state)
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
        if self.state.borrow()["enabled"] != true
            || runtime_ready(&self.state.borrow())
            || self.acting.load(Ordering::SeqCst)
            || self.installation_prompt_open.replace(true)
        {
            return;
        }
        self.installation_offer.borrow_mut().pending = false;
        let prompt = MessageDialog::builder(&self.parent,
            "将从已有兼容程序准备共享的天才搭档 Server；本机没有可用来源时，从官方下载安装。\n\n天才搭档和天工协助共用这一份运行端，安装后长期保留。现在准备？",
            "安装天才搭档 Server")
            .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion).build();
        let accepted = prompt.show_modal() == ID_YES;
        prompt.destroy();
        self.installation_prompt_open.set(false);
        if accepted {
            self.detail.set_label("正在准备天才搭档 Server…");
            self.detail
                .set_foreground_color(theme::theme().ink_secondary);
            self.post_path(
                "/api/desktop/assistance/prepare",
                json!({"confirmed":true}),
                Action::Prepare,
            );
        } else {
            self.detail.set_label(
                "已取消准备；天才搭档 Server 尚未就绪。需要时点击“准备天才搭档 Server”。",
            );
            self.detail
                .set_foreground_color(theme::theme().ink_secondary);
            self.page_scroll.layout();
            self.page_scroll.fit_inside();
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
            if !matches!(action, Action::Status) {
                // Only the UI consumer releases this flag. Releasing it in the
                // worker races an enable response against its consent prompt.
                self.acting.store(false, Ordering::SeqCst);
            }
            if matches!(action, Action::Settings(true)) {
                self.installation_offer.borrow_mut().request();
            }
            match result {
                Ok(value) => {
                    if let Action::Send(text) = &action {
                        if self.input.get_value() == *text {
                            self.input.clear();
                        }
                    }
                    if matches!(action, Action::Clear) {
                        self.finish_chat_motion();
                        self.input.clear();
                    }
                    if value.get("enabled").is_some() {
                        self.apply(value);
                    } else {
                        self.last_poll.set(Instant::now() - Duration::from_secs(30));
                    }
                    if matches!(action, Action::Status | Action::Settings(_)) {
                        let offer = self.installation_offer.borrow_mut().after_status(
                            &self.state.borrow(),
                            self.acting.load(Ordering::SeqCst),
                            self.active.get()
                                && self.parent.is_shown()
                                && !self.parent.is_iconized(),
                        );
                        if offer {
                            self.confirm_install();
                        }
                    }
                }
                Err(error) => {
                    self.detail.set_label(&error);
                    self.detail.set_foreground_color(theme::theme().error);
                    self.chat_status.set_label("暂时无法完成，请重试");
                    self.chat_status.set_tooltip(&error);
                    self.chat_status.set_foreground_color(theme::theme().error);
                    // Restore the saved switch after a failed settings request.
                    self.enabled
                        .set_value(self.state.borrow()["enabled"] == true);
                    if matches!(action, Action::Settings(true)) {
                        // The setting may have saved before its status read
                        // failed. Check saved readiness before offering setup.
                        self.fetch();
                    }
                }
            }
        }
        self.update_buttons();
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
        let ready = runtime_ready(&value);
        let logged_in = value["loggedIn"] == true;
        let old = self.state.borrow();
        if (old["loggedIn"] == true && !logged_in)
            || (old["conversationId"].is_string()
                && old["conversationId"] != value["conversationId"])
        {
            self.finish_chat_motion();
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
        let detail = if enabled && !ready {
            value["runtime"]["detail"]
                .as_str()
                .unwrap_or("天才搭档 Server 尚未就绪，请确认准备运行端；取消不会下载或安装。")
        } else {
            value["detail"]
                .as_str()
                .unwrap_or("请启用天才搭档后准备运行端")
        };
        self.detail.set_label(detail);
        self.detail
            .set_foreground_color(theme::theme().ink_secondary);
        self.chat_status.set_label(if value["running"] == true {
            "正在处理…"
        } else if !ready {
            "运行端尚未就绪"
        } else if !logged_in {
            "请先登录小分队账户"
        } else if value["modelServiceReady"] != true {
            "模型服务尚未就绪"
        } else if value["cleanupPending"] == true {
            "正在清理临时内容…"
        } else {
            "可以开始对话"
        });
        self.chat_status.set_tooltip(detail);
        self.chat_status
            .set_foreground_color(theme::theme().ink_secondary);
        self.sync_messages(&value);
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
            self.finish_chat_motion();
            self.chat.show(false);
        }
        *self.state.borrow_mut() = value;
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
        self.position_chat();
        self.layout_chat();
        self.update_buttons();
        self.apply_navigation();
    }

    fn update_buttons(&self) {
        let value = self.state.borrow();
        let busy = self.acting.load(Ordering::SeqCst);
        let running = value["running"] == true;
        let installed_ready = runtime_ready(&value);
        let ready = value["enabled"] == true && installed_ready;
        self.prepare
            .enable(value["enabled"] == true && !busy && !installed_ready);
        self.prepare.show(!installed_ready);
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
        let actions_changed = self.stop.is_shown() != running;
        self.stop.show(running);
        self.send.show(!running);
        self.clear.enable(!busy);
        self.approve.enable(!busy);
        self.reject.enable(!busy);
        self.credential.enable(!busy);
        self.enabled.enable(value.get("enabled").is_some() && !busy);
        if actions_changed {
            self.layout_chat();
        }
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
                Action::Credential,
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

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn chat_motion_reverses_continuously_and_respects_reduced_motion() {
        let start = Instant::now();
        let mut motion = ChatMotion::default();
        motion.retarget(false, true, start);
        let reverse_at = start + Duration::from_millis(40);
        let intermediate = motion.position(reverse_at);
        assert!(intermediate > 0.0 && intermediate < 1.0);
        motion.retarget(true, true, reverse_at);
        assert_eq!(motion.position(reverse_at), intermediate);
        for milliseconds in [0, 16, 40, 80, 160, 320] {
            let value = motion.position(reverse_at + Duration::from_millis(milliseconds));
            assert!(value >= 0.0 && value <= intermediate);
        }
        assert_eq!(motion.position(reverse_at + CHAT_MOTION_DURATION), 0.0);
        assert!(motion.finished(reverse_at + CHAT_MOTION_DURATION));
        motion.retarget(false, true, reverse_at);
        motion.retarget(false, false, reverse_at + Duration::from_millis(20));
        assert!(motion.started.is_none());
        assert_eq!(motion.position(reverse_at), 1.0);
    }

    #[test]
    fn floating_chat_fits_normal_small_and_scaled_content_areas() {
        for (area, preferred) in [
            (Rect::new(20, 56, 940, 710), Size::new(420, 560)),
            (Rect::new(20, 56, 660, 390), Size::new(420, 560)),
            (Rect::new(20, 56, 300, 270), Size::new(420, 560)),
            (Rect::new(30, 84, 990, 585), Size::new(630, 840)),
        ] {
            let chat = fit_chat(area, preferred);
            assert!(chat.x >= area.x && chat.y >= area.y);
            assert_eq!(chat.x + chat.width, area.x + area.width);
            assert_eq!(chat.y + chat.height, area.y + area.height);
            assert!(chat.width > 0 && chat.width <= preferred.width);
            assert!(chat.height > 0 && chat.height <= preferred.height);
        }
        assert_eq!(
            fit_chat(Rect::new(20, 56, 300, 270), Size::new(420, 560)),
            Rect::new(20, 56, 300, 270)
        );
    }
}

#[cfg(test)]
mod installation_tests {
    use super::*;

    #[test]
    fn disabled_or_ready_server_does_not_offer_installation() {
        for state in [
            json!({"enabled":false,"runtimeReady":false}),
            json!({"enabled":true,"runtimeReady":true}),
            json!({"enabled":true,"runtime":{"ready":true}}),
        ] {
            let mut offer = InstallationOffer::default();
            offer.request();
            assert!(!offer.after_status(&state, false, true));
        }
    }

    #[test]
    fn missing_server_keeps_offer_until_idle_visible_and_then_only_once() {
        let state = json!({"enabled":true,"runtimeReady":false,"runtimeNeedsPreparation":true});
        let mut offer = InstallationOffer::default();
        assert!(!offer.after_status(&state, false, true));
        offer.request();
        assert!(!offer.after_status(&Value::Null, false, true));
        assert!(!offer.after_status(&state, true, true));
        assert!(!offer.after_status(&state, false, false));
        assert!(offer.after_status(&state, false, true));
        for _ in 0..4 {
            assert!(!offer.after_status(&state, false, true));
        }
        // Another explicit enable or visit permits a new human decision.
        offer.request();
        assert!(offer.after_status(&state, false, true));
    }

    #[test]
    fn nested_readiness_or_missing_preparation_flag_can_still_offer_setup() {
        for state in [
            json!({"enabled":true,"runtime":{"ready":false,"needsPreparation":true}}),
            json!({"enabled":true,"runtimeReady":false}),
        ] {
            let mut offer = InstallationOffer::default();
            offer.request();
            assert!(offer.after_status(&state, false, true));
        }
    }
}

#[cfg(all(test, windows))]
#[test]
fn assistant_account_switch_and_pending_actions_fit_the_hub_overlay() {
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
            // The hidden parent never receives the normal first-show layout.
            // Allocate each containing panel before sizing its child overlay.
            parent.layout();
            host.layout();
            shell.panel.layout();
            shell.assistant_host.layout();
            ui.position_chat();
            ui.layout_chat();
            shell.navigate(PageId::Assistant);
            let mut state=json!({"conversationId":"fixture-a","enabled":false,"runtimeReady":true,"loggedIn":true,"modelServiceReady":true,
                "runtime":{"installed":true,"canUninstall":true},"model":{"id":"fixture-model","label":"管理员模型（合成）"},"messages":[],"running":false,"detail":"合成状态，无真实调用"});
            ui.apply(state.clone());
            assert!(!shell.assistant_launcher.is_shown());
            assert!(shell.assistant_launcher.get_label().is_empty());
            assert_eq!(ui.chat.get_parent().unwrap().handle_ptr(),shell.assistant_host.handle_ptr());
            assert!(ui.uninstall.is_enabled());
            state["enabled"]=true.into();state["runtime"]["canUninstall"]=false.into();state["running"]=true.into();
            state["messages"]=json!([{"id":"u1","role":"user","text":"帮我调整模型配置"},{"id":"a1","role":"assistant","text":"请确认以下删除操作"}]);
            state["pendingDeletion"]=json!({"nonce":"fixture-delete","summary":"确认删除以下合成配置？","items":(0..20).map(|i|format!("合成渠道 {i}")).collect::<Vec<_>>()});
            ui.apply(state.clone());
            assert!(shell.assistant_launcher.is_shown());
            assert!(ui.stop.is_enabled());assert!(!ui.send.is_enabled());
            assert!(!ui.uninstall.is_enabled());assert!(ui.approve.is_shown());
            ui.position_chat();ui.layout_chat();
            let size=ui.chat.get_client_size();
            assert!(size.width>0 && size.height>0,"hidden overlay layout was not initialized: {size:?}");
            for control in [&ui.input as &dyn WxWidget,&ui.send,&ui.stop,&ui.clear] {
                if !control.is_shown() {continue;}
                let screen=control.client_to_screen(Point::new(0,0));
                let p=ui.chat.screen_to_client(screen);let control_size=control.get_size();
                assert!(p.x>=0 && p.y>=0 && p.x+control_size.width<=size.width+1 && p.y+control_size.height<=size.height+1,"assistant action clipped: {p:?} {control_size:?} in {size:?}");
            }
            assert!(ui.cards.borrow().iter().any(|card|card.body.primary_text().get_value().contains("帮我调整模型配置")));
            let first=ui.cards.borrow()[0].body.primary_text();
            first.set_selection(0,2);
            state["messages"][1]["text"]="请确认以下删除操作，确认后继续。".into();
            ui.apply(state.clone());
            assert_eq!(ui.cards.borrow()[0].body.primary_text().handle_ptr(),first.handle_ptr());
            assert_eq!(first.get_selection(),(0,2));
            ui.input.set_value("旧账户尚未发送的草稿");
            state["conversationId"]="fixture-b".into();state["messages"]=json!([]);state["pendingDeletion"]=Value::Null;state["running"]=false.into();
            ui.apply(state);
            assert!(ui.input.get_value().is_empty());
            assert!(ui.cards.borrow().is_empty());
            assert!(!ui.chat.is_shown() && !parent.is_shown());
            ui.chat.show(true);
            assert!(ui.chat.raise_child_overlay());
            native_overlay_tests::assert_native_order(&ui);
            ui.chat.hide();
            super::native_model_review::capture(&ui.chat,&format!("assistant-chat-{suffix}"));
            parent.destroy();
        }
    }).unwrap();
}

#[cfg(all(test, windows))]
mod native_overlay_tests {
    use super::*;
    use std::{ffi::c_void, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{HWND, POINT},
        UI::WindowsAndMessaging::{
            ChildWindowFromPointEx, DispatchMessageW, GA_ROOT, GW_CHILD, GWL_STYLE, GetAncestor,
            GetWindow, GetWindowLongPtrW, IsChild, MSG, PM_REMOVE, PeekMessageW, SendMessageW,
            TranslateMessage, WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WS_CHILD, WS_CLIPSIBLINGS,
            WindowFromPoint,
        },
    };
    use wxdragon::{
        bitmap::Bitmap,
        dc::{BlitConfig, DeviceContext, MemoryDC, ScreenDC},
        widgets::SimpleBook,
    };

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDCEx(window: HWND, region: *mut c_void, flags: u32) -> *mut c_void;
        fn ReleaseDC(window: HWND, dc: *mut c_void) -> i32;
        fn SetThreadDpiAwarenessContext(context: *mut c_void) -> *mut c_void;
        fn GetDpiForWindow(window: HWND) -> u32;
        fn GetFocus() -> HWND;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn PtVisible(dc: *mut c_void, x: i32, y: i32) -> i32;
    }
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmFlush() -> i32;
        fn DwmSetWindowAttribute(
            window: HWND,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
    }

    // The Rust test executable has no desktop-app DPI manifest. Native HWND
    // coordinates and ScreenDC pixels must share physical coordinates, just
    // as they do in the packaged GUI. Restore only this test thread afterward.
    struct ReviewDpiContext(*mut c_void);
    impl ReviewDpiContext {
        fn enter() -> Self {
            let previous = unsafe { SetThreadDpiAwarenessContext(-4isize as *mut c_void) };
            assert!(
                !previous.is_null(),
                "PerMonitorV2 test DPI context was not set"
            );
            Self(previous)
        }
    }
    impl Drop for ReviewDpiContext {
        fn drop(&mut self) {
            unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }

    pub(super) fn assert_native_order(ui: &AssistantUi) {
        let chat = ui.chat.get_handle();
        let host = ui.shell.assistant_host.get_handle();
        let size = ui.chat.get_client_size();
        let point = ui
            .chat
            .client_to_screen(Point::new(size.width / 2, size.height / 2));
        let point = ui.shell.assistant_host.screen_to_client(point);
        unsafe {
            assert_ne!(GetWindowLongPtrW(chat, GWL_STYLE) & WS_CHILD as isize, 0);
            assert_eq!(GetAncestor(chat, GA_ROOT), ui.parent.get_handle());
            assert_ne!(
                GetWindowLongPtrW(ui.shell.book.get_handle(), GWL_STYLE) & WS_CLIPSIBLINGS as isize,
                0,
                "wx style must have reached the native book HWND"
            );
            assert_eq!(
                ChildWindowFromPointEx(
                    host,
                    POINT {
                        x: point.x,
                        y: point.y
                    },
                    0
                ),
                chat,
                "native content hit test must reach the overlay before the business book"
            );
        }
    }

    fn pump_native_messages() {
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            for _ in 0..400 {
                if PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            DwmFlush();
        }
    }

    fn layout_fixture(parent: &Frame, host: &Panel, ui: &AssistantUi) {
        parent.layout();
        host.layout();
        ui.shell.panel.layout();
        ui.shell.assistant_host.layout();
        ui.shell.book.layout();
        ui.position_chat();
        ui.layout_chat();
    }

    fn nested_business_page(
        shell: &Shell,
        id: PageId,
    ) -> (SimpleBook, Vec<(ScrolledWindow, Panel, MessageText)>) {
        let page = Panel::builder(&shell.book).build();
        let book = SimpleBook::builder(&page).build();
        let mut pages = Vec::new();
        for index in 0..2 {
            let scroll = ScrolledWindow::builder(&book)
                .with_style(ScrolledWindowStyle::VScroll)
                .build();
            scroll.set_scroll_rate(0, scroll.from_dip(8));
            let column = BoxSizer::builder(Orientation::Vertical).build();
            let input = MessageText::builder(&scroll)
                .with_style(TextCtrlStyle::MultiLine)
                .with_value("业务页原生输入框（合成数据）")
                .build();
            input.set_min_size(visual::dip_size(&input, 100, 110));
            column.add(&input, 0, SizerFlag::Expand, 0);
            let canvas = Panel::builder(&scroll).build();
            canvas.set_min_size(visual::dip_size(&canvas, 100, 1000));
            canvas.set_background_style(BackgroundStyle::Paint);
            canvas.on_paint(move |_| {
                let base = AutoBufferedPaintDC::new(&canvas);
                let dc = GraphicsDC::new(&base);
                let c = visual::Canvas::new(&dc, visual::scale(&canvas));
                c.clear(Colour::rgb(157, 35 + index * 20, 67));
                c.label(
                    "业务图表（合成）",
                    Rect::new(20, 24, 240, 40),
                    16,
                    true,
                    Colour::rgb(255, 255, 255),
                    false,
                );
            });
            column.add(&canvas, 0, SizerFlag::Expand, 0);
            scroll.set_sizer(column, false);
            assert!(book.add_page(&scroll, "合成页", index == 0, None));
            pages.push((scroll, canvas, input));
        }
        let column = BoxSizer::builder(Orientation::Vertical).build();
        column.add(&book, 1, SizerFlag::Expand, 0);
        page.set_sizer(column, false);
        shell.register(id, &page);
        (book, pages)
    }

    fn native_wheel(window: HWND, screen: Point, rotation: i16, keys: u16, horizontal: bool) {
        let delta = (rotation as u16 as usize) << 16 | keys as usize;
        let position = (screen.x as u16 as u32 | (screen.y as u16 as u32) << 16) as isize;
        unsafe {
            SendMessageW(
                window,
                if horizontal {
                    WM_MOUSEHWHEEL
                } else {
                    WM_MOUSEWHEEL
                },
                delta,
                position,
            );
        }
        pump_native_messages();
    }

    /// Sends native Windows wheel messages to the production widgets. This
    /// opens only a synthetic frame: no Hub backend, account or model is used.
    #[test]
    #[ignore = "shows an isolated native chat fixture and sends Windows wheel messages"]
    fn assistant_transcript_wheel_routes_native_children_and_preserves_reading() {
        let _dpi_context = ReviewDpiContext::enter();
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let parent = Frame::builder().with_title("Hub 滚轮隔离验证（合成）")
                .with_position(Point::new(40, 40)).build();
            parent.set_size(visual::dip_size(&parent, 1168, 754));
            let host = Panel::builder(&parent).build();
            let shell = Shell::new(&host, &parent);
            let api = ApiClient::new("http://127.0.0.1:1".into(),
                super::super::text::GuiText::new(super::super::text::GuiLocale::ZhCn));
            let ui = AssistantUi::build(&parent, &shell, api, false);
            let column = BoxSizer::builder(Orientation::Vertical).build();
            column.add(&shell.panel, 1, SizerFlag::Expand, 0);
            host.set_sizer(column, false);
            let outer = BoxSizer::builder(Orientation::Vertical).build();
            outer.add(&host, 1, SizerFlag::Expand, 0);
            parent.set_sizer(outer, false);
            // HWND hit tests use final geometry; suppress only this fixture's
            // first-show DWM transform, as in the existing screen review.
            let disabled = 1i32;
            assert_eq!(unsafe { DwmSetWindowAttribute(parent.get_handle(), 3,
                (&disabled as *const i32).cast(), std::mem::size_of_val(&disabled) as u32) }, 0);
            parent.show(true);
            parent.raise();
            layout_fixture(&parent, &host, &ui);
            let mut state = json!({"enabled":true,"runtimeReady":true,"loggedIn":true,
                "modelServiceReady":true,"running":false,"conversationId":"wheel-fixture",
                "messages":(0..30).map(|index|json!({"id":format!("wheel-{index}"),
                    "role":"tool","text":format!("第 {index} 条合成回复\n可选择和复制的消息内容")})).collect::<Vec<_>>()});
            ui.apply(state.clone());
            // Keep a full native background region visible even when scroll
            // units round the final few pixels of the normal 8-DIP spacer.
            ui.transcript.get_sizer().unwrap().add_spacer(ui.transcript.from_dip(48));
            ui.chat.show(true);
            ui.position_chat();
            ui.layout_chat();
            pump_native_messages();
            let transcript = ui.transcript;
            let (_, unit) = transcript.get_scroll_pixels_per_unit();
            let bottom = || {
                transcript.scroll_coords(-1, transcript.get_virtual_size().height / unit.max(1));
                pump_native_messages();
                transcript.view_start().y
            };
            let maximum = bottom();
            assert!(maximum * unit > transcript.get_client_size().height * 3,
                "fixture needs several viewports of native messages");

            // The final sizer spacer is real transcript background. Exercise
            // its default wxScrollHelper path as the reference wheel distance.
            let size = transcript.get_client_size();
            let blank = transcript.client_to_screen(Point::new(transcript.from_dip(2), size.height - 2));
            assert_eq!(unsafe { WindowFromPoint(POINT { x:blank.x, y:blank.y }) }, transcript.get_handle());
            native_wheel(transcript.get_handle(), blank, 120, 0, false);
            let step = maximum - transcript.view_start().y;
            assert!(step > 0, "native background wheel must scroll with the current OS settings");

            let start = maximum / 2;
            transcript.scroll_coords(-1, start);
            pump_native_messages();
            let top = transcript.client_to_screen(Point::new(0, 0));
            let height = transcript.get_client_size().height;
            let (body, row, bubble, caption) = ui.cards.borrow().iter().find_map(|card| {
                let origin = card.row.client_to_screen(Point::new(0, 0));
                (origin.y >= top.y && origin.y + card.row.get_size().height < top.y + height)
                    .then(|| (card.body.primary_text(), card.row, card.bubble, card.caption.unwrap()))
            }).expect("a complete message card must be visible");
            body.set_selection(0, 4);
            let selection = body.get_selection();
            let text = body.get_value();
            for (name, control, point) in [
                ("正文", body.get_handle(), body.client_to_screen(Point::new(body.from_dip(8), body.get_client_size().height / 2))),
                ("卡片空白", row.get_handle(), row.client_to_screen(Point::new(row.get_client_size().width - row.from_dip(20), row.get_client_size().height / 2))),
                ("气泡边距", bubble.get_handle(), bubble.client_to_screen(Point::new(bubble.from_dip(6), bubble.get_client_size().height / 2))),
                ("结果标题", caption.get_handle(), caption.client_to_screen(Point::new(caption.from_dip(4), caption.get_client_size().height / 2))),
            ] {
                transcript.scroll_coords(-1, start);
                pump_native_messages();
                assert_eq!(unsafe { WindowFromPoint(POINT {x:point.x, y:point.y}) }, control,
                    "wheel target must be the visible {name} widget");
                native_wheel(control, point, 120, 0, false);
                assert_eq!(start - transcript.view_start().y, step, "{name}: one event must scroll once");
                transcript.scroll_coords(-1, start);
                native_wheel(control, point, -120, 0, false);
                assert_eq!(transcript.view_start().y - start, step, "{name}: reverse wheel must scroll down");
            }
            assert_eq!(body.get_selection(), selection);
            assert_eq!(body.get_value(), text);

            // Fractional wheel messages accumulate in the same native helper,
            // including when the pointer crosses from text to card background.
            transcript.scroll_coords(-1, start);
            let point = body.client_to_screen(Point::new(8, 8));
            native_wheel(body.get_handle(), point, 60, 0, false);
            assert_eq!(transcript.view_start().y, start);
            native_wheel(row.get_handle(), point, 60, 0, false);
            assert_eq!(start - transcript.view_start().y, step);

            let passed_through = Rc::new(Cell::new(0));
            let passed = passed_through.clone();
            body.on_mouse_wheel(move |event| {
                passed.set(passed.get() + 1);
                event.skip(true);
            });
            native_wheel(body.get_handle(), point, 120, 0x8, false); // MK_CONTROL
            assert_eq!(passed_through.get(), 1, "Ctrl+wheel must retain its native handling");
            transcript.scroll_coords(-1, start);
            native_wheel(body.get_handle(), point, 120, 0, true);
            assert_eq!(passed_through.get(), 2, "horizontal wheel must retain its native handling");
            assert_eq!(transcript.view_start().y, start, "horizontal wheel must not become vertical");

            // Streaming updates preserve an older reading position/selection;
            // at the end, new content continues following the latest message.
            transcript.scroll_coords(-1, start);
            state["messages"][29]["text"] = "追加中的合成回复\n第一段\n第二段".into();
            ui.apply(state.clone());
            pump_native_messages();
            assert_eq!(transcript.view_start().y, start);
            assert_eq!(body.get_selection(), selection);
            bottom();
            assert!(ui.at_latest_message());
            state["messages"][29]["text"] = "追加中的合成回复\n第一段\n第二段\n第三段\n第四段".into();
            ui.apply(state);
            pump_native_messages();
            assert!(ui.at_latest_message());

            // The separate composer keeps its own native multiline scrolling.
            ui.input.set_value(&(0..40).map(|index|format!("输入框草稿 {index}\n")).collect::<String>());
            ui.input.set_focus();
            ui.input.set_insertion_point_end();
            pump_native_messages();
            let editor = unsafe { GetWindow(ui.input.get_handle(), GW_CHILD) };
            assert!(!editor.is_null());
            let first_line = unsafe { SendMessageW(editor, 0xce, 0, 0) }; // EM_GETFIRSTVISIBLELINE
            assert!(first_line > 0);
            let outside = transcript.view_start();
            let point = ui.input.client_to_screen(Point::new(20, 20));
            native_wheel(editor, point, 120, 0, false);
            assert!(unsafe { SendMessageW(editor, 0xce, 0, 0) } < first_line);
            assert_eq!(transcript.view_start(), outside);
            eprintln!("native wheel: background/text/row/bubble/caption, both directions, partial deltas, modifiers, reading/follow, composer passed");
            parent.destroy();
            pump_native_messages();
        }).unwrap();
    }

    #[test]
    #[ignore = "shows an isolated native chat fixture and exercises motion timers"]
    fn assistant_chat_motion_keeps_layout_and_handles_interruptions() {
        let _dpi_context = ReviewDpiContext::enter();
        let timer_after_shutdown = RefCell::new(None);
        wxdragon::app::render_offscreen(|| {
            theme::init(theme::ThemeMode::Light);
            let previous_motion = theme::motion_enabled();
            theme::set_motion_enabled(true);
            let parent = Frame::builder()
                .with_title("Hub 对话动画隔离验证（合成）")
                .build();
            parent.set_client_size(visual::dip_size(&parent, 1168, 900));
            let host = Panel::builder(&parent).build();
            let shell = Shell::new(&host, &parent);
            let api = ApiClient::new(
                "http://127.0.0.1:1".into(),
                super::super::text::GuiText::new(super::super::text::GuiLocale::ZhCn),
            );
            let ui = AssistantUi::build(&parent, &shell, api, false);
            // Exercise show_chat, including its focus, without any HTTP worker.
            ui.fetching.store(true, Ordering::SeqCst);
            let column = BoxSizer::builder(Orientation::Vertical).build();
            column.add(&shell.panel, 1, SizerFlag::Expand, 0);
            host.set_sizer(column, false);
            let outer = BoxSizer::builder(Orientation::Vertical).build();
            outer.add(&host, 1, SizerFlag::Expand, 0);
            parent.set_sizer(outer, false);
            parent.show(true);
            parent.raise();
            layout_fixture(&parent, &host, &ui);
            let mut state = json!({"enabled":true,"runtimeReady":true,"loggedIn":true,
                "modelServiceReady":true,"running":false,"conversationId":"motion-fixture",
                "messages":[{"id":"reply","role":"assistant","text":"保留当前消息和选区。"}]});
            ui.apply(state.clone());
            ui.input.set_value("收起后保留的草稿");
            let body = ui.cards.borrow()[0].body.primary_text();
            body.set_selection(0, 2);
            let body_selection = body.get_selection();
            ui.show_chat();
            assert!(ui.chat.is_shown());
            assert!(ui.motion.get().started.is_some());
            assert!(ui.motion_timer.borrow().as_ref().unwrap().is_running());
            let focus = unsafe { GetFocus() };
            assert!(
                focus == ui.input.get_handle()
                    || unsafe { IsChild(ui.input.get_handle(), focus) } != 0,
                "opening must focus the composer without waiting for motion"
            );
            let chat_size = ui.chat.get_client_size();
            let input_size = ui.input.get_client_size();
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(20));
                pump_native_messages();
                assert_eq!(ui.chat.get_client_size(), chat_size);
                assert_eq!(ui.input.get_client_size(), input_size);
                let area = shell.assistant_overlay_area();
                let position = ui.chat.get_position();
                assert!(
                    position.y >= area.y && position.y + chat_size.height <= area.y + area.height
                );
                assert_native_order(&ui);
            }
            ui.collapse();
            assert_eq!(ui.motion.get().target, 1.0);
            ui.show_chat();
            assert_eq!(ui.motion.get().target, 0.0);
            std::thread::sleep(CHAT_MOTION_DURATION + Duration::from_millis(30));
            pump_native_messages();
            assert!(ui.chat.is_shown());
            assert!(!ui.motion_timer.borrow().as_ref().unwrap().is_running());
            assert_eq!(ui.input.get_value(), "收起后保留的草稿");
            assert_eq!(body.get_selection(), body_selection);
            let settled = fit_chat(
                shell.assistant_overlay_area(),
                visual::dip_size(&ui.chat, 420, 560),
            );
            assert_eq!(ui.chat.get_position(), Point::new(settled.x, settled.y));
            ui.collapse();
            std::thread::sleep(CHAT_MOTION_DURATION + Duration::from_millis(30));
            pump_native_messages();
            assert!(!ui.chat.is_shown());
            assert!(!ui.motion_timer.borrow().as_ref().unwrap().is_running());

            theme::set_motion_enabled(false);
            ui.show_chat();
            assert!(ui.chat.is_shown() && ui.motion.get().started.is_none());
            ui.collapse();
            assert!(!ui.chat.is_shown());
            theme::set_motion_enabled(true);
            ui.show_chat();
            parent.hide();
            ui.tick_chat_motion();
            assert!(
                ui.chat.is_shown(),
                "tray-style hiding must retain the open conversation"
            );
            assert!(!ui.motion_timer.borrow().as_ref().unwrap().is_running());
            parent.show(true);
            ui.position_chat();
            assert_native_order(&ui);
            ui.collapse();
            state["conversationId"] = "motion-fixture-next".into();
            state["messages"] = json!([]);
            ui.apply(state.clone());
            assert!(ui.input.get_value().is_empty());
            assert!(!ui.motion_timer.borrow().as_ref().unwrap().is_running());
            ui.show_chat();
            state["enabled"] = false.into();
            ui.apply(state.clone());
            assert!(!ui.chat.is_shown());
            assert!(!ui.motion_timer.borrow().as_ref().unwrap().is_running());
            state["enabled"] = true.into();
            ui.apply(state);
            ui.show_chat();
            *timer_after_shutdown.borrow_mut() = Some(ui.motion_timer.clone());
            parent.destroy();
            pump_native_messages();
            theme::set_motion_enabled(previous_motion);
        })
        .unwrap();
        // Top-level Destroy queues deletion for wx idle. The raw Win32 pump
        // above does not run idle; render_offscreen's wxEntryCleanup does.
        assert!(
            timer_after_shutdown
                .borrow()
                .as_ref()
                .unwrap()
                .borrow()
                .is_none(),
            "actual native destruction must release the motion timer"
        );
        eprintln!(
            "native motion: focus/layout/clipping, reverse, collapse/continue, reduced motion, hide, account switch, disable and destroy passed"
        );
    }

    fn assert_screen_pixels(ui: &AssistantUi, background: &Panel, label: &str) {
        assert_native_order(ui);
        let size = ui.parent.get_client_size();
        let screen = ui.parent.client_to_screen(Point::new(0, 0));
        let mut bitmap = Bitmap::new(size.width, size.height).unwrap();
        {
            let source = ScreenDC::new();
            let mut target = MemoryDC::new();
            target.select_object(&mut bitmap);
            assert!(target.blit(
                &source,
                BlitConfig::new(0, 0, size.width, size.height, screen.x, screen.y)
            ));
        }
        let mut pixels = bitmap.get_rgba_data().unwrap();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        let image =
            image::RgbaImage::from_raw(size.width as u32, size.height as u32, pixels).unwrap();
        if let Some(directory) = std::env::var_os("TIANCAI_NATIVE_REVIEW_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            image
                .save(directory.join(format!("assistant-overlay-screen-{label}.png")))
                .unwrap();
            let chat_size = ui.chat.get_client_size();
            let chat_origin = ui.chat.client_to_screen(Point::new(0, 0));
            let probes = [
                chat_size.height / 3,
                chat_size.height * 2 / 3,
                chat_size.height - ui.chat.from_dip(40),
            ]
            .map(|y| {
                let point = ui
                    .chat
                    .client_to_screen(Point::new(chat_size.width - ui.chat.from_dip(5), y));
                let local = ui.parent.screen_to_client(point);
                json!({"screen":[point.x,point.y],"client":[local.x,local.y],
                    "rgba":image.get_pixel(local.x as u32,local.y as u32).0})
            });
            let metadata = json!({"actualScreenCapture":true,"syntheticFixture":true,
                "requestedThreadDpiAwareness":"PerMonitorV2","dwmTransitionsDisabled":true,
                "windowDpi":unsafe{GetDpiForWindow(ui.parent.get_handle())},
                "scale":visual::scale(&ui.parent),"clientScreenOrigin":[screen.x,screen.y],
                "clientSize":[size.width,size.height],"chatScreenOrigin":[chat_origin.x,chat_origin.y],
                "chatClientSize":[chat_size.width,chat_size.height],"probes":probes});
            std::fs::write(
                directory.join(format!("assistant-overlay-screen-{label}.json")),
                serde_json::to_vec_pretty(&metadata).unwrap(),
            )
            .unwrap();
        }
        let chat_size = ui.chat.get_client_size();
        for y in [
            chat_size.height / 3,
            chat_size.height * 2 / 3,
            chat_size.height - ui.chat.from_dip(40),
        ] {
            let point = ui
                .chat
                .client_to_screen(Point::new(chat_size.width - ui.chat.from_dip(5), y));
            let hit = unsafe {
                WindowFromPoint(POINT {
                    x: point.x,
                    y: point.y,
                })
            };
            assert!(
                hit == ui.chat.get_handle() || unsafe { IsChild(ui.chat.get_handle(), hit) } != 0,
                "screen hit intercepted by the business page at {point:?}: {hit:?}"
            );
            let local = ui.parent.screen_to_client(point);
            let pixel = image.get_pixel(local.x as u32, local.y as u32);
            let expected = theme::theme().bg_card;
            assert!(
                (pixel[0] as i16 - expected.r as i16).abs() <= 3
                    && (pixel[1] as i16 - expected.g as i16).abs() <= 3
                    && (pixel[2] as i16 - expected.b as i16).abs() <= 3,
                "background painted over overlay at {point:?}: {pixel:?}"
            );
            // Test the native leaf DC as well: a raised parent's sibling must
            // exclude its entire descendant branch from painting here.
            let local = background.screen_to_client(point);
            let bg_size = background.get_client_size();
            if local.x >= 0 && local.y >= 0 && local.x < bg_size.width && local.y < bg_size.height {
                let dc = unsafe { GetDCEx(background.get_handle(), null_mut(), 0x2 | 0x10) };
                assert!(!dc.is_null());
                let visible = unsafe { PtVisible(dc, local.x, local.y) };
                unsafe { ReleaseDC(background.get_handle(), dc) };
                assert_eq!(
                    visible, 0,
                    "business descendant DC includes the overlay rectangle"
                );
            }
        }
        for control in [&ui.input as &dyn WxWidget, &ui.send] {
            let size = control.get_client_size();
            let point = control.client_to_screen(Point::new(size.width / 2, size.height / 2));
            let hit = unsafe {
                WindowFromPoint(POINT {
                    x: point.x,
                    y: point.y,
                })
            };
            assert!(
                hit == control.get_handle() || unsafe { IsChild(control.get_handle(), hit) } != 0,
                "input/send target was obscured: {hit:?}"
            );
        }
    }

    /// Explicitly run with --ignored --exact and --test-threads=1. This shows
    /// only an isolated synthetic native fixture; no Hub data/backend is used.
    #[test]
    #[ignore = "shows an isolated native fixture and captures actual screen pixels"]
    fn assistant_overlay_stays_above_nested_native_pages_on_screen() {
        let _dpi_context = ReviewDpiContext::enter();
        wxdragon::app::render_offscreen(|| {
            for (mode, suffix) in [
                (theme::ThemeMode::Light, "light"),
                (theme::ThemeMode::Dark, "dark"),
            ] {
                theme::init(mode);
                let parent = Frame::builder()
                    .with_title("Hub 浮层隔离验证（合成）")
                    .with_position(Point::new(40, 40))
                    .build();
                let host = Panel::builder(&parent).build();
                let shell = Shell::new(&host, &parent);
                let api = ApiClient::new(
                    "http://127.0.0.1:1".into(),
                    super::super::text::GuiText::new(super::super::text::GuiLocale::ZhCn),
                );
                let ui = AssistantUi::build(&parent, &shell, api, false);
                let mut pages = Vec::new();
                for id in [PageId::Squad, PageId::Settings] {
                    let (book, children) = nested_business_page(&shell, id);
                    pages.push((id, book, children));
                }
                let column = BoxSizer::builder(Orientation::Vertical).build();
                column.add(&shell.panel, 1, SizerFlag::Expand, 0);
                host.set_sizer(column, false);
                let outer = BoxSizer::builder(Orientation::Vertical).build();
                outer.add(&host, 1, SizerFlag::Expand, 0);
                parent.set_sizer(outer, false);
                let u = ui.clone();
                shell.on_changed(move |id| u.enter(id));
                parent.set_size(visual::dip_size(&parent, 1168, 754));
                // ScreenDC captures the composed image, including show/resize
                // animations, while HWND hit tests use the final rectangle.
                // Disable transitions only for this synthetic review frame.
                let disabled = 1i32;
                assert_eq!(
                    unsafe {
                        DwmSetWindowAttribute(
                            parent.get_handle(),
                            3, // DWMWA_TRANSITIONS_FORCEDISABLED
                            (&disabled as *const i32).cast(),
                            std::mem::size_of_val(&disabled) as u32,
                        )
                    },
                    0,
                    "synthetic review frame could not disable DWM transitions"
                );
                parent.show(true);
                parent.raise();
                ui.apply(json!({"enabled":true,"runtimeReady":true,"loggedIn":true,
                    "modelServiceReady":true,"running":false,"conversationId":"native-fixture",
                    "messages":[{"id":"fixture-user","role":"user","text":"检查这组配置"},
                    {"id":"fixture-answer","role":"assistant","text":"可以，先查看当前配置。"}]}));
                ui.chat.show(true);
                for (width, height) in [(1168, 754), (900, 560)] {
                    parent.set_size(visual::dip_size(&parent, width, height));
                    layout_fixture(&parent, &host, &ui);
                    for (id, book, children) in &pages {
                        shell.navigate(*id);
                        for (index, (scroll, canvas, input)) in children.iter().enumerate() {
                            book.change_selection(index);
                            book.layout();
                            scroll.layout();
                            scroll.fit_inside();
                            scroll.scroll_coords(0, index as i32 * 18);
                            input.change_value("刷新后的原生编辑框（合成）");
                            canvas.refresh(true, None);
                            input.refresh(true, None);
                            parent.refresh(true, None);
                            pump_native_messages();
                            assert_screen_pixels(
                                &ui,
                                canvas,
                                &format!("{suffix}-{width}-{}-{index}", id.title()),
                            );
                        }
                    }
                }
                ui.input.set_value("收起后保留的草稿");
                ui.collapse();
                ui.finish_chat_motion();
                assert!(!ui.chat.is_shown());
                ui.chat.show(true);
                ui.position_chat();
                assert_eq!(ui.input.get_value(), "收起后保留的草稿");
                parent.iconize(true);
                pump_native_messages();
                assert!(parent.is_iconized());
                assert!(ui.chat.is_shown());
                parent.iconize(false);
                pump_native_messages();
                ui.position_chat();
                assert_native_order(&ui);
                parent.hide();
                assert!(ui.chat.is_shown());
                parent.show(true);
                ui.position_chat();
                pump_native_messages();
                assert_native_order(&ui);
                parent.destroy();
                pump_native_messages();
            }
        })
        .unwrap();
    }
}
