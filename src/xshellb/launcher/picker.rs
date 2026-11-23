use super::{class, collect, menu_label, title, wide};
use anyhow::{Result, ensure};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    UI::WindowsAndMessaging::*,
};

// Both common file dialogs use edt1 (the classic filename edit) or cmb13
// (the filename combo). The modern shell hosts an Edit under an ordinary
// ComboBox; trusting just any Edit would also match its search/address bar.
fn filename_edit(edit: HWND, dialog: HWND) -> bool {
    if class(edit) != "Edit"
        || unsafe {
            IsWindowVisible(edit) == 0 || GetWindowLongW(edit, GWL_STYLE) as u32 & WS_DISABLED != 0
        }
    {
        return false;
    }
    if unsafe { GetDlgCtrlID(edit) } == 0x480 {
        return true;
    }
    let mut parent = unsafe { GetParent(edit) };
    while !parent.is_null() && parent != dialog {
        if unsafe { GetDlgCtrlID(parent) } == 0x47c
            && matches!(class(parent).as_str(), "ComboBox" | "ComboBoxEx32")
        {
            return true;
        }
        parent = unsafe { GetParent(parent) };
    }
    false
}

pub(super) fn file_controls(dialog: HWND) -> Option<(HWND, HWND)> {
    if class(dialog) != "#32770" {
        return None;
    }
    let button = unsafe { GetDlgItem(dialog, IDOK) };
    if button.is_null() || class(button) != "Button" {
        return None;
    }
    let label = menu_label(&title(button));
    // Some installations give the picker a custom caption; recognize its
    // file controls and Open action rather than requiring a localized title.
    let open = [
        "open", "打开", "開啟", "開く", "열기", "öffnen", "ouvrir", "abrir", "run", "运行", "執行",
        "执行",
    ];
    if !open.contains(&label.as_str())
        && !(["确定", "確定", "ok"].contains(&label.as_str())
            && [
                "run script",
                "运行脚本",
                "執行腳本",
                "执行脚本",
                "选择脚本",
                "select script",
            ]
            .contains(&menu_label(&title(dialog)).as_str()))
    {
        return None;
    }
    let mut children = Vec::new();
    unsafe {
        EnumChildWindows(
            dialog,
            Some(collect),
            &mut children as *mut Vec<HWND> as LPARAM,
        );
    }
    let mut edits = children.into_iter().filter(|h| filename_edit(*h, dialog));
    let edit = edits.next()?;
    edits.next().is_none().then_some((edit, button))
}

pub(super) fn fill_and_submit(dialog: HWND, script: &Path) -> Result<bool> {
    let Some((edit, button)) = file_controls(dialog) else {
        return Ok(false);
    };
    let path = wide(script.as_os_str());
    let mut result = 0;
    ensure!(
        unsafe {
            SendMessageTimeoutW(
                edit,
                WM_SETTEXT,
                0,
                path.as_ptr() as LPARAM,
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                800,
                &mut result,
            )
        } != 0
            && result != 0,
        "Xshell 文件选择框没有接受内置脚本路径"
    );
    if unsafe { GetWindowLongW(button, GWL_STYLE) as u32 & WS_DISABLED != 0 } {
        return Ok(false);
    }
    ensure!(
        unsafe { PostMessageW(button, BM_CLICK, 0, 0) } != 0,
        "Xshell 未接受打开脚本操作"
    );
    Ok(true)
}

#[cfg(test)]
mod tests;
