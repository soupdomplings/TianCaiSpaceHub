//! Visibility of the user-revealed sharing entry; separate from authorization.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Default, Deserialize, Serialize)]
struct Preferences {
    #[serde(default)]
    visible: bool,
}

fn path(config: &Path) -> PathBuf {
    super::super::state_root(config)
        .join("workbuddy-direct")
        .join("preferences.json")
}

pub(super) fn visible(config: &Path) -> Result<bool> {
    crate::desktop::transaction::read(&path(config))?
        .map(|bytes| serde_json::from_slice::<Preferences>(&bytes))
        .transpose()
        .context("WorkBuddy 分享入口设置无法读取")
        .map(|preferences| preferences.unwrap_or_default().visible)
}

pub(super) fn write(config: &Path, visible: bool) -> Result<()> {
    if self::visible(config)? == visible {
        return Ok(());
    }
    crate::desktop::transaction::atomic_write(
        &path(config),
        &serde_json::to_vec(&Preferences { visible })?,
    )
}
