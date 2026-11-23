//! The optional checks and per-command permissions shared with the Xshell script.
use super::CHECKS;
use crate::desktop::transaction;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

pub(crate) const EXTRAS: &[&str] = &[
    "ls", "ls -l", "ls -a", "ls -la", "ls -al", "ls -lh", "ls -lah", "uname -s", "uname -r",
    "uname -m", "whoami", "date",
];
const SIMPLE: &[&str] = &["arch", "nproc", "nproc --all", "df", "free", "uname"];
// Short switches with no operands. The Xshell script receives this same table.
pub(super) const FLAGS: &[(&str, &str)] = &[
    ("ls", "1alhSt"),
    ("df", "hHTiPk"),
    ("free", "bhkmgtw"),
    ("uname", "asnrvm"),
    ("id", "ugGn"),
    ("date", "u"),
    ("pwd", "PL"),
    ("uptime", "ps"),
];

fn extra_allowed(command: &str) -> bool {
    if EXTRAS.contains(&command) || SIMPLE.contains(&command) {
        return true;
    }
    if command.len() > 96 {
        return false;
    }
    let Some((name, tail)) = command.split_once(' ') else {
        return false;
    };
    let Some((_, flags)) = FLAGS.iter().find(|(family, _)| *family == name) else {
        return false;
    };
    tail.split(' ').all(|arg| {
        arg.strip_prefix('-')
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| flags.as_bytes().contains(&c)))
    })
}
const FILE: &str = "check-policy.json";
const LEGACY: &str = "approve-built-in-checks";

#[cfg(feature = "gui")]
pub(super) fn catalog_commands() -> Vec<&'static str> {
    EXTRAS.iter().chain(SIMPLE.iter()).copied().collect()
}

#[cfg(feature = "gui")]
pub(crate) fn supported_commands() -> String {
    let builtin = CHECKS
        .iter()
        .map(|(_, c)| *c)
        .collect::<Vec<_>>()
        .join("；");
    let extra = catalog_commands().join("；");
    let flags = FLAGS
        .iter()
        .map(|(name, opts)| format!("{name}：{opts}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "内置命令：{builtin}\n\n其他只读命令：{extra}\n\n下列命令也支持组合短选项，例如 free -m、uname -a、df -hT。可用选项：\n{flags}"
    )
}

#[cfg(feature = "gui")]
pub(crate) fn builtin_checks() -> &'static [(&'static str, &'static str)] {
    CHECKS
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Policy {
    pub(crate) approved: BTreeSet<String>,
    pub(crate) extra: BTreeSet<String>,
}

fn builtin(key: &str) -> Option<&'static str> {
    CHECKS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, cmd)| *cmd)
}

