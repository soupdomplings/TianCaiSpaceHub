use super::{accessible::Step, menu_item, wide};
use std::{collections::BTreeSet, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{BOOL, FreeLibrary, HMODULE},
    System::LibraryLoader::{
        EnumResourceNamesW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        LoadLibraryExW,
    },
    UI::WindowsAndMessaging::{DestroyMenu, HMENU, LoadMenuW, RT_MENU},
};

/// A command is meaningful only under Tools > Script in this version's menu.
pub(super) fn from_menu(menu: HMENU) -> Option<(u32, u32)> {
    from_menu_for(menu, Step::Run)
}

pub(super) fn from_menu_for(menu: HMENU, step: Step) -> Option<(u32, u32)> {
    let (_, tools, tools_state) = menu_item(menu, Step::Tools.names())?;
    let (_, script, script_state) = menu_item(tools, Step::Script.names())?;
    let leaf = |action: Step| {
        let (id, child, state) = menu_item(script, action.names())?;
        (child.is_null() && (1..=u16::MAX as u32).contains(&id))
            .then_some((id, tools_state | script_state | state))
    };
    let run = leaf(Step::Run)?;
    match step {
        Step::Run => Some(run),
        Step::Cancel => leaf(Step::Cancel).filter(|(id, _)| *id != run.0),
        _ => None,
    }
}

type MenuIds = (u32, Option<u32>);

unsafe extern "system" fn collect(
    module: HMODULE,
    _: *const u16,
    name: *const u16,
    output: isize,
) -> BOOL {
    let menu = unsafe { LoadMenuW(module, name) };
    if !menu.is_null() {
        if let Some((id, _)) = from_menu(menu) {
            let cancel = from_menu_for(menu, Step::Cancel).map(|(id, _)| id);
            unsafe { (&mut *(output as *mut BTreeSet<MenuIds>)).insert((id, cancel)) };
        }
        unsafe { DestroyMenu(menu) };
    }
    1
}

/// The Xshell 8 ribbon may expose neither an HMENU nor accessible menu items.
/// Read the vendor's linked resources as data; do not execute the image.
pub(super) fn from_image(path: &Path) -> Option<u32> {
    from_image_pair(path).map(|(run, _)| run)
}

pub(super) fn from_image_pair(path: &Path) -> Option<MenuIds> {
    let path = wide(path.as_os_str());
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    if module.is_null() {
        return None;
    }
    let mut ids = BTreeSet::new();
    let enumerated = unsafe {
        EnumResourceNamesW(
            module,
            RT_MENU,
            Some(collect),
            &mut ids as *mut BTreeSet<MenuIds> as isize,
        )
    };
    unsafe { FreeLibrary(module) };
    if enumerated == 0 {
        return None;
    }
    agreed_pair(&ids)
}

