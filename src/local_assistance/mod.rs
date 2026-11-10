//! TianGong owns the interaction; isolated Codex app-server owns only subtasks.
mod binary;
mod catalogue;
mod host;
mod process;
mod protocol;
mod records;
pub mod settings;
#[cfg(test)]
mod tests;
pub mod web;
mod worker;

use crate::app_state::SharedState;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, mpsc, watch};

pub const MAX_TASKS: usize = 64;
pub const MAX_TEXT: usize = 64 * 1024;
pub const MAX_WIRE: usize = 4 * 1024 * 1024;
#[derive(Default)]
pub struct Service {
    pub inner: Mutex<Inner>,
    pub changed: Notify,
    pub setup: Mutex<()>,
}
#[derive(Default)]
pub struct Inner {
    pub host: Option<String>,
    pub host_detail: String,
    pub trigger_state: String,
    pub contexts: HashMap<String, Parent>,
    pub tasks: HashMap<String, Task>,
    pub admitted: HashMap<(String, String), (String, String)>,
}
#[derive(Clone)]
pub struct Parent {
    pub host: String,
    pub session: String,
    pub request: String,
    pub run: Option<String>,
    pub cwd: PathBuf,
    pub query: String,
    pub writable: bool,
    pub at: Instant,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Running,
    WaitingUser,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
    Stopped,
    Cleared,
}
impl Phase {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Stopped | Self::Cleared
        )
    }
}
pub struct Task {
    pub id: String,
    pub context: String,
    pub host: String,
    pub session: String,
    pub cwd: PathBuf,
    pub goal: String,
    pub key: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub route: String,
    pub route_fingerprint: String,
    pub api_token: String,
    pub auto_clear: bool,
    pub phase: Phase,
    pub detail: String,
    pub result: String,
    pub thread: Option<String>,
    pub turn: Option<String>,
    pub turns: u8,
    pub pending: Vec<Prompt>,
    pub progress: Vec<String>,
    pub files: Vec<String>,
    pub commands: Vec<Value>,
    pub at: Instant,
    pub updated: Instant,
    pub controller: mpsc::Sender<Control>,
    pub worker_stopped: bool,
    pub closing: bool,
    pub delete_requested: bool,
    pub process_pid: Option<u32>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub nonce: String,
    pub request_id: Value,
    pub method: String,
    pub params: Value,
    pub at_ms: u128,
}
pub enum Control {
    Cancel,
    Finish,
    Followup(String),
    Decision { nonce: String, response: Value },
    Delete,
}

impl Task {
    fn erase(&mut self) {
        self.api_token.clear();
        self.result.clear();
        self.pending.clear();
        self.progress.clear();
        self.files.clear();
        self.commands.clear();
        self.closing = true;
        self.bump(Phase::Cleared, "协助对话已清理，天工中的结果保留");
    }
    pub fn view(&self, details: bool) -> Value {
        let mut v = json!({"id":self.id,"session":self.session,"phase":self.phase,"detail":self.detail,
            "title":self.goal.chars().take(72).collect::<String>(),
            "model":self.model,"reasoningEffort":self.reasoning_effort,"turns":self.turns,"workerStopped":self.worker_stopped,"closing":self.closing,
            "elapsedSeconds":self.at.elapsed().as_secs(),
            "progress":self.progress,"files":self.files,"commands":self.commands});
        if details {
            v["result"] = self.result.clone().into();
            v["pending"] = json!(self.pending);
        }
        v
    }
    pub fn bump(&mut self, phase: Phase, detail: impl Into<String>) {
        self.phase = phase;
        self.detail = detail.into();
        self.updated = Instant::now();
    }
    pub fn progress(&mut self, message: impl Into<String>) {
        let s = message.into();
        if self.progress.last() != Some(&s) {
            self.progress.push(s);
        }
        if self.progress.len() > 16 {
            self.progress.remove(0);
        }
        self.updated = Instant::now();
    }
}

