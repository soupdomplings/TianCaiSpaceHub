//! One ephemeral assistant conversation per Hub process. Model auth is owned by Hub.
pub(crate) mod model;
mod proxy;
pub(crate) mod settings;
mod telemetry;
pub(crate) mod tools;
mod web;
mod worker;

use crate::{
    app_state::SharedState,
    desktop::squad::{ExpectedAccount, Squad},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc, oneshot};

pub(crate) use web::router;
#[derive(Serialize)]
struct Message {
    id: String,
    role: &'static str,
    text: String,
    status: &'static str,
}
#[derive(Default)]
struct Inner {
    conversation: String,
    enabled: bool,
    logged_in: bool,
    config: Option<model::Configuration>,
    detail: String,
    cleanup_error: Option<String>,
    running: bool,
    process_active: bool,
    messages: Vec<Message>,
    bridge: Option<proxy::Bridge>,
}
type Reply = oneshot::Sender<Result<()>>;
enum Control {
    Settings(bool, Reply),
    Send(String, Reply),
    Stop(Reply),
    Clear(Reply),
    Shutdown(Reply),
    Config {
        generation: String,
        account: ExpectedAccount,
        authoritative: bool,
        result: Result<model::Configuration>,
    },
    Tool {
        epoch: String,
        turn: String,
        id: Value,
        result: Result<Value>,
    },
}
pub(crate) struct AssistantState {
    inner: Mutex<Inner>,
    tx: mpsc::UnboundedSender<Control>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Control>>>,
    shutting_down: AtomicBool,
    pub(crate) tools: Mutex<tools::ToolState>,
}
impl Default for AssistantState {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            inner: Mutex::new(Inner::default()),
            tx,
            rx: Mutex::new(Some(rx)),
            shutting_down: AtomicBool::new(false),
            tools: Mutex::new(Default::default()),
        }
    }
}
pub(crate) async fn service(state: &SharedState) -> Result<Arc<Squad>> {
    let config = state.config.lock().await.clone();
    state
        .desktop_squad
        .lock()
        .await
        .get(&state.config_path, &config)
}
async fn command(state: &SharedState, make: impl FnOnce(Reply) -> Control) -> Result<()> {
    ensure!(
        !state.assistant.shutting_down.load(Ordering::Acquire),
        "Hub 正在退出"
    );
    let (tx, rx) = oneshot::channel();
    state
        .assistant
        .tx
        .send(make(tx))
        .context("天才搭档后台尚未启动")?;
    rx.await.context("天才搭档后台已退出")?
}
pub(crate) async fn configure(state: &SharedState, enabled: bool) -> Result<()> {
    let _setup = state.local_assistance.setup.lock().await;
    command(state, |reply| Control::Settings(enabled, reply)).await
}
pub(crate) async fn send(state: &SharedState, text: String) -> Result<()> {
    ensure!(
        !text.trim().is_empty() && text.len() <= 64 * 1024,
        "请输入 64 KiB 内的问题"
    );
    command(state, |reply| Control::Send(text.trim().to_owned(), reply)).await
}
pub(crate) async fn stop(state: &SharedState) -> Result<()> {
    command(state, Control::Stop).await
}
pub(crate) async fn clear(state: &SharedState) -> Result<()> {
    command(state, Control::Clear).await
}
pub(crate) async fn has_active(state: &SharedState) -> bool {
    let inner = state.assistant.inner.lock().await;
    inner.process_active || inner.bridge.is_some()
}
tokio::task_local! {
    static TOOL_ACCOUNT: (String, ExpectedAccount);
}
pub(crate) async fn ensure_current_account(state: &SharedState) -> Result<()> {
    let (epoch, account) = TOOL_ACCOUNT
        .try_with(Clone::clone)
        .context("工具缺少当前会话身份")?;
    let bridge = state
        .assistant
        .inner
        .lock()
        .await
        .bridge
        .clone()
        .context("天才搭档会话已结束")?;
    bridge.ensure_account(&epoch, &account)
}
#[cfg(test)]
pub(crate) async fn test_tool_scope<F: std::future::Future>(
    state: &SharedState,
    future: F,
) -> F::Output {
    let bridge = {
        let mut inner = state.assistant.inner.lock().await;
        inner
            .bridge
            .get_or_insert_with(|| {
                // No requests are sent. Tests exercise the same epoch/account checks as production.
                let config = crate::config::AppConfig::default();
                let squad = Arc::new(Squad::new(&state.config_path, &config).unwrap());
                let account = squad
                    .assistant_test_identity("assistant-test-generation")
                    .unwrap();
                proxy::Bridge::new(
                    "assistant-test-epoch".into(),
                    "fixture-bridge-token".into(),
                    squad,
                    account,
                    "fixture-model".into(),
                )
            })
            .clone()
    };
    let scope = bridge.scope();
    TOOL_ACCOUNT.scope(scope, future).await
}
pub(crate) async fn status(state: &SharedState) -> Result<Value> {
    let inner = state.assistant.inner.lock().await;
    let enabled = inner.enabled;
    let mut value = json!({"enabled":enabled,"loggedIn":inner.logged_in,"conversationId":inner.conversation,
        "modelServiceReady":inner.config.is_some(),"model":inner.config.as_ref().map(|c|&c.model),
        "billing":{"payer":"administrator","chargeUser":false,"chargeOrganization":false},"notice":inner.config.as_ref().and_then(|c|c.notice.as_deref()),
        "detail":inner.cleanup_error.as_ref().unwrap_or(&inner.detail),"cleanupPending":inner.cleanup_error.is_some(),
        "running":inner.running,"messages":inner.messages});
    drop(inner);
    let runtime = crate::local_assistance::runtime::check(&state.config_path, None);
    let mut runtime = serde_json::to_value(runtime)?;
    let assistance_enabled = crate::local_assistance::settings::read(&state.config_path)?
        .0
        .enabled;
    runtime["canUninstall"] = json!(
        runtime["installed"].as_bool().unwrap_or(false)
            && !enabled
            && !assistance_enabled
            && !has_active(state).await
    );
    value["runtimeReady"] = runtime["ready"].clone();
    value["runtimeNeedsPreparation"] = runtime["needsPreparation"].clone();
    value["runtime"] = runtime;
    for (key, entry) in tools::public_status(state)
        .await
        .as_object()
        .into_iter()
        .flatten()
    {
        value[key] = entry.clone();
    }
    Ok(value)
}
pub(crate) async fn recover_runs(config_path: &Path) -> Result<()> {
    worker::recover(config_path).await
}
pub(crate) async fn shutdown_and_cleanup(state: &SharedState) {
    if state.assistant.shutting_down.swap(true, Ordering::AcqRel) {
        return;
    }
    // Cancel a stream immediately, including while a server config request is pending.
    if let Some(bridge) = &state.assistant.inner.lock().await.bridge {
        bridge.cancel();
    }
    tools::clear(state).await;
    let (tx, rx) = oneshot::channel();
    if state.assistant.tx.send(Control::Shutdown(tx)).is_ok() {
        let _ = tokio::time::timeout(Duration::from_secs(20), rx).await;
    }
}
async fn finish(state: &SharedState, status: &'static str, detail: impl Into<String>) {
    let detail = detail.into();
    let mut inner = state.assistant.inner.lock().await;
    inner.running = false;
    inner.detail = detail.clone();
    for message in inner.messages.iter_mut().filter(|m| m.status == "running") {
        message.status = status;
        if status == "error" && message.role == "assistant" && message.text.trim().is_empty() {
            message.text = detail.clone();
        }
    }
}
async fn close_worker(state: &SharedState, worker: &mut Option<worker::Worker>) -> Result<()> {
    if let Some(bridge) = state.assistant.inner.lock().await.bridge.take() {
        bridge.cancel();
    }
    tools::clear(state).await;
    if let Some(current) = worker.as_mut() {
        if let Err(error) = current.close().await {
            state.assistant.inner.lock().await.cleanup_error =
                Some(format!("临时后台尚未清理，请点击清空重试：{error}"));
            return Err(error);
        }
    }
    *worker = None;
    let mut inner = state.assistant.inner.lock().await;
    inner.process_active = false;
    inner.cleanup_error = None;
    Ok(())
}
fn fetch_config(
    state: &SharedState,
    squad: Arc<Squad>,
    account: ExpectedAccount,
    generation: String,
) -> tokio::task::JoinHandle<()> {
    let tx = state.assistant.tx.clone();
    tokio::spawn(async move {
        let (authoritative, result) = match squad.assistant_config(&account).await {
            Ok(value) => (true, model::Configuration::parse(value)),
            Err(error) => (false, Err(error)),
        };
        let _ = tx.send(Control::Config {
            generation,
            account,
            authoritative,
            result,
        });
    })
}
/// Parent owns spawning this once, after recovery and before serving desktop APIs.
pub(crate) async fn run(state: SharedState) {
    let Some(mut rx) = state.assistant.rx.lock().await.take() else {
        return;
    };
    {
        let mut inner = state.assistant.inner.lock().await;
        inner.conversation = uuid::Uuid::new_v4().to_string();
        match settings::read(&state.config_path) {
            Ok(settings) => {
                inner.enabled = settings.enabled;
                inner.detail = if settings.enabled {
                    "正在检查天才搭档"
                } else {
                    "天才搭档已停用"
                }
                .into();
            }
            Err(error) => inner.detail = error.to_string(),
        }
    }
    let (events_tx, mut events) = mpsc::channel::<(String, Result<Value>)>(64);
    let mut worker: Option<worker::Worker> = None;
    let mut account: Option<ExpectedAccount> = None;
    let mut fetch: Option<tokio::task::JoinHandle<()>> = None;
    let mut fetch_generation = String::new();
    let mut next_fetch = Instant::now();
    let mut start_allowed = true;
    let mut queued: Option<String> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_runtime = Instant::now() - Duration::from_secs(5);
    let mut runtime_ready = false;
    loop {
        tokio::select! {
            Some(control)=rx.recv()=>{
                match control {
                    Control::Shutdown(reply)=>{
                        if let Some(fetch)=fetch.take(){fetch.abort();}
                        let result=close_worker(&state,&mut worker).await;
                        finish(&state,"stopped","天才搭档已退出").await;
                        let _=reply.send(result);break;
                    }
                    Control::Settings(enabled,reply)=>{
                        let result=settings::write(&state.config_path,enabled);
                        if result.is_ok(){
                            state.assistant.inner.lock().await.enabled=enabled;
                            if let Some(fetch)=fetch.take(){fetch.abort();}
                            fetch_generation.clear();next_fetch=Instant::now();
                            close_worker(&state,&mut worker).await.ok();
                            queued=None;account=None;start_allowed=true;
                            let mut inner=state.assistant.inner.lock().await;
                            inner.config=None;inner.messages.clear();inner.running=false;
                            inner.conversation=uuid::Uuid::new_v4().to_string();
                            inner.detail=if enabled {"正在检查天才搭档"}else{"天才搭档已停用"}.into();
                        }
                        let _=reply.send(result);
                    }
                    Control::Clear(reply)=>{
                        queued=None;start_allowed=false;
                        let result=close_worker(&state,&mut worker).await;
                        if result.is_ok(){let mut inner=state.assistant.inner.lock().await;
                            inner.messages.clear();inner.conversation=uuid::Uuid::new_v4().to_string();}
                        finish(&state,"stopped",if result.is_ok(){"对话已清空，下次发送会建立新会话"}else{"临时会话尚未清理，请重试"}).await;
                        let _=reply.send(result);
                    }
                    Control::Stop(reply)=>{
                        queued=None;start_allowed=false;
                        tools::cancel_pending(&state).await;
                        let result=if let Some(worker)=&mut worker {worker.interrupt().await}else{Ok(())};
                        if let Some(bridge)=&state.assistant.inner.lock().await.bridge {bridge.cancel();}
                        if worker.as_ref().is_none_or(|w|!w.has_active_turn()) {finish(&state,"stopped","已停止").await;}
                        let _=reply.send(result);
                    }
                    Control::Send(text,reply)=>{
                        let valid={let inner=state.assistant.inner.lock().await;
                            if !inner.enabled {Err(anyhow::anyhow!("请先启用天才搭档"))}
                            else if let Some(error)=&inner.cleanup_error {Err(anyhow::anyhow!("{error}"))}
                            else if inner.running {Err(anyhow::anyhow!("请等待当前回复，或先停止"))}
                            else if inner.config.is_none(){Err(anyhow::anyhow!("{}",inner.detail))}
                            else if !runtime_ready{Err(anyhow::anyhow!("请先准备天才搭档 Server"))}else{Ok(())}};
                        if valid.is_ok(){
                            let mut inner=state.assistant.inner.lock().await;
                            inner.messages.push(Message{id:uuid::Uuid::new_v4().to_string(),role:"user",text:text.clone(),status:"completed"});
                            inner.messages.push(Message{id:uuid::Uuid::new_v4().to_string(),role:"assistant",text:String::new(),status:"running"});
                            inner.running=true;inner.detail="正在思考".into();queued=Some(text);start_allowed=true;
                        }
                        let _=reply.send(valid);
                    }
                    Control::Config{generation,account:expected,authoritative,result}=>{
                        if generation!=fetch_generation || account.as_ref()!=Some(&expected) || !state.assistant.inner.lock().await.enabled {continue;}
                        fetch=None;
                        next_fetch=Instant::now()+Duration::from_secs(60);
                        let changed=authoritative && {let inner=state.assistant.inner.lock().await;
                            result.as_ref().ok()!=inner.config.as_ref()};
                        if changed {
                            queued=None;close_worker(&state,&mut worker).await.ok();
                            let mut inner=state.assistant.inner.lock().await;
                            inner.running=false;inner.messages.clear();inner.conversation=uuid::Uuid::new_v4().to_string();
                        }
                        let mut inner=state.assistant.inner.lock().await;
                        match result {Ok(config)=>{inner.config=Some(config);if changed||!inner.running {inner.detail="模型已就绪".into();}}
                            Err(error)=>{if authoritative {inner.config=None;}inner.detail=error.to_string();}}
                    }
                    Control::Tool{epoch,turn,id,result}=>{
                        if let Some(worker)=&mut worker {if worker.epoch==epoch {if let Err(error)=worker.tool_result(&turn,id,result).await{
                            finish(&state,"error",error.to_string()).await;
                        }}}
                    }
                }
            }
            Some(event)=events.recv()=>{
                if worker.as_ref().is_none_or(|w|w.epoch!=event.0) {continue;}
                let outcome=worker.as_mut().unwrap().event(&state,event.1).await;
                if let Err(error)=outcome {
                    queued=None;start_allowed=false;
                    close_worker(&state,&mut worker).await.ok();
                    finish(&state,"error",error.to_string()).await;
                }
            }
            _=tick.tick()=>{
                if state.assistant.shutting_down.load(Ordering::Acquire){continue;}
                if last_runtime.elapsed()>=Duration::from_secs(5){
                    runtime_ready=crate::local_assistance::runtime::resolve_ready(&state.config_path).is_ok();
                    last_runtime=Instant::now();
                }
                let squad=match service(&state).await {Ok(s)=>s,Err(error)=>{finish(&state,"error",error.to_string()).await;continue;}};
                let current=squad.assistant_identity().unwrap_or_default();
                let enabled=state.assistant.inner.lock().await.enabled;
                if account!=current {
                    if let Some(fetch)=fetch.take(){fetch.abort();}
                    fetch_generation.clear();next_fetch=Instant::now();
                    close_worker(&state,&mut worker).await.ok();
                    queued=None;start_allowed=true;account=current;
                    let mut inner=state.assistant.inner.lock().await;
                    inner.logged_in=account.is_some();inner.config=None;inner.messages.clear();inner.running=false;
                    inner.conversation=uuid::Uuid::new_v4().to_string();
                    inner.detail=if !enabled {"天才搭档已停用"}else if account.is_none(){"请先登录天才小分队"}else{"正在读取管理员模型配置"}.into();
                } else if !enabled { continue; }
                if enabled && fetch.is_none() && Instant::now()>=next_fetch {
                    if let Some(account)=&account {
                        fetch_generation=uuid::Uuid::new_v4().to_string();
                        fetch=Some(fetch_config(&state,squad,account.clone(),fetch_generation.clone()));
                    }
                }
                if account.is_none(){state.assistant.inner.lock().await.detail="请先登录天才小分队".into();}
                if let Some(current_worker)=&worker {if let Err(error)=current_worker.check_timeout(){
                    queued=None;start_allowed=false;let detail=error.to_string();
                    close_worker(&state,&mut worker).await.ok();finish(&state,"error",detail).await;
                }}
            }
        }
        if state.assistant.shutting_down.load(Ordering::Acquire) {
            continue;
        }
        if worker.is_none() && start_allowed && runtime_ready {
            let config = state.assistant.inner.lock().await.config.clone();
            if let (Some(config), Some(account)) = (config, account.clone()) {
                let result = async {
                    let squad = service(&state).await?;
                    let ready =
                        crate::local_assistance::runtime::resolve_ready(&state.config_path)?;
                    worker::Worker::start(&state, ready, squad, account, config, events_tx.clone())
                        .await
                }
                .await;
                match result {
                    Ok(started) => worker = Some(started),
                    Err(error) => {
                        start_allowed = false;
                        queued = None;
                        finish(&state, "error", error.to_string()).await;
                    }
                }
            }
        }
        if worker.as_ref().is_some_and(|w| w.ready()) {
            if let Some(text) = queued.take() {
                if let Err(error) = worker.as_mut().unwrap().turn(&text).await {
                    finish(&state, "error", error.to_string()).await;
                }
            }
        }
    }
}
