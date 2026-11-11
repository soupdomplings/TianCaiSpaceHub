//! Short operation results share one native notice per top-level window.
//! Business state, form validation and progress remain with their controls.
use super::{
    controls::{Button, ButtonKind, TextCtrl},
    theme,
    visual::{Canvas, Icon},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use wxdragon::{dc::GraphicsDC, prelude::*, timer::Timer, window::WxWidgetDowncast};

const NOTICE_MILLISECONDS: i32 = 4_000;
const COUNTDOWN_TICK_MILLISECONDS: i32 = 50;

fn countdown_remaining(started: Option<Instant>, now: Instant) -> f64 {
    let Some(started) = started else { return 0.0 };
    let duration = Duration::from_millis(NOTICE_MILLISECONDS as u64);
    duration
        .saturating_sub(now.saturating_duration_since(started))
        .as_secs_f64()
        / duration.as_secs_f64()
}

thread_local! {
    static NOTICES: RefCell<Vec<Rc<Notice>>> = const { RefCell::new(Vec::new()) };
    static MAIN_AREA: RefCell<Option<(Frame, Panel)>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
enum Owner {
    Frame(Frame),
    Dialog(Dialog),
}
impl WxWidget for Owner {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        match self {
            Self::Frame(w) => w.handle_ptr(),
            Self::Dialog(w) => w.handle_ptr(),
        }
    }
    fn is_valid(&self) -> bool {
        match self {
            Self::Frame(w) => w.is_valid(),
            Self::Dialog(w) => w.is_valid(),
        }
    }
}
impl WxEvtHandler for Owner {
    unsafe fn get_event_handler_ptr(&self) -> *mut wxdragon::ffi::wxd_EvtHandler_t {
        self.handle_ptr().cast()
    }
}
impl WindowEvents for Owner {}
impl Owner {
    fn visible(self) -> bool {
        self.is_shown() && !matches!(self, Self::Frame(frame) if frame.is_iconized())
    }
}

fn owner(parent: &dyn WxWidget) -> Option<Owner> {
    if let Some(dialog) = parent.downcast_ref::<Dialog>() {
        return Some(Owner::Dialog(*dialog));
    }
    if let Some(frame) = parent.downcast_ref::<Frame>() {
        return Some(Owner::Frame(*frame));
    }
    if let Some(window) = parent.downcast_ref::<Window>()
        && let Some(owner) = owner_window(*window)
    {
        return Some(owner);
    }
    let mut ancestor = parent.get_parent();
    while let Some(window) = ancestor {
        if let Some(owner) = owner_window(window) {
            return Some(owner);
        }
        ancestor = window.get_parent();
    }
    None
}
fn owner_window(window: Window) -> Option<Owner> {
    if window.get_class_name().as_deref() == Some("wxDialog") {
        // wxDragon exposes Dialog::from_ptr, but has no Dialog RTTI trait.
        // This wrapper is made only after the native class was checked.
        return Some(Owner::Dialog(unsafe {
            Dialog::from_ptr(window.as_ptr().cast())
        }));
    }
    window.as_widget::<Frame>().map(Owner::Frame)
}

/// The main content area excludes the navigation rail and native title controls.
pub(super) fn install(frame: Frame, area: Panel) {
    MAIN_AREA.with(|slot| slot.borrow_mut().replace((frame, area)));
}

pub(super) fn dismiss_all() {
    let notices = NOTICES.with(|slot| slot.borrow().clone());
    for notice in notices {
        notice.dismiss_now();
    }
}

pub(super) fn success(parent: &dyn WxWidget, message: &str) {
    notify(parent, message, false);
}
pub(super) fn error(parent: &dyn WxWidget, message: &str) {
    notify(parent, message, true);
}

fn notify(parent: &dyn WxWidget, message: &str, error: bool) {
    if message.trim().is_empty() {
        return;
    }
    let Some(owner) = owner(parent) else {
        details(
            parent,
            if error {
                "操作未完成"
            } else {
                "操作结果"
            },
            message,
        );
        return;
    };
    // A background completion must never reopen or foreground a hidden Hub.
    if !owner.visible() {
        return;
    }
    let existing = NOTICES.with(|slot| {
        slot.borrow()
            .iter()
            .find(|notice| notice.owner.handle_ptr() == owner.handle_ptr())
            .cloned()
    });
    let notice = existing.unwrap_or_else(|| {
        let notice = Notice::new(owner);
        NOTICES.with(|slot| slot.borrow_mut().push(notice.clone()));
        notice
    });
    notice.present(message, error);
}

fn summary(message: &str) -> String {
    let first = message
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("");
    let mut chars = first.chars();
    let value = chars.by_ref().take(44).collect::<String>();
    if chars.next().is_some() {
        format!("{value}…")
    } else {
        value
    }
}

struct Notice {
    id: usize,
    owner: Owner,
    panel: Panel,
    message: StaticText,
    progress: Panel,
    close: Button,
    full: RefCell<String>,
    error: std::cell::Cell<bool>,
    started: Cell<Option<Instant>>,
    timer: RefCell<Option<Timer<Panel>>>,
    motion: super::motion::PositionMotion,
}
impl Notice {
    fn new(owner: Owner) -> Rc<Self> {
        let panel = Panel::builder(&owner)
            .with_style(PanelStyle::BorderNone)
            .build();
        panel.hide();
        panel.set_name("operation-notice");
        panel.set_background_color(theme::theme().bg_card);
        panel.set_background_style(BackgroundStyle::Paint);
        panel.set_font(&theme::font(theme::TextRole::Body));
        let message = StaticText::builder(&panel).build();
        // Horizontal shrinking permits wrapping. A zero vertical minimum
        // overrides the native text height and collapses the entire label.
        message.set_min_size(Size::new(0, -1));
        message.set_foreground_color(theme::theme().ink_primary);
        let close = Button::builder(&panel)
            .with_label("关闭")
            .with_icon(Icon::WindowClose)
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(32, 30))
            .build();
        close.set_tooltip("关闭提示");
        let col = BoxSizer::builder(Orientation::Vertical).build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add_spacer(panel.from_dip(30));
        row.add(
            &message,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            panel.from_dip(8),
        );
        row.add(&close, 0, SizerFlag::AlignCenterVertical, 0);
        col.add_sizer(
            &row,
            1,
            SizerFlag::Expand | SizerFlag::All,
            panel.from_dip(12),
        );
        let progress = Panel::builder(&panel)
            .with_style(PanelStyle::BorderNone)
            .build();
        progress.set_name("operation-notice-countdown");
        progress.set_min_size(Size::new(0, panel.from_dip(3)));
        progress.set_background_style(BackgroundStyle::Paint);
        col.add(
            &progress,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            panel.from_dip(8),
        );
        panel.set_sizer(col, false);
        let ui = Rc::new(Self {
            id: owner.handle_ptr() as usize,
            owner,
            panel,
            message,
            progress,
            close,
            full: Default::default(),
            error: Default::default(),
            started: Default::default(),
            timer: Default::default(),
            motion: super::motion::PositionMotion::new(&panel),
        });
        let weak = Rc::downgrade(&ui);
        progress.on_paint(move |_| {
            let Some(ui) = weak.upgrade() else { return };
            let dc = AutoBufferedPaintDC::new(&progress);
            let graphics = GraphicsDC::new(&dc);
            let scale = super::visual::scale(&progress);
            let c = Canvas::new(&graphics, scale);
            let size = progress.get_client_size();
            let width = (size.width as f64 / scale) as i32;
            let height = (size.height as f64 / scale) as i32;
            c.clear(theme::theme().bg_card);
            c.rounded(
                Rect::new(0, 0, width, height),
                1,
                theme::theme().divider,
                None,
            );
            let remaining = countdown_remaining(ui.started.get(), Instant::now());
            c.rounded(
                Rect::new(0, 0, (width as f64 * remaining).ceil() as i32, height),
                1,
                if ui.error.get() {
                    theme::theme().error
                } else {
                    theme::theme().ok
                },
                None,
            );
        });
        let weak = Rc::downgrade(&ui);
        panel.on_paint(move |_| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let dc = AutoBufferedPaintDC::new(&panel);
            let graphics = GraphicsDC::new(&dc);
            let scale = super::visual::scale(&panel);
            let c = Canvas::new(&graphics, scale);
            let size = panel.get_client_size();
            c.clear(theme::theme().bg_app);
            c.rounded(
                Rect::new(
                    0,
                    0,
                    (size.width as f64 / scale) as i32,
                    (size.height as f64 / scale) as i32,
                ),
                theme::SURFACE_RADIUS,
                theme::theme().bg_card,
                Some(theme::theme().border),
            );
            let color = if ui.error.get() {
                theme::theme().error
            } else {
                theme::theme().ok
            };
            let mid = ((size.height - panel.from_dip(11)) as f64 / scale) as i32 / 2;
            c.dot(24, mid, 9, color);
            c.dot(24, mid, 7, theme::theme().bg_card);
            if ui.error.get() {
                c.line(
                    Point::new(21, mid - 3),
                    Point::new(27, mid + 3),
                    color,
                    false,
                );
                c.line(
                    Point::new(27, mid - 3),
                    Point::new(21, mid + 3),
                    color,
                    false,
                );
            } else {
                c.line(Point::new(20, mid), Point::new(23, mid + 3), color, false);
                c.line(
                    Point::new(23, mid + 3),
                    Point::new(28, mid - 3),
                    color,
                    false,
                );
            }
        });
        let weak = Rc::downgrade(&ui);
        close.on_click(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.dismiss();
            }
        });
        let weak = Rc::downgrade(&ui);
        message.on_mouse_left_up(move |event| {
            if let Some(ui) = weak.upgrade() {
                ui.show_details();
            }
            event.skip(false);
        });
        let weak = Rc::downgrade(&ui);
        panel.on_mouse_left_up(move |event| {
            let source = match &event {
                WindowEventData::MouseButton(mouse) => mouse.event.event.get_event_object(),
                WindowEventData::General(raw) => raw.get_event_object(),
                _ => None,
            };
            if source.is_some_and(|window| window.as_ptr() == panel.handle_ptr()) {
                if let Some(ui) = weak.upgrade() {
                    ui.show_details();
                }
                event.skip(false);
            } else {
                event.skip(true);
            }
        });
        let weak = Rc::downgrade(&ui);
        let timer = Timer::new(&panel);
        timer.on_tick(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.tick(Instant::now());
            }
        });
        ui.timer.borrow_mut().replace(timer);
        let weak = Rc::downgrade(&ui);
        owner.on_size(move |event| {
            if let Some(ui) = weak.upgrade() {
                if ui.panel.is_shown() {
                    ui.motion.finish();
                    ui.position();
                }
            }
            event.skip(true);
        });
        MAIN_AREA.with(|slot| {
            if let Some((frame, area)) = *slot.borrow()
                && frame.handle_ptr() == owner.handle_ptr()
            {
                let weak = Rc::downgrade(&ui);
                area.on_size(move |event| {
                    if let Some(ui) = weak.upgrade()
                        && ui.panel.is_shown()
                    {
                        ui.motion.finish();
                        ui.position();
                    }
                    event.skip(true);
                });
            }
        });
        let ptr = owner.handle_ptr();
        owner.on_destroy(move |event| {
            let own = matches!(&event, WindowEventData::General(raw)
                if raw.get_event_object().is_some_and(|w| w.as_ptr() == ptr));
            if own {
                NOTICES.with(|slot| slot.borrow_mut().retain(|notice| notice.id != ptr as usize));
                MAIN_AREA.with(|slot| {
                    if slot
                        .borrow()
                        .as_ref()
                        .is_some_and(|(frame, _)| !frame.is_valid() || frame.handle_ptr() == ptr)
                    {
                        slot.borrow_mut().take();
                    }
                });
            }
            event.skip(true);
        });
        ui
    }
    fn present(&self, message: &str, error: bool) {
        if let Some(timer) = self.timer.borrow().as_ref() {
            timer.stop();
        }
        self.full.replace(message.into());
        self.error.set(error);
        self.started.set(Some(Instant::now()));
        self.message.set_label(&summary(message));
        self.message.set_tooltip("点击查看完整信息");
        self.position();
        self.motion.enter(
            self.panel,
            self.panel.get_position(),
            Point::new(self.panel.from_dip(12), 0),
            super::motion::POPUP_MS,
        );
        self.panel.raise_child_overlay();
        self.panel.refresh(false, None);
        self.progress.refresh(false, None);
        if let Some(timer) = self.timer.borrow().as_ref() {
            timer.start(COUNTDOWN_TICK_MILLISECONDS, false);
        }
    }
    fn tick(&self, now: Instant) {
        if countdown_remaining(self.started.get(), now) == 0.0 {
            self.dismiss();
        } else {
            self.progress.refresh(false, None);
        }
    }
    fn show_details(&self) {
        let message = self.full.borrow().clone();
        details(
            &self.owner,
            if self.error.get() {
                "操作未完成"
            } else {
                "操作结果"
            },
            &message,
        );
    }
    fn position(&self) {
        let size = self.owner.get_client_size();
        let mut area = Rect::new(0, 0, size.width, size.height);
        MAIN_AREA.with(|slot| {
            if let Some((frame, anchor)) = *slot.borrow()
                && frame.handle_ptr() == self.owner.handle_ptr()
                && anchor.is_valid()
            {
                let point = self
                    .owner
                    .screen_to_client(anchor.client_to_screen(Point::new(0, 0)));
                let size = anchor.get_client_size();
                area = Rect::new(point.x, point.y, size.width, size.height);
            }
        });
        let inset = self.panel.from_dip(12);
        let width = self
            .panel
            .from_dip(400)
            .min((area.width - inset * 2).max(1));
        let content_width = (width - self.panel.from_dip(98)).max(1);
        self.message.set_label(&summary(&self.full.borrow()));
        self.message.wrap(content_width);
        let height = (self.message.get_best_size().height + self.panel.from_dip(35))
            .max(self.panel.from_dip(67));
        self.panel.set_size_with_pos(
            area.x + area.width - width - inset,
            area.y + inset,
            width,
            height,
        );
        self.panel.layout();
    }
    fn dismiss(&self) {
        if let Some(timer) = self.timer.borrow().as_ref() {
            timer.stop();
        }
        if self.started.take().is_some() {
            self.motion.exit(
                self.panel,
                self.panel.get_position(),
                Point::new(self.panel.from_dip(12), 0),
                super::motion::POPUP_MS,
            );
        }
    }
    fn dismiss_now(&self) {
        if let Some(timer) = self.timer.borrow().as_ref() {
            timer.stop();
        }
        self.started.set(None);
        self.motion.hide_now(&self.panel);
    }
}

