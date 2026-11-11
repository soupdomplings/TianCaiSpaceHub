use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Settings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub personality: super::personality::Personality,
    #[serde(default, rename = "fullAccess")]
    pub full_access: bool,
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
    let mut settings = read(config_path)?;
    settings.enabled = enabled;
    save(config_path, &settings)
}
pub(crate) fn write_personality(
    config_path: &Path,
    personality: super::personality::Personality,
) -> Result<()> {
    let mut settings = read(config_path)?;
    settings.personality = personality;
    save(config_path, &settings)
}
pub(crate) fn write_full_access(config_path: &Path, full_access: bool) -> Result<()> {
    let mut settings = read(config_path)?;
    settings.full_access = full_access;
    save(config_path, &settings)
}
fn save(config_path: &Path, settings: &Settings) -> Result<()> {
    crate::desktop::transaction::atomic_write(
        &root(config_path).join("settings.json"),
        &serde_json::to_vec(settings)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn personality_persists_without_changing_enable_and_old_settings_default() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("hub.toml");
        assert_eq!(
            read(&config).unwrap().personality,
            super::super::personality::Personality::Professional
        );
        write(&config, true).unwrap();
        assert!(!read(&config).unwrap().full_access);
        write_full_access(&config, true).unwrap();
        write_personality(&config, super::super::personality::Personality::Catgirl).unwrap();
        assert!(read(&config).unwrap().enabled);
        assert!(read(&config).unwrap().full_access);
        write(&config, false).unwrap();
        assert_eq!(
            read(&config).unwrap().personality,
            super::super::personality::Personality::Catgirl
        );
        let old: Settings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(
            old.personality,
            super::super::personality::Personality::Professional
        );
        assert!(!old.full_access);
    }
}
