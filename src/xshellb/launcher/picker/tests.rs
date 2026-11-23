use super::fill_and_submit;
use crate::xshellb::launcher::{DialogOwner, class, pid, wide};
use std::{
    ffi::{OsStr, OsString},
    os::windows::{ffi::OsStringExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    ptr, thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::HWND as ComHwnd,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST, FOS_NOCHANGEDIR, FileOpenDialog,
            IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::w,
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, OFN_DONTADDTORECENT, OFN_EXPLORER,
            OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OPENFILENAMEW,
        },
        WindowsAndMessaging::*,
    },
};

// Run the actual dialog in a separate *test* process. It can be reaped even if
// a modal validation prompt or a failed assertion prevents the dialog closing.
struct FixtureChild(Child);
impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Xshell.exe owns the frame on a pumping UI thread while XshellCore.exe owns
// the modal common dialog. Keep a second frame to reject a different target.
struct Frames {
    selected: HWND,
    other: HWND,
    pump: Option<thread::JoinHandle<()>>,
}

impl Frames {
    fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let pump = thread::spawn(move || unsafe {
            let first = window("STATIC", ptr::null_mut(), 0, WS_POPUP);
            let other = window("STATIC", ptr::null_mut(), 0, WS_POPUP);
            tx.send((first as usize, other as usize)).unwrap();
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            DestroyWindow(other);
            DestroyWindow(first);
        });
        let (first, other) = rx.recv_timeout(Duration::from_secs(4)).unwrap();
        Self {
            selected: first as HWND,
            other: other as HWND,
            pump: Some(pump),
        }
    }
}
impl Drop for Frames {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(
                GetWindowThreadProcessId(self.selected, ptr::null_mut()),
                WM_QUIT,
                0,
                0,
            );
        }
        self.pump.take().unwrap().join().unwrap();
    }
}

fn window(kind: &str, parent: HWND, id: usize, style: u32) -> HWND {
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            wide(OsStr::new(kind)).as_ptr(),
            ptr::null(),
            style,
            -30000,
            -30000,
            300,
            100,
            parent,
            id as _,
            ptr::null_mut(),
            ptr::null(),
        )
    };
    assert!(!hwnd.is_null(), "fixture could not create a {kind} window");
    hwnd
}

fn classic_dialog(owner: HWND, dir: &Path) -> PathBuf {
    let directory = wide(dir.as_os_str());
    let title = wide(OsStr::new("运行脚本"));
    let mut buffer = [0u16; 2048];
    let mut descriptor = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrInitialDir: directory.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        Flags: OFN_DONTADDTORECENT | OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR,
        ..unsafe { std::mem::zeroed() }
    };
    assert_ne!(
        unsafe { GetOpenFileNameW(&mut descriptor) },
        0,
        "GetOpenFileNameW cancelled or failed: {}",
        unsafe { CommDlgExtendedError() }
    );
    PathBuf::from(OsString::from_wide(
        &buffer[..buffer.iter().position(|&c| c == 0).unwrap()],
    ))
}

fn modern_dialog(owner: HWND) -> PathBuf {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).unwrap();
        dialog
            .SetOptions(
                dialog.GetOptions().unwrap()
                    | FOS_DONTADDTORECENT
                    | FOS_FILEMUSTEXIST
                    | FOS_NOCHANGEDIR,
            )
            .unwrap();
        dialog.SetTitle(w!("运行脚本")).unwrap();
        dialog.Show(Some(ComHwnd(owner))).unwrap();
        let result = dialog
            .GetResult()
            .unwrap()
            .GetDisplayName(SIGDN_FILESYSPATH)
            .unwrap();
        let path = PathBuf::from(OsString::from_wide(result.as_wide()));
        CoTaskMemFree(Some(result.0.cast()));
        path
    }
}

// Names and IDs from our own synthetic dialog only. Do not read field values.
fn control_ids(dlg: HWND) -> Vec<String> {
    let mut children: Vec<HWND> = Vec::new();
    unsafe {
        EnumChildWindows(
            dlg,
            Some(crate::xshellb::launcher::collect),
            &mut children as *mut _ as isize,
        );
    }
    children
        .into_iter()
        .filter_map(|h| {
            let name = class(h);
            matches!(
                name.as_str(),
                "Button" | "Edit" | "ComboBox" | "ComboBoxEx32" | "DirectUIHWND"
            )
            .then(|| format!("{}:{:x}", name, unsafe { GetDlgCtrlID(h) }))
        })
        .take(16)
        .collect()
}

