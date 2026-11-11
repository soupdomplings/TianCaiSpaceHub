//! Brand only the Hub-owned copy. Original vendor resources and version stay.
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub(super) const NAME: &str = "天才搭档 Server";
pub(super) fn executable() -> &'static str {
    if cfg!(windows) {
        "天才搭档 Server.exe"
    } else {
        "天才搭档 Server"
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    type Handle = *mut c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn BeginUpdateResourceW(file: *const u16, delete: i32) -> Handle;
        fn UpdateResourceW(
            update: Handle,
            kind: *const u16,
            name: *const u16,
            language: u16,
            data: *const c_void,
            size: u32,
        ) -> i32;
        fn EndUpdateResourceW(update: Handle, discard: i32) -> i32;
        fn LoadLibraryExW(file: *const u16, reserved: Handle, flags: u32) -> Handle;
        fn FreeLibrary(module: Handle) -> i32;
        fn EnumResourceLanguagesW(
            module: Handle,
            kind: *const u16,
            name: *const u16,
            callback: unsafe extern "system" fn(Handle, *const u16, *const u16, u16, isize) -> i32,
            context: isize,
        ) -> i32;
    }
    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(file: *const u16, ignored: *mut u32) -> u32;
        fn GetFileVersionInfoW(file: *const u16, ignored: u32, size: u32, data: *mut c_void)
        -> i32;
    }
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    #[derive(Debug)]
    struct Block {
        key: String,
        text: bool,
        value: Vec<u8>,
        children: Vec<Block>,
    }
    fn word(bytes: &[u8], at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(
            bytes
                .get(at..at + 2)
                .context("版本资源不完整")?
                .try_into()?,
        ))
    }
    fn aligned(at: usize) -> usize {
        (at + 3) & !3
    }
    impl Block {
        fn parse(bytes: &[u8], start: usize) -> Result<(Self, usize)> {
            let end = start + usize::from(word(bytes, start)?);
            ensure!(end <= bytes.len() && end > start + 6, "版本资源长度无效");
            let length = usize::from(word(bytes, start + 2)?);
            let text = word(bytes, start + 4)? == 1;
            let mut at = start + 6;
            let mut key = Vec::new();
            loop {
                ensure!(at + 2 <= end, "版本资源键不完整");
                let c = word(bytes, at)?;
                at += 2;
                if c == 0 {
                    break;
                }
                key.push(c);
            }
            let key = String::from_utf16(&key)?;
            at = aligned(at);
            let value_end = at + length * if text { 2 } else { 1 };
            ensure!(value_end <= end || length == 0, "版本资源值无效");
            let value = if length == 0 {
                Vec::new()
            } else {
                bytes[at..value_end].to_vec()
            };
            at = aligned(value_end);
            let mut children = Vec::new();
            while at + 6 < end && word(bytes, at)? > 0 {
                let (child, next) = Self::parse(&bytes[..end], at)?;
                children.push(child);
                at = aligned(next);
            }
            Ok((
                Self {
                    key,
                    text,
                    value,
                    children,
                },
                end,
            ))
        }
        fn string(key: &str, value: &str) -> Self {
            Self {
                key: key.into(),
                text: true,
                value: value
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect(),
                children: Vec::new(),
            }
        }
        fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
            let start = out.len();
            out.extend_from_slice(&[0; 6]);
            for c in self.key.encode_utf16().chain(Some(0)) {
                out.extend_from_slice(&c.to_le_bytes());
            }
            while out.len() % 4 != 0 {
                out.push(0);
            }
            out.extend_from_slice(&self.value);
            for child in &self.children {
                while out.len() % 4 != 0 {
                    out.push(0);
                }
                child.encode(out)?;
            }
            let length = u16::try_from(out.len() - start).context("版本资源过大")?;
            let value_length = u16::try_from(self.value.len() / if self.text { 2 } else { 1 })?;
            out[start..start + 2].copy_from_slice(&length.to_le_bytes());
            out[start + 2..start + 4].copy_from_slice(&value_length.to_le_bytes());
            out[start + 4..start + 6].copy_from_slice(&u16::from(self.text).to_le_bytes());
            Ok(())
        }
        fn brand(&mut self) {
            if self.key == "StringFileInfo" {
                if self.children.is_empty() {
                    self.children.push(Self {
                        key: "040904b0".into(),
                        text: true,
                        value: vec![],
                        children: vec![],
                    });
                }
                for table in &mut self.children {
                    for (key, value) in [
                        ("FileDescription", NAME),
                        ("ProductName", NAME),
                        ("InternalName", NAME),
                        ("OriginalFilename", executable()),
                    ] {
                        let replacement = Self::string(key, value);
                        if let Some(old) = table.children.iter_mut().find(|b| b.key == key) {
                            *old = replacement;
                        } else {
                            table.children.push(replacement);
                        }
                    }
                }
            } else {
                for child in &mut self.children {
                    child.brand();
                }
            }
        }
    }
    fn resource(path: &Path) -> Result<Block> {
        let path = wide(path);
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
        if size == 0 {
            let fixed = [
                0xfeef04bdu32,
                0x00010000,
                0,
                0,
                0,
                0,
                0x3f,
                0,
                0x40004,
                1,
                0,
                0,
                0,
            ];
            return Ok(Block {
                key: "VS_VERSION_INFO".into(),
                text: false,
                value: fixed.into_iter().flat_map(u32::to_le_bytes).collect(),
                children: vec![
                    Block {
                        key: "StringFileInfo".into(),
                        text: true,
                        value: vec![],
                        children: vec![],
                    },
                    Block {
                        key: "VarFileInfo".into(),
                        text: true,
                        value: vec![],
                        children: vec![Block {
                            key: "Translation".into(),
                            text: false,
                            value: vec![9, 4, 0xb0, 4],
                            children: vec![],
                        }],
                    },
                ],
            });
        }
        ensure!(size <= 1024 * 1024, "上游版本资源过大");
        let mut bytes = vec![0; size as usize];
        ensure!(
            unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, bytes.as_mut_ptr().cast()) } != 0,
            "无法读取上游程序版本资源：{}",
            std::io::Error::last_os_error()
        );
        Block::parse(&bytes, 0).map(|(resource, _)| resource)
    }
    pub(super) fn version(path: &Path) -> Option<String> {
        fn find(block: &Block) -> Option<String> {
            if block.key == "ProductVersion" || block.key == "FileVersion" {
                let words: Vec<u16> = block
                    .value
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .take_while(|c| *c != 0)
                    .collect();
                return String::from_utf16(&words)
                    .ok()
                    .filter(|s| !s.trim().is_empty());
            }
            block.children.iter().find_map(find)
        }
        resource(path).ok().as_ref().and_then(find)
    }
    pub(super) fn apply(path: &Path) -> Result<()> {
        let mut resource = resource(path)?;
        if !resource.children.iter().any(|b| b.key == "StringFileInfo") {
            resource.children.push(Block {
                key: "StringFileInfo".into(),
                text: true,
                value: vec![],
                children: vec![],
            });
        }
        resource.brand();
        let mut bytes = Vec::new();
        resource.encode(&mut bytes)?;
        let path = wide(path);
        let mut languages = Vec::<u16>::new();
        unsafe extern "system" fn language(
            _: Handle,
            _: *const u16,
            _: *const u16,
            id: u16,
            context: isize,
        ) -> i32 {
            unsafe {
                (*(context as *mut Vec<u16>)).push(id);
            }
            1
        }
        unsafe {
            let module = LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), 0x22);
            if !module.is_null() {
                EnumResourceLanguagesW(
                    module,
                    16usize as *const u16,
                    1usize as *const u16,
                    language,
                    &mut languages as *mut _ as isize,
                );
                FreeLibrary(module);
            }
            if languages.is_empty() {
                languages.push(0x409);
            }
            let update = BeginUpdateResourceW(path.as_ptr(), 0);
            ensure!(
                !update.is_null(),
                "无法准备品牌程序元数据：{}",
                std::io::Error::last_os_error()
            );
            for id in languages {
                if UpdateResourceW(
                    update,
                    16usize as *const u16,
                    1usize as *const u16,
                    id,
                    bytes.as_ptr().cast(),
                    bytes.len().try_into()?,
                ) == 0
                {
                    let error = std::io::Error::last_os_error();
                    EndUpdateResourceW(update, 1);
                    anyhow::bail!("无法写入品牌程序元数据：{error}");
                }
            }
            ensure!(
                EndUpdateResourceW(update, 0) != 0,
                "无法保存品牌程序元数据：{}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
    #[cfg(test)]
    #[test]
    fn branding_changes_copy_metadata_and_preserves_original_version() {
        let temp = tempfile::tempdir().unwrap();
        let source = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/where.exe");
        let source_version = version(&source);
        let copy = temp.path().join(executable());
        std::fs::copy(&source, &copy).unwrap();
        // System files can obtain localized strings from adjacent MUI files.
        // Compare the copy before/after branding, without importing those files.
        let before = version(&copy);
        let fixed = resource(&copy).unwrap().value;
        apply(&copy).unwrap();
        let block = resource(&copy).unwrap();
        assert_eq!(block.value, fixed);
        let info = block
            .children
            .iter()
            .find(|b| b.key == "StringFileInfo")
            .unwrap();
        for table in &info.children {
            let field = table
                .children
                .iter()
                .find(|b| b.key == "FileDescription")
                .unwrap();
            assert_eq!(field.value, Block::string("FileDescription", NAME).value);
            assert!(table.children.iter().any(|b| b.key == "LegalCopyright"));
        }
        assert_eq!(version(&copy), before);
        assert_eq!(version(&source), source_version);
        let mut encoded = Vec::new();
        block.encode(&mut encoded).unwrap();
        assert!(Block::parse(&encoded, 0).is_ok());
    }
}

pub(super) fn apply(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::apply(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}
pub(super) fn version(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        windows::version(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}
