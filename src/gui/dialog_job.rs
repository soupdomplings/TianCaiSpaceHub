//! Keep editable drafts open on failure; only confirmed saves close the dialog.
use super::controls::Button;
use super::{api::ApiClient, feedback, loading::LoadingIndicator};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wxdragon::{prelude::*, timer::Timer};

type JobResult = Arc<Mutex<Option<Result<Value, String>>>>;
type Finished = Rc<RefCell<Option<Box<dyn FnOnce(Result<Value, String>)>>>>;
type Scope = Rc<dyn Fn() -> Option<Value>>;
type Busy = Rc<dyn Fn(bool, &str)>;

thread_local! {
    static ASYNC_HOSTS: RefCell<Vec<Rc<AsyncHost>>> = const { RefCell::new(Vec::new()) };
    static DIALOG_SCOPES: RefCell<Vec<(usize, usize, Rc<Cell<bool>>)>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn track_dialog(dialog: Dialog) -> Rc<Cell<bool>> {
    let id = dialog.handle_ptr() as usize;
    let mut root = id;
    let mut parent = dialog.get_parent();
    while let Some(window) = parent {
        root = window.handle_ptr() as usize;
        parent = window.get_parent();
    }
    let alive = Rc::new(Cell::new(true));
    DIALOG_SCOPES.with(|scopes| scopes.borrow_mut().push((root, id, alive.clone())));
    dialog.on_destroy(move |event| {
        if matches!(&event, WindowEventData::General(raw)
            if raw.get_event_object().is_some_and(|window| window.as_ptr() as usize == id))
        {
            finish_dialog_id(id);
        }
        event.skip(true);
    });
    alive
}
fn finish_dialog_id(id: usize) {
    DIALOG_SCOPES.with(|scopes| {
        scopes.borrow_mut().retain(|(_, dialog, alive)| {
            if *dialog == id {
                alive.set(false);
                false
            } else {
                true
            }
        })
    });
}
pub(super) fn finish_dialog(dialog: Dialog) {
    finish_dialog_id(dialog.handle_ptr() as usize);
}

struct AsyncHost {
    frame: Frame,
    job: InlineJob,
    scope: Scope,
    busy: Busy,
    refresh: Rc<dyn Fn()>,
}

/// Small account reads and actions reuse the existing page busy state. No
/// waiting dialog or nested event loop is created; only real forms stay modal.
pub(super) fn install_async_host(
    frame: Frame,
    owner: Panel,
    status: StaticText,
    loading: LoadingIndicator,
    scope: impl Fn() -> Option<Value> + 'static,
    busy: impl Fn(bool, &str) + 'static,
    refresh: impl Fn() + 'static,
) {
    let id = frame.handle_ptr() as usize;
    let host = Rc::new(AsyncHost {
        frame,
        job: InlineJob::with_loading(owner, status, loading),
        scope: Rc::new(scope),
        busy: Rc::new(busy),
        refresh: Rc::new(refresh),
    });
    ASYNC_HOSTS.with(|hosts| {
        let mut hosts = hosts.borrow_mut();
        hosts.retain(|host| host.frame.handle_ptr() as usize != id);
        hosts.push(host);
    });
    frame.on_destroy(move |event| {
        if matches!(&event, WindowEventData::General(raw)
            if raw.get_event_object().is_some_and(|window| window.as_ptr() as usize == id))
        {
            ASYNC_HOSTS.with(|hosts| {
                hosts
                    .borrow_mut()
                    .retain(|host| host.frame.handle_ptr() as usize != id)
            });
        }
        event.skip(true);
    });
}
pub(super) fn refresh_async_host(frame: &Frame) {
    let host = ASYNC_HOSTS.with(|hosts| {
        hosts
            .borrow()
            .iter()
            .find(|host| host.frame.handle_ptr() == frame.handle_ptr())
            .cloned()
    });
    if let Some(host) = host
        && (host.scope)().is_some()
        && !host.job.is_running()
    {
        (host.refresh)();
    }
}

pub(super) fn run_async(
    frame: &Frame,
    api: ApiClient,
    title: &str,
    path: &str,
    mut body: Value,
    finished: impl FnOnce(Option<Value>) + 'static,
) -> bool {
    let host = ASYNC_HOSTS.with(|hosts| {
        hosts
            .borrow()
            .iter()
            .find(|host| host.frame.handle_ptr() == frame.handle_ptr())
            .cloned()
    });
    let Some(host) = host else { return false };
    if host.job.is_running() {
        return false;
    }
    let Some(scope) = (host.scope)() else {
        return false;
    };
    let local_scope = DIALOG_SCOPES.with(|scopes| {
        scopes
            .borrow()
            .iter()
            .rev()
            .find(|(root, _, alive)| *root == frame.handle_ptr() as usize && alive.get())
            .map(|(_, _, alive)| alive.clone())
    });
    if path == "/api/desktop/squad/request" {
        body["expected_account"] = scope["account"].clone();
    }
    let report_success = body["method"]
        .as_str()
        .is_some_and(|method| method != "GET");
    (host.busy)(true, title);
    let weak = Rc::downgrade(&host);
    host.job
        .start_with_feedback(api, title, path, body, report_success, move |result| {
            let Some(host) = weak.upgrade() else { return };
            (host.busy)(false, "");
            // Page/account changes invalidate the continuation that would open a
            // real form. The backend also receives the existing account guard.
            if continuation_is_current((host.scope)(), &scope, local_scope.as_deref()) {
                finished(result.ok());
            }
        })
}

fn continuation_is_current(
    current: Option<Value>,
    expected: &Value,
    dialog_alive: Option<&Cell<bool>>,
) -> bool {
    dialog_alive.is_none_or(Cell::get) && current.as_ref() == Some(expected)
}

/// One page-owned operation. The worker never touches native controls, and a
/// hidden page still receives completion without opening another window.
pub(super) struct InlineJob {
    owner: Panel,
    _timer_owner: Panel,
    status: StaticText,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    pending: JobResult,
    running: Rc<Cell<bool>>,
    finished: Finished,
    title: Rc<RefCell<String>>,
    report_success: Rc<Cell<bool>>,
    loading: LoadingIndicator,
}
impl InlineJob {
    pub fn new(owner: Panel, status: StaticText) -> Self {
        Self::with_loading(owner, status, LoadingIndicator::new(&owner, status))
    }
    fn with_loading(owner: Panel, status: StaticText, loading: LoadingIndicator) -> Self {
        let pending: JobResult = Default::default();
        let running = Rc::new(Cell::new(false));
        let finished: Finished = Default::default();
        let title = Rc::new(RefCell::new(String::new()));
        let report_success = Rc::new(Cell::new(true));
        let report = report_success.clone();
        let completing_loading = loading.clone();
        let timer_store: Rc<RefCell<Option<Timer<Panel>>>> = Default::default();
        // wxdragon binds all TIMER events on the owner. Keep page polling and
        // this operation on separate native handlers without another dialog.
        let timer_owner = Panel::builder(&owner).with_size(Size::new(0, 0)).build();
        timer_owner.hide();
        let timer = Timer::new(&timer_owner);
        let (slot, busy, callback, operation, stop_timer) = (
            pending.clone(),
            running.clone(),
            finished.clone(),
            title.clone(),
            timer_store.clone(),
        );
        timer.on_tick(move |_| {
            let Some(outcome) = slot.lock().ok().and_then(|mut s| s.take()) else {
                return;
            };
            if let Some(timer) = stop_timer.borrow().as_ref() {
                timer.stop();
            }
            busy.set(false);
            completing_loading.finish();
            let outcome = outcome.map_err(|error| action_error(&operation.borrow(), &error));
            let message = match &outcome {
                Ok(_) => format!("{}完成", operation.borrow()),
                Err(error) => error.clone(),
            };
            if outcome.is_err() || report.get() {
                operation_feedback(owner, status, &message, outcome.is_err());
            } else {
                status.set_label("");
                status.hide();
                owner.layout();
            }
            let completed = callback.borrow_mut().take();
            if let Some(finished) = completed {
                finished(outcome);
            }
        });
        timer_store.borrow_mut().replace(timer);
        Self {
            owner,
            _timer_owner: timer_owner,
            status,
            timer: timer_store,
            pending,
            running,
            finished,
            title,
            report_success,
            loading,
        }
    }
    pub fn is_running(&self) -> bool {
        self.running.get()
    }
    pub fn feedback(&self, message: &str, error: bool) {
        operation_feedback(self.owner, self.status, message, error);
    }
    pub fn start(
        &self,
        api: ApiClient,
        title: &str,
        path: &str,
        body: Value,
        finished: impl FnOnce(Result<Value, String>) + 'static,
    ) -> bool {
        self.start_with_feedback(api, title, path, body, true, finished)
    }
    fn start_with_feedback(
        &self,
        api: ApiClient,
        title: &str,
        path: &str,
        body: Value,
        report_success: bool,
        finished: impl FnOnce(Result<Value, String>) + 'static,
    ) -> bool {
        if self.running.replace(true) {
            return false;
        }
        self.report_success.set(report_success);
        *self.title.borrow_mut() = title.into();
        *self.finished.borrow_mut() = Some(Box::new(finished));
        self.loading.start(&format!("正在{title}…"));
        let (pending, path) = (self.pending.clone(), path.to_string());
        std::thread::spawn(move || {
            let result = api.desktop_post(&path, &body);
            if let Ok(mut slot) = pending.lock() {
                *slot = Some(result);
            }
            wxdragon::wake_up_idle();
        });
        if let Some(timer) = self.timer.borrow().as_ref() {
            timer.start(100, false);
        }
        true
    }
}
impl Drop for InlineJob {
    fn drop(&mut self) {
        self.loading.finish();
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.stop();
        }
    }
}
fn operation_feedback(owner: Panel, status: StaticText, message: &str, error: bool) {
    status.set_label("");
    status.hide();
    owner.layout();
    if error {
        feedback::error(&owner, message);
    } else {
        feedback::success(&owner, message);
    }
}
fn action_error(title: &str, error: &str) -> String {
    if error.trim().is_empty() {
        format!("{title}未完成，后台未返回原因。请重新读取状态后再操作。")
    } else {
        error.into()
    }
}
pub(super) struct DialogJob {
    timer: Rc<RefCell<Option<Timer<Dialog>>>>,
    pub result: Rc<RefCell<Option<Value>>>,
    pub start: Rc<dyn Fn()>,
}
impl Drop for DialogJob {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.stop();
        }
    }
}

