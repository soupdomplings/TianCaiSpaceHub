//! One native repair window. Inspection never starts a write operation.
use super::{
    text::{GuiLocale, GuiText},
    theme,
};
use crate::codex_repair_center::{self as repair, Kind, Outcome, Record};
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
fn tr<'a>(en: bool, zh: &'a str, english: &'a str) -> &'a str {
    if en { english } else { zh }
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
enum Task {
    Check(Kind, String, bool),
    Apply(Kind, String, bool),
    Restore(PathBuf),
}
enum Update {
    Progress(String),
    Finished(Result<Outcome, String>, Vec<Record>),
}
fn start(sender: mpsc::Sender<Update>, task: Task) {
    let fallback = sender.clone();
    if thread::Builder::new()
        .name("codex-repair-centre".into())
        .spawn(move || {
            let progress = |s: &str| {
                let _ = sender.send(Update::Progress(s.into()));
            };
            let result = std::panic::catch_unwind(|| match task {
                Task::Check(kind, evidence, proxy) => {
                    repair::run(kind, false, &evidence, proxy, progress)
                }
                Task::Apply(kind, evidence, proxy) => {
                    repair::run(kind, true, &evidence, proxy, progress)
                }
                Task::Restore(path) => repair::restore(&path, progress),
            });
            let result = match result {
                Ok(r) => r.map_err(|e| e.to_string()),
                Err(_) => {
                    Err("检查或修复意外中断，请保留备份；可以重新打开窗口查看修复记录。".into())
                }
            };
            let _ = sender.send(Update::Finished(
                result,
                repair::records().unwrap_or_default(),
            ));
            wxdragon::wake_up_idle();
        })
        .is_err()
    {
        let _ = fallback.send(Update::Finished(
            Err("无法开始后台检查，请稍后重试。".into()),
            Vec::new(),
        ));
    }
}
fn open_folder(path: &std::path::Path) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        Command::new("explorer.exe").arg(path).spawn()
    }
    #[cfg(not(windows))]
    {
        Command::new("/usr/bin/open").arg(path).spawn()
    }
}
fn show(parent: &Frame, en: bool) {
    let size = super::ai_gw_channel_dialog_size(parent, Size::new(800, 650));
    let dialog = Dialog::builder(
        parent,
        tr(
            en,
            "ChatGPT(Codex) 检查与修复",
            "ChatGPT(Codex) check and repair",
        ),
    )
    .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
    .with_size(size.width, size.height)
    .build();
    dialog.set_min_size(Size::new(500.min(size.width), 350.min(size.height)));
    dialog.set_escape_id(ID_NONE);
    dialog.set_background_color(theme::theme().bg_card);
    let panel = ScrolledWindow::builder(&dialog)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    panel.set_background_color(theme::theme().bg_card);
    panel.set_min_size(Size::new(0, 0));
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let intro=StaticText::builder(&panel).with_label(tr(en,"先选择遇到的问题并检查，再按结果修复。检查不改文件；修复前退出 ChatGPT/Codex，Hub 保持打开。","Choose an issue and inspect it first. Checks do not change files. Quit ChatGPT/Codex before applying a repair; Hub stays open.")).build();
    intro.wrap((size.width - 75).max(300));
    sizer.add(&intro, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let selector = Choice::builder(&panel).build();
    for kind in Kind::ALL {
        selector.append(kind.label());
    }
    selector.set_selection(0);
    sizer.add(
        &selector,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let help = TextCtrl::builder(&panel)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
        .with_size(Size::new(-1, 76))
        .with_value(Kind::Startup.help())
        .build();
    sizer.add(&help, 0, SizerFlag::Expand | SizerFlag::All, 12);
    let evidence_label = StaticText::builder(&panel)
        .with_label(tr(
            en,
            "错误提示（Windows 10 截图专项必填，其他项可留空）",
            "Error text (required for Windows 10 screenshot repair only)",
        ))
        .build();
    sizer.add(
        &evidence_label,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let evidence = TextCtrl::builder(&panel).build();
    sizer.add(
        &evidence,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
        16,
    );
    let mcp_proxy=CheckBox::builder(&panel).with_label(tr(en,"为已配置的 stdio MCP 传递本机已有的代理变量（仅 MCP 修复，默认关闭）","Forward existing proxy variables to configured stdio MCP servers (MCP repair only, off by default)")).build();
    sizer.add(&mcp_proxy, 0, SizerFlag::All, 16);
    mcp_proxy.enable(false);
    let status = StaticText::builder(&panel)
        .with_label(tr(
            en,
            "正在检查所选问题…",
            "Inspecting the selected issue…",
        ))
        .build();
    status.wrap((size.width - 75).max(300));
    sizer.add(
        &status,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let detail = TextCtrl::builder(&panel)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
        .with_size(Size::new(-1, 180))
        .build();
    sizer.add(&detail, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let record_label = StaticText::builder(&panel)
        .with_label(tr(
            en,
            "修复记录（重启 Hub 后仍可打开与恢复）",
            "Repair records (available after restarting Hub)",
        ))
        .build();
    sizer.add(
        &record_label,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let record = Choice::builder(&panel).build();
    sizer.add(&record, 0, SizerFlag::Expand | SizerFlag::All, 16);
    panel.set_sizer(sizer, true);
    panel.set_scroll_rate(0, 10);
    panel.fit_inside();
    let footer = Panel::builder(&dialog).build();
    footer.set_background_color(theme::theme().bg_card);
    let actions = BoxSizer::builder(Orientation::Vertical).build();
    let primary = BoxSizer::builder(Orientation::Horizontal).build();
    let check = Button::builder(&footer)
        .with_label(tr(en, "检查所选问题", "Check selected issue"))
        .build();
    let apply = Button::builder(&footer)
        .with_label(tr(en, "修复所选问题", "Repair selected issue"))
        .build();
    let close = Button::builder(&footer)
        .with_label(tr(en, "关闭", "Close"))
        .build();
    for b in [&check, &apply, &close] {
        primary.add(b, 0, SizerFlag::Right, 8);
    }
    actions.add_sizer(&primary, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let secondary = BoxSizer::builder(Orientation::Horizontal).build();
    let restore = Button::builder(&footer)
        .with_label(tr(en, "恢复所选记录", "Restore selected record"))
        .build();
    let folder = Button::builder(&footer)
        .with_label(tr(en, "打开备份", "Open backup"))
        .build();
    for b in [&restore, &folder] {
        secondary.add(b, 0, SizerFlag::Right, 8);
    }
    #[cfg(target_os = "macos")]
    {
        let permissions = Button::builder(&footer)
            .with_label(tr(en, "系统权限设置", "System permissions"))
            .build();
        secondary.add(&permissions, 0, SizerFlag::Right, 8);
        permissions.on_click(move |_| {
            let _ = Command::new("/usr/bin/open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn();
        });
    }
    actions.add_sizer(
        &secondary,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
        10,
    );
    footer.set_sizer(actions, true);
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    outer.add(&panel, 1, SizerFlag::Expand, 0);
    outer.add(&footer, 0, SizerFlag::Expand, 0);
    dialog.set_sizer(outer, true);
    let busy = Rc::new(Cell::new(true));
    let checked = Rc::new(Cell::new(None::<Kind>));
    let records = Rc::new(RefCell::new(Vec::<Record>::new()));
    let (sender, receiver) = mpsc::channel();
    let timer = Timer::new(&dialog);
    for b in [check, apply, restore, folder, close] {
        b.enable(false);
    }
    selector.enable(false);
    {
        let busy = busy.clone();
        let checked = checked.clone();
        let records = records.clone();
        timer.on_tick(move |_|{while let Ok(update)=receiver.try_recv(){match update{
        Update::Progress(message)=>status.set_label(&message),
        Update::Finished(result,list)=>{busy.set(false);selector.enable(true);check.enable(true);close.enable(true);let kind=Kind::ALL[selector.get_selection().unwrap_or(0)as usize];mcp_proxy.enable(kind==Kind::Mcp);let preferred=result.as_ref().ok().and_then(|r|r.backup.clone());
            record.clear();for item in &list{record.append(&item.label);}let selected=preferred.and_then(|p|list.iter().position(|r|r.path==p)).unwrap_or(0);if !list.is_empty(){record.set_selection(selected as u32);}restore.enable(list.get(selected).is_some_and(|r|r.can_restore));*records.borrow_mut()=list;record.enable(true);folder.enable(!records.borrow().is_empty());
            match result{Ok(result)=>{checked.set(Some(kind));apply.enable(result.needs_repair);status.set_label(if result.applied{tr(en,"修复已执行，请按下方说明验证","Repair applied — follow the validation notes below")}else{tr(en,"检查处理已结束","Inspection completed")});let mut message=result.message;if let Some(path)=result.backup{message.push_str(&format!("\n\n{}{}",tr(en,"备份目录：","Backup: "),path.display()));}detail.change_value(&message);},Err(error)=>{checked.set(None);apply.enable(false);status.set_label(tr(en,"已停止，请查看原因；未识别的情况不会强行修复","Stopped — see the reason below; unknown layouts are not patched"));detail.change_value(&error);}}
        }
    }}});
    }
    let begin = {
        let busy = busy.clone();
        move || {
            busy.set(true);
            for b in [check, apply, restore, folder, close] {
                b.enable(false);
            }
            selector.enable(false);
            record.enable(false);
            mcp_proxy.enable(false);
            status.set_label(tr(
                en,
                "正在处理所选问题，Hub 保持运行…",
                "Working on the selected issue; Hub stays open…",
            ));
        }
    };
    {
        let busy = busy.clone();
        let records = records.clone();
        record.on_selection_changed(move |_| {
            if !busy.get() {
                restore.enable(
                    record
                        .get_selection()
                        .and_then(|i| records.borrow().get(i as usize).map(|r| r.can_restore))
                        .unwrap_or(false),
                );
            }
        });
    }
    {
        let busy = busy.clone();
        let checked = checked.clone();
        let sender = sender.clone();
        let begin = begin.clone();
        check.on_click(move |_| {
            if busy.get() {
                return;
            }
            let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
            checked.set(None);
            begin();
            start(
                sender.clone(),
                Task::Check(kind, evidence.get_value(), mcp_proxy.is_checked()),
            );
        });
    }
    {
        let busy = busy.clone();
        let checked = checked.clone();
        selector.on_selection_changed(move |_| {
            if busy.get() {
                return;
            }
            let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
            checked.set(None);
            apply.enable(false);
            mcp_proxy.enable(kind == Kind::Mcp);
            help.change_value(kind.help());
            detail.change_value(tr(
                en,
                "点击“检查所选问题”查看本机结果。",
                "Click Check selected issue to inspect this computer.",
            ));
            status.set_label(tr(en, "尚未检查本项", "This issue has not been inspected"));
        });
    }
    {
        let busy = busy.clone();
        let checked = checked.clone();
        let sender = sender.clone();
        let begin = begin.clone();
        apply.on_click(move |_|{if busy.get(){return;}let kind=Kind::ALL[selector.get_selection().unwrap_or(0)as usize];if checked.get()!=Some(kind){return;}if matches!(kind,Kind::Desktop|Kind::Discovery|Kind::Surface|Kind::Proxy)&&!super::external_import::confirm(&dialog,tr(en,"本项可能更新客户端文件，并准备备份和恢复包。请先退出 ChatGPT/Codex；Hub 保持打开。Windows 可能出现系统权限请求，macOS 会验证本地签名。继续修复？","This repair may update client files and prepare a recovery backup. Quit ChatGPT/Codex; keep Hub open. Windows may request system permission; macOS verifies a local signature. Continue?")){return;}begin();start(sender.clone(),Task::Apply(kind,evidence.get_value(),mcp_proxy.is_checked()));});
    }
    {
        let busy = busy.clone();
        let records = records.clone();
        let sender = sender.clone();
        let begin = begin.clone();
        restore.on_click(move |_|{if busy.get(){return;}let path=record.get_selection().and_then(|i|records.borrow().get(i as usize).map(|r|r.path.clone()));let Some(path)=path else{return};if !super::external_import::confirm(&dialog,tr(en,"将恢复所选修复前的内容，原问题也可能恢复。请先退出 ChatGPT/Codex；后续被修改的文件不会覆盖。继续？","Restore the selected backup? The original issue may return. Quit ChatGPT/Codex first. Later file changes will not be overwritten.")){return;}begin();start(sender.clone(),Task::Restore(path));});
    }
    {
        let records = records.clone();
        folder.on_click(move |_| {
            if let Some(path) = record
                .get_selection()
                .and_then(|i| records.borrow().get(i as usize).map(|r| r.path.clone()))
            {
                if open_folder(&path).is_err() {
                    super::show_error(
                        &dialog,
                        tr(
                            en,
                            "无法打开备份目录，可从结果中复制路径。",
                            "Cannot open the backup folder; copy the path from the result.",
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
    start(sender, Task::Check(Kind::Startup, String::new(), false));
    dialog.show_modal();
    timer.stop();
    dialog.destroy();
}
