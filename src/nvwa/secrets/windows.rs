use std::{
    fs,
    io::{Read, Write},
    path::Path,
    ptr,
};

use anyhow::{Result, bail, ensure};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

const ENTROPY: &[u8] = b"TianCaiSpaceHub/NVWA/user-secrets/v1";
const MAX_BLOB_BYTES: u64 = 128 * 1024;

pub(super) fn get(root: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(root.join(format!("{name}.dpapi"))) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("无法读取 NVWA 系统保护凭据"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("无法读取 NVWA 系统保护凭据"))?;
    ensure!(
        bytes.len() as u64 <= MAX_BLOB_BYTES,
        "NVWA 系统保护凭据超出大小限制"
    );
    Ok(Some(protect(&bytes, false)?))
}

pub(super) fn set(root: &Path, name: &str, value: &[u8]) -> Result<()> {
    let bytes = protect(value, true)?;
    let mut temp = tempfile::NamedTempFile::new_in(root)
        .map_err(|_| anyhow::anyhow!("无法创建 NVWA 系统保护凭据文件"))?;
    temp.write_all(&bytes)
        .map_err(|_| anyhow::anyhow!("无法保存 NVWA 系统保护凭据"))?;
    temp.as_file()
        .sync_all()
        .map_err(|_| anyhow::anyhow!("无法保存 NVWA 系统保护凭据"))?;
    temp.persist(root.join(format!("{name}.dpapi")))
        .map_err(|_| anyhow::anyhow!("无法原子替换 NVWA 系统保护凭据"))?;
    Ok(())
}

pub(super) fn delete(root: &Path, name: &str) -> Result<()> {
    match fs::remove_file(root.join(format!("{name}.dpapi"))) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => bail!("无法删除 NVWA 系统保护凭据"),
    }
}

fn protect(value: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: value.len() as u32,
        pbData: value.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // The absence of LOCAL_MACHINE binds DPAPI protection to the current Windows user.
    let success = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                ptr::null(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    ensure!(
        success != 0 && !output.pbData.is_null(),
        "Windows 用户凭据保护操作失败"
    );
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}