fn native_roundtrip(modern: bool) {
    const FIXTURE: &str = "TIANCAI_TEST_XSHELL_NATIVE_PICKER";
    const FRAME: &str = "TIANCAI_TEST_XSHELL_PARENT_FRAME";
    if let Some(dir) = std::env::var_os(FIXTURE) {
        let dir = PathBuf::from(dir);
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let owner = if modern {
            std::env::var(FRAME).unwrap().parse::<usize>().unwrap() as HWND
        } else {
            window("STATIC", ptr::null_mut(), 0, WS_POPUP)
        };
        let tmp = dir.join("owner.tmp");
        std::fs::write(&tmp, (owner as usize).to_string()).unwrap();
        std::fs::rename(tmp, dir.join("owner")).unwrap();
        let path = if modern {
            modern_dialog(owner)
        } else {
            classic_dialog(owner, &dir)
        };
        unsafe {
            if !modern {
                DestroyWindow(owner);
            }
            CoUninitialize();
        }
        std::fs::write(dir.join("selected"), path.to_str().unwrap()).unwrap();
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("测试 空格 xshellb_attach.py");
    std::fs::write(&script, b"# inert test file").unwrap();
    let this_thread = thread::current();
    let frames = modern.then(Frames::new);
    let exe = std::env::current_exe().unwrap();
    let mut expected = frames
        .as_ref()
        .map(|frames| DialogOwner::new(frames.selected, std::process::id(), exe.clone()));
    let mut child = FixtureChild(
        Command::new(&exe)
            .args(["--exact", this_thread.name().unwrap(), "--nocapture"])
            .env(FIXTURE, dir.path())
            .env(
                FRAME,
                frames
                    .as_ref()
                    .map_or(0, |f| f.selected as usize)
                    .to_string(),
            )
            .creation_flags(0x08000000) // no console
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(12);
    let mut submitted = false;
    let mut found = false;
    let mut ids = Vec::new();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "isolated picker exited with {status}");
            break;
        }
        assert!(
            Instant::now() < until,
            "native picker timeout (found={found}, submitted={submitted}, native controls={ids:?})"
        );
        if !submitted
            && let Ok(owner) = std::fs::read_to_string(dir.path().join("owner"))
            && let Ok(owner) = owner.parse::<usize>()
        {
            let owner = owner as HWND;
            let dlg = if let (Some(expected), Some(frames)) = (expected.as_mut(), &frames) {
                let matches = expected.dialogs();
                assert!(matches.len() <= 1, "unexpected synthetic popup count");
                let Some(&dlg) = matches.first() else {
                    thread::sleep(Duration::from_millis(45));
                    continue;
                };
                assert_ne!(
                    pid(dlg),
                    pid(owner),
                    "old same-process test must reject this dialog"
                );
                let mut wrong_frame =
                    DialogOwner::new(frames.other, std::process::id(), exe.clone());
                assert!(!wrong_frame.dialogs().contains(&dlg));
                let mut wrong_program =
                    DialogOwner::new(owner, std::process::id(), dir.path().join("unrelated.exe"));
                assert!(!wrong_program.dialogs().contains(&dlg));
                dlg
            } else {
                unsafe { GetLastActivePopup(owner) }
            };
            if !dlg.is_null()
                && dlg != owner
                && pid(dlg) == child.0.id()
                && unsafe { GetWindow(dlg, GW_OWNER) == owner && IsWindowVisible(dlg) != 0 }
                && class(dlg) == "#32770"
            {
                found = true;
                ids = control_ids(dlg);
                if super::file_controls(dlg).is_some() {
                    // A previous attempt might leave exactly this standard
                    // picker open. A new attempt must leave its contents alone.
                    let preexisting = if let Some(expected) = expected.as_ref() {
                        expected.require_ready()
                    } else {
                        DialogOwner::new(owner, child.0.id(), exe.with_file_name("unused.exe"))
                            .require_ready()
                    };
                    assert!(
                        preexisting
                            .unwrap_err()
                            .to_string()
                            .contains("请先点「取消」")
                    );
                }
                submitted = fill_and_submit(dlg, &script)
                    .unwrap_or_else(|e| panic!("{e}; native controls={ids:?}"));
            }
        }
        thread::sleep(Duration::from_millis(45));
    }
    assert!(submitted, "picker returned without automatic submission");
    // Windows may expand an 8.3 TEMP directory (e.g. ADMINI~1) when
    // returning the selected path. Compare the actual existing files.
    let selected = PathBuf::from(std::fs::read_to_string(dir.path().join("selected")).unwrap());
    assert_eq!(
        selected.canonicalize().unwrap(),
        script.canonicalize().unwrap()
    );
}

#[test]
fn automatically_selects_exact_file_in_classic_picker() {
    native_roundtrip(false);
}

#[test]
fn automatically_selects_exact_file_in_a_child_process_modern_picker() {
    native_roundtrip(true);
}

#[test]
fn unrelated_edit_and_disabled_button_are_not_used_as_a_picker() {
    let owner = window("STATIC", ptr::null_mut(), 0, WS_POPUP);
    let edit = window("EDIT", owner, 712, WS_CHILD | WS_VISIBLE);
    window(
        "BUTTON",
        owner,
        IDOK as usize,
        WS_CHILD | WS_VISIBLE | WS_DISABLED,
    );
    let result = fill_and_submit(owner, Path::new(r"C:\unused fixture.py"));
    let edit_len = unsafe { GetWindowTextLengthW(edit) };
    unsafe {
        DestroyWindow(owner);
    }
    assert!(!result.unwrap_or(false));
    assert_eq!(edit_len, 0);
}
