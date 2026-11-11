//! Only feature-owned temporary runs; persistent shared Server stays untouched.
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
    Ok(serde_json::to_value(stop_and_cleanup(state, false).await?)?)
}

#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub cleared: usize,
    pub retained: usize,
    pub detail: String,
}

/// Startup recovery handles only owned, already-exited runs. An unrecognized
/// or live PID is retained rather than killed from a stale marker.
pub fn recover_runs(path: &std::path::Path) -> Result<CleanupReport> {
    recover_except(path, &std::collections::HashSet::new())
}
fn recover_except(
    path: &std::path::Path,
    active: &std::collections::HashSet<String>,
) -> Result<CleanupReport> {
    let runs = settings::root(path).join("runs");
    transaction::reject_links(&runs)?;
    let mut report = CleanupReport::default();
    if runs.is_dir() {
        for entry in std::fs::read_dir(&runs)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !uuid::Uuid::parse_str(&id).is_ok_and(|value| value.to_string() == id) {
                continue;
            }
            if active.contains(&id) {
                report.retained += 1;
                continue;
            }
            match worker::remove_owned(path, &id) {
                Ok(()) => report.cleared += 1,
                Err(_) => report.retained += 1,
            }
        }
        let _ = std::fs::remove_dir(&runs);
    }
    // Current workers keep all native temporary logs below their owned run.
    // A separate logs root is removed only when an explicit ownership marker
    // proves it was created by this feature, never for arbitrary local logs.
    let logs = settings::root(path).join("logs");
    if let Some(marker) = transaction::read(&logs.join("owner.json"))?
        && serde_json::from_slice::<Value>(&marker)?["owner"] == "tiancai-local-assistance-v1"
    {
        transaction::reject_links(&logs)?;
        reject_log_links(&logs)?;
        std::fs::remove_dir_all(&logs)?;
    }
    report.detail = format!(
        "已清理 {} 份协助临时内容；{} 份仍运行或归属待核对的内容已保留。共享 Server 与项目文件保留。",
        report.cleared, report.retained
    );
    Ok(report)
}
fn reject_log_links(root: &std::path::Path) -> Result<()> {
    let mut pending = vec![root.to_owned()];
    let mut count = 0;
    while let Some(path) = pending.pop() {
        transaction::reject_links(&path)?;
        count += 1;
        ensure!(count <= 20_000, "协助日志过多，未清理");
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        }
    }
    Ok(())
}

pub async fn shutdown_and_cleanup(state: &SharedState) -> Result<CleanupReport> {
    stop_and_cleanup(state, true).await
}
async fn stop_and_cleanup(state: &SharedState, exiting: bool) -> Result<CleanupReport> {
    let _gate = state.local_assistance.setup.lock().await;
    {
        let mut inner = state.local_assistance.inner.lock().await;
        inner.cleaning = true;
        for task in inner.tasks.values_mut() {
            task.closing = true;
            // This coordinator owns directory removal after all workers stop.
            // Do not race the worker's per-task Delete path on the same run.
            task.delete_requested = false;
            task.pending.clear();
            if !task.worker_stopped {
                let _ = task.controller.try_send(Control::Shutdown);
            }
        }
    }
    state.local_assistance.changed.notify_waiters();
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let notified = state.local_assistance.changed.notified();
        if state
            .local_assistance
            .inner
            .lock()
            .await
            .tasks
            .values()
            .all(|task| task.worker_stopped)
        {
            break;
        }
        if tokio::time::timeout_at(until, notified).await.is_err() {
            break;
        }
    }
    let active = state
        .local_assistance
        .inner
        .lock()
        .await
        .tasks
        .values()
        .filter(|task| !task.worker_stopped)
        .map(|task| task.id.clone())
        .collect();
    let result = recover_except(&state.config_path, &active);
    let mut inner = state.local_assistance.inner.lock().await;
    for task in inner
        .tasks
        .values_mut()
        .filter(|t| t.worker_stopped && t.phase != Phase::Cleared)
    {
        task.erase();
    }
    if !exiting {
        inner.cleaning = false;
    }
    result
}
