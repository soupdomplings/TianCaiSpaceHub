//! Delayed, page-local loading feedback. Requests and result polling keep their
//! original lifetime; this component only borrows an existing status line.
use super::{controls::Button, theme};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer};

const DELAY: Duration = Duration::from_millis(180);
const DOT_MS: u64 = 280;
const BUTTON_MS: i32 = 50;
thread_local! {
    static PENDING: RefCell<Vec<Weak<State>>> = const { RefCell::new(Vec::new()) };
}

/// Reuse the main window's existing idle callback for show/restore/preference
/// changes. Only actual pending operations are present; no extra idle is asked
/// for and hidden indicators do not keep a polling timer alive.
pub(super) fn refresh_pending() {
    let pending = PENDING.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.retain(|weak| {
            weak.upgrade()
                .is_some_and(|state| state.run.borrow().is_some())
        });
        slot.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
    });
    let now = Instant::now();
    for state in pending {
        state.refresh(now);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presentation {
    Paused,
    Waiting(i32),
    Static,
    Dots(usize),
}
fn presentation(started: Instant, now: Instant, visible: bool, motion: bool) -> Presentation {
    if !visible {
        return Presentation::Paused;
    }
    let elapsed = now.saturating_duration_since(started);
    if elapsed < DELAY {
        return Presentation::Waiting((DELAY - elapsed).as_millis().max(1) as i32);
    }
    if motion {
        Presentation::Dots(((elapsed - DELAY).as_millis() / u128::from(DOT_MS) % 3) as usize)
    } else {
        Presentation::Static
    }
}
struct Run {
    started: Instant,
    message: String,
    previous: String,
    foreground: Colour,
    shown: bool,
    last: Option<String>,
}
#[derive(Clone, Copy)]
enum Target {
    Status(StaticText),
    Button(Button),
}
impl Target {
    fn label(self) -> String {
        match self {
            Self::Status(status) => status.get_label(),
            Self::Button(button) => button.get_label(),
        }
    }
    fn set_label(self, label: &str) {
        match self {
            Self::Status(status) => status.set_label(label),
            Self::Button(button) => button.set_label(label),
        }
    }
    fn activity(self, frame: Option<u8>) {
        if let Self::Button(button) = self {
            button.set_activity_frame(frame);
        }
    }
}
impl WxWidget for Target {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        match self {
            Self::Status(status) => status.handle_ptr(),
            Self::Button(button) => button.handle_ptr(),
        }
    }
}
struct State {
    target: Target,
    timer_owner: Panel,
    timer: RefCell<Option<Timer<Panel>>>,
    run: RefCell<Option<Run>>,
}
impl State {
    fn stop_timer(&self) {
        if let Some(timer) = self.timer.borrow().as_ref() {
            timer.stop();
        }
    }
    fn finish(&self) {
        self.stop_timer();
        let Some(run) = self.run.borrow_mut().take() else {
            return;
        };
        let window = self.target;
        if window.is_valid() && run.last.as_deref() == Some(self.target.label().as_str()) {
            self.target.set_label(&run.previous);
            window.set_foreground_color(run.foreground);
            if window.is_shown() != run.shown {
                window.show(run.shown);
                if let Some(parent) = window.get_parent() {
                    parent.layout();
                }
            }
        }
        if window.is_valid() {
            self.target.activity(None);
        }
    }
    fn refresh(&self, now: Instant) {
        let window = self.target;
        if !window.is_valid() || !self.timer_owner.is_valid() {
            self.stop_timer();
            self.run.borrow_mut().take();
            return;
        }
        let mut running = self.run.borrow_mut();
        let Some(run) = running.as_mut() else {
            self.stop_timer();
            return;
        };
        // A result, validation message or a newly rendered page owns its text.
        // Never turn that text back into an older loading notice.
        let current = self.target.label();
        if current != run.last.as_ref().unwrap_or(&run.previous).as_str() {
            self.stop_timer();
            self.target.activity(None);
            running.take();
            return;
        }
        let state = presentation(run.started, now, visible(window), theme::motion_enabled());
        match state {
            Presentation::Paused => {
                self.stop_timer();
                self.target.activity(None);
            }
            Presentation::Waiting(delay) => {
                if let Some(timer) = self.timer.borrow().as_ref()
                    && !timer.is_running()
                {
                    timer.start(delay, true);
                }
            }
            Presentation::Static | Presentation::Dots(_) => {
                let label = match state {
                    Presentation::Dots(frame) if matches!(self.target, Target::Status(_)) => {
                        let stem = run.message.trim_end_matches(['…', '.', ' ']);
                        format!("{stem} {}", ["·  ", "·· ", "···"][frame])
                    }
                    _ => run.message.clone(),
                };
                if run.last.as_ref() != Some(&label) {
                    self.target.set_label(&label);
                    if matches!(self.target, Target::Status(_)) {
                        window.set_foreground_color(theme::theme().ink_secondary);
                    }
                    run.last = Some(label);
                }
                self.target.activity(match state {
                    Presentation::Dots(_) => Some(
                        ((now.saturating_duration_since(run.started).as_millis()
                            / BUTTON_MS as u128)
                            % 32) as u8,
                    ),
                    Presentation::Static => Some(0),
                    _ => None,
                });
                if !window.is_shown() {
                    window.show(true);
                    if let Some(parent) = window.get_parent() {
                        parent.layout();
                    }
                }
                if let Some(timer) = self.timer.borrow().as_ref() {
                    if matches!(state, Presentation::Dots(_)) {
                        let interval = match self.target {
                            Target::Status(_) => DOT_MS as i32,
                            Target::Button(_) => BUTTON_MS,
                        };
                        if !timer.is_running() || timer.get_interval() != interval {
                            timer.start(interval, false);
                        }
                    } else {
                        timer.stop();
                    }
                }
            }
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.finish();
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.stop();
        }
    }
}

