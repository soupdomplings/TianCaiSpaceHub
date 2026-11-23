//! One native repair window. Inspection never starts a write operation.
use super::{
    text::{GuiLocale, GuiText},
    theme,
};
use crate::codex_app_config::ProjectlessChoice;
use crate::codex_repair_center::{self as repair, Inspection, Kind, Record, RepairOptions};
use crate::gui::controls::{Button, ButtonKind};
use crate::gui::controls::{Choice, TextCtrl};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
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
    Inspect {
        kind: Kind,
        apply: bool,
        evidence: String,
        options: RepairOptions,
        selected: BTreeMap<String, String>,
    },
    Restore(repair::RestorePreview),
}
enum Update {
    Progress(String),
    Finished(Result<Inspection, String>, Vec<Record>),
}
struct RepairPreview {
    kind: Kind,
    options: RepairOptions,
    automatic: bool,
    project_counts: Option<(usize, usize)>,
    choices: Vec<ProjectlessChoice>,
}
impl RepairPreview {
    fn selected_action(
        &self,
        kind: Kind,
        options: RepairOptions,
        checked: impl Fn(usize) -> bool,
    ) -> Option<BTreeMap<String, String>> {
        if (kind, options) != (self.kind, self.options) {
            return None;
        }
        let selected: BTreeMap<_, _> = self
            .choices
            .iter()
            .enumerate()
            .filter(|(index, _)| checked(*index))
            .map(|(_, row)| (row.thread_id.clone(), row.project_id.clone()))
            .collect();
        (self.automatic || !selected.is_empty()).then_some(selected)
    }
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
                Task::Inspect {
                    kind,
                    apply,
                    evidence,
                    options,
                    selected,
                } => repair::run(kind, apply, &evidence, options, &selected, progress),
                Task::Restore(preview) => {
                    repair::restore(&preview, progress).map(|r| (r, Vec::new(), None))
                }
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
// Undo lives in a separate modal; closing it without an explicitly confirmed
// preview can never produce a Restore task.
fn choose_backup(
    parent: &Dialog,
    en: bool,
    records: Vec<Record>,
) -> Option<repair::RestorePreview> {
    let dialog = Dialog::builder(parent, tr(en, "备份与撤销", "Backups and undo"))
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .with_size(
            parent.from_dip(500).min(parent.get_size().width),
            parent.from_dip(245),
        )
        .build();
    dialog.set_background_color(theme::theme().bg_card);
    super::motion::install_dialog(dialog);
    let outer = BoxSizer::builder(Orientation::Vertical).build();
    let label = StaticText::builder(&dialog)
        .with_label(tr(en, "选择要查看的修复记录", "Choose a repair record"))
        .build();
    outer.add(&label, 0, SizerFlag::Expand | SizerFlag::All, 16);
    let list = Choice::builder(&dialog).build();
    list.append(tr(en, "请选择一条记录", "Select a record"));
    for record in &records {
        list.append(&record.label);
    }
    list.set_selection(0);
    outer.add(
        &list,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let note = StaticText::builder(&dialog)
        .with_label(tr(
            en,
            "这里会将之前的修复退回。要找回项目，请返回修复页。",
            "Undo reverses a previous repair. To recover projects, return to the repair page.",
        ))
        .build();
    note.wrap((parent.get_size().width.min(parent.from_dip(500)) - parent.from_dip(45)).max(200));
    outer.add(&note, 1, SizerFlag::Expand | SizerFlag::All, 16);
    let row = BoxSizer::builder(Orientation::Horizontal).build();
    let folder = Button::builder(&dialog)
        .with_label(tr(en, "打开备份目录", "Open folder"))
        .build();
    let view = Button::builder(&dialog)
        .with_label(tr(en, "查看撤销影响…", "Review changes…"))
        .build();
    let back = Button::builder(&dialog)
        .with_id(ID_CANCEL)
        .with_label(tr(en, "返回修复页", "Back"))
        .build();
    folder.enable(false);
    view.enable(false);
    back.set_default();
    back.set_focus();
    row.add(&folder, 0, SizerFlag::Right, 8);
    row.add_stretch_spacer(1);
    row.add(&view, 0, SizerFlag::Right, 8);
    row.add(&back, 0, SizerFlag::default(), 0);
    outer.add_sizer(&row, 0, SizerFlag::Expand | SizerFlag::All, 12);
    dialog.set_sizer(outer, true);
    let records = Rc::new(records);
    let current = move || {
        list.get_selection()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| records.get(i as usize).cloned())
    };
    let get = current.clone();
    list.on_selection_changed(move |_| {
        let selected = get();
        view.enable(selected.as_ref().is_some_and(|r| r.can_restore));
        folder.enable(selected.is_some());
    });
    let get = current.clone();
    folder.on_click(move |_| {
        if let Some(r) = get()
            && open_folder(&r.path).is_err()
        {
            super::show_error(
                &dialog,
                tr(
                    en,
                    "无法打开备份目录。",
                    "Could not open the backup folder.",
                ),
            );
        }
    });
    let confirmed = Rc::new(RefCell::new(None));
    let result = confirmed.clone();
    view.on_click(move |_| {
        let Some(record) = current().filter(|r| r.can_restore) else {
            return;
        };
        let preview = match repair::preview_restore(&record) {
            Ok(p) => p,
            Err(e) => {
                super::show_error(&dialog, &e.to_string());
                return;
            }
        };
        let verb = if preview.is_project_undo {
            tr(en, "撤销这次项目修复", "Undo this project repair")
        } else {
            tr(en, "还原这份备份", "Restore this backup")
        };
        if super::feedback::confirm(&dialog, verb, &preview.message, verb) {
            result.borrow_mut().replace(preview);
            dialog.end_modal(ID_OK);
        }
    });
    back.on_click(move |_| dialog.end_modal(ID_CANCEL));
    dialog.centre();
    let action = if dialog.show_modal() == ID_OK {
        confirmed.borrow_mut().take()
    } else {
        None
    };
    dialog.destroy();
    action
}
fn scope_label(en: bool, cards: usize, automatic_chats: usize, checked: usize) -> String {
    let total = automatic_chats + checked;
    if en {
        format!("Projects to add: {cards}   ·   Chats to group: {total} ({checked} selected)")
    } else {
        format!("补回 {cards} 个项目   ·   归类 {total} 个会话（其中勾选 {checked} 个）")
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
    super::motion::install_dialog(dialog);
    dialog.set_min_size(Size::new(500.min(size.width), 350.min(size.height)));
    dialog.set_escape_id(ID_NONE);
    dialog.set_background_color(theme::theme().bg_card);
    let panel = ScrolledWindow::builder(&dialog)
        .with_style(ScrolledWindowStyle::VScroll)
        .build();
    panel.set_background_color(theme::theme().bg_card);
    panel.set_min_size(Size::new(0, 0));
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let intro = StaticText::builder(&panel)
        .with_label(tr(
            en,
            "选择要检查的问题。应用修复前，请完全退出 ChatGPT / Codex。",
            "Choose an issue. Quit ChatGPT / Codex before applying a repair.",
        ))
        .build();
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
    let help = StaticText::builder(&panel)
        .with_label(Kind::Startup.help())
        .build();
    help.wrap((size.width - 75).max(300));
    sizer.add(&help, 0, SizerFlag::Expand | SizerFlag::All, 12);
    let evidence_label = StaticText::builder(&panel)
        .with_label(tr(
            en,
            "截图失败时显示的错误提示",
            "Error shown when taking a screenshot",
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
    evidence_label.show(false);
    evidence.show(false);
    mcp_proxy.show(false);
    let legacy = CheckBox::builder(&panel)
        .with_label(tr(
            en,
            "包含我确认属于自己的旧会话（未记录账号）",
            "Include older chats without a recorded account that I recognize as mine",
        ))
        .build();
    sizer.add(&legacy, 0, SizerFlag::All, 16);
    legacy.show(false);
    let selected_options = move |kind: Kind| RepairOptions {
        proxy_for_mcp: kind == Kind::Mcp && mcp_proxy.is_checked(),
        include_legacy: kind == Kind::ProjectHistory && legacy.is_checked(),
    };
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
        .with_size(Size::new(-1, panel.from_dip(112)))
        .build();

    let pick_label = StaticText::builder(&panel)
        .with_label(tr(
            en,
            "勾选要移回项目的“无项目”会话（可多选）",
            "Select ungrouped conversations to move back to their projects",
        ))
        .build();
    sizer.add(
        &pick_label,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let select_row = Panel::builder(&panel).build();
    let select_layout = BoxSizer::builder(Orientation::Horizontal).build();
    let all = Button::builder(&select_row)
        .with_label(tr(en, "全选", "Select all"))
        .build();
    let none = Button::builder(&select_row)
        .with_label(tr(en, "清空勾选", "Clear selection"))
        .build();
    select_layout.add(&all, 0, SizerFlag::Right, 8);
    select_layout.add(&none, 0, SizerFlag::default(), 0);
    select_row.set_sizer(select_layout, true);
    select_row.show(false);
    sizer.add(
        &select_row,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        16,
    );
    let picks = CheckListBox::builder(&panel)
        .with_style(CheckListBoxStyle::HScroll)
        .with_size(Size::new(-1, panel.from_dip(165)))
        .build();
    picks.set_name(tr(
        en,
        "选择要归入项目的会话",
        "Conversations to move into projects",
    ));
    picks.set_background_color(theme::theme().bg_card);
    picks.set_foreground_color(theme::theme().ink_primary);
    sizer.add(&picks, 1, SizerFlag::Expand | SizerFlag::All, 12);
    pick_label.show(false);
    picks.show(false);
    sizer.add(&detail, 0, SizerFlag::Expand | SizerFlag::All, 12);
    detail.show(false);
    panel.set_sizer(sizer, true);
    panel.set_scroll_rate(0, 10);
    panel.fit_inside();
    let footer = Panel::builder(&dialog).build();
    footer.set_background_color(theme::theme().bg_card);
    let actions = BoxSizer::builder(Orientation::Vertical).build();
    let scope = StaticText::builder(&footer).build();
    actions.add(
        &scope,
        0,
        SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right,
        12,
    );
    scope.show(false);
    let primary = BoxSizer::builder(Orientation::Horizontal).build();
    let check = Button::builder(&footer)
        .with_label(tr(en, "检查所选问题", "Check selected issue"))
        .build();
    let apply = Button::builder(&footer)
        .with_label(tr(en, "应用修复", "Apply repair"))
        .with_kind(ButtonKind::Primary)
        .build();
    let close = Button::builder(&footer)
        .with_label(tr(en, "关闭", "Close"))
        .build();
    for b in [&check, &apply, &close] {
        primary.add(b, 0, SizerFlag::Right, 8);
    }
    actions.add_sizer(&primary, 0, SizerFlag::Expand | SizerFlag::All, 10);
    let secondary = BoxSizer::builder(Orientation::Horizontal).build();
    let history = Button::builder(&footer)
        .with_kind(ButtonKind::Ghost)
        .with_label(tr(en, "备份与撤销…", "Backups and undo…"))
        .build();
    secondary.add(&history, 0, SizerFlag::Right, 8);
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
    let checked = Rc::new(RefCell::new(None::<RepairPreview>));
    let clear_preview = {
        let checked = checked.clone();
        move || {
            checked.borrow_mut().take();
            scope.show(false);
            select_row.show(false);
            picks.clear();
            picks.show(false);
            pick_label.show(false);
            apply.enable(false);
            panel.layout();
            panel.fit_inside();
        }
    };
    let records = Rc::new(RefCell::new(Vec::<Record>::new()));
    let update_action = {
        let checked = checked.clone();
        let records = records.clone();
        move || {
            let count = (0..picks.get_count())
                .filter(|i| picks.is_checked(*i))
                .count();
            let borrowed = checked.borrow();
            let p = borrowed.as_ref();
            let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
            let valid = p.filter(|p| p.kind == kind && p.options == selected_options(kind));
            apply.enable(valid.is_some_and(|p| p.automatic || count > 0));
            if let Some((cards, chats)) = valid.and_then(|p| p.project_counts) {
                scope.set_label(&scope_label(en, cards, chats, count));
                scope.show(true);
            } else {
                scope.show(false);
            }
            history.enable(count == 0 && !records.borrow().is_empty());
            all.enable(count < (picks.get_count() as usize));
            none.enable(count > 0);
            footer.layout();
            dialog.layout();
        }
    };
    let (sender, receiver) = mpsc::channel();
    let timer = Timer::new(&dialog);
    for b in [check, apply, history, close] {
        b.enable(false);
    }
    selector.enable(false);
    {
        let busy = busy.clone();
        let checked = checked.clone();
        let records = records.clone();
        let update_action = update_action.clone();
        timer.on_tick(move |_| {
            while let Ok(update) = receiver.try_recv() {
                match update {
                    Update::Progress(message) => status.set_label(&message),
                    Update::Finished(result, list) => {
                        busy.set(false);
                        selector.enable(true);
                        check.enable(true);
                        close.enable(true);
                        let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
                        mcp_proxy.enable(kind == Kind::Mcp);
                        legacy.enable(kind == Kind::ProjectHistory);
                        *records.borrow_mut() = list;
                        history.enable(!records.borrow().is_empty());
                        detail.show(true);
                        match result {
                            Ok((result, choices, project_counts)) => {
                                if !choices.is_empty() {
                                    picks.freeze();
                                    for row in &choices {
                                        let title = if row.title.trim().is_empty() { &row.thread_id } else { &row.title };
                                        let label = if cfg!(windows) {
                                            format!("{title}\n{}{}", tr(en, "归入项目：", "Project: "), row.project_name)
                                        } else {
                                            format!("{title}   →   {}", row.project_name)
                                        };
                                        picks.append(&label);
                                    }
                                    picks.thaw();
                                    select_row.show(true);
                                    picks.show(true);
                                    pick_label.show(true);
                                    panel.layout();
                                    panel.fit_inside();
                                }
                                *checked.borrow_mut() = (result.needs_repair || !choices.is_empty() || project_counts.is_some()).then(|| RepairPreview {
                                    kind, options: selected_options(kind), automatic: result.needs_repair, project_counts, choices
                                });
                                update_action();
                                status.set_label(if result.applied {
                                    tr(en, "修复已执行，请按下方说明验证", "Repair applied — follow the validation notes below")
                                } else if project_counts == Some((0, 0)) && picks.get_count() == 0 {
                                    tr(en, "当前没有可应用的项目修复，请查看检查结果", "No project repair to apply. See the inspection details.")
                                } else { tr(en, "检查处理已结束", "Inspection completed") });
                                detail.change_value(&result.message);
                            }
                            Err(error) => {
                                checked.borrow_mut().take();
                                apply.enable(false);
                                status.set_label(tr(en, "已停止，请查看原因；未识别的情况不会强行修复", "Stopped — see the reason below; unknown layouts are not patched"));
                                detail.change_value(&error);
                            }
                        }
                        panel.layout(); panel.fit_inside(); dialog.layout();
                    }
                }
            }
        });
    }
    let begin = {
        let busy = busy.clone();
        let clear_preview = clear_preview.clone();
        move || {
            busy.set(true);
            clear_preview();
            for b in [check, apply, history, close] {
                b.enable(false);
            }
            selector.enable(false);
            detail.show(false);
            mcp_proxy.enable(false);
            legacy.enable(false);
            status.set_label(tr(
                en,
                "正在处理所选问题，Hub 保持运行…",
                "Working on the selected issue; Hub stays open…",
            ));
        }
    };
    {
        let update = update_action.clone();
        picks.on_toggled(move |_| update());
        for (button, value) in [(all, true), (none, false)] {
            let update = update_action.clone();
            button.on_click(move |_| {
                picks.freeze();
                for i in 0..picks.get_count() {
                    picks.check(i, value);
                }
                picks.thaw();
                update();
            });
        }
    }
    {
        let busy = busy.clone();
        let sender = sender.clone();
        let begin = begin.clone();
        check.on_click(move |_| {
            if busy.get() {
                return;
            }
            let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
            begin();
            start(
                sender.clone(),
                Task::Inspect {
                    kind,
                    apply: false,
                    evidence: evidence.get_value(),
                    options: selected_options(kind),
                    selected: BTreeMap::new(),
                },
            );
        });
    }
    {
        let busy = busy.clone();
        let clear_preview = clear_preview.clone();
        let records = records.clone();
        selector.on_selection_changed(move |_| {
            if busy.get() {
                return;
            }
            let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
            clear_preview();
            history.enable(!records.borrow().is_empty());
            mcp_proxy.enable(kind == Kind::Mcp);
            let project = kind == Kind::ProjectHistory;
            legacy.set_value(false);
            legacy.enable(project);
            legacy.show(project);
            evidence_label.show(kind == Kind::Screenshot);
            evidence.show(kind == Kind::Screenshot);
            mcp_proxy.show(kind == Kind::Mcp);
            detail.show(false);
            apply.set_label(if project {
                tr(en, "应用项目修复", "Apply project repair")
            } else {
                tr(en, "应用修复", "Apply repair")
            });
            panel.layout();
            panel.fit_inside();
            help.set_label(kind.help());
            help.wrap((panel.get_size().width - 36).max(220));
            panel.layout();
            panel.fit_inside();
            detail.change_value(tr(
                en,
                "点击“检查所选问题”查看本机结果。",
                "Click Check selected issue to inspect this computer.",
            ));
            status.set_label(tr(en, "尚未检查本项", "This issue has not been inspected"));
        });
    }
    {
        let clear_preview = clear_preview.clone();
        let records = records.clone();
        let invalidate = move || {
            clear_preview();
            history.enable(!records.borrow().is_empty());
            status.set_label(tr(en, "已更改检查选项", "Inspection options changed"));
            detail.change_value(tr(
                en,
                "请再次点击“检查所选问题”查看本次修复范围。",
                "Check the selected issue again to preview what will change.",
            ));
        };
        let other = invalidate.clone();
        legacy.on_toggled(move |_| invalidate());
        mcp_proxy.on_toggled(move |_| other());
    }
    {
        let busy = busy.clone();
        let checked = checked.clone();
        let sender = sender.clone();
        let begin = begin.clone();
        apply.on_click(move |_| {
            if busy.get() { return; }
            let kind = Kind::ALL[selector.get_selection().unwrap_or(0) as usize];
            let options = selected_options(kind);
            let Some(selected) = checked.borrow().as_ref().and_then(|p| p.selected_action(kind, options, |i| picks.is_checked(i as u32))) else { return; };
            if matches!(kind, Kind::Desktop | Kind::Discovery | Kind::Surface | Kind::Proxy) && !super::external_import::confirm(&dialog,tr(en,"本项可能更新客户端文件，并准备备份和恢复包。请先退出 ChatGPT/Codex；Hub 保持打开。Windows 可能出现系统权限请求，macOS 会验证本地签名。继续修复？","This repair may update client files and prepare a recovery backup. Quit ChatGPT/Codex; keep Hub open. Windows may request system permission; macOS verifies a local signature. Continue?")) { return; }
            begin();
            start(sender.clone(), Task::Inspect { kind, apply: true, evidence: evidence.get_value(), options, selected });
        });
    }
    {
        let busy = busy.clone();
        let records = records.clone();
        let sender = sender.clone();
        let begin = begin.clone();
        history.on_click(move |_| {
            if busy.get() || (0..picks.get_count()).any(|i| picks.is_checked(i)) {
                return;
            }
            let Some(preview) = choose_backup(&dialog, en, records.borrow().clone()) else {
                return;
            };
            begin();
            start(sender.clone(), Task::Restore(preview));
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
    start(
        sender,
        Task::Inspect {
            kind: Kind::Startup,
            apply: false,
            evidence: String::new(),
            options: RepairOptions::default(),
            selected: BTreeMap::new(),
        },
    );
    dialog.show_modal();
    timer.stop();
    dialog.destroy();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_summary_counts_both_previous_and_selected_work() {
        assert_eq!(
            scope_label(false, 4, 19, 1),
            "补回 4 个项目   ·   归类 20 个会话（其中勾选 1 个）"
        );
        assert_eq!(
            scope_label(false, 0, 0, 0),
            "补回 0 个项目   ·   归类 0 个会话（其中勾选 0 个）"
        );
    }

    #[test]
    fn optional_only_repair_uses_checked_ids_and_requires_the_same_preview() {
        let mut preview = RepairPreview {
            kind: Kind::ProjectHistory,
            options: RepairOptions::default(),
            automatic: false,
            project_counts: Some((4, 19)),
            choices: ["a", "b", "c"]
                .map(|id| ProjectlessChoice {
                    thread_id: id.into(),
                    project_id: format!("target-{id}"),
                    title: "相同的标题".into(),
                    project_name: "同名项目".into(),
                })
                .into(),
        };
        let options = preview.options;
        assert!(
            preview
                .selected_action(Kind::ProjectHistory, options, |_| false)
                .is_none()
        );
        assert_eq!(
            preview.selected_action(Kind::ProjectHistory, options, |i| i != 1),
            Some(BTreeMap::from([
                ("a".into(), "target-a".into()),
                ("c".into(), "target-c".into())
            ]))
        );
        assert!(
            preview
                .selected_action(
                    Kind::ProjectHistory,
                    RepairOptions {
                        include_legacy: true,
                        ..options
                    },
                    |_| true
                )
                .is_none()
        );
        assert!(
            preview
                .selected_action(Kind::Startup, options, |_| true)
                .is_none()
        );
        preview.automatic = true;
        assert_eq!(
            preview.selected_action(Kind::ProjectHistory, options, |_| false),
            Some(BTreeMap::new())
        );
    }
}
