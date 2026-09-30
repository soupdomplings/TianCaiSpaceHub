use std::{
    ffi::c_void,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

type CFRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CFRef,
        bytes: *const u8,
        len: isize,
        directory: u8,
    ) -> CFRef;
    fn CFRelease(value: CFRef);
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSRegisterURL(url: CFRef, update: u8) -> i32;
}

fn bundle_for_executable(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    if executable.file_name()? != "CodexHub"
        || macos.file_name()? != "MacOS"
        || contents.file_name()? != "Contents"
        || bundle.extension()? != "app"
    {
        return None;
    }
    Some(bundle.to_path_buf())
}

pub(super) fn register() -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|_| "无法读取程序位置 / Cannot locate executable")?;
    let bundle = bundle_for_executable(&executable)
        .filter(|bundle| bundle.join("Contents/Info.plist").is_file())
        .ok_or("请从已复制到应用程序目录的 TianCaiSpace Hub.app 注册 / Registration requires the packaged App bundle")?;
    let bytes = bundle.as_os_str().as_bytes();
    // CoreFoundation copies the path bytes. The owned reference is released
    // after Launch Services updates this exact bundle, including paths with spaces.
    let status = unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        );
        if url.is_null() {
            return Err("无法读取 App 地址 / Invalid App bundle path".into());
        }
        let result = LSRegisterURL(url, 1);
        CFRelease(url);
        result
    };
    if status != 0 {
        return Err(
            "无法注册网页导入，请将 App 复制到应用程序目录后重试 / Registration failed".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_packaged_app_paths() {
        assert_eq!(
            bundle_for_executable(Path::new(
                "/Applications/TianCaiSpace Hub.app/Contents/MacOS/CodexHub"
            )),
            Some(PathBuf::from("/Applications/TianCaiSpace Hub.app"))
        );
        for path in [
            "/usr/local/bin/codexhub",
            "/tmp/Fake.app/CodexHub",
            "/tmp/Hub/Contents/MacOS/CodexHub",
        ] {
            assert!(bundle_for_executable(Path::new(path)).is_none());
        }
    }
}
