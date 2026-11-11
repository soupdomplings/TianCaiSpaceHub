//! Short, interruptible position changes for native containers. Sizes, focus,
//! requests and business state stay with their existing owners.
use super::theme;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer, widgets::SimpleBook};

const TICK_MS: i32 = 16;
pub(super) const PAGE_MS: u64 = 150;
pub(super) const POPUP_MS: u64 = 130;

#[derive(Clone, Copy)]
pub(super) enum Target {
    Panel(Panel),
    Dialog(Dialog),
    Page(SimpleBook, usize),
    NotebookPage(Notebook, usize),
}
impl From<Panel> for Target {
    fn from(panel: Panel) -> Self {
        Self::Panel(panel)
    }
}
impl From<Dialog> for Target {
    fn from(dialog: Dialog) -> Self {
        Self::Dialog(dialog)
    }
}
impl WxWidget for Target {
    fn handle_ptr(&self) -> *mut wxdragon::ffi::wxd_Window_t {
        match self {
            Self::Panel(panel) => panel.handle_ptr(),
            Self::Dialog(dialog) => dialog.handle_ptr(),
            Self::Page(book, index) if book.is_valid() && *index < book.get_page_count() => book
                .get_page(*index)
                .map(|page| page.handle_ptr())
                .unwrap_or(std::ptr::null_mut()),
            Self::Page(_, _) => std::ptr::null_mut(),
            Self::NotebookPage(book, index)
                if book.is_valid() && *index < book.get_page_count() =>
            {
                book.get_page(*index)
                    .map(|page| page.handle_ptr())
                    .unwrap_or(std::ptr::null_mut())
            }
            Self::NotebookPage(_, _) => std::ptr::null_mut(),
        }
    }
}

#[derive(Clone, Copy)]
struct Path {
    from: Point,
    to: Point,
    started: Instant,
    duration: Duration,
}
impl Path {
    fn point(self, now: Instant) -> Point {
        let progress = self.progress(now);
        let eased = 1.0 - (1.0 - progress).powi(3);
        Point::new(
            self.from.x + ((self.to.x - self.from.x) as f64 * eased).round() as i32,
            self.from.y + ((self.to.y - self.from.y) as f64 * eased).round() as i32,
        )
    }
    fn progress(self, now: Instant) -> f64 {
        (now.saturating_duration_since(self.started).as_secs_f64()
            / self.duration.as_secs_f64().max(f64::EPSILON))
        .clamp(0.0, 1.0)
    }
}
struct Run {
    target: Target,
    path: Path,
    last: Point,
    hide: bool,
}
struct State {
    _timer_owner: Panel,
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
        if let Some(run) = self.run.borrow_mut().take() {
            if run.target.is_valid() {
                run.target.move_window(run.path.to.x, run.path.to.y);
                if run.hide {
                    run.target.hide();
                }
            }
        }
    }
    fn tick(&self, now: Instant) {
        if self
            .run
            .borrow()
            .as_ref()
            .is_some_and(|run| run.target.is_valid() && run.target.get_position() != run.last)
        {
            // Resizing/layout or an actual user drag takes precedence.
            self.stop_timer();
            if let Some(run) = self.run.borrow_mut().take()
                && run.hide
                && run.target.is_valid()
            {
                run.target.hide();
            }
            return;
        }
        let finish = {
            let run = self.run.borrow();
            run.as_ref().is_none_or(|run| {
                !theme::motion_enabled() || !visible(&run.target) || run.path.progress(now) >= 1.0
            })
        };
        if finish {
            self.finish();
            return;
        }
        if let Some(run) = self.run.borrow_mut().as_mut() {
            let point = run.path.point(now);
            run.target.move_window(point.x, point.y);
            run.last = point;
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.stop();
        }
    }
}

