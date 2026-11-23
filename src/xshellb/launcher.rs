//! Open the vendor's local script picker in an explicitly selected window.
//! Only this local picker accepts input; no shortcuts, clipboard, or terminal input.
use anyhow::{Result, bail, ensure};
mod accessible;
mod menu_resource;
mod picker;
#[cfg(test)]
mod restart_tests;
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{BOOL, CloseHandle, HWND, LPARAM},
    Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    },
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::*,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    handle: usize,
    pid: u32,
    pub title: String,
    tab: Vec<(usize, usize)>,
}

impl Window {
    pub(crate) fn process_id(&self) -> u32 {
        self.pid
    }

    fn ensure_current(&self) -> Result<()> {
        let frame = self.handle as HWND;
        ensure!(
            pid(frame) == self.pid
                && title(frame) == self.title
                && self.tab == tab_marker(frame, self.pid),
            "Xshell 窗口或当前标签页已变化，已停止本次接入。"
        );
        Ok(())
    }
}

unsafe extern "system" fn collect(window: HWND, out: LPARAM) -> BOOL {
    unsafe { (&mut *(out as *mut Vec<HWND>)).push(window) };
    1
}

fn top_windows() -> Vec<HWND> {
    let mut windows = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut windows as *mut Vec<HWND> as LPARAM) };
    windows
}

fn pid(window: HWND) -> u32 {
    let mut result = 0;
    unsafe { GetWindowThreadProcessId(window, &mut result) };
    result
}

fn image(pid: u32) -> Option<PathBuf> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buf = vec![0u16; 32768];
    let mut size = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut size) };
    unsafe { CloseHandle(process) };
    (ok != 0).then(|| PathBuf::from(OsString::from_wide(&buf[..size as usize])))
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn is_xshell8(pid: u32) -> bool {
    let Some(path) = image(pid) else { return false };
    if !path
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("xshell.exe"))
    {
        return false;
    }
    let path = wide(path.as_os_str());
    let mut unused = 0;
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut unused) };
    if size == 0 || size > 1024 * 1024 {
        return false;
    }
    let mut data = vec![0u32; (size as usize).div_ceil(4)];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return false;
    }
    let mut version = ptr::null_mut();
    let mut len = 0;
    if unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            [92u16, 0].as_ptr(),
            &mut version,
            &mut len,
        )
    } == 0
        || version.is_null()
        || len < size_of::<VS_FIXEDFILEINFO>() as u32
    {
        return false;
    }
    let info = unsafe { version.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    info.dwSignature == 0xFEEF04BD
        && (info.dwProductVersionMS >> 16 == 8 || info.dwFileVersionMS >> 16 == 8)
}

fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

fn class(hwnd: HWND) -> String {
    let mut buf = [0u16; 80];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

// A title alone need not distinguish two tabs. Prefer the active MDI child,
// native tab selection and visible embedded Core views when exposed by Xshell.
fn tab_marker(frame: HWND, frame_pid: u32) -> Vec<(usize, usize)> {
    use windows_sys::Win32::UI::Controls::TCM_GETCURSEL;
    let mut children = Vec::<HWND>::new();
    unsafe { EnumChildWindows(frame, Some(collect), &mut children as *mut _ as LPARAM) };
    let mut current = Vec::new();
    for child in children {
        if unsafe { IsWindowVisible(child) } == 0 {
            continue;
        }
        let msg = match class(child).as_str() {
            "MDIClient" => WM_MDIGETACTIVE,
            "SysTabControl32" => TCM_GETCURSEL,
            _ if pid(child) != frame_pid && pid(unsafe { GetParent(child) }) == frame_pid => {
                current.push((child as usize, pid(child) as usize));
                continue;
            }
            _ => continue,
        };
        let mut value = 0;
        if unsafe {
            SendMessageTimeoutW(
                child,
                msg,
                0,
                0,
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                200,
                &mut value,
            )
        } != 0
        {
            current.push((child as usize, value));
        }
    }
    current.sort_unstable();
    current
}

pub fn windows() -> Vec<Window> {
    let mut versions = std::collections::HashMap::new();
    top_windows()
        .into_iter()
        .filter_map(|hwnd| {
            if unsafe { IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() } {
                return None;
            }
            let title = title(hwnd);
            if title.is_empty() {
                return None;
            }
            let pid = pid(hwnd);
            versions
                .entry(pid)
                .or_insert_with(|| is_xshell8(pid))
                .then_some(Window {
                    handle: hwnd as usize,
                    pid,
                    title,
                    tab: Vec::new(),
                })
        })
        .collect()
}

fn menu_label(raw: &str) -> String {
    let text = raw
        .split('\t')
        .next()
        .unwrap_or_default()
        .replace('&', "")
        .replace('（', "(")
        .replace('）', ")");
    fn trim(s: &str) -> &str {
        s.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '…'))
    }
    let mut label = trim(&text).trim_start();
    if let Some((left, suffix)) = label.rsplit_once('(')
        && suffix.len() == 2
        && suffix.ends_with(')')
        && suffix.as_bytes()[0].is_ascii_alphabetic()
    {
        label = trim(left);
    }
    label.to_ascii_lowercase()
}

