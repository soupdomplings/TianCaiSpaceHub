//! Exercise the native menu protocol with our own off-screen window. The
//! fixture neither loads Xshell nor opens/runs a script or issues terminal I/O.
use super::{
    DialogOwner, MenuStart, Window, start_from_live_menu, start_menu, tab_marker, title, wide,
};
use std::{cell::Cell, ffi::OsStr, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Controls::{
            ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TCIF_PARAM, TCITEMW,
            TCM_INSERTITEMW, TCM_SETCURSEL,
        },
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const RUN: u32 = 3161;
const CANCEL: u32 = 4142;

#[derive(Copy, Clone, PartialEq)]
enum Scenario {
    Idle,
    Running,
    SwitchedTab,
    DuplicateRibbonScripts,
}

struct State {
    scenario: Scenario,
    scripts: HMENU,
    tab: HWND,
    cancelled: Cell<u32>,
    refreshes_after_cancel: Cell<u32>,
}

unsafe extern "system" fn fixture_proc(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
    _: usize,
    data: usize,
) -> isize {
    let state = unsafe { &*(data as *const State) };
    if msg == WM_COMMAND && w == CANCEL as usize && l == 0 {
        state.cancelled.set(state.cancelled.get() + 1);
        if state.scenario == Scenario::SwitchedTab {
            unsafe {
                SendMessageW(state.tab, TCM_SETCURSEL, 1, 0);
            }
        }
        return 0;
    }
    if msg == WM_INITMENUPOPUP && w == state.scripts as usize {
        if state.cancelled.get() != 0 {
            state
                .refreshes_after_cancel
                .set(state.refreshes_after_cancel.get() + 1);
        }
        // Cancellation is accepted at once, but the runtime is busy for
        // another refresh. The cached flags change only on WM_INITMENUPOPUP.
        let busy = state.scenario != Scenario::Idle && state.refreshes_after_cancel.get() < 2;
        unsafe {
            EnableMenuItem(
                state.scripts,
                RUN,
                if busy { MF_GRAYED } else { MF_ENABLED },
            );
            EnableMenuItem(
                state.scripts,
                CANCEL,
                if busy { MF_ENABLED } else { MF_GRAYED },
            );
        }
        return 0;
    }
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

struct Frame(HWND);
impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            RemoveWindowSubclass(self.0, Some(fixture_proc), 1);
            DestroyWindow(self.0); // also owns the menu and tab control
        }
    }
}

fn native_menu(scenario: Scenario) {
    unsafe {
        let init = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES,
        };
        assert_ne!(InitCommonControlsEx(&init), 0);
        let (menu, tools, scripts) = (CreateMenu(), CreatePopupMenu(), CreatePopupMenu());
        for (parent, flags, id, label) in [
            (menu, MF_POPUP, tools as usize, "Tools"),
            (tools, MF_POPUP, scripts as usize, "Script"),
            (scripts, MF_STRING, RUN as usize, "Run"),
            (scripts, MF_STRING, CANCEL as usize, "Cancel"),
        ] {
            assert_ne!(
                AppendMenuW(parent, flags, id, wide(OsStr::new(label)).as_ptr()),
                0
            );
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            wide(OsStr::new("STATIC")).as_ptr(),
            wide(OsStr::new("Same test window")).as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            -30000,
            -30000,
            280,
            140,
            ptr::null_mut(),
            menu,
            ptr::null_mut(),
            ptr::null(),
        );
        assert!(!hwnd.is_null());
        let tab = CreateWindowExW(
            0,
            wide(OsStr::new("SysTabControl32")).as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            200,
            80,
            hwnd,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
        );
        assert!(!tab.is_null());
        let item = TCITEMW {
            mask: TCIF_PARAM,
            ..std::mem::zeroed()
        };
        for index in 0..2 {
            assert_eq!(
                SendMessageW(tab, TCM_INSERTITEMW, index, &item as *const _ as isize),
                index as isize
            );
        }
        SendMessageW(tab, TCM_SETCURSEL, 0, 0);
        let state = State {
            scenario,
            scripts,
            tab,
            cancelled: Cell::new(0),
            refreshes_after_cancel: Cell::new(0),
        };
        // Dropped before State, including on assertion failure.
        let _frame = Frame(hwnd);
        assert_ne!(
            SetWindowSubclass(hwnd, Some(fixture_proc), 1, &state as *const _ as usize),
            0
        );
        let pid = std::process::id();
        let target = Window {
            handle: hwnd as usize,
            pid,
            title: title(hwnd),
            tab: tab_marker(hwnd, pid),
        };
        assert!(target.tab.contains(&(tab as usize, 0)));
        let owner = DialogOwner::new(hwnd, pid, PathBuf::new());
        if scenario == Scenario::DuplicateRibbonScripts {
            // Replace the native menu with two identically captioned ribbon
            // controls. A localized label cannot choose a unique Script item.
            assert_ne!(SetMenu(hwnd, ptr::null_mut()), 0);
            DestroyMenu(menu);
            for x in [0, 64] {
                assert!(
                    !CreateWindowExW(
                        0,
                        wide(OsStr::new("BUTTON")).as_ptr(),
                        wide(OsStr::new("脚本")).as_ptr(),
                        WS_CHILD | WS_VISIBLE,
                        x,
                        0,
                        60,
                        25,
                        hwnd,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null()
                    )
                    .is_null()
                );
            }
            assert!(GetMenu(hwnd).is_null());
            assert!(
                matches!(
                    start_menu(&target, &owner, GetMenu(hwnd), || Some((RUN, Some(CANCEL))))
                        .unwrap(),
                    MenuStart::Posted(_)
                ),
                "a verified resource pair must bypass ribbon navigation"
            );
            assert_eq!(state.cancelled.get(), 1);
            let mut msg: MSG = std::mem::zeroed();
            assert_ne!(
                PeekMessageW(&mut msg, hwnd, WM_COMMAND, WM_COMMAND, PM_REMOVE),
                0
            );
            assert_eq!((msg.wParam, msg.lParam), (RUN as usize, 0));
            assert_eq!(
                PeekMessageW(&mut msg, hwnd, WM_COMMAND, WM_COMMAND, PM_REMOVE),
                0
            );
            return;
        }
        let result = start_from_live_menu(&target, &owner, menu);
        match scenario {
            Scenario::Idle => {
                assert_eq!(result.unwrap(), RUN);
                assert_eq!(state.cancelled.get(), 0);
            }
            Scenario::Running => {
                assert_eq!(result.unwrap(), RUN);
                assert_eq!(state.cancelled.get(), 1);
                assert!(state.refreshes_after_cancel.get() >= 2);
            }
            Scenario::SwitchedTab => {
                assert_eq!(target.title, title(hwnd));
                assert!(result.unwrap_err().to_string().contains("标签页已变化"));
                assert_eq!(state.cancelled.get(), 1);
            }
            Scenario::DuplicateRibbonScripts => unreachable!(),
        }
    }
}

#[test]
fn native_menu_waits_for_run_after_a_single_cancel() {
    native_menu(Scenario::Running);
}

#[test]
fn native_menu_leaves_idle_scripts_alone() {
    native_menu(Scenario::Idle);
}

#[test]
fn native_menu_rejects_tab_switch_in_the_same_frame() {
    native_menu(Scenario::SwitchedTab);
}

#[test]
fn duplicate_ribbon_script_labels_use_one_verified_resource_run() {
    native_menu(Scenario::DuplicateRibbonScripts);
}