#[derive(Clone)]
pub(super) struct PositionMotion(Rc<State>);
impl PositionMotion {
    pub fn new(parent: &dyn WxWidget) -> Self {
        // wxdragon binds every TIMER on its owner. A private, hidden Panel
        // keeps animation events away from polling and job-completion timers.
        let timer_owner = Panel::builder(parent).with_size(Size::new(0, 0)).build();
        timer_owner.set_name("native-motion-timer");
        timer_owner.hide();
        let state = Rc::new(State {
            _timer_owner: timer_owner,
            timer: Default::default(),
            run: Default::default(),
        });
        let timer = Timer::new(&timer_owner);
        let weak = Rc::downgrade(&state);
        timer.on_tick(move |_| {
            if let Some(state) = weak.upgrade() {
                state.tick(Instant::now());
            }
        });
        state.timer.borrow_mut().replace(timer);
        let id = timer_owner.handle_ptr() as usize;
        let weak = Rc::downgrade(&state);
        timer_owner.on_destroy(move |event| {
            if own_destroy(&event, id) {
                if let Some(state) = weak.upgrade() {
                    state.stop_timer();
                    state.run.borrow_mut().take();
                }
            }
            event.skip(true);
        });
        Self(state)
    }
    pub fn finish(&self) {
        self.0.finish();
    }
    /// Finalize a pending hide immediately when the app itself is hidden.
    pub fn hide_now(&self, target: &dyn WxWidget) {
        self.finish();
        target.hide();
    }
    pub fn enter(&self, target: impl Into<Target>, home: Point, offset: Point, milliseconds: u64) {
        self.start(target.into(), home, offset, milliseconds, false);
    }
    pub fn exit(&self, target: impl Into<Target>, home: Point, offset: Point, milliseconds: u64) {
        self.start(target.into(), home, offset, milliseconds, true);
    }
    fn start(&self, target: Target, home: Point, offset: Point, milliseconds: u64, hide: bool) {
        let now = Instant::now();
        if !target.is_valid() {
            self.finish();
            return;
        }
        let previous = self.0.run.borrow_mut().take();
        let continued = previous
            .as_ref()
            .filter(|run| run.target.handle_ptr() == target.handle_ptr())
            .map(|run| run.path.point(now));
        if let Some(run) = previous {
            if continued.is_none() && run.target.is_valid() {
                run.target.move_window(run.path.to.x, run.path.to.y);
                if run.hide {
                    run.target.hide();
                }
            }
        }
        self.0.stop_timer();
        let displaced = Point::new(home.x + offset.x, home.y + offset.y);
        let to = if hide { displaced } else { home };
        if !theme::motion_enabled() || !parents_visible(&target) {
            target.move_window(home.x, home.y);
            target.show(!hide);
            return;
        }
        let from = continued.unwrap_or(if hide {
            target.get_position()
        } else {
            displaced
        });
        target.move_window(from.x, from.y);
        target.show(true);
        self.0.run.replace(Some(Run {
            target,
            path: Path {
                from,
                to,
                started: now,
                duration: Duration::from_millis(milliseconds),
            },
            last: from,
            hide,
        }));
        if let Some(timer) = self.0.timer.borrow().as_ref() {
            timer.start(TICK_MS, false);
        }
    }
}

fn own_destroy(event: &WindowEventData, id: usize) -> bool {
    matches!(event, WindowEventData::General(raw)
        if raw.get_event_object().is_some_and(|window| window.as_ptr() as usize == id))
}
fn visible(window: &dyn WxWidget) -> bool {
    if !window.is_valid() || !window.is_shown() {
        return false;
    }
    parents_visible(window)
}
fn parents_visible(window: &dyn WxWidget) -> bool {
    let mut ancestor = window.get_parent();
    while let Some(window) = ancestor {
        if !window.is_valid() || !window.is_shown() {
            return false;
        }
        if let Some(frame) = window.as_widget::<Frame>() {
            if frame.is_iconized() {
                return false;
            }
        }
        ancestor = window.get_parent();
    }
    true
}