struct Prompt {
    dialog: Dialog,
    body: ScrolledWindow,
    text: TextCtrl,
    accept: Option<Button>,
    close: Button,
}
fn build_prompt(parent: &dyn WxWidget, title: &str, message: &str, action: Option<&str>) -> Prompt {
    let parent_size = parent.get_client_size();
    let width = parent
        .from_dip(600)
        .min((parent_size.width - parent.from_dip(16)).max(parent.from_dip(320)));
    let height = parent
        .from_dip(if action.is_some() { 360 } else { 440 })
        .min((parent_size.height - parent.from_dip(16)).max(parent.from_dip(240)));
    let dialog = Dialog::builder(parent, title)
        .with_size(width, height)
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    super::motion::install_dialog(dialog);
    dialog.set_background_color(theme::theme().bg_app);
    dialog.set_font(&theme::font(theme::TextRole::Body));
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    let heading = StaticText::builder(&dialog).with_label(title).build();
    heading.set_font(&theme::font(theme::TextRole::PageTitle));
    heading.set_foreground_color(theme::theme().ink_primary);
    outer.add(
        &heading,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(20),
    );
    let body = ScrolledWindow::builder(&dialog)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    body.set_min_size(Size::new(0, 0));
    body.set_background_color(theme::theme().bg_app);
    body.set_scroll_rate(0, body.from_dip(12));
    let col = BoxSizer::builder(Orientation::Vertical).build();
    let text = TextCtrl::builder(&body)
        .with_value(message)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
        .build();
    text.set_min_size(Size::new(0, 0));
    col.add(&text, 1, SizerFlag::Expand, 0);
    body.set_sizer(col, false);
    outer.add(
        &body,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        dialog.from_dip(20),
    );
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    if action.is_none() {
        let copy = Button::builder(&dialog)
            .with_label("复制详情")
            .with_kind(ButtonKind::Ghost)
            .build();
        let message = message.to_string();
        copy.on_click(move |_| {
            if wxdragon::clipboard::Clipboard::get().set_text(&message) {
                copy.set_label("已复制");
            }
        });
        actions.add(&copy, 0, SizerFlag::default(), 0);
    }
    actions.add_stretch_spacer(1);
    let close = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label(if action.is_some() { "取消" } else { "关闭" })
        .build();
    close.on_click(move |_| dialog.end_modal(ID_CANCEL));
    actions.add(&close, 0, SizerFlag::Right, dialog.from_dip(8));
    let accept = action.map(|action| {
        let button = Button::builder(&dialog)
            .with_label(action)
            .with_kind(ButtonKind::Primary)
            .build();
        button.on_click(move |_| dialog.end_modal(ID_OK));
        actions.add(&button, 0, SizerFlag::default(), 0);
        button
    });
    outer.add_sizer(
        &actions,
        0,
        SizerFlag::Expand | SizerFlag::All,
        dialog.from_dip(20),
    );
    dialog.set_sizer(outer, false);
    dialog.layout();
    body.layout();
    body.fit_inside();
    dialog.centre();
    Prompt {
        dialog,
        body,
        text,
        accept,
        close,
    }
}