fn menu_item(menu: HMENU, names: &[&str]) -> Option<(u32, HMENU, u32)> {
    if menu.is_null() {
        return None;
    }
    let mut result = None;
    for index in 0..unsafe { GetMenuItemCount(menu) }.max(0) as u32 {
        let mut buf = [0u16; 128];
        let len = unsafe {
            GetMenuStringW(
                menu,
                index,
                buf.as_mut_ptr(),
                buf.len() as i32,
                MF_BYPOSITION,
            )
        };
        let label = menu_label(&String::from_utf16_lossy(&buf[..len.max(0) as usize]));
        if names.contains(&label.as_str()) {
            if result.is_some() {
                return None;
            }
            result = Some((
                unsafe { GetMenuItemID(menu, index as i32) },
                unsafe { GetSubMenu(menu, index as i32) },
                unsafe { GetMenuState(menu, index, MF_BYPOSITION) },
            ));
        }
    }
    result
}

struct DialogOwner {
    frame: HWND,
    process: u32,
    core_path: PathBuf,
    core: std::cell::Cell<Option<u32>>,
}

impl DialogOwner {
    fn new(frame: HWND, process: u32, core_path: PathBuf) -> Self {
        Self {
            frame,
            process,
            core_path,
            core: std::cell::Cell::new(None),
        }
    }

    fn dialogs(&self) -> Vec<HWND> {
        top_windows()
            .into_iter()
            .filter(|&w| {
                if unsafe { IsWindowVisible(w) == 0 || GetAncestor(w, GA_ROOTOWNER) != self.frame }
                    || class(w) != "#32770"
                {
                    return false;
                }
                let process = pid(w);
                if process == self.process || self.core.get() == Some(process) {
                    return true;
                }
                // In multiprocess Xshell 8, the file dialog is owned by the frame
                // but created by that frame process's XshellCore.exe child.
                if image(process).is_some_and(|p| {
                    p.as_os_str()
                        .eq_ignore_ascii_case(self.core_path.as_os_str())
                }) && parent_process(process) == Some(self.process)
                {
                    self.core.set(Some(process));
                    return true;
                }
                false
            })
            .collect()
    }

    fn require_ready(&self) -> Result<()> {
        let dialogs = self.dialogs();
        ensure!(
            !dialogs
                .iter()
                .any(|&dialog| picker::file_controls(dialog).is_some()),
            "Xshell 已有打开的文件选择框。请先点「取消」，再从 Hub 点「启动脚本」。"
        );
        ensure!(
            dialogs.is_empty()
                && unsafe { GetWindowLongW(self.frame, GWL_STYLE) as u32 & WS_DISABLED == 0 },
            "所选 Xshell 窗口正被另一个对话框占用，请关闭该对话框后重试。"
        );
        Ok(())
    }
}

fn parent_process(pid: u32) -> Option<u32> {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut next = Process32FirstW(snapshot, &mut entry);
        let mut parent = None;
        while next != 0 {
            if entry.th32ProcessID == pid {
                parent = Some(entry.th32ParentProcessID);
                break;
            }
            next = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        parent
    }
}

fn cancel_script(target: &Window, owner: &DialogOwner, id: u32) -> Result<()> {
    owner.require_ready()?;
    target.ensure_current()?;
    let mut result = 0;
    // Do not queue Run behind an unacknowledged Cancel; in particular, a
    // vendor modal dialog must never be mistaken for our later file picker.
    ensure!(
        unsafe {
            SendMessageTimeoutW(
                target.handle as HWND,
                WM_COMMAND,
                id as usize,
                0,
                SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                2500,
                &mut result,
            )
        } != 0,
        "Xshell 没有完成「取消脚本」，本次未启动新脚本。请检查所选标签页。"
    );
    target.ensure_current()?;
    owner.require_ready()
}