pub(super) fn bind(
    dialog: Dialog,
    button: Button,
    status: StaticText,
    api: ApiClient,
    request: impl Fn() -> Result<(String, Value), String> + 'static,
) -> DialogJob {
    bind_with_error(dialog, button, status, api, request, |_| {})
}
pub(super) fn bind_with_error(
    dialog: Dialog,
    button: Button,
    status: StaticText,
    api: ApiClient,
    request: impl Fn() -> Result<(String, Value), String> + 'static,
    on_error: impl Fn(&str) + 'static,
) -> DialogJob {
    bind_job(dialog, button, status, api, request, on_error, None)
}
/// Keep a member's combined detail/editor open after a confirmed save.
pub(super) fn bind_open(
    dialog: Dialog,
    button: Button,
    status: StaticText,
    api: ApiClient,
    request: impl Fn() -> Result<(String, Value), String> + 'static,
    on_saved: impl Fn(Value) + 'static,
) -> DialogJob {
    bind_job(
        dialog,
        button,
        status,
        api,
        request,
        |_| {},
        Some(Box::new(on_saved)),
    )
}
fn bind_job(
    dialog: Dialog,
    button: Button,
    status: StaticText,
    api: ApiClient,
    request: impl Fn() -> Result<(String, Value), String> + 'static,
    on_error: impl Fn(&str) + 'static,
    on_saved: Option<Box<dyn Fn(Value)>>,
) -> DialogJob {
    let store: JobResult = Arc::new(Mutex::new(None));
    let result = Rc::new(RefCell::new(None));
    let timer_store: Rc<RefCell<Option<Timer<Dialog>>>> = Rc::new(RefCell::new(None));
    let in_flight = Arc::new(AtomicBool::new(false));
    let loading = LoadingIndicator::new(&dialog, status);
    let close_busy = in_flight.clone();
    dialog.on_close(move |event| {
        if close_busy.load(Ordering::SeqCst) {
            if let WindowEventData::General(event) = event {
                event.veto();
            }
        } else {
            event.skip(true);
        }
    });
    let timer = Timer::new(&dialog);
    {
        let (store, result, timer_store, in_flight) = (
            store.clone(),
            result.clone(),
            timer_store.clone(),
            in_flight.clone(),
        );
        let loading = loading.clone();
        timer.on_tick(move |_| {
            let outcome = store.lock().ok().and_then(|mut s| s.take());
            if let Some(outcome) = outcome {
                if let Some(timer) = timer_store.borrow().as_ref() {
                    timer.stop();
                }
                in_flight.store(false, Ordering::SeqCst);
                loading.finish();
                dialog.enable(true);
                button.enable(true);
                match outcome {
                    Ok(value) => {
                        *result.borrow_mut() = Some(value.clone());
                        status.set_label("");
                        status.hide();
                        if let Some(saved) = &on_saved {
                            feedback::success(&dialog, "操作完成");
                            saved(value);
                        } else {
                            dialog.end_modal(ID_OK);
                            if let Some(parent) = dialog.get_parent() {
                                feedback::success(&parent, "操作完成");
                            }
                        }
                    }
                    Err(error) => {
                        let error = action_error("保存", &error);
                        on_error(&error);
                        status.set_label("");
                        status.hide();
                        feedback::error(&dialog, &error);
                        dialog.layout();
                    }
                }
            }
        });
    }
    timer_store.borrow_mut().replace(timer);
    let start = {
        let (timer_store, store) = (timer_store.clone(), store.clone());
        Rc::new(move || {
            if in_flight.load(Ordering::SeqCst) {
                return;
            }
            let (path, body) = match request() {
                Ok(v) => v,
                Err(e) => {
                    let e = action_error("保存", &e);
                    status.set_label(&e);
                    status.show(true);
                    status.set_foreground_color(super::theme::theme().error);
                    status.wrap((dialog.get_client_size().width - dialog.from_dip(40)).max(100));
                    dialog.layout();
                    return;
                }
            };
            in_flight.store(true, Ordering::SeqCst);
            dialog.enable(false);
            button.enable(false);
            status.set_label("");
            status.hide();
            loading.start(if path == "/api/desktop/assistance/prepare" {
                "正在准备天才搭档 Server，请稍候…"
            } else if path == "/api/desktop/assistance" {
                "正在保存并检查天才搭档 Server…"
            } else {
                "正在保存…"
            });
            let (store, api) = (store.clone(), api.clone());
            std::thread::spawn(move || {
                let outcome = api.desktop_post(&path, &body);
                if let Ok(mut slot) = store.lock() {
                    *slot = Some(outcome);
                }
                wxdragon::wake_up_idle();
            });
            if let Some(timer) = timer_store.borrow().as_ref() {
                timer.start(100, false);
            }
        }) as Rc<dyn Fn()>
    };
    let click = start.clone();
    button.on_click(move |_| click());
    DialogJob {
        timer: timer_store,
        result,
        start,
    }
}

