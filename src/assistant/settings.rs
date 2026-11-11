use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Settings {
    #[serde(default)]
    pub enabled: bool,
}
pub(crate) fn root(config_path: &Path) -> PathBuf {
    crate::desktop::state_root(config_path).join("assistant")
}
pub(crate) fn read(config_path: &Path) -> Result<Settings> {
    crate::desktop::transaction::read(&root(config_path).join("settings.json"))?
        .map(|bytes| serde_json::from_slice(&bytes).context("天才搭档设置无法读取"))
        .transpose()
        .map(|settings| settings.unwrap_or_default())
}
pub(crate) fn write(config_path: &Path, enabled: bool) -> Result<()> {
    crate::desktop::transaction::atomic_write(
        &root(config_path).join("settings.json"),
        &serde_json::to_vec(&Settings { enabled })?,
    )
}
