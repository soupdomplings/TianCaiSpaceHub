use super::*;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LinkOwner {
    session_id: String,
    task_id: String,
    user_id: String,
}

pub(super) struct LinkFile {
    path: PathBuf,
    before: Option<Vec<u8>>,
    value: Value,
    changed: bool,
}

impl LinkFile {
    pub fn load(hub_config: &Path) -> Result<Self> {
        let path = hub_config.with_extension("gmclaw-sessions.json");
        let before = memory::read(&path)?;
        ensure!(
            before
                .as_ref()
                .is_none_or(|bytes| bytes.len() <= 8 * 1024 * 1024),
            "天工 IM 关联记录超过 8 MiB，未修改历史"
        );
        let value: Value = match before.as_deref() {
            Some(bytes) => {
                serde_json::from_slice(bytes).context("天工 IM 关联记录格式不兼容，未修改历史")?
            }
            None => json!({"version":1,"links":[]}),
        };
        ensure!(
            value.get("version").and_then(Value::as_u64) == Some(1),
            "天工 IM 关联记录版本不兼容，未修改历史"
        );
        let rows = value
            .get("links")
            .and_then(Value::as_array)
            .context("天工 IM 关联记录缺少会话列表")?;
        ensure!(
            rows.len() <= 20_000
                && rows.iter().all(|row| row
                    .get("session_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.is_empty())),
            "天工 IM 关联记录包含无法识别的会话，未修改历史"
        );
        Ok(Self {
            path,
            before,
            value,
            changed: false,
        })
    }

    pub fn change(&mut self, session: &str, from: &str, to: &str) -> Result<Vec<LinkOwner>> {
        let mut originals = Vec::new();
        for row in self.value["links"]
            .as_array_mut()
            .context("天工 IM 关联列表格式不兼容")?
        {
            if row.get("session_id").and_then(Value::as_str) != Some(session) {
                continue;
            }
            let context = row
                .get_mut("context")
                .and_then(Value::as_object_mut)
                .context("天工 IM 会话缺少归属记录")?;
            ensure!(
                context.get("user_id").and_then(Value::as_str) == Some(from),
                "天工 IM 会话归属已变化，未修改历史"
            );
            originals.push(LinkOwner {
                session_id: session.into(),
                task_id: context
                    .get("task_id")
                    .and_then(Value::as_str)
                    .context("天工 IM 任务身份缺失")?
                    .into(),
                user_id: from.into(),
            });
            context.insert("user_id".into(), Value::String(to.into()));
            self.changed = true;
        }
        Ok(originals)
    }

    pub fn restore(&mut self, session: &str, original: &str) -> Result<()> {
        for row in self.value["links"]
            .as_array_mut()
            .context("天工 IM 关联列表格式不兼容")?
        {
            if row.get("session_id").and_then(Value::as_str) != Some(session) {
                continue;
            }
            let context = row
                .get_mut("context")
                .and_then(Value::as_object_mut)
                .context("天工 IM 会话缺少归属记录")?;
            context.insert("user_id".into(), Value::String(original.into()));
            self.changed = true;
        }
        Ok(())
    }

    pub fn edit(self) -> Result<Option<memory::Edit>> {
        if !self.changed {
            return Ok(None);
        }
        let after = serde_json::to_vec_pretty(&self.value)?;
        ensure!(after.len() <= 8 * 1024 * 1024, "天工 IM 关联记录过大");
        Ok(Some(memory::Edit {
            path: self.path,
            before: self.before,
            after: Some(after),
        }))
    }
}

pub(super) fn check(
    file: &LinkFile,
    entry: &HistoryEntry,
    task_ids: &[String],
    recovery_owner: Option<&str>,
) -> Result<()> {
    for row in file.value["links"]
        .as_array()
        .context("天工 IM 关联列表格式不兼容")?
    {
        if row.get("session_id").and_then(Value::as_str) != Some(&entry.session_id) {
            continue;
        }
        ensure!(
            row.get("pending").and_then(Value::as_bool) == Some(false)
                && row.get("uncertain").and_then(Value::as_bool) == Some(false),
            "所选会话仍有待审批或状态未知的 IM 任务，请先处理后重试"
        );
        let context = row.get("context").context("天工 IM 会话缺少身份信息")?;
        let owner = context
            .get("user_id")
            .and_then(Value::as_str)
            .context("天工 IM 会话归属未知")?;
        ensure!(
            owner == entry.user_id || recovery_owner == Some(owner),
            "天工 IM 关联与会话归属不一致，未覆盖现有记录"
        );
        ensure!(
            context
                .get("task_id")
                .and_then(Value::as_str)
                .is_some_and(|id| task_ids.iter().any(|task| task == id)),
            "天工 IM 关联与桌面任务不一致，未修改历史"
        );
        let project = context.get("project").context("天工 IM 会话项目未知")?;
        ensure!(
            project.get("id").and_then(Value::as_str) == Some(&entry.project_id),
            "天工 IM 关联与会话项目不一致，未修改历史"
        );
        let directory = project
            .get("path")
            .and_then(Value::as_str)
            .context("天工 IM 会话目录未知")?;
        ensure!(
            memory::path_key(Path::new(directory)) == memory::path_key(Path::new(&entry.work_dir)),
            "天工 IM 关联与会话目录不一致，未修改历史"
        );
    }
    Ok(())
}
