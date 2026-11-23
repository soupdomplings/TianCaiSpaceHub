use super::{Window, class, menu_label, pid, top_windows};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{Foundation::HWND, System::Com::*, UI::Accessibility::*},
    core::Interface,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOTOWNER, GWL_STYLE, GetAncestor, GetWindowLongW, IsWindowVisible, WS_DISABLED,
};

#[derive(Copy, Clone, PartialEq)]
pub(super) enum Step {
    Tools,
    Script,
    Run,
    Cancel,
}

impl Step {
    pub(super) fn names(self) -> &'static [&'static str] {
        match self {
            Self::Tools => &["tools", "工具"],
            Self::Script => &["script", "scripts", "脚本", "腳本"],
            Self::Run => &[
                "run",
                "run script",
                "运行",
                "运行脚本",
                "執行",
                "執行腳本",
                "执行",
                "执行脚本",
            ],
            Self::Cancel => &["cancel", "cancel script", "取消", "取消脚本", "取消腳本"],
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Tools => "工具",
            Self::Script => "脚本",
            Self::Run => "运行脚本",
            Self::Cancel => "取消脚本",
        }
    }
}

fn candidate(
    step: Step,
    owner: u32,
    process: i32,
    kind: UIA_CONTROLTYPE_ID,
    name: &str,
    menu_context: bool,
    visible: bool,
) -> bool {
    process as u32 == owner
        && visible
        && (menu_context || kind == UIA_MenuItemControlTypeId)
        && step.names().contains(&menu_label(name).as_str())
        && [
            UIA_MenuItemControlTypeId,
            UIA_ButtonControlTypeId,
            UIA_SplitButtonControlTypeId,
        ]
        .contains(&kind)
}

fn children(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
) -> Vec<IUIAutomationElement> {
    let mut result = Vec::new();
    let mut child = unsafe { walker.GetFirstChildElement(element) };
    while let Ok(current) = child {
        if result.len() >= 96 {
            break;
        }
        child = unsafe { walker.GetNextSiblingElement(&current) };
        result.push(current);
    }
    result
}

// Only walk the frame's chrome, not documents, terminal text, tabs or other menu items.
fn find(
    walker: &IUIAutomationTreeWalker,
    root: &IUIAutomationElement,
    process: u32,
    step: Step,
    in_branch: bool,
    deadline: Instant,
) -> Result<Option<IUIAutomationElement>> {
    let mut queue = std::collections::VecDeque::from([(root.clone(), in_branch, 0)]);
    let mut found = None;
    let mut visited = 0;
    while let Some((item, menu_context, depth)) = queue.pop_front() {
        if Instant::now() >= deadline {
            bail!("Xshell 的菜单无响应，请重试");
        }
        visited += 1;
        ensure!(
            visited <= 512,
            "Xshell 菜单结构过大，无法确定「{}」",
            step.label()
        );
        let Ok(owner) = (unsafe { item.CurrentProcessId() }) else {
            continue;
        };
        if owner as u32 != process {
            continue;
        }
        let Ok(kind) = (unsafe { item.CurrentControlType() }) else {
            continue;
        };
        let actionable = [
            UIA_MenuItemControlTypeId,
            UIA_ButtonControlTypeId,
            UIA_SplitButtonControlTypeId,
        ]
        .contains(&kind);
        if depth > 0 && actionable {
            let label = unsafe { item.CurrentName() }
                .unwrap_or_default()
                .to_string();
            let visible = unsafe { item.CurrentIsOffscreen() }.is_ok_and(|v| !v.as_bool());
            if candidate(step, process, owner, kind, &label, menu_context, visible) {
                ensure!(
                    found.is_none(),
                    "Xshell 有多个「{}」菜单项，请手动选择",
                    step.label()
                );
                found = Some(item);
            }
            continue;
        }
        let bar = [
            UIA_MenuBarControlTypeId,
            UIA_MenuControlTypeId,
            UIA_ToolBarControlTypeId,
        ]
        .contains(&kind);
        if depth < 7
            && (depth == 0
                || bar
                || [
                    UIA_PaneControlTypeId,
                    UIA_GroupControlTypeId,
                    UIA_CustomControlTypeId,
                ]
                .contains(&kind))
        {
            for child in children(walker, &item) {
                queue.push_back((child, menu_context || bar, depth + 1));
            }
        }
    }
    Ok(found)
}

fn popups(target: &Window) -> HashSet<usize> {
    top_windows()
        .into_iter()
        .filter(|w| unsafe {
            let w = *w;
            w as usize != target.handle
                && IsWindowVisible(w) != 0
                && pid(w) == target.pid
                && GetAncestor(w, GA_ROOTOWNER) as usize == target.handle
                && class(w) != "#32770"
        })
        .map(|w| w as usize)
        .collect()
}

fn require_enabled(step: Step, enabled: bool) -> Result<()> {
    ensure!(enabled, "Xshell 的「{}」当前不可用", step.label());
    Ok(())
}