impl Policy {
    #[cfg(feature = "gui")]
    pub(crate) fn batch_display(&self) -> (String, String) {
        (
            self.approved
                .iter()
                .map(|k| builtin(k).unwrap_or(k))
                .collect::<Vec<_>>()
                .join("; "),
            self.extra
                .difference(&self.approved)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    #[cfg(feature = "gui")]
    pub(crate) fn from_batch(auto: &str, ask: &str) -> Result<Self> {
        let mut next = Self::default();
        let mut unknown = Vec::new();
        for (field, approved) in [(ask, false), (auto, true)] {
            for raw in field.split([';', '；', '\r', '\n']) {
                let cmd = raw
                    .split([' ', '\t'])
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if cmd.is_empty() {
                    continue;
                }
                if let Some((key, _)) = CHECKS.iter().find(|(_, body)| *body == cmd) {
                    if approved {
                        next.approved.insert((*key).to_owned());
                    }
                } else if extra_allowed(&cmd) {
                    next.extra.insert(cmd.clone());
                    if approved {
                        next.approved.insert(cmd);
                    }
                } else if !unknown.contains(&cmd) {
                    unknown.push(cmd);
                }
            }
        }
        anyhow::ensure!(
            unknown.is_empty(),
            "不支持这些巡检命令：{}。请修改后再保存。",
            unknown.join("；")
        );
        Ok(next)
    }

    pub(crate) fn load(root: &Path) -> Result<Self> {
        if let Some(bytes) = transaction::read(&root.join(FILE))? {
            let mut policy: Self =
                serde_json::from_slice(&bytes).context("无法读取巡检命令设置")?;
            policy.extra.retain(|k| extra_allowed(k));
            policy
                .approved
                .retain(|k| builtin(k).is_some() || policy.extra.contains(k));
            return Ok(policy);
        }
        let mut policy = Self::default();
        if transaction::read(&root.join(LEGACY))?.as_deref() == Some(b"true") {
            policy
                .approved
                .extend(CHECKS.iter().map(|(k, _)| (*k).to_owned()));
        }
        Ok(policy)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn save(&self, root: &Path) -> Result<()> {
        crate::desktop::prepare_private_directory(root)?;
        // Older running scripts only understand the all-six marker. Never leave it
        // granting a command after the user saves a narrower selection.
        let old = root.join(LEGACY);
        if transaction::read(&old)?.as_deref() == Some(b"true") {
            transaction::atomic_write(&old, b"false")?;
        }
        transaction::atomic_write(&root.join(FILE), &serde_json::to_vec(self)?)
    }
}

pub(crate) fn preapproved(root: &Path, key: &str) -> bool {
    Policy::load(root).is_ok_and(|p| p.approved.contains(key))
}

pub(crate) fn resolve_check(root: &Path, key: &str) -> Option<String> {
    if let Some(command) = builtin(key) {
        return Some(command.to_owned());
    }
    if !extra_allowed(key) {
        return None;
    }
    Policy::load(root).ok()?.extra.get(key).cloned()
}

pub(crate) fn enabled_extra_keys(root: &Path) -> Vec<String> {
    Policy::load(root)
        .unwrap_or_default()
        .extra
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(feature = "gui")]
    #[test]
    fn mixed_batch_round_trips_exact_dispatch_and_keeps_preexisting_ask_only_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let p = Policy::from_batch(
            "  pwd； df   -h;\r\nfree -m; uname -sr;free -m;",
            "date; uname -sr",
        )
        .unwrap();
        p.save(root).unwrap();
        let loaded = Policy::load(root).unwrap();
        for key in ["directory", "disk", "free -m", "uname -sr"] {
            assert!(preapproved(root, key));
        }
        assert!(!preapproved(root, "date"));
        assert_eq!(
            resolve_check(root, "uname -sr").as_deref(),
            Some("uname -sr")
        );
        assert_eq!(resolve_check(root, "uname -a"), None);
        let (a, b) = loaded.batch_display();
        let again = Policy::from_batch(&a, &b).unwrap();
        assert_eq!(p.extra, again.extra);
        assert_eq!(p.approved, again.approved);
        let err = Policy::from_batch("date; ls | wc; free -m; uname --foo", "")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("ls | wc") && err.contains("uname --foo"));
        assert!(preapproved(root, "directory"));
    }

    #[test]
    fn legacy_approval_cannot_grant_extras_or_override_a_broken_new_policy() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join(LEGACY), b"true").unwrap();
        assert!(preapproved(root, "identity"));
        assert_eq!(resolve_check(root, "ls -l"), None);
        fs::write(root.join(FILE), br#"{"approved":["host"]}"#).unwrap();
        assert!(!preapproved(root, "identity"));
        assert!(!preapproved(root, "host"));
        assert_eq!(resolve_check(root, "host").as_deref(), Some("hostname"));
        assert!(enabled_extra_keys(root).is_empty());
    }

    #[test]
    fn an_extra_must_be_an_exact_enabled_command_even_when_listed_as_approved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join(FILE), br#"{"extra":["ls -l","date","ls -l /tmp","ls;pwd"],"approved":["date","whoami","ls;pwd","ls -l /tmp","directory"]}"#).unwrap();
        assert_eq!(enabled_extra_keys(root), ["date", "ls -l"]);
        assert_eq!(resolve_check(root, "ls -l").as_deref(), Some("ls -l"));
        assert!(!preapproved(root, "ls -l"));
        assert!(preapproved(root, "date"));
        assert!(preapproved(root, "directory"));
        for key in ["whoami", "ls;pwd", "ls -l /tmp", "ls\npwd", ""] {
            assert!(resolve_check(root, key).is_none());
            assert!(!preapproved(root, key));
        }
    }

    #[cfg(feature = "gui")]
    #[test]
    fn saving_a_narrower_policy_revokes_the_old_all_six_grant() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join(LEGACY), b"true").unwrap();
        let mut p = Policy::load(root).unwrap();
        p.approved.remove("host");
        p.save(root).unwrap();
        assert_eq!(fs::read(root.join(LEGACY)).unwrap(), b"false");
        assert!(!preapproved(root, "host"));
        assert!(preapproved(root, "identity"));
    }
}
