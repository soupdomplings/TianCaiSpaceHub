//! Repair runs on a worker while the installed Hub and its gateway stay open.
use super::{
    text::{GuiLocale, GuiText},
    theme,
};
use crate::codex_startup_repair::{self as repair, RepairOutcome};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
use wxdragon::{id::ID_NONE, prelude::*, timer::Timer};

fn label<'a>(english: bool, zh: &'a str, en: &'a str) -> &'a str {
    if english { en } else { zh }
}

pub(super) fn bind_button(
    button: &Button,
    frame: &Frame,
    text: GuiText,
    in_flight: &Arc<AtomicBool>,
) {
    let frame = *frame;
    let in_flight = in_flight.clone();
    button.on_click(move |_| {
        if in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        show(&frame, text.locale == GuiLocale::EnUs);
        in_flight.store(false, Ordering::SeqCst);
    });
}

enum Update {
    Progress(String),
    Finished {
        result: Result<RepairOutcome, String>,
        restoring: bool,
    },
}

fn start_job(sender: mpsc::Sender<Update>, restore: Option<PathBuf>) {
    let restoring = restore.is_some();
    let failed_sender = sender.clone();
    let spawned = thread::Builder::new()
        .name("codex-startup-repair".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(|| {
                if let Some(backup) = restore {
                    let _ = sender.send(Update::Progress("正在恢复修复前的备份…".into()));
                    repair::restore(&backup).map(|()| RepairOutcome {
                        message:
                            "已恢复修复前的原库。原启动错误也可能随之恢复，请保留备份进一步检查。"
                                .into(),
                        backup: Some(backup),
                        applied: false,
                    })
                } else {
                    repair::run(|message| {
                        let _ = sender.send(Update::Progress(message.to_string()));
                    })
                }
            });
            let result = match result {
                Ok(result) => result.map_err(|error| error.to_string()),
                Err(_) => Err("检查意外中断，请保留备份并导出连接诊断包进一步检查。".into()),
            };
            let _ = sender.send(Update::Finished { result, restoring });
            wxdragon::wake_up_idle();
        });
    if spawned.is_err() {
        let _ = failed_sender.send(Update::Finished {
            result: Err("无法开始检查，请稍后重试。".into()),
            restoring,
        });
    }
}

