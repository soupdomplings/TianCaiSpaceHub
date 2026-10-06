//! Native URL events are queued only in memory, then handled by the GUI timer.
use crate::external_import::{ImportLink, ipc};
use std::{
    cell::RefCell,
    collections::HashSet,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};
use wxdragon::{prelude::*, timer::Timer};

pub(super) struct NativeEvents {
    pub receiver: Receiver<Result<ImportLink, String>>,
    overflow: Arc<AtomicBool>,
}

impl NativeEvents {
    pub fn attach(app: App) -> Self {
        let (sender, receiver) = mpsc::sync_channel(16);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = overflow.clone();
        app.on_open_url(move |raw| {
            if sender.try_send(ImportLink::parse(&raw)).is_err() {
                callback_overflow.store(true, Ordering::Relaxed);
            }
        });
        Self { receiver, overflow }
    }

    pub fn take_overflow(&self) -> bool {
        self.overflow.swap(false, Ordering::Relaxed)
    }
}

/// Launch Services normally targets the existing app. If a second installed
/// copy is launched, keep its event loop alive long enough to receive and relay
/// its initial Apple Event instead of dropping the URL at the single-instance gate.
pub(in crate::gui) fn relay_to_running_instance() {
    let result = wxdragon::main(|app| {
        let native = NativeEvents::attach(app);
        let frame = Frame::builder().with_title("TianCaiSpaceHub").build();
        app.set_top_window(&frame);
        frame.show(false);
        let timer = Rc::new(Timer::new(&frame));
        let timer_weak = Rc::downgrade(&timer);
        let (sender, receiver) = mpsc::sync_channel::<Result<(), String>>(16);
        let mut active = 0usize;
        let mut seen = HashSet::new();
        let mut deadline = Instant::now() + Duration::from_secs(12);
        timer.on_tick(move |_| {
            let Some(timer) = timer_weak.upgrade() else {
                return;
            };
            // Error dialogs can enter a nested event loop.
            timer.stop();
            while let Ok(result) = native.receiver.try_recv() {
                match result {
                    Ok(link) => {
                        if seen.contains(&link.identity()) {
                            continue;
                        }
                        if seen.len() >= 16 {
                            super::super::show_error(
                                &frame,
                                "待处理导入过多，请从网页重试 / Too many pending imports",
                            );
                            continue;
                        }
                        seen.insert(link.identity());
                        active += 1;
                        let sender = sender.clone();
                        std::thread::spawn(move || {
                            let _ = sender.send(ipc::forward(&link));
                        });
                    }
                    Err(error) => super::super::show_error(&frame, &error),
                }
                deadline = Instant::now() + Duration::from_secs(1);
            }
            if native.take_overflow() {
                super::super::show_error(
                    &frame,
                    "待处理导入过多，请从网页重试 / Too many pending imports",
                );
            }
            while let Ok(result) = receiver.try_recv() {
                active = active.saturating_sub(1);
                if let Err(error) = result {
                    super::super::show_error(&frame, &error);
                }
                deadline = Instant::now() + Duration::from_secs(1);
            }
            if active == 0 && Instant::now() >= deadline {
                frame.close(true);
            } else {
                timer.start(100, false);
            }
        });
        timer.start(100, false);
        // Keep the timer alive until the relay exits, without an Rc cycle.
        let timer_owner = RefCell::new(Some(timer));
        frame.on_close(move |_| {
            if let Some(timer) = timer_owner.borrow_mut().take() {
                timer.stop();
            }
            frame.destroy();
            app.exit_main_loop();
        });
    });
    if result.is_err() {
        crate::external_import::registration::show_startup_error(
            "无法启动导入转交，请退出旧 Hub 后重试 / Cannot start import handoff",
        );
    }
}