pub(super) fn run(
    parent: &Frame,
    api: ApiClient,
    title: &str,
    path: &str,
    body: Value,
) -> Option<Value> {
    let dialog = Dialog::builder(parent, title).with_size(540, 230).build();
    dialog.set_background_color(super::theme::theme().bg_app);
    dialog.set_font(&super::theme::font(super::theme::TextRole::Body));
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    let panel = Panel::builder(&dialog).build();
    panel.set_background_color(super::theme::theme().bg_app);
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let status = StaticText::builder(&panel)
        .with_label(&format!("正在{title}…"))
        .build();
    status.set_foreground_color(super::theme::theme().ink_secondary);
    column.add(
        &status,
        1,
        SizerFlag::Expand | SizerFlag::All,
        panel.from_dip(20),
    );
    let button = Button::builder(&panel).with_label("关闭").build();
    button.hide();
    column.add(
        &button,
        0,
        SizerFlag::AlignRight | SizerFlag::All,
        panel.from_dip(16),
    );
    panel.set_sizer(column, true);
    outer.add(&panel, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(outer, true);
    let result = Rc::new(RefCell::new(None));
    let job = InlineJob::new(panel, status);
    let busy = job.running.clone();
    dialog.on_close(move |event| {
        if busy.get() {
            if let WindowEventData::General(event) = event {
                event.veto();
            }
        } else {
            event.skip(true);
        }
    });
    button.on_click(move |_| dialog.end_modal(ID_CANCEL));
    let saved = result.clone();
    job.start(api, title, path, body, move |outcome| match outcome {
        Ok(value) => {
            *saved.borrow_mut() = Some(value);
            dialog.end_modal(ID_OK);
        }
        Err(_) => {
            button.show(true);
            panel.layout();
        }
    });
    let accepted = dialog.show_modal() == ID_OK;
    let value = result.borrow_mut().take();
    drop(job);
    dialog.destroy();
    if accepted { value } else { None }
}

pub(super) fn post(api: &ApiClient, path: &str, body: &Value) -> Result<Value, String> {
    let response = api
        .request_text(
            api.http
                .post(format!("{}{}", api.base_url, path))
                .header("x-tiancai-desktop", "1")
                .json(body)
                .timeout(Duration::from_secs(
                    if matches!(
                        path,
                        "/api/desktop/assistance/prepare" | "/api/desktop/assistance"
                    ) {
                        600
                    } else {
                        120
                    },
                )),
        )
        .map_err(|error| {
            if path.starts_with("/api/desktop/squad/") {
                super::squad_browser::friendly_error(&error)
            } else {
                error
            }
        })?;
    serde_json::from_str(&response).map_err(|_| "后台返回内容无效，请重新读取状态".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn async_continuation_requires_current_page_account_and_open_dialog() {
        let expected = json!({"serial":3,"account":{"userId":7,"generation":2}});
        let alive = Cell::new(true);
        assert!(continuation_is_current(
            Some(expected.clone()),
            &expected,
            None
        ));
        assert!(continuation_is_current(
            Some(expected.clone()),
            &expected,
            Some(&alive)
        ));
        assert!(!continuation_is_current(None, &expected, Some(&alive)));
        for changed in [
            json!({"serial":4,"account":{"userId":7,"generation":2}}),
            json!({"serial":3,"account":{"userId":8,"generation":2}}),
            json!({"serial":3,"account":{"userId":7,"generation":3}}),
        ] {
            assert!(!continuation_is_current(
                Some(changed),
                &expected,
                Some(&alive)
            ));
        }
        alive.set(false);
        assert!(!continuation_is_current(
            Some(expected.clone()),
            &expected,
            Some(&alive)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn hidden_dialog_scope_ends_before_native_destroy_and_tracks_its_root() {
        wxdragon::app::render_offscreen(|| {
            let frame = Frame::builder().with_title("hidden account scope").build();
            let panel = Panel::builder(&frame).build();
            let status = StaticText::builder(&panel).build();
            let job = InlineJob::new(panel, status);
            assert_ne!(job._timer_owner.handle_ptr(), panel.handle_ptr());
            assert_eq!(
                job._timer_owner.get_parent().unwrap().handle_ptr(),
                panel.handle_ptr()
            );
            assert!(!job._timer_owner.is_shown());
            let dialog = Dialog::builder(&panel, "synthetic business form").build();
            let alive = track_dialog(dialog);
            let id = dialog.handle_ptr() as usize;
            assert!(alive.get());
            DIALOG_SCOPES.with(|scopes| {
                assert!(scopes.borrow().iter().any(|(root, tracked, _)| *root
                    == frame.handle_ptr() as usize
                    && *tracked == id));
            });
            finish_dialog(dialog);
            assert!(!alive.get());
            DIALOG_SCOPES.with(|scopes| {
                assert!(scopes.borrow().iter().all(|(_, tracked, _)| *tracked != id));
            });
            assert!(!dialog.is_shown() && !frame.is_shown());
            dialog.destroy();
            drop(job);
            frame.destroy();
        })
        .unwrap();
    }
}