#[derive(Clone)]
pub(super) struct LoadingIndicator(Rc<State>);
impl LoadingIndicator {
    pub fn new(owner: &dyn WxWidget, status: StaticText) -> Self {
        Self::with_target(owner, Target::Status(status))
    }
    pub fn for_button(owner: &dyn WxWidget, button: Button) -> Self {
        Self::with_target(owner, Target::Button(button))
    }
    fn with_target(owner: &dyn WxWidget, target: Target) -> Self {
        // wxdragon binds every TIMER on the owner. This hidden native owner
        // prevents animation ticks from invoking page or dialog job polling.
        let timer_owner = Panel::builder(owner).with_size(Size::new(0, 0)).build();
        timer_owner.set_name("loading-indicator-timer");
        timer_owner.hide();
        let state = Rc::new(State {
            target,
            timer_owner,
            timer: Default::default(),
            run: Default::default(),
        });
        let timer = Timer::new(&timer_owner);
        let weak = Rc::downgrade(&state);
        timer.on_tick(move |_| {
            if let Some(state) = weak.upgrade() {
                state.refresh(Instant::now());
            }
        });
        state.timer.borrow_mut().replace(timer);
        let weak = Rc::downgrade(&state);
        let id = timer_owner.handle_ptr() as usize;
        timer_owner.on_destroy(move |event| {
            if matches!(&event, WindowEventData::General(raw)
                if raw.get_event_object().is_some_and(|window| window.as_ptr() as usize == id))
                && let Some(state) = weak.upgrade()
            {
                state.stop_timer();
                state.run.borrow_mut().take();
            }
            event.skip(true);
        });
        Self(state)
    }
    /// Call only after the original operation actually enters its busy state.
    /// Repeating the same pending message does not reset the delay or dots.
    pub fn start(&self, message: &str) {
        if self
            .0
            .run
            .borrow()
            .as_ref()
            .is_some_and(|run| run.message == message)
        {
            return;
        }
        self.finish();
        let window = self.0.target;
        if !window.is_valid() || !self.0.timer_owner.is_valid() {
            return;
        }
        self.0.run.replace(Some(Run {
            started: Instant::now(),
            message: message.into(),
            previous: self.0.target.label(),
            foreground: window.get_foreground_color(),
            shown: window.is_shown(),
            last: None,
        }));
        PENDING.with(|pending| pending.borrow_mut().push(Rc::downgrade(&self.0)));
        self.0.refresh(Instant::now());
    }
    /// Finish before writing the actual result. No minimum display duration.
    pub fn finish(&self) {
        self.0.finish();
        let current = Rc::as_ptr(&self.0);
        PENDING.with(|pending| pending.borrow_mut().retain(|weak| weak.as_ptr() != current));
    }
}