fn details(parent: &dyn WxWidget, title: &str, message: &str) {
    let prompt = build_prompt(parent, title, message, None);
    prompt.dialog.show_modal();
    prompt.dialog.destroy();
}
pub(super) fn information(parent: &dyn WxWidget, title: &str, message: &str) {
    details(parent, title, message);
}
pub(super) fn confirm(parent: &dyn WxWidget, title: &str, message: &str, action: &str) -> bool {
    let prompt = build_prompt(parent, title, message, Some(action));
    let accepted = prompt.dialog.show_modal() == ID_OK;
    prompt.dialog.destroy();
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_summary_is_short_and_keeps_full_details_separate() {
        assert_eq!(summary("\n配置已保存\n下一步说明"), "配置已保存");
        let source = "较长中文信息".repeat(60);
        let result = summary(&source);
        assert_eq!(result.chars().count(), 45);
        assert!(result.ends_with('…'));
        assert!(source.len() > result.len());
    }
    #[test]
    fn result_countdown_expires_and_restarts_for_success_and_error() {
        let start = Instant::now();
        assert_eq!(countdown_remaining(None, start), 0.0);
        assert_eq!(countdown_remaining(Some(start), start), 1.0);
        assert_eq!(
            countdown_remaining(Some(start), start + Duration::from_secs(2)),
            0.5
        );
        assert_eq!(
            countdown_remaining(Some(start), start + Duration::from_secs(4)),
            0.0
        );
        let next = start + Duration::from_secs(3);
        assert_eq!(countdown_remaining(Some(next), next), 1.0);
    }
    #[cfg(windows)]
    #[test]
    fn hidden_result_notice_has_readable_text_details_area_and_expiring_progress() {
        wxdragon::app::render_offscreen(|| {
            for mode in [theme::ThemeMode::Light, theme::ThemeMode::Dark] {
                theme::init(mode);
                let frame = Frame::builder().with_title("hidden result layout").build();
                frame.set_client_size(Size::new(760, 540));
                let notice = Notice::new(Owner::Frame(frame));
                for error in [false, true] {
                    let full = "合成操作结果：这段正文应正常显示。\n完整原因、处理建议和可复制的说明保留。";
                    notice.present(full, error);
                    let label = notice.message.get_size();
                    assert!(label.width > 0 && label.height >= notice.message.get_char_height());
                    let point = notice.message.get_position();
                    assert!(point.x > 0 && point.y >= 0);
                    assert!(point.y + label.height <= notice.progress.get_position().y);
                    assert_eq!(notice.message.get_label(), summary(full));
                    assert_eq!(*notice.full.borrow(), full);
                    assert!(notice.progress.get_size().width > 0);
                    assert!(notice.timer.borrow().as_ref().unwrap().is_running());
                    let start = notice.started.get().unwrap();
                    notice.tick(start + Duration::from_secs(2));
                    assert!(notice.panel.is_shown());
                    notice.tick(start + Duration::from_secs(4));
                    assert!(!notice.panel.is_shown());
                    assert!(!notice.timer.borrow().as_ref().unwrap().is_running());
                }
                assert!(!frame.is_shown());
                frame.destroy();
            }
        }).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "isolated hidden operation feedback controls; run with --test-threads=1"]
    fn native_feedback_keeps_notices_and_confirmation_actions_inside_their_owner() {
        wxdragon::app::render_offscreen(|| {
            for (mode, suffix) in [
                (theme::ThemeMode::Light, "light"),
                (theme::ThemeMode::Dark, "dark"),
            ] {
                theme::init(mode);
                let frame = Frame::builder()
                    .with_title("hidden synthetic feedback")
                    .build();
                frame.set_client_size(super::super::visual::dip_size(&frame, 900, 560));
                let anchor = Panel::builder(&frame).build();
                anchor.set_size_with_pos(
                    frame.from_dip(180),
                    frame.from_dip(44),
                    frame.from_dip(720),
                    frame.from_dip(516),
                );
                install(frame, anchor);
                let notice = Notice::new(Owner::Frame(frame));
                notice.present("模型已保存", false);
                assert!(notice.timer.borrow().as_ref().unwrap().is_running());
                let point = notice.panel.get_position();
                let size = notice.panel.get_size();
                assert!(point.x >= anchor.get_position().x);
                assert!(point.x + size.width <= frame.get_client_size().width);
                notice.present("合成操作失败\n完整原因与处理建议保留在本机详情中。", true);
                assert!(notice.timer.borrow().as_ref().unwrap().is_running());
                assert!(notice.close.is_shown());
                notice.dismiss();
                assert!(!notice.panel.is_shown());
                for action in [None, Some("删除")] {
                    let prompt = build_prompt(
                        &frame,
                        "合成确认",
                        &"原生提示详细信息。\n".repeat(18),
                        action,
                    );
                    let bounds = prompt.dialog.get_client_size();
                    for button in [Some(prompt.close), prompt.accept].into_iter().flatten() {
                        let point = prompt
                            .dialog
                            .screen_to_client(button.client_to_screen(Point::new(0, 0)));
                        assert!(
                            point.x >= 0
                                && point.y >= 0
                                && point.y + button.get_size().height <= bounds.height
                        );
                    }
                    assert!(prompt.body.get_client_size().height > 0);
                    assert!(prompt.text.get_value().contains("详细信息"));
                    assert!(!prompt.dialog.is_shown());
                    super::super::native_model_review::capture(
                        &prompt.dialog,
                        &format!(
                            "feedback-prompt-{}-{suffix}",
                            if action.is_some() {
                                "confirm"
                            } else {
                                "details"
                            }
                        ),
                    );
                    prompt.dialog.destroy();
                }
                assert!(!frame.is_shown());
                frame.destroy();
            }
        })
        .unwrap();
    }
}
