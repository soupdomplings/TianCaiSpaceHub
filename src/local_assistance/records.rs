//! Recoverable ownership inventory, including records left by a Hub restart.
use super::*;
use crate::desktop::transaction;

pub(super) fn list(path: &std::path::Path) -> Result<Vec<Value>> {
    let root = settings::root(path).join("runs");
    transaction::reject_links(&root)?;
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root)?.take(1024) {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let dir = worker::run_dir(path, &id)?;
        let marker: Value = serde_json::from_slice(
            &transaction::read(&dir.join("owner.json"))?.context("协助归属信息缺失，未自动处理")?,
        )?;
        ensure!(
            marker["owner"] == "tiancai-local-assistance-v1" && marker["task"] == id,
            "协助归属信息不一致，未自动处理"
        );
        let running = marker["pid"]
            .as_u64()
            .map(|p| process::alive(p.try_into()?))
            .transpose()?
            .unwrap_or(false);
        let summary = transaction::read(&dir.join("summary.json"))?
            .map(|b| serde_json::from_slice::<Value>(&b))
            .transpose()?
            .unwrap_or(json!({}));
        out.push(
            json!({"id":id,"title":summary["title"],"phase":summary["phase"],"canClear":!running}),
        );
    }
    Ok(out)
}

pub(super) async fn clear_finished(state: &SharedState) -> Result<Value> {
    let _gate = state.local_assistance.setup.lock().await;
    let mut inner = state.local_assistance.inner.lock().await;
    let mut cleared = 0;
    let mut retained = 0;
    for record in list(&state.config_path)? {
        let id = record["id"].as_str().unwrap();
        if record["canClear"] != true || inner.tasks.get(id).is_some_and(|t| !t.worker_stopped) {
            retained += 1;
            continue;
        }
        worker::remove_owned(&state.config_path, id)?;
        if let Some(task) = inner.tasks.get_mut(id) {
            task.erase();
        }
        cleared += 1;
    }
    // Auto-cleared tasks may have only in-memory feedback left.
    for task in inner
        .tasks
        .values_mut()
        .filter(|t| t.worker_stopped && t.phase != Phase::Cleared)
    {
        task.erase();
    }
    Ok(
        json!({"cleared":cleared,"retained":retained,"detail":format!("已清理 {cleared} 份协助对话；{retained} 份运行中或待核对的记录已保留。天工主对话与项目文件保留。" )}),
    )
}