fn trigger(
    item: &IUIAutomationElement,
    step: Step,
    before: impl FnOnce() -> Result<()>,
) -> Result<()> {
    require_enabled(step, unsafe { item.CurrentIsEnabled() }?.as_bool())?;
    unsafe {
        if matches!(step, Step::Tools | Step::Script)
            && let Ok(pattern) = item.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                UIA_ExpandCollapsePatternId,
            )
        {
            if pattern.CurrentExpandCollapseState()? != ExpandCollapseState_Expanded {
                before()?;
                pattern.Expand()?;
            }
            return Ok(());
        }
        if let Ok(pattern) =
            item.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
        {
            before()?;
            pattern.Invoke()?;
        } else {
            let pattern = item
                .GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                    UIA_LegacyIAccessiblePatternId,
                )
                .with_context(|| format!("Xshell 没有提供可操作的「{}」菜单项", step.label()))?;
            before()?;
            pattern.DoDefaultAction()?;
        }
    }
    Ok(())
}

pub(super) enum MenuResult {
    // No interaction has taken place, so the resource command may be used once.
    NoToolsMenu,
    Invoked,
}

fn check_frame(target: &Window, deadline: Instant) -> Result<()> {
    let hwnd = target.handle as _;
    ensure!(
        Instant::now() < deadline,
        "等待 Xshell 菜单响应超时，请重试。"
    );
    target.ensure_current()?;
    ensure!(
        unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_DISABLED == 0 },
        "Xshell 正在等待对话框，请先处理该窗口中的提示。"
    );
    Ok(())
}

// Run and Cancel are siblings in Tools > Script. Do not search the frame
// for a generic "Cancel" (a dialog, for example, could expose one).
fn cancel_sibling(
    walker: &IUIAutomationTreeWalker,
    run: &IUIAutomationElement,
    target_pid: u32,
) -> Result<Option<IUIAutomationElement>> {
    let parent = unsafe { walker.GetParentElement(run) }?;
    let mut found = None;
    for sibling in children(walker, &parent) {
        let kind = unsafe { sibling.CurrentControlType() }?;
        if ![
            UIA_MenuItemControlTypeId,
            UIA_ButtonControlTypeId,
            UIA_SplitButtonControlTypeId,
        ]
        .contains(&kind)
        {
            continue;
        }
        if candidate(
            Step::Cancel,
            target_pid,
            unsafe { sibling.CurrentProcessId() }?,
            kind,
            &unsafe { sibling.CurrentName() }?.to_string(),
            true,
            !unsafe { sibling.CurrentIsOffscreen() }?.as_bool(),
        ) {
            ensure!(
                found.is_none(),
                "脚本菜单有多个「取消」，无法确定要停止的脚本。"
            );
            found = Some(sibling);
        }
    }
    Ok(found)
}

#[derive(PartialEq, Debug)]
enum ScriptAction {
    Run,
    Cancel,
    Wait,
}

fn next_action(
    run_enabled: bool,
    cancel_enabled: bool,
    already_cancelled: bool,
) -> Result<ScriptAction> {
    if run_enabled {
        return Ok(ScriptAction::Run);
    }
    if already_cancelled {
        return Ok(ScriptAction::Wait);
    }
    ensure!(
        cancel_enabled,
        "该标签页的「运行脚本」和「取消脚本」均不可用，无法启动接入。"
    );
    Ok(ScriptAction::Cancel)
}