fn agreed_pair(ids: &BTreeSet<MenuIds>) -> Option<MenuIds> {
    let &(run, cancel) = ids.first()?;
    ids.iter()
        .all(|(id, _)| *id == run)
        .then_some((run, if ids.len() == 1 { cancel } else { None }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateMenu, CreatePopupMenu, MF_DISABLED, MF_POPUP, MF_STRING,
    };

    fn fixture(
        labels: [&str; 3],
        leaf_id: u32,
        duplicate: bool,
        script_flags: u32,
    ) -> Option<(u32, u32)> {
        unsafe {
            let (menu, tools, scripts) = (CreateMenu(), CreatePopupMenu(), CreatePopupMenu());
            assert!(!menu.is_null() && !tools.is_null() && !scripts.is_null());
            for (parent, id, label, flags) in [
                (menu, tools as usize, labels[0], MF_POPUP),
                (tools, scripts as usize, labels[1], MF_POPUP | script_flags),
                (scripts, leaf_id as usize, labels[2], MF_STRING),
            ] {
                assert_ne!(
                    AppendMenuW(parent, flags, id, wide(OsStr::new(label)).as_ptr()),
                    0
                );
            }
            if duplicate {
                assert_ne!(
                    AppendMenuW(
                        scripts,
                        MF_STRING,
                        (leaf_id + 1) as usize,
                        wide(OsStr::new(labels[2])).as_ptr()
                    ),
                    0
                );
            }
            let result = from_menu(menu);
            DestroyMenu(menu);
            result
        }
    }

    #[test]
    fn uses_version_specific_command_and_respects_disabled_ancestors() {
        let en = fixture(["&Tools", "Sc&ript", "&Run Script"], 1732, false, 0).unwrap();
        let zh = fixture(
            ["工具(&T)", "脚本(&R)", "运行脚本(&R)"],
            3901,
            false,
            MF_DISABLED,
        )
        .unwrap();
        assert_eq!((en.0, zh.0), (1732, 3901));
        assert_eq!(en.1 & MF_DISABLED, 0);
        assert_ne!(zh.1 & MF_DISABLED, 0);
    }

    #[test]
    fn rejects_ambiguous_and_unrelated_menu_actions() {
        assert!(fixture(["Tools", "Script", "Run Script"], 154, true, 0).is_none());
        assert!(fixture(["Tools", "Script", "Run Script Recording"], 154, false, 0).is_none());
        assert!(fixture(["Tools", "Macros", "Run"], 154, false, 0).is_none());
    }

    #[test]
    fn cancel_must_be_a_distinct_sibling_of_run_and_uses_its_own_state() {
        unsafe {
            let (menu, tools, scripts) = (CreateMenu(), CreatePopupMenu(), CreatePopupMenu());
            let append = |parent, flags, id, label: &str| {
                assert_ne!(
                    AppendMenuW(parent, flags, id, wide(OsStr::new(label)).as_ptr()),
                    0
                );
            };
            append(menu, MF_POPUP, tools as usize, "&Tools");
            append(tools, MF_POPUP, scripts as usize, "Sc&ript");
            append(tools, MF_STRING, 201, "Cancel");
            append(scripts, MF_STRING | MF_DISABLED, 301, "Run Script");
            append(scripts, MF_STRING, 302, "Cancel Recording");
            assert!(from_menu_for(menu, Step::Cancel).is_none());
            append(scripts, MF_STRING, 303, "取消脚本(&C)");
            let (id, state) = from_menu_for(menu, Step::Cancel).unwrap();
            assert_eq!((id, state & MF_DISABLED), (303, 0));
            append(scripts, MF_STRING, 304, "Cancel Script");
            assert!(from_menu_for(menu, Step::Cancel).is_none());
            DestroyMenu(menu);
        }
    }

    #[test]
    fn conflicting_menu_resources_never_choose_a_cancel_or_an_ambiguous_run() {
        assert_eq!(
            agreed_pair(&[(17, Some(31)), (17, Some(35))].into()),
            Some((17, None))
        );
        assert_eq!(
            agreed_pair(&[(17, None), (17, Some(31))].into()),
            Some((17, None))
        );
        assert_eq!(agreed_pair(&[(18, Some(31)), (17, Some(31))].into()), None);
    }

    #[test]
    #[ignore = "requires a local Xshell 8 installation; reads only PE resources"]
    fn installed_english_and_chinese_menus_agree() {
        let path =
            std::path::PathBuf::from(std::env::var_os("TIANCAI_XSHELL_MENU_FIXTURE").unwrap());
        let (run, cancel) =
            from_image_pair(&path).expect("unique script picker in Xshell menu resources");
        let cancel = cancel.expect("distinct cancel command in the same Tools > Script menu");
        assert_ne!(run, cancel);
        assert_eq!(from_image(&path), Some(run));
        assert_eq!(
            from_image_pair(&path.with_file_name("Languages").join("XshellCHS.dll")),
            Some((run, Some(cancel)))
        );
    }
}