fn live_command(menu: HMENU, step: accessible::Step) -> Option<(u32, bool)> {
    menu_resource::from_menu_for(menu, step)
        .map(|(id, flags)| (id, flags & (MF_DISABLED | MF_GRAYED) == 0))
}

// A native menu can cache disabled flags until its popup initializes.
// Refresh only the existing Script submenu owned by this Xshell frame.
fn refresh_live_menu(target: &Window, menu: HMENU) -> Result<()> {
    let Some((_, tools, _)) = menu_item(menu, accessible::Step::Tools.names()) else {
        return Ok(());
    };
    let Some((_, scripts, _)) = menu_item(tools, accessible::Step::Script.names()) else {
        return Ok(());
    };
    if scripts.is_null() {
        return Ok(());
    }
    let Some(index) = (0..unsafe { GetMenuItemCount(tools) })
        .find(|&i| unsafe { GetSubMenu(tools, i) } == scripts)
    else {
        return Ok(());
    };
    let mut result = 0;
    ensure!(
        unsafe {
            SendMessageTimeoutW(
                target.handle as HWND,
                WM_INITMENUPOPUP,
                scripts as usize,
                index as LPARAM,
                SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                600,
                &mut result,
            )
        } != 0,
        "Xshell 未返回脚本菜单状态。"
    );
    Ok(())
}

fn start_from_live_menu(target: &Window, owner: &DialogOwner, menu: HMENU) -> Result<u32> {
    use accessible::Step;
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut cancelled = false;
    loop {
        target.ensure_current()?;
        ensure!(
            unsafe { GetMenu(target.handle as HWND) } == menu,
            "Xshell 脚本菜单已变化。"
        );
        refresh_live_menu(target, menu)?;
        let (run, enabled) = live_command(menu, Step::Run)
            .ok_or_else(|| anyhow::anyhow!("找不到所选标签页的「运行脚本」。"))?;
        if enabled {
            return Ok(run);
        }
        if !cancelled {
            let (cancel, _) = live_command(menu, Step::Cancel)
                .filter(|(id, enabled)| *enabled && *id != run)
                .ok_or_else(|| {
                    anyhow::anyhow!("当前标签页的「运行脚本」和「取消脚本」均不可用。")
                })?;
            cancel_script(target, owner, cancel)?;
            cancelled = true;
        }
        ensure!(
            Instant::now() < deadline,
            "已请求取消旧脚本，Xshell 尚未允许启动新脚本。"
        );
        std::thread::sleep(Duration::from_millis(75));
    }
}

fn post_run(target: &Window, owner: &DialogOwner, id: u32) -> Result<()> {
    owner.require_ready()?;
    target.ensure_current()?;
    ensure!(
        unsafe { PostMessageW(target.handle as HWND, WM_COMMAND, id as usize, 0) } != 0,
        "无法向所选 Xshell 窗口发送「运行脚本」。"
    );
    Ok(())
}

