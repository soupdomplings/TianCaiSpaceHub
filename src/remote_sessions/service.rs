use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::sync::{Mutex, broadcast};

use super::{
    codex::CodexTarget,
    types::{RemoteEvent, Scope, busy},
};

const EVENT_WINDOW: usize = 512;

// Only target thread/list responses populate this in-memory index. A display
// name may change; the connection id, epoch and native stream must still match.
#[derive(Default)]
pub(super) struct ListedWorkspaces(HashMap<String, (CodexTarget, HashSet<PathBuf>)>);
impl ListedWorkspaces {
    fn same_source(a: &CodexTarget, b: &CodexTarget) -> bool {
        a.id == b.id
            && a.epoch == b.epoch
            && a.key == b.key
            && a.client_id == b.client_id
            && a.stream_id == b.stream_id
    }
    pub fn keep_sources(&mut self, targets: &[CodexTarget]) {
        self.0
            .retain(|_, (source, _)| targets.iter().any(|t| Self::same_source(t, source)));
    }
    pub fn remember<'a>(&mut self, target: &CodexTarget, paths: impl Iterator<Item = &'a str>) {
        let (current, listed) = self
            .0
            .entry(target.id.clone())
            .or_insert_with(|| (target.clone(), HashSet::new()));
        if target.epoch < current.epoch {
            return;
        }
        if !Self::same_source(current, target) {
            *current = target.clone();
            listed.clear();
        }
        listed.extend(
            paths
                .map(Path::new)
                .filter(|p| p.is_absolute())
                .map(Path::to_owned),
        );
    }
    pub fn selected(&self, target: &CodexTarget, cwd: &Path) -> Option<&Path> {
        let (current, listed) = self.0.get(&target.id)?;
        Self::same_source(current, target)
            .then(|| listed.get(cwd))
            .flatten()
            .map(PathBuf::as_path)
    }
}