struct Work {
    id: String,
    parent: Parent,
    settings: settings::Settings,
    writable: bool,
    control: mpsc::Receiver<Control>,
}
pub async fn admit(
    state: SharedState,
    context: &str,
    goal: &str,
    key: &str,
    analysis_only: bool,
) -> Result<Value> {
    let (view, work) = reserve(&state, context, goal, key, analysis_only).await?;
    if let Some(work) = work {
        tokio::spawn(async move {
            worker::run(
                state,
                work.id,
                work.parent,
                work.settings,
                work.writable,
                work.control,
            )
            .await;
        });
    }
    Ok(view)
}
async fn reserve(
    state: &SharedState,
    context: &str,
    goal: &str,
    key: &str,
    analysis_only: bool,
) -> Result<(Value, Option<Work>)> {
    ensure!(
        !goal.trim().is_empty() && goal.len() <= MAX_TEXT && !key.is_empty() && key.len() <= 128,
        "协助目标或问题标识无效"
    );
    let (settings, _) = settings::read(&state.config_path)?;
    let config = state.config.lock().await.clone();
    settings::validate(&config, &settings)?;
    ensure!(settings.enabled, "本机协助已停用");
    let provider = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| {
            p.enabled
                && !p.is_client_reserved()
                && crate::ai_gateway::config::provider_route_id(p) == settings.route
                && p.matches_model(&settings.model)
        })
        .context("协助来源已变化")?;
    let fingerprint = settings::source_fingerprint(provider)?;
    let reasoning_effort = settings::resolved_effort(&config, &settings)?;
    let mut inner = state.local_assistance.inner.lock().await;
    let parent = inner
        .contexts
        .get(context)
        .cloned()
        .context("天工任务上下文已失效，请在天工重新开始本次任务")?;
    ensure!(
        inner.host.as_deref() == Some(&parent.host)
            && parent.at.elapsed() < Duration::from_secs(7200),
        "天工连接已变化，请重新开始任务"
    );
    if let Some(task) = inner
        .tasks
        .values()
        .find(|t| t.context == context && t.key == key)
    {
        ensure!(
            task.goal == goal,
            "同一问题标识已用于不同目标，请读取原任务，不能重复执行"
        );
        return Ok((task.view(true), None));
    }
    let ledger_key = (context.to_string(), key.to_string());
    let goal_fingerprint = crate::desktop::transaction::fingerprint(Some(goal.as_bytes()));
    if let Some((original, id)) = inner.admitted.get(&ledger_key) {
        ensure!(*original == goal_fingerprint, "同一问题标识已用于不同目标");
        return Ok((
            json!({"id":id,"phase":"cleared","workerStopped":true,"detail":"该问题已执行并清理，不会重复执行"}),
            None,
        ));
    }
    // Stopped summaries may leave memory; idempotency tombstones remain for this
    // context's lifetime, so a repeated problem cannot silently run again.
    if inner.tasks.len() >= MAX_TASKS {
        let oldest = inner
            .tasks
            .values()
            .filter(|t| t.worker_stopped && t.phase.terminal())
            .min_by_key(|t| t.updated)
            .map(|t| t.id.clone());
        if let Some(id) = oldest {
            inner.tasks.remove(&id);
        }
    }
    ensure!(
        inner.tasks.len() < MAX_TASKS,
        "协助记录已满，请等待后台退出后重试"
    );
    ensure!(
        inner.admitted.keys().filter(|(c, _)| c == context).count() < 64,
        "本次天工任务协助次数已达上限，请结束后重新开始任务"
    );
    ensure!(
        inner.tasks.values().filter(|t| !t.worker_stopped).count() < 3,
        "已有三项协助会话，请先采纳结果并结束其中一项"
    );
    ensure!(!inner.tasks.values().any(|t|busy(t) && (t.session==parent.session || paths_overlap(&t.cwd,&parent.cwd))),
        "此任务或项目已有协助正在处理，请等待原任务，避免同时修改文件");
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel(16);
    let writable = parent.writable && settings.allow_workspace_write && !analysis_only;
    let now = Instant::now();
    inner
        .admitted
        .insert(ledger_key, (goal_fingerprint, id.clone()));
    inner.tasks.insert(
        id.clone(),
        Task {
            id: id.clone(),
            context: context.into(),
            host: parent.host.clone(),
            session: parent.session.clone(),
            cwd: parent.cwd.clone(),
            goal: goal.into(),
            key: key.into(),
            model: settings.model.clone(),
            reasoning_effort,
            route: settings.route.clone(),
            route_fingerprint: fingerprint,
            api_token: settings::token(),
            auto_clear: settings.auto_clear,
            phase: Phase::Starting,
            detail: "正在准备本机协助".into(),
            result: String::new(),
            thread: None,
            turn: None,
            turns: 1,
            pending: vec![],
            progress: vec![],
            files: vec![],
            commands: vec![],
            at: now,
            updated: now,
            controller: tx,
            worker_stopped: false,
            closing: false,
            delete_requested: false,
            process_pid: None,
        },
    );
    let view = inner.tasks[&id].view(true);
    drop(inner);
    Ok((
        view,
        Some(Work {
            id,
            parent,
            settings,
            writable,
            control: rx,
        }),
    ))
}
pub fn paths_overlap(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        let a = PathBuf::from(a.to_string_lossy().to_lowercase());
        let b = PathBuf::from(b.to_string_lossy().to_lowercase());
        return a.starts_with(&b) || b.starts_with(&a);
    }
    #[cfg(not(windows))]
    {
        a.starts_with(b) || b.starts_with(a)
    }
}
fn busy(t: &Task) -> bool {
    !t.worker_stopped && (!t.phase.terminal() || t.closing)
}
pub(super) fn live(inner: &Inner, task: &Task) -> bool {
    inner.host.as_deref() == Some(task.host.as_str())
        && inner
            .contexts
            .get(&task.context)
            .is_some_and(|p| p.host == task.host && p.at.elapsed() < Duration::from_secs(7200))
        && !task.closing
}
pub(super) async fn ensure_live(state: &SharedState, id: &str) -> Result<()> {
    let (settings, _) = settings::read(&state.config_path)?;
    ensure!(settings.enabled, "本机协助已停用");
    let inner = state.local_assistance.inner.lock().await;
    let t = inner.tasks.get(id).context("协助任务已结束")?;
    ensure!(
        live(&inner, t),
        "天工任务已停止或连接已变化，后台协助已取消"
    );
    Ok(())
}
pub async fn task_view(state: &SharedState, context: &str, id: &str) -> Result<Value> {
    let inner = state.local_assistance.inner.lock().await;
    let task = inner.tasks.get(id).context("协助任务不存在或已清理")?;
    ensure!(task.context == context, "此协助不属于当前天工任务");
    Ok(task.view(true))
}
pub async fn control(
    state: &SharedState,
    context: Option<&str>,
    id: &str,
    control: Control,
) -> Result<Value> {
    let mut inner = state.local_assistance.inner.lock().await;
    let task = inner.tasks.get(id).context("协助任务不存在或已清理")?;
    if let Some(context) = context {
        ensure!(task.context == context, "此协助不属于当前天工任务");
    }
    if let Control::Followup(text) = &control {
        ensure!(
            live(&inner, task)
                && task.phase == Phase::Completed
                && !task.worker_stopped
                && task.turns < 3,
            "协助已结束或已达到追问上限，请采纳当前结果"
        );
        ensure!(
            !text.trim().is_empty() && text.len() <= MAX_TEXT,
            "追问内容无效"
        );
        ensure!(
            !inner.tasks.values().any(|other| other.id != id
                && busy(other)
                && (other.session == task.session || paths_overlap(&other.cwd, &task.cwd))),
            "此项目已有新的协助任务，请采纳当前结果，不能并发追问旧任务"
        );
    }
    if task.worker_stopped {
        if matches!(control, Control::Delete) {
            worker::remove_owned(&state.config_path, id)?;
            let t = inner.tasks.get_mut(id).unwrap();
            t.erase();
            return Ok(t.view(true));
        }
        return Ok(task.view(true));
    }
    if task.closing {
        let task = inner.tasks.get_mut(id).unwrap();
        if matches!(control, Control::Delete) {
            task.delete_requested = true;
        }
        return Ok(task.view(true));
    }
    let tx = task.controller.clone();
    let permit = tx
        .try_reserve()
        .map_err(|_| anyhow::anyhow!("协助正在处理上一项操作，请稍后重试"))?;
    let task = inner.tasks.get_mut(id).unwrap();
    match &control {
        Control::Followup(_) => task.bump(Phase::Starting, "正在准备补充处理"),
        Control::Cancel | Control::Delete | Control::Finish => {
            task.closing = true;
            task.delete_requested = matches!(control, Control::Delete);
            task.pending.clear();
            if !task.phase.terminal() {
                task.bump(Phase::Cancelling, "正在停止协助");
            }
        }
        Control::Decision { .. } => {}
    }
    permit.send(control);
    Ok(task.view(true))
}
pub async fn disconnect(state: &SharedState, host: &str) {
    let mut inner = state.local_assistance.inner.lock().await;
    inner.contexts.retain(|_, p| p.host != host);
    if inner.host.as_deref() == Some(host) {
        inner.host = None;
        inner.host_detail = "天工协助连接中断；正在停止后台任务".into();
        inner.trigger_state = "disconnected".into();
    }
    for task in inner
        .tasks
        .values_mut()
        .filter(|t| t.host == host && !t.worker_stopped)
    {
        task.closing = true;
        task.pending.clear();
        if !task.phase.terminal() {
            task.bump(Phase::Cancelling, "天工连接已变化，正在停止协助");
        }
        let _ = task.controller.try_send(Control::Cancel);
    }
}
pub async fn run_monitor(state: SharedState, shutdown: watch::Receiver<bool>) {
    tokio::join!(host::monitor(state.clone(), shutdown.clone()), host::catalogue_monitor(state, shutdown));
}