enum MenuStart {
    Posted(&'static str),
    InspectRibbon { run_if_no_tools: Option<u32> },
}

fn start_menu(
    target: &Window,
    owner: &DialogOwner,
    live_menu: HMENU,
    version_commands: impl FnOnce() -> Option<(u32, Option<u32>)>,
) -> Result<MenuStart> {
    if menu_resource::from_menu(live_menu).is_some() {
        post_run(
            target,
            owner,
            start_from_live_menu(target, owner, live_menu)?,
        )?;
        return Ok(MenuStart::Posted("窗口菜单"));
    }
    let resources = live_menu.is_null().then(version_commands).flatten();
    if let Some((run, Some(cancel))) = resources {
        // Ribbon builds can expose *several* accessibility elements named
        // Script. A unique version-specific command pair avoids expanding the
        // ribbon at all. Never fall through to UIA after issuing a command.
        cancel_script(target, owner, cancel)?;
        post_run(target, owner, run)?;
        return Ok(MenuStart::Posted("版本菜单"));
    }
    Ok(MenuStart::InspectRibbon {
        run_if_no_tools: resources.map(|(run, _)| run),
    })
}

pub fn run(mut target: Window, script: &Path) -> Result<()> {
    let hwnd = target.handle as HWND;
    ensure!(
        pid(hwnd) == target.pid && title(hwnd) == target.title && is_xshell8(target.pid),
        "Xshell 窗口已变化，请重新选择"
    );
    ensure!(script.is_absolute() && script.is_file(), "找不到内置脚本");
    let executable =
        image(target.pid).ok_or_else(|| anyhow::anyhow!("所选 Xshell 已关闭，请重新选择"))?;
    let owner = DialogOwner::new(
        hwnd,
        target.pid,
        executable.with_file_name("XshellCore.exe"),
    );
    owner.require_ready()?;
    target.tab = tab_marker(hwnd, target.pid);
    let live_menu = unsafe { GetMenu(hwnd) };
    let mut automation = None;
    let issued = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut source = "功能区菜单";
    match start_menu(&target, &owner, live_menu, || {
        menu_resource::from_image_pair(&executable)
    })? {
        MenuStart::Posted(via) => source = via,
        MenuStart::InspectRibbon { run_if_no_tools } => {
            // With no unique resource pair, ask the live accessibility provider.
            // Invoke can stay on the COM stack while the picker is modal.
            let (tx, rx) = std::sync::mpsc::channel();
            let target = target.clone();
            let issued = issued.clone();
            let deadline = Instant::now() + Duration::from_secs(8);
            std::thread::spawn(move || {
                let _ = tx.send(accessible::open(&target, issued, deadline));
            });
            automation = Some((rx, run_if_no_tools));
        }
    }
    let mut deadline = Instant::now() + Duration::from_secs(10);
    let mut submitted = None;
    let mut saw_dialog = false;
    while Instant::now() < deadline {
        let dialogs = owner.dialogs();
        saw_dialog |= !dialogs.is_empty();
        if let Some(dialog) = submitted {
            if !dialogs.contains(&dialog) {
                return Ok(());
            }
        } else if let [dialog] = dialogs.as_slice()
            && (automation.is_none() || issued.load(std::sync::atomic::Ordering::Acquire))
        {
            target.ensure_current()?;
            if picker::fill_and_submit(*dialog, script)? {
                submitted = Some(*dialog);
            }
        }
        if let Some((rx, run_if_no_tools)) = automation.as_ref() {
            match rx.try_recv() {
                Ok(Ok(accessible::MenuResult::Invoked)) => automation = None,
                Ok(Ok(accessible::MenuResult::NoToolsMenu)) => {
                    // This result guarantees no accessibility action occurred.
                    // A late/missing dialog is never grounds to send Run twice.
                    ensure!(
                        live_menu.is_null(),
                        "未在所选 Xshell 窗口找到「工具 → 脚本 → 运行」。"
                    );
                    let id = (*run_if_no_tools).ok_or_else(|| {
                        anyhow::anyhow!("所选 Xshell 版本没有可识别的「工具 → 脚本 → 运行」入口。")
                    })?;
                    post_run(&target, &owner, id)?;
                    source = "版本菜单";
                    automation = None;
                    deadline = Instant::now() + Duration::from_secs(10);
                }
                Ok(Err(e)) => bail!("Xshell 功能区：{e}"),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    bail!("Xshell 菜单操作中断，请重试")
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(70));
    }
    if submitted.is_some() {
        bail!("Xshell 未接受内置脚本，文件选择框仍然打开。");
    }
    if saw_dialog {
        bail!("Xshell 已弹出窗口，但 Hub 没有识别到可提交的脚本文件框。");
    }
    if unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_DISABLED != 0 } {
        bail!(
            "所选 Xshell 窗口正在等待一个对话框，但 Hub 未能确认它是本次脚本选择框。请回到 Xshell 查看弹窗。"
        );
    }
    bail!("已从 Xshell {source}请求运行脚本，但没有检测到此窗口的文件选择框。")
}

#[cfg(test)]
mod tests {
    use super::menu_label;

    #[test]
    fn strips_access_keys_and_ellipsis_from_known_menu_labels() {
        assert_eq!(menu_label("&Run...\tAlt+R"), "run");
        assert_eq!(menu_label("运行(&R)…"), "运行");
        assert_ne!(menu_label("Run Script Recording"), "run");
    }
}