#[derive(Clone)]
pub(super) enum ApprovalSource {
    Codex {
        target: CodexTarget,
        request_id: Value,
        method: String,
        params: Value,
    },
    Tiangong {
        run_id: String,
        call_ids: Vec<String>,
        authorization_identity: String,
    },
}
#[derive(Clone)]
pub(super) struct PendingApproval {
    pub public: Value,
    pub source: ApprovalSource,
    pub claimed: bool,
}
pub(super) struct Feed {
    pub events: VecDeque<RemoteEvent>,
    pub tx: broadcast::Sender<RemoteEvent>,
    pub status: String,
    pub turn_id: Option<String>,
    pub source: Option<CodexTarget>,
    pub session_cwd: Option<PathBuf>,
    pub approvals: HashMap<String, PendingApproval>,
    pub requests: VecDeque<(String, Value)>,
    pub observer_running: bool,
    pub subscribed: bool,
    /// A confirmed thread/start on this live connection; its rollout may not
    /// exist until the first turn. Never infer this state from a read error.
    pub awaiting_first_turn: bool,
}
impl Default for Feed {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(EVENT_WINDOW);
        Self {
            events: VecDeque::new(),
            tx,
            status: "idle".into(),
            turn_id: None,
            source: None,
            session_cwd: None,
            approvals: HashMap::new(),
            requests: VecDeque::new(),
            observer_running: false,
            subscribed: false,
            awaiting_first_turn: false,
        }
    }
}
pub(crate) struct SessionService {
    pub(super) feeds: Mutex<HashMap<Scope, Feed>>,
    pub(super) listed_workspaces: Mutex<ListedWorkspaces>,
    next_event: AtomicU64,
    pub(super) file_gate: Mutex<()>,
    // Pending JSON-RPC requests are not message history. Keep only the small
    // set still awaiting a native decision so opening the page later can
    // present the original request ID, rather than inventing a replacement.
    pending_native: Mutex<VecDeque<(Scope, String, PendingApproval)>>,
}
impl Default for SessionService {
    fn default() -> Self {
        Self {
            feeds: Mutex::new(HashMap::new()),
            listed_workspaces: Mutex::default(),
            next_event: AtomicU64::new((crate::types::now_ms() as u64).saturating_mul(1000)),
            file_gate: Mutex::new(()),
            pending_native: Mutex::new(VecDeque::new()),
        }
    }
}
impl SessionService {
    pub(super) async fn watch(&self, scope: &Scope, source: Option<CodexTarget>) {
        let pending = self
            .pending_native
            .lock()
            .await
            .iter()
            .filter(|(s, _, p)| {
                s == scope
                    && match (&source, &p.source) {
                        (
                            Some(target),
                            ApprovalSource::Codex {
                                target: original, ..
                            },
                        ) => target == original,
                        _ => false,
                    }
            })
            .map(|(_, id, p)| (id.clone(), p.clone()))
            .collect::<Vec<_>>();
        let mut feeds = self.feeds.lock().await;
        let feed = feeds.entry(scope.clone()).or_default();
        if let Some(source) = source {
            if feed.source.as_ref().is_some_and(|old| old != &source) {
                feed.events.clear();
                feed.approvals.clear();
                feed.status = "idle".into();
                feed.turn_id = None;
                feed.subscribed = false;
                feed.session_cwd = None;
                feed.awaiting_first_turn = false;
            }
            feed.source = Some(source);
        }
        for (id, pending) in pending {
            feed.approvals.entry(id).or_insert(pending);
        }
        if !feed.approvals.is_empty() {
            feed.status = "waiting_approval".into();
        }
    }
    pub(super) async fn publish(
        &self,
        scope: &Scope,
        kind: &str,
        item_id: Option<String>,
        data: Value,
    ) {
        let mut feeds = self.feeds.lock().await;
        let feed = feeds.entry(scope.clone()).or_default();
        if kind == "turn.updated" {
            if let Some(status) = data["status"].as_str() {
                feed.status = status.into();
            }
            if let Some(turn) = data["turnId"].as_str() {
                feed.turn_id = Some(turn.into());
            }
            if !busy(&feed.status) {
                feed.approvals.clear();
            }
        }
        let event = RemoteEvent {
            id: self.next_event.fetch_add(1, Ordering::Relaxed),
            kind: kind.into(),
            client_id: scope.client.clone(),
            session_id: scope.session.clone(),
            turn_id: feed.turn_id.clone(),
            item_id,
            data,
        };
        if feed.events.len() >= EVENT_WINDOW {
            feed.events.pop_front();
        }
        feed.events.push_back(event.clone());
        let _ = feed.tx.send(event);
    }
    pub(super) async fn snapshot(
        &self,
        scope: &Scope,
    ) -> (String, Option<String>, Vec<Value>, u64) {
        let feeds = self.feeds.lock().await;
        feeds
            .get(scope)
            .map(|f| {
                (
                    f.status.clone(),
                    f.turn_id.clone(),
                    f.approvals.values().map(|p| p.public.clone()).collect(),
                    f.events.back().map_or(0, |e| e.id),
                )
            })
            .unwrap_or_else(|| ("idle".into(), None, Vec::new(), 0))
    }
    pub(super) async fn reserve(&self, scope: &Scope, request_id: &str) -> Result<Option<Value>> {
        ensure!(
            !request_id.is_empty() && request_id.len() <= 256,
            "发送请求缺少有效标识"
        );
        let mut feeds = self.feeds.lock().await;
        let feed = feeds.entry(scope.clone()).or_default();
        if let Some((_, response)) = feed.requests.iter().find(|(id, _)| id == request_id) {
            return Ok(Some(response.clone()));
        }
        ensure!(
            !busy(&feed.status),
            "当前会话仍在执行或等待确认，请先完成或停止"
        );
        feed.status = "starting".into();
        // Remember admission before the native write: an ambiguous transport
        // failure must never make a repeated browser request execute twice.
        if feed.requests.len() >= 128 {
            feed.requests.pop_front();
        }
        feed.requests
            .push_back((request_id.into(), json!({"status":"unknown"})));
        Ok(None)
    }
    pub(super) async fn receipt(&self, scope: &Scope, request_id: &str, response: Value) {
        if let Some(feed) = self.feeds.lock().await.get_mut(scope) {
            if let Some((_, value)) = feed.requests.iter_mut().find(|(id, _)| id == request_id) {
                *value = response;
            }
        }
    }
    pub(super) async fn add_approval(
        &self,
        scope: &Scope,
        id: String,
        public: Value,
        source: ApprovalSource,
    ) {
        {
            let mut feeds = self.feeds.lock().await;
            let approvals = &mut feeds.entry(scope.clone()).or_default().approvals;
            if approvals.contains_key(&id) {
                return;
            }
            approvals.insert(
                id,
                PendingApproval {
                    public: public.clone(),
                    source,
                    claimed: false,
                },
            );
        }
        self.publish(
            scope,
            "approval.requested",
            None,
            json!({"approval":public}),
        )
        .await;
        self.publish(
            scope,
            "turn.updated",
            None,
            json!({"status":"waiting_approval"}),
        )
        .await;
    }
    pub(super) async fn claim_approval(&self, scope: &Scope, id: &str) -> Result<PendingApproval> {
        let mut feeds = self.feeds.lock().await;
        let pending = feeds
            .get_mut(scope)
            .and_then(|f| f.approvals.get_mut(id))
            .ok_or_else(|| anyhow::anyhow!("这项确认已处理或已失效，请刷新会话"))?;
        ensure!(!pending.claimed, "这项确认正在提交，请勿重复操作");
        pending.claimed = true;
        Ok(pending.clone())
    }
    pub(super) async fn resolve_approval(&self, scope: &Scope, id: &str) {
        self.pending_native
            .lock()
            .await
            .retain(|(s, key, _)| s != scope || key != id);
        if let Some(feed) = self.feeds.lock().await.get_mut(scope) {
            feed.approvals.remove(id);
        }
        self.publish(scope, "approval.resolved", None, json!({"id":id}))
            .await;
    }
    pub(super) async fn remember_native(&self, scope: Scope, id: String, pending: PendingApproval) {
        let mut requests = self.pending_native.lock().await;
        requests.retain(|(s, key, _)| s != &scope || key != &id);
        if requests.len() >= 128 {
            requests.pop_front();
        }
        requests.push_back((scope, id, pending));
    }
    pub(super) async fn resolve_native(
        &self,
        epoch: u64,
        client: &str,
        stream: &str,
        request_id: &Value,
    ) {
        let scopes = self
            .pending_native
            .lock()
            .await
            .iter()
            .filter_map(|(scope, id, p)| match &p.source {
                ApprovalSource::Codex {
                    target,
                    request_id: original,
                    ..
                } if target.epoch == epoch
                    && target.client_id == client
                    && target.stream_id == stream
                    && original == request_id =>
                {
                    Some((scope.clone(), id.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (scope, id) in scopes {
            self.resolve_approval(&scope, &id).await;
        }
    }
    pub(super) async fn clear_native(&self, epoch: u64, client: &str, stream: &str, session: &str) {
        self.pending_native.lock().await.retain(|(scope, _, p)| {
            !(scope.session == session
                && match &p.source {
                    ApprovalSource::Codex { target, .. } => {
                        target.epoch == epoch
                            && target.client_id == client
                            && target.stream_id == stream
                    }
                    _ => false,
                })
        });
    }
}