fn visible(window: Target) -> bool {
    // A hidden status line may be made visible after the delay. Its ancestors,
    // rather than its own initial visibility, decide whether animation runs.
    let mut parent = window.get_parent();
    while let Some(window) = parent {
        if !window.is_valid() || !window.is_shown() {
            return false;
        }
        if let Some(frame) = window.as_widget::<Frame>()
            && frame.is_iconized()
        {
            return false;
        }
        parent = window.get_parent();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_delay_dots_and_reduced_motion_follow_real_elapsed_time() {
        let started = Instant::now();
        assert_eq!(
            presentation(started, started, true, true),
            Presentation::Waiting(180)
        );
        assert_eq!(
            presentation(started, started + Duration::from_millis(179), true, true),
            Presentation::Waiting(1)
        );
        assert_eq!(
            presentation(started, started + DELAY, true, true),
            Presentation::Dots(0)
        );
        assert_eq!(
            presentation(
                started,
                started + DELAY + Duration::from_millis(DOT_MS),
                true,
                true
            ),
            Presentation::Dots(1)
        );
        assert_eq!(
            presentation(started, started + Duration::from_secs(5), true, false),
            Presentation::Static
        );
        assert_eq!(
            presentation(started, started + Duration::from_secs(5), false, true),
            Presentation::Paused
        );
    }

    #[cfg(windows)]
    #[test]
    fn hidden_loading_uses_a_private_timer_and_preserves_new_result_text() {
        wxdragon::app::render_offscreen(|| {
            let frame = Frame::builder()
                .with_title("hidden loading fixture")
                .build();
            let page = Panel::builder(&frame).build();
            let status = StaticText::builder(&page).with_label("12 条").build();
            let loading = LoadingIndicator::new(&page, status);
            loading.start("读取中…");
            assert_eq!(status.get_label(), "12 条");
            refresh_pending();
            assert!(!loading.0.timer.borrow().as_ref().unwrap().is_running());
            assert_ne!(loading.0.timer_owner.handle_ptr(), page.handle_ptr());
            assert!(!loading.0.timer_owner.is_shown());
            loading.finish();
            assert_eq!(status.get_label(), "12 条");
            loading.start("正在保存…");
            status.set_label("保存成功");
            loading.0.refresh(Instant::now() + Duration::from_secs(1));
            assert!(loading.0.run.borrow().is_none());
            loading.finish();
            assert_eq!(status.get_label(), "保存成功");
            let button = Button::builder(&page).with_label("刷新").build();
            button.set_selected(true);
            let button_loading = LoadingIndicator::for_button(&page, button);
            button_loading.start("读取中…");
            assert_eq!(button.get_label(), "刷新");
            // Model the already-rendered busy label without showing a window.
            button.set_label("读取中…");
            button_loading.0.run.borrow_mut().as_mut().unwrap().last = Some("读取中…".into());
            button.set_activity_frame(Some(3));
            button_loading.finish();
            assert_eq!(button.get_label(), "刷新");
            assert!(button.selected());
            button_loading.start("读取中…");
            button.set_label("最新结果");
            button_loading.finish();
            assert_eq!(button.get_label(), "最新结果");
            PENDING.with(|pending| assert!(pending.borrow().is_empty()));
            assert!(!frame.is_shown());
            drop(button_loading);
            drop(loading);
            frame.destroy();
        })
        .unwrap();
    }
}
