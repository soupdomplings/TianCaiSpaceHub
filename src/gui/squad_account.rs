//! Global profile badge. Only account API metadata, never browser credentials.
use super::{api::ApiClient, controls::Button};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use wxdragon::{prelude::*, timer::Timer};
type Slot = Arc<Mutex<Option<(u64, Result<Value, String>)>>>;
#[derive(Clone)]
pub(super) struct AccountBadge {
    api: ApiClient,
    button: Button,
    pending: Slot,
    busy: Rc<Cell<bool>>,
    serial: Rc<Cell<u64>>,
    timer: Rc<RefCell<Option<Timer<Panel>>>>,
}
use super::squad_browser::render_profile as render;
impl AccountBadge {
    pub fn new(parent: &Panel, button: Button, api: ApiClient) -> Self {
        let ui = Self {
            api,
            button,
            pending: Default::default(),
            busy: Default::default(),
            serial: Default::default(),
            timer: Default::default(),
        };
        let timer = Timer::new(parent);
        let u = ui.clone();
        let ticks = Rc::new(Cell::new(0u32));
        timer.on_tick(move |_| {
            if let Some((serial, result)) = u.pending.lock().ok().and_then(|mut p| p.take()) {
                if serial == u.serial.get() {
                    u.busy.set(false);
                    if let Ok(state) = result {
                        render(u.button, &state);
                    }
                }
            }
            ticks.set(ticks.get() + 1);
            if ticks.get() % 120 == 0 {
                u.refresh();
            }
        });
        timer.start(500, false);
        ui.timer.borrow_mut().replace(timer);
        render(ui.button, &Value::Null);
        ui.refresh();
        ui
    }
    pub fn refresh(&self) {
        if self.busy.replace(true) {
            return;
        }
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        let (api, slot) = (self.api.clone(), self.pending.clone());
        std::thread::spawn(move || {
            let result =
                api.get_with_timeout("/api/desktop/squad/profile", Duration::from_secs(30));
            if let Ok(mut value) = slot.lock() {
                if value.as_ref().is_none_or(|(old, _)| *old <= serial) {
                    *value = Some((serial, result));
                }
            }
            wxdragon::wake_up_idle();
        });
    }
    pub fn set(&self, state: &Value) {
        self.serial.set(self.serial.get() + 1);
        self.busy.set(false);
        render(self.button, state);
    }
}