fn show(parent: &Frame, english: bool) {
    let dialog = Dialog::builder(
        parent,
        label(
            english,
            "修复无法加载组织设置",
            "Repair organization settings error",
        ),
    )
    .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
    .with_size(680, 460)
    .build();
    dialog.set_min_size(Size::new(520, 360));
    dialog.set_escape_id(ID_NONE);
    dialog.set_background_color(theme::theme().bg_card);
    let panel = Panel::builder(&dialog).build();
    panel.set_background_color(theme::theme().bg_card);
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let status = StaticText::builder(&panel)
        .with_label(label(english, "正在自动检查…", "Checking automatically…"))
        .build();
    status.set_foreground_color(theme::theme().ink_primary);
    status.wrap(620);
    sizer.add(&status, 0, SizerFlag::Expand | SizerFlag::All, 18);
    let detail = TextCtrl::builder(&panel).with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
        .with_value(label(english,
            "请保持 ChatGPT/Codex 完全退出。Hub 保持打开。\n确认是已支持的状态库冲突时，先备份、验证副本，再应用修复。",
            "Keep ChatGPT/Codex fully closed. Hub stays open.\nConfirmed state conflicts are backed up and verified on a copy before repair.")).build();
    sizer.add(
        &detail,
        1,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        18,
    );
    let actions = BoxSizer::builder(Orientation::Horizontal).build();
    let retry = Button::builder(&panel)
        .with_label(label(english, "重新检查", "Check again"))
        .build();
    let restore_button = Button::builder(&panel)
        .with_label(label(english, "恢复本次修复", "Restore backup"))
        .build();
    let folder = Button::builder(&panel)
        .with_label(label(english, "打开备份", "Open backup"))
        .build();
    let close = Button::builder(&panel)
        .with_label(label(english, "关闭", "Close"))
        .build();
    for button in [&retry, &restore_button, &folder, &close] {
        button.enable(false);
        actions.add(button, 0, SizerFlag::Right, 8);
    }
    sizer.add_sizer(&actions, 0, SizerFlag::Expand | SizerFlag::All, 18);
    panel.set_sizer(sizer, true);
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add(&panel, 1, SizerFlag::Expand, 0);
    dialog.set_sizer(outer, true);
    let busy = Rc::new(Cell::new(true));
    let backup = Rc::new(RefCell::new(None::<PathBuf>));
    let can_restore = Rc::new(Cell::new(false));
    let (sender, receiver) = mpsc::channel();
    let timer = Timer::new(&dialog);
    {
        let busy = busy.clone();
        let backup = backup.clone();
        let can_restore = can_restore.clone();
        timer.on_tick(move |_| {
            while let Ok(update) = receiver.try_recv() {
                match update {
                    Update::Progress(message) => {
                        detail.change_value(&message);
                    }
                    Update::Finished { result, restoring } => {
                        busy.set(false);
                        retry.enable(true);
                        close.enable(true);
                        match result {
                            Ok(outcome) => {
                                status.set_label(label(
                                    english,
                                    "检查处理已结束",
                                    "Check completed",
                                ));
                                let mut message = outcome.message;
                                if outcome.applied || !can_restore.get() || restoring {
                                    if outcome.backup.is_some() {
                                        *backup.borrow_mut() = outcome.backup;
                                    }
                                    can_restore.set(outcome.applied);
                                }
                                if let Some(path) = backup.borrow().as_ref() {
                                    message.push_str(&format!(
                                        "\n\n{}{}",
                                        label(english, "备份目录：", "Backup: "),
                                        path.display()
                                    ));
                                }
                                detail.change_value(&message);
                            }
                            Err(error) => {
                                status.set_label(label(
                                    english,
                                    "已停止，请查看下方原因",
                                    "Stopped — see the reason below",
                                ));
                                detail.change_value(&error);
                            }
                        }
                        folder.enable(backup.borrow().is_some());
                        restore_button.enable(can_restore.get());
                    }
                }
            }
        });
    }
    {
        let busy = busy.clone();
        let sender = sender.clone();
        retry.on_click(move |_| {
            if busy.replace(true) {
                return;
            }
            for button in [retry, close, restore_button, folder] {
                button.enable(false);
            }
            status.set_label(label(english, "正在重新检查…", "Checking again…"));
            start_job(sender.clone(), None);
        });
    }
    {
        let busy = busy.clone();
        let sender = sender.clone();
        let backup = backup.clone();
        restore_button.on_click(move |_| {
            if busy.get() { return; }
            let Some(path) = backup.borrow().clone() else { return; };
            if !super::external_import::confirm(&dialog, label(english,
                "将恢复本次修复前的原库，原启动错误也可能恢复。请先完全退出 ChatGPT/Codex；只有修复后未产生新记录时才允许恢复。继续？",
                "Restore the pre-repair database? The original startup error may return. Fully quit ChatGPT/Codex first. Restore is allowed only if the repaired files are unchanged.")) { return; }
            busy.set(true);
            for button in [retry, close, restore_button, folder] { button.enable(false); }
            start_job(sender.clone(), Some(path));
        });
    }
    {
        let backup = backup.clone();
        folder.on_click(move |_| {
            if let Some(path) = backup.borrow().as_ref() {
                #[cfg(windows)]
                let result = Command::new("explorer.exe").arg(path).spawn();
                #[cfg(not(windows))]
                let result = Command::new("/usr/bin/open").arg(path).spawn();
                if result.is_err() {
                    super::show_error(
                        &dialog,
                        label(
                            english,
                            "无法打开备份文件夹，可以从上方复制路径。",
                            "Cannot open the backup folder; copy its path from above.",
                        ),
                    );
                }
            }
        });
    }
    {
        let busy = busy.clone();
        close.on_click(move |_| {
            if !busy.get() {
                dialog.end_modal(ID_CANCEL);
            }
        });
    }
    dialog.on_close(move |event| {
        if busy.get()
            && let WindowEventData::General(raw) = &event
            && raw.can_veto()
        {
            raw.veto();
            return;
        }
        dialog.end_modal(ID_CANCEL);
    });
    dialog.center();
    timer.start(150, false);
    start_job(sender, None);
    dialog.show_modal();
    timer.stop();
    dialog.destroy();
}
