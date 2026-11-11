//! Keep editable drafts open on failure; only confirmed saves close the dialog.
use super::api::ApiClient;
use super::controls::Button;
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

/// One page-owned operation. The worker never touches native controls, and a
/// hidden page still receives completion without opening another window.
pub(super) struct InlineJob {
    owner: Panel,
    status: StaticText,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
    pending: JobResult,
    running: Rc<Cell<bool>>,
    finished: Finished,
    title: Rc<RefCell<String>>,
}
impl InlineJob {
    pub fn new(owner: Panel, status: StaticText) -> Self {
        let pending: JobResult = Default::default();
        let running = Rc::new(Cell::new(false));
        let finished: Finished = Default::default();
        let title = Rc::new(RefCell::new(String::new()));
        let timer_store: Rc<RefCell<Option<Timer<Panel>>>> = Default::default();
        let timer = Timer::new(&owner);
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
            let outcome = outcome.map_err(|error| action_error(&operation.borrow(), &error));
            let message = match &outcome {
                Ok(_) => format!("{}完成", operation.borrow()),
                Err(error) => error.clone(),
            };
            inline_feedback(owner, status, &message, outcome.is_err());
            let completed = callback.borrow_mut().take();
            if let Some(finished) = completed {
                finished(outcome);
            }
        });
        timer_store.borrow_mut().replace(timer);
        Self {
            owner,
            status,
            timer: timer_store,
            pending,
            running,
            finished,
            title,
        }
    }
    pub fn is_running(&self) -> bool {
        self.running.get()
    }
    pub fn feedback(&self, message: &str, error: bool) {
        inline_feedback(self.owner, self.status, message, error);
    }
    pub fn start(
        &self,
        api: ApiClient,
        title: &str,
        path: &str,
        body: Value,
        finished: impl FnOnce(Result<Value, String>) + 'static,
    ) -> bool {
        if self.running.replace(true) {
            return false;
        }
        *self.title.borrow_mut() = title.into();
        *self.finished.borrow_mut() = Some(Box::new(finished));
        self.feedback(&format!("正在{title}…"), false);
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
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.stop();
        }
    }
}
fn inline_feedback(owner: Panel, status: StaticText, message: &str, error: bool) {
    status.set_label(message);
    status.set_foreground_color(if error {
        super::theme::theme().error
    } else {
        super::theme::theme().ink_secondary
    });
    status.wrap((owner.get_client_size().width - owner.from_dip(40)).max(100));
    status.show(!message.is_empty());
    owner.layout();
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
        timer.on_tick(move |_| {
            let outcome = store.lock().ok().and_then(|mut s| s.take());
            if let Some(outcome) = outcome {
                if let Some(timer) = timer_store.borrow().as_ref() {
                    timer.stop();
                }
                in_flight.store(false, Ordering::SeqCst);
                dialog.enable(true);
                button.enable(true);
                match outcome {
                    Ok(value) => {
                        *result.borrow_mut() = Some(value.clone());
                        if let Some(saved) = &on_saved {
                            status.set_label("已保存");
                            status.set_foreground_color(super::theme::theme().ok);
                            saved(value);
                        } else {
                            dialog.end_modal(ID_OK);
                        }
                    }
                    Err(error) => {
                        let error = action_error("保存", &error);
                        on_error(&error);
                        status.set_label(&error);
                        status.set_foreground_color(super::theme::theme().error);
                        status
                            .wrap((dialog.get_client_size().width - dialog.from_dip(40)).max(100));
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
                    status.set_foreground_color(super::theme::theme().error);
                    status.wrap((dialog.get_client_size().width - dialog.from_dip(40)).max(100));
                    dialog.layout();
                    return;
                }
            };
            in_flight.store(true, Ordering::SeqCst);
            dialog.enable(false);
            button.enable(false);
            status.set_foreground_color(super::theme::theme().ink_secondary);
            status.set_label(if path == "/api/desktop/assistance/prepare" {
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
