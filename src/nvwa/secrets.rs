use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Clone)]
pub struct SecretStore {
    root: PathBuf,
    gate: Arc<Mutex<()>>,
}

impl SecretStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        #[cfg(windows)]
        std::fs::create_dir_all(&root)
            .map_err(|_| anyhow::anyhow!("无法创建 NVWA 系统保护凭据目录"))?;
        Ok(Self {
            root,
            gate: Arc::new(Mutex::new(())),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let name = self.name(key)?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("NVWA 凭据存储锁不可用"))?;
        #[cfg(windows)]
        {
            windows::get(&self.root, &name)
        }
        #[cfg(target_os = "macos")]
        {
            macos::get(&name)
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = name;
            anyhow::bail!("NVWA 系统保护凭据仅支持 Windows 和 macOS")
        }
    }

    pub fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        ensure!(
            !value.is_empty() && value.len() <= 64 * 1024,
            "NVWA 凭据大小无效"
        );
        let name = self.name(key)?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("NVWA 凭据存储锁不可用"))?;
        #[cfg(windows)]
        {
            windows::set(&self.root, &name, value)
        }
        #[cfg(target_os = "macos")]
        {
            macos::set(&name, value)
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = (name, value);
            anyhow::bail!("NVWA 系统保护凭据仅支持 Windows 和 macOS")
        }
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        let name = self.name(key)?;
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("NVWA 凭据存储锁不可用"))?;
        #[cfg(windows)]
        {
            windows::delete(&self.root, &name)
        }
        #[cfg(target_os = "macos")]
        {
            macos::delete(&name)
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = name;
            anyhow::bail!("NVWA 系统保护凭据仅支持 Windows 和 macOS")
        }
    }

    fn name(&self, key: &str) -> Result<String> {
        ensure!(
            !key.is_empty() && key.len() <= 256 && !key.chars().any(char::is_control),
            "NVWA 凭据引用无效"
        );
        let mut hash = Sha256::new();
        hash.update(self.root.to_string_lossy().as_bytes());
        hash.update([0]);
        hash.update(key.as_bytes());
        Ok(hex::encode(hash.finalize()))
    }
}