thread_local! {
    static BOOKS: RefCell<Vec<(usize, SimpleBook, PositionMotion)>> = const { RefCell::new(Vec::new()) };
}
/// Commit selection immediately, then move the incoming page by only 8 DIP.
/// Repeated background selection of the same page does not restart anything.
pub(super) fn change_page(book: SimpleBook, index: usize) {
    let previous = book.selection();
    if previous == index as i32 {
        return;
    }
    book.freeze();
    book.change_selection(index);
    book.layout();
    page_changed(book, previous);
    book.thaw();
}

/// Call after a page's existing synchronous layout when its caller needs to
/// populate native controls before the entrance begins.
pub(super) fn page_changed(book: SimpleBook, previous: i32) {
    if previous == book.selection() {
        return;
    }
    let motion = BOOKS.with(|books| {
        let mut books = books.borrow_mut();
        books.retain(|(_, book, _)| book.is_valid());
        books
            .iter()
            .find(|(_, existing, _)| existing.handle_ptr() == book.handle_ptr())
            .map(|(_, _, motion)| motion.clone())
    });
    if let Some(motion) = &motion {
        motion.finish();
    }
    if visible(&book) && theme::motion_enabled() {
        if let Some(page) = book.get_page(book.selection() as usize) {
            let motion = motion.unwrap_or_else(|| {
                let motion = PositionMotion::new(&book);
                let id = book.handle_ptr() as usize;
                BOOKS.with(|books| books.borrow_mut().push((id, book, motion.clone())));
                book.on_destroy(move |event| {
                    if own_destroy(&event, id) {
                        BOOKS.with(|books| books.borrow_mut().retain(|(book, _, _)| *book != id));
                    }
                    event.skip(true);
                });
                motion
            });
            motion.enter(
                Target::Page(book, book.selection() as usize),
                page.get_position(),
                Point::new(book.from_dip(8), 0),
                PAGE_MS,
            );
        }
    }
}

/// Shared form/confirmation entrance. No close is delayed and no modal loop
/// is added; the real dialog keeps its existing focus, result and close logic.
pub(super) fn install_dialog(dialog: Dialog) {
    let motion = PositionMotion::new(&dialog);
    let entered = Rc::new(Cell::new(false));
    dialog.on_activate(move |event| {
        if matches!(&event, WindowEventData::Activate(data) if data.is_active())
            && visible(&dialog)
            && !entered.replace(true)
        {
            motion.enter(
                dialog,
                dialog.get_position(),
                Point::new(0, dialog.from_dip(8)),
                PAGE_MS,
            );
        }
        event.skip(true);
    });
}

