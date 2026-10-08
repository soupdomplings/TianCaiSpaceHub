use anyhow::{Result, bail};
use security_framework::passwords::{
    PasswordOptions, delete_generic_password, generic_password, set_generic_password,
};

const SERVICE: &str = "TianCaiSpaceHub.NVWA";
const NOT_FOUND: i32 = -25300;

pub(super) fn get(name: &str) -> Result<Option<Vec<u8>>> {
    match generic_password(PasswordOptions::new_generic_password(SERVICE, name)) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.code() == NOT_FOUND => Ok(None),
        Err(_) => bail!("macOS 钥匙串读取 NVWA 凭据失败"),
    }
}

pub(super) fn set(name: &str, value: &[u8]) -> Result<()> {
    set_generic_password(SERVICE, name, value)
        .map_err(|_| anyhow::anyhow!("macOS 钥匙串保存 NVWA 凭据失败"))
}

pub(super) fn delete(name: &str) -> Result<()> {
    match delete_generic_password(SERVICE, name) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == NOT_FOUND => Ok(()),
        Err(_) => bail!("macOS 钥匙串删除 NVWA 凭据失败"),
    }
}
