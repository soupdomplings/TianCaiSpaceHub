//! Keep editable drafts open on failure; only confirmed saves close the dialog.
use super::api::ApiClient;
use super::controls::Button;
use serde_json::Value;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wxdragon::{prelude::*, timer::Timer};

type JobResult = Arc<Mutex<Option<Result<Value, String>>>>;
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
            status.set_label("正在保存…");
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
    let column = BoxSizer::builder(Orientation::Vertical).build();
    let heading = StaticText::builder(&dialog).with_label(title).build();
    heading.set_font(&super::theme::font(super::theme::TextRole::Title));
    heading.set_foreground_color(super::theme::theme().ink_primary);
    column.add(
        &heading,
        0,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        dialog.from_dip(20),
    );
    let status = StaticText::builder(&dialog).with_label("正在处理…").build();
    status.set_foreground_color(super::theme::theme().ink_secondary);
    column.add(&status, 1, SizerFlag::Expand | SizerFlag::All, 20);
    let button = Button::builder(&dialog).with_label("重试").build();
    column.add(&button, 0, SizerFlag::AlignRight | SizerFlag::All, 16);
    dialog.set_sizer(column, true);
    let path = path.to_string();
    let job = bind(dialog, button, status, api, move || {
        Ok((path.clone(), body.clone()))
    });
    (job.start)();
    let accepted = dialog.show_modal() == ID_OK;
    let value = job.result.borrow().clone();
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
                .timeout(Duration::from_secs(120)),
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