/// Native request-detail tabs use the same entrance without replacing their
/// platform notebook or delaying its page selection.
pub(super) fn install_notebook(book: Notebook) {
    let motion = PositionMotion::new(&book);
    book.on_page_changed(move |event| {
        if event.get_selection() != event.get_old_selection() {
            motion.finish();
            book.layout();
            if visible(&book) && theme::motion_enabled() {
                if let Some(index) = event.get_selection().filter(|index| *index >= 0) {
                    if let Some(page) = book.get_page(index as usize) {
                        motion.enter(
                            Target::NotebookPage(book, index as usize),
                            page.get_position(),
                            Point::new(book.from_dip(8), 0),
                            PAGE_MS,
                        );
                    }
                }
            }
        }
        event.base.skip(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_position_path_continues_from_current_point_and_finishes_latest_target() {
        let now = Instant::now();
        let first = Path {
            from: Point::new(8, 0),
            to: Point::new(0, 0),
            started: now,
            duration: Duration::from_millis(PAGE_MS),
        };
        let middle = now + Duration::from_millis(60);
        let point = first.point(middle);
        assert!(point.x > 0 && point.x < 8);
        let reverse = Path {
            from: point,
            to: Point::new(8, 0),
            started: middle,
            duration: Duration::from_millis(POPUP_MS),
        };
        assert_eq!(reverse.point(middle), point);
        assert_eq!(
            reverse.point(middle + Duration::from_millis(POPUP_MS)),
            Point::new(8, 0)
        );
        assert_eq!(first.point(now + Duration::from_secs(1)), Point::new(0, 0));
    }

    #[cfg(windows)]
    #[test]
    fn hidden_native_motion_skips_timers_and_reduced_motion_uses_final_state() {
        wxdragon::app::render_offscreen(|| {
            let previous = theme::motion_enabled();
            theme::set_motion_enabled(true);
            let frame = Frame::builder().with_title("hidden native motion").build();
            let panel = Panel::builder(&frame).build();
            let motion = PositionMotion::new(&panel);
            assert_ne!(motion.0._timer_owner.handle_ptr(), panel.handle_ptr());
            assert!(!motion.0._timer_owner.is_shown());
            let target = Panel::builder(&panel).build();
            motion.enter(target, Point::new(10, 20), Point::new(8, 0), PAGE_MS);
            assert_eq!(target.get_position(), Point::new(10, 20));
            assert!(motion.0.run.borrow().is_none());
            assert!(!motion.0.timer.borrow().as_ref().unwrap().is_running());
            theme::set_motion_enabled(false);
            motion.exit(target, Point::new(10, 20), Point::new(8, 0), POPUP_MS);
            assert!(!target.is_shown());
            assert_eq!(target.get_position(), Point::new(10, 20));
            let external = Point::new(30, 40);
            for hide in [true, false] {
                target.move_window(external.x, external.y);
                target.show(true);
                let now = Instant::now();
                motion.0.run.replace(Some(Run {
                    target: target.into(),
                    path: Path {
                        from: Point::new(10, 20),
                        to: Point::new(22, 20),
                        started: now,
                        duration: Duration::from_millis(POPUP_MS),
                    },
                    last: Point::new(10, 20),
                    hide,
                }));
                motion.0.tick(now);
                assert_eq!(target.is_shown(), !hide);
                assert_eq!(target.get_position(), external);
                assert!(motion.0.run.borrow().is_none());
                assert!(!motion.0.timer.borrow().as_ref().unwrap().is_running());
            }
            assert!(!frame.is_shown());
            drop(motion);
            frame.destroy();
            theme::set_motion_enabled(previous);
        })
        .unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn hidden_page_switches_keep_latest_target_and_new_notice_cancels_pending_exit() {
        wxdragon::app::render_offscreen(|| {
            let previous = theme::motion_enabled();
            theme::set_motion_enabled(true);
            let frame = Frame::builder()
                .with_title("hidden interrupted native motion")
                .build();
            let book = SimpleBook::builder(&frame).build();
            for label in ["first", "second", "third"] {
                let page = Panel::builder(&book).build();
                book.add_page(&page, label, label == "first", None);
            }
            for index in [2, 1, 2, 2] {
                change_page(book, index);
                assert_eq!(book.selection(), index as i32);
            }
            BOOKS.with(|books| {
                assert!(
                    books
                        .borrow()
                        .iter()
                        .all(|(_, existing, _)| existing.handle_ptr() != book.handle_ptr())
                )
            });

            let notice = Panel::builder(&frame).build();
            let motion = PositionMotion::new(&notice);
            let now = Instant::now();
            motion.0.run.replace(Some(Run {
                target: notice.into(),
                path: Path {
                    from: Point::new(10, 20),
                    to: Point::new(22, 20),
                    started: now,
                    duration: Duration::from_millis(POPUP_MS),
                },
                last: Point::new(10, 20),
                hide: true,
            }));
            motion.enter(notice, Point::new(10, 20), Point::new(12, 0), POPUP_MS);
            motion.0.tick(now + Duration::from_secs(1));
            assert!(notice.is_shown());
            assert_eq!(notice.get_position(), Point::new(10, 20));
            assert!(motion.0.run.borrow().is_none());
            assert!(!motion.0.timer.borrow().as_ref().unwrap().is_running());
            assert!(!frame.is_shown());
            drop(motion);
            frame.destroy();
            theme::set_motion_enabled(previous);
        })
        .unwrap();
    }
}