pub(super) fn open(
    target: &Window,
    issued: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<MenuResult> {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .context("无法初始化 Windows 菜单识别")?;
    let _com = Apartment;
    let ui: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .context("无法使用 Windows 菜单识别")?;
    if let Ok(ui2) = ui.cast::<IUIAutomation2>() {
        unsafe {
            ui2.SetConnectionTimeout(800)?;
            ui2.SetTransactionTimeout(1100)?;
        }
    }
    let walker = unsafe { ui.ControlViewWalker() }?;
    let Some(mut run) = run_menu(&ui, &walker, target, deadline)? else {
        return Ok(MenuResult::NoToolsMenu);
    };
    let mut cancelled = false;
    let mut reopened = false;
    loop {
        check_frame(target, deadline)?;
        if !unsafe { run.CurrentIsOffscreen() }.is_ok_and(|v| !v.as_bool()) {
            ensure!(
                cancelled && !reopened,
                "所选 Xshell 标签页的脚本菜单已关闭，请重试。"
            );
            run = run_menu(&ui, &walker, target, deadline)?
                .context("已取消旧脚本，但没有找到可重新打开的脚本菜单。")?;
            reopened = true;
        }
        let available = unsafe { run.CurrentIsEnabled() }?.as_bool();
        let cancel = if available || cancelled {
            None
        } else {
            cancel_sibling(&walker, &run, target.pid)?
        };
        let can_cancel = cancel
            .as_ref()
            .is_some_and(|v| unsafe { v.CurrentIsEnabled() }.is_ok_and(|v| v.as_bool()));
        let action = next_action(available, can_cancel, cancelled)?;
        check_frame(target, deadline)?;
        match action {
            ScriptAction::Run => {
                trigger(&run, Step::Run, || {
                    check_frame(target, deadline)?;
                    issued.store(true, Ordering::Release);
                    Ok(())
                })?;
                return Ok(MenuResult::Invoked);
            }
            ScriptAction::Cancel => {
                let cancel = cancel.context("无法确定同一脚本菜单的「取消」项。")?;
                if unsafe { run.CurrentIsEnabled() }?.as_bool() {
                    continue;
                }
                trigger(&cancel, Step::Cancel, || check_frame(target, deadline))?;
                cancelled = true;
            }
            ScriptAction::Wait => {}
        }
        ensure!(
            Instant::now() < deadline,
            "已请求取消脚本，但「运行」尚未恢复可用。"
        );
        std::thread::sleep(Duration::from_millis(80));
    }
}

fn run_menu(
    ui: &IUIAutomation,
    walker: &IUIAutomationTreeWalker,
    target: &Window,
    deadline: Instant,
) -> Result<Option<IUIAutomationElement>> {
    check_frame(target, deadline)?;
    let frame = unsafe { ui.ElementFromHandle(HWND(target.handle as _)) }?;
    let mut parent = frame;
    let mut existing = HashSet::new();
    for step in [Step::Tools, Step::Script, Step::Run] {
        let until = (Instant::now() + Duration::from_millis(1400)).min(deadline);
        let item = loop {
            if let Some(item) = find(
                walker,
                &parent,
                target.pid,
                step,
                step != Step::Tools,
                deadline,
            )? {
                break item;
            }
            if step != Step::Tools {
                let mut matched = None;
                for hwnd in popups(target).difference(&existing) {
                    if let Ok(root) = unsafe { ui.ElementFromHandle(HWND(*hwnd as _)) }
                        && let Some(item) = find(walker, &root, target.pid, step, true, deadline)?
                    {
                        ensure!(matched.is_none(), "Xshell 有多个活动菜单，请关闭后重试");
                        matched = Some(item);
                    }
                }
                if let Some(item) = matched {
                    break item;
                }
            }
            if step == Step::Tools {
                return Ok(None);
            }
            if Instant::now() >= until {
                bail!("未在所选 Xshell 窗口找到「{}」菜单", step.label());
            }
            std::thread::sleep(Duration::from_millis(55));
        };
        check_frame(target, deadline)?;
        if step == Step::Run {
            return Ok(Some(item));
        }
        existing = popups(target);
        trigger(&item, step, || check_frame(target, deadline))?;
        parent = item;
    }
    unreachable!("the last step returns the Run item")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_waits_for_run_without_requesting_cancel_twice() {
        let mut cancelled = false;
        let mut actions = vec![];
        for (run, cancel) in [(false, true), (false, true), (false, false), (true, false)] {
            let action = next_action(run, cancel, cancelled).unwrap();
            cancelled |= action == ScriptAction::Cancel;
            actions.push(action);
        }
        assert_eq!(
            actions,
            [
                ScriptAction::Cancel,
                ScriptAction::Wait,
                ScriptAction::Wait,
                ScriptAction::Run
            ]
        );
        assert_eq!(next_action(true, true, false).unwrap(), ScriptAction::Run);
        assert!(next_action(false, false, false).is_err());
        for name in ["Cancel Session", "取消连接", "Stop Script Recording"] {
            assert!(!candidate(
                Step::Cancel,
                21,
                21,
                UIA_MenuItemControlTypeId,
                name,
                true,
                true
            ));
        }
    }
    #[test]
    fn disabled_run_menu_is_reported_before_invocation() {
        assert!(
            require_enabled(Step::Run, false)
                .unwrap_err()
                .to_string()
                .contains("不可用")
        );
        assert!(require_enabled(Step::Script, true).is_ok());
    }

    #[test]
    fn selects_named_menu_or_toolbar_only_on_target_and_not_a_recording_action() {
        assert!(candidate(
            Step::Tools,
            21,
            21,
            UIA_ButtonControlTypeId,
            "工具（&T）",
            true,
            true
        ));
        assert!(candidate(
            Step::Run,
            21,
            21,
            UIA_MenuItemControlTypeId,
            "Run... (&R)",
            true,
            true
        ));
        assert!(!candidate(
            Step::Run,
            21,
            23,
            UIA_MenuItemControlTypeId,
            "Run",
            true,
            true
        ));
        assert!(!candidate(
            Step::Run,
            21,
            21,
            UIA_ButtonControlTypeId,
            "Run",
            false,
            true
        ));
        assert!(!candidate(
            Step::Run,
            21,
            21,
            UIA_MenuItemControlTypeId,
            "Run Script Recording",
            true,
            true
        ));
        assert!(!candidate(
            Step::Run,
            21,
            21,
            UIA_MenuItemControlTypeId,
            "Run",
            true,
            false
        ));
    }
}
