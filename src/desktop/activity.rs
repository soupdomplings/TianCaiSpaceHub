//! In-flight evidence is independent of persisted request logging.
use super::clients::ClientId;
use crate::app_state::SharedState;
use axum::{
    body::Body,
    extract::{Request, State},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use futures_util::StreamExt;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const INTERNAL_HEADER: &str = "x-tiancai-internal-activity";
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub client: Option<ClientId>,
    /// Set by a trusted Hub entry, never inferred from caller headers.
    pub source: Option<&'static str>,
    pub entry: Option<String>,
    pub model: Option<String>,
    pub channel: Option<String>,
    pub route: Option<String>,
    pub log_id: Option<i64>,
    pub started_at: i64,
    pub last_evidence_at: i64,
    pub phase: String,
}
struct Entry {
    value: Activity,
    touched: Instant,
}
#[derive(Default)]
struct Evidence {
    active: BTreeMap<String, Entry>,
    completed: std::collections::VecDeque<(Instant, Activity)>,
}
#[derive(Clone, Default)]
pub struct ActivityStore(Arc<Mutex<Evidence>>);
impl ActivityStore {
    pub(crate) fn begin(&self, path: &str) -> Guard {
        let id = uuid::Uuid::new_v4().to_string();
        let (client, entry) = identity(path);
        let now = crate::ai_gateway::request_log::now_ms();
        let value = Activity {
            id: id.clone(),
            client,
            source: None,
            entry,
            model: None,
            channel: None,
            route: None,
            log_id: None,
            started_at: now,
            last_evidence_at: now,
            phase: "routing".into(),
        };
        if let Ok(mut evidence) = self.0.lock() {
            let rows = &mut evidence.active;
            if rows.len() < 4096 {
                rows.insert(
                    id.clone(),
                    Entry {
                        value,
                        touched: Instant::now(),
                    },
                );
            }
        }
        Guard {
            store: self.clone(),
            id,
        }
    }
    pub(crate) fn begin_local_assistance(&self) -> Guard {
        let guard = self.begin("");
        self.update(&guard.id, |activity| {
            activity.client = Some(ClientId::Tiangong);
            activity.source = Some(super::request_source::LOCAL_ASSISTANCE_MCP);
        });
        guard
    }
    pub(crate) fn begin_assistant(&self) -> Guard {
        let guard = self.begin("");
        self.update(&guard.id, |activity| {
            activity.source = Some(super::request_source::GENIUS_BUDDY);
        });
        guard
    }
    pub fn annotate(&self, id: &str, model: &str, channel: &str, route: &str) {
        self.update(id, |v| {
            v.model = Some(model.into());
            v.channel = Some(channel.into());
            v.route = Some(route.into());
            v.phase = "upstream".into();
        });
    }
    pub fn log(&self, id: &str, log_id: i64) {
        self.update(id, |v| v.log_id = Some(log_id));
    }
    pub(crate) fn touch(&self, id: &str) {
        self.update(id, |v| v.phase = "streaming".into());
    }
    pub fn identify(&self, id: &str, client: ClientId, entry: &str) {
        self.update(id, |v| {
            v.client = Some(client);
            if !entry.is_empty() {
                v.entry = Some(entry.into());
            }
        });
    }
    pub(crate) fn identify_headers(&self, id: &str, headers: &axum::http::HeaderMap) {
        // Dedicated URL/config identities take priority. Header evidence only
        // labels observation; it never authorizes or changes model routing.
        if self.get(id).is_some_and(|v| v.client.is_some()) {
            return;
        }
        if let Some(client) = header_identity(headers) {
            self.identify(id, client, "");
        }
    }
    fn update(&self, id: &str, change: impl FnOnce(&mut Activity)) {
        if let Ok(mut evidence) = self.0.lock() {
            if let Some(row) = evidence.active.get_mut(id) {
                change(&mut row.value);
                row.value.last_evidence_at = crate::ai_gateway::request_log::now_ms();
                row.touched = Instant::now();
            }
        }
    }
    pub fn get(&self, id: &str) -> Option<Activity> {
        self.0.lock().ok()?.active.get(id).map(|e| e.value.clone())
    }
    pub fn snapshot(&self) -> Vec<Activity> {
        self.0
            .lock()
            .map(|evidence| {
                evidence
                    .active
                    .values()
                    .map(|r| {
                        let mut v = r.value.clone();
                        if r.touched.elapsed() > Duration::from_secs(5) {
                            v.phase = "waiting".into();
                        }
                        v
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Short-lived display evidence only; never written to the request log or
    /// counted as an active request. Allows a graph poll to see fast requests.
    pub fn recently_completed(&self) -> Vec<Activity> {
        let Ok(mut evidence) = self.0.lock() else {
            return vec![];
        };
        while evidence
            .completed
            .front()
            .is_some_and(|(at, _)| at.elapsed() > Duration::from_secs(5))
        {
            evidence.completed.pop_front();
        }
        evidence.completed.iter().map(|(_, v)| v.clone()).collect()
    }
}
pub(crate) struct Guard {
    store: ActivityStore,
    pub(crate) id: String,
}
impl Guard {
    /// The model response body owns both ordinary and managed request evidence.
    /// Dropping a cancelled stream ends the activity just like reading its end.
    pub(crate) fn response(self, response: Response) -> Response {
        self.store.update(&self.id, |v| {
            v.phase = if response.status().is_success() {
                "streaming"
            } else {
                "failed"
            }
            .into()
        });
        let (parts, body) = response.into_parts();
        let stream = futures_util::stream::unfold(
            (body.into_data_stream(), self),
            |(mut body, guard)| async move {
                match body.next().await {
                    Some(chunk) => {
                        guard.store.update(&guard.id, |_| {});
                        Some((chunk, (body, guard)))
                    }
                    None => None,
                }
            },
        );
        Response::from_parts(parts, Body::from_stream(stream))
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut evidence) = self.store.0.lock() {
            if let Some(mut row) = evidence.active.remove(&self.id) {
                row.value.phase = "ended".into();
                row.value.last_evidence_at = crate::ai_gateway::request_log::now_ms();
                evidence.completed.push_back((Instant::now(), row.value));
                while evidence.completed.len() > 256 {
                    evidence.completed.pop_front();
                }
            }
        }
    }
}
fn identity(path: &str) -> (Option<ClientId>, Option<String>) {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    if let Some(i) = parts.iter().position(|p| *p == "clients") {
        let client = parts
            .get(i + 1)
            .and_then(|key| ClientId::ALL.into_iter().find(|c| c.key() == *key));
        return (
            client,
            parts
                .get(i + 2)
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                .map(|s| s.to_string()),
        );
    }
    for (key, client) in [
        ("workbuddy", ClientId::WorkBuddy),
        ("gmclaw", ClientId::Tiangong),
    ] {
        if let Some(i) = parts.iter().position(|p| *p == key) {
            return (
                Some(client),
                parts
                    .get(i + 1)
                    .filter(|v| **v != "v1")
                    .map(|s| s.to_string()),
            );
        }
    }
    (None, None)
}

fn header_identity(headers: &axum::http::HeaderMap) -> Option<ClientId> {
    let text = |key: &str| {
        headers
            .get(key)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let agent = text("user-agent");
    let originator = text("originator");
    if agent.starts_with("codex")
        || agent.starts_with("chatgpt")
        || matches!(
            originator.as_str(),
            "codex_cli_rs" | "codex_desktop_rs" | "codex_vscode"
        )
        || headers.contains_key("x-codex-turn-metadata")
        || headers.contains_key("x-codex-window-id")
    {
        Some(ClientId::Codex)
    } else if agent.starts_with("workbuddy") || headers.contains_key("x-workbuddy-session-id") {
        Some(ClientId::WorkBuddy)
    } else {
        None
    }
}
pub async fn track(State(state): State<SharedState>, mut request: Request, next: Next) -> Response {
    request.headers_mut().remove(INTERNAL_HEADER);
    let path = request.uri().path();
    if !(path.ends_with("/responses")
        || path.ends_with("/responses/compact")
        || path.ends_with("/completions")
        || path.ends_with("/messages")
        || path.contains("/images/")
        || path.ends_with("/alpha/search"))
        || request.method() != axum::http::Method::POST
    {
        return next.run(request).await;
    }
    let guard = state.desktop_activity.begin(path);
    let id = guard.id.clone();
    state
        .desktop_activity
        .identify_headers(&id, request.headers());
    request.headers_mut().insert(
        INTERNAL_HEADER,
        HeaderValue::from_str(&id).expect("UUID header"),
    );
    let response = next.run(request).await;
    guard.response(response)
}

#[cfg(test)]
#[path = "activity/tests.rs"]
mod live_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_is_owned_by_body_lifetime_and_keeps_equal_models_separate() {
        let store = ActivityStore::default();
        let a =
            store.begin("/clients/vscode/00000000-0000-0000-0000-000000000001/v1/chat/completions");
        let b = store.begin("/gmclaw/entry-b/v1/chat/completions");
        store.annotate(&a.id, "same", "a", "route-a");
        store.annotate(&b.id, "same", "b", "route-b");
        assert_eq!(store.snapshot().len(), 2);
        drop(a);
        let rows = store.snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].route.as_deref(), Some("route-b"));
        let recent = store.recently_completed();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].route.as_deref(), Some("route-a"));
        assert_eq!(recent[0].phase, "ended");
        drop(b);
        assert!(store.snapshot().is_empty());
        assert_eq!(store.recently_completed().len(), 2);
        for (at, _) in &mut store.0.lock().unwrap().completed {
            *at = Instant::now() - Duration::from_secs(6);
        }
        assert!(store.recently_completed().is_empty());
    }
}
