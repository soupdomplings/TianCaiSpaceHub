//! Display-only evidence of tool calls actually forwarded by Hub. No arguments,
//! results, credentials, subprocess starts or remote checks are recorded here.
use super::clients::ClientId;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCall {
    pub id: String,
    pub resource: String,
    pub name: String,
    pub client: ClientId,
    pub dumpling: bool,
    pub active: bool,
    pub at: i64,
}
#[derive(Clone, Default)]
pub struct McpActivityStore(Arc<Mutex<BTreeMap<String, (McpCall, Option<Instant>)>>>);
impl McpActivityStore {
    pub fn begin(&self, resource: &str, name: &str, client: ClientId, dumpling: bool) -> McpGuard {
        let id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut calls) = self.0.lock() {
            calls.retain(|_, (_, ended)| {
                ended.is_none_or(|at| at.elapsed() < Duration::from_secs(5))
            });
            if calls.len() < 1024 {
                calls.insert(
                    id.clone(),
                    (
                        McpCall {
                            id: id.clone(),
                            resource: resource.into(),
                            name: name.into(),
                            client,
                            dumpling,
                            active: true,
                            at: crate::ai_gateway::request_log::now_ms(),
                        },
                        None,
                    ),
                );
            }
        }
        McpGuard {
            store: self.clone(),
            id,
        }
    }
    pub fn snapshot(&self) -> Vec<McpCall> {
        let Ok(mut calls) = self.0.lock() else {
            return vec![];
        };
        calls.retain(|_, (_, ended)| ended.is_none_or(|at| at.elapsed() < Duration::from_secs(5)));
        calls.values().map(|(call, _)| call.clone()).collect()
    }
}
pub struct McpGuard {
    store: McpActivityStore,
    id: String,
}
impl Drop for McpGuard {
    fn drop(&mut self) {
        if let Ok(mut calls) = self.store.0.lock() {
            if let Some((call, ended)) = calls.get_mut(&self.id) {
                call.active = false;
                call.at = crate::ai_gateway::request_log::now_ms();
                *ended = Some(Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_calls_keep_identity_and_completion_is_never_active() {
        let store = McpActivityStore::default();
        let a = store.begin("one", "fixture", ClientId::Tiangong, false);
        let b = store.begin("one", "fixture", ClientId::Codex, true);
        assert_eq!(store.snapshot().iter().filter(|c| c.active).count(), 2);
        drop(a);
        let rows = store.snapshot();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .any(|c| !c.active && c.client == ClientId::Tiangong && !c.dumpling)
        );
        assert!(
            rows.iter()
                .any(|c| c.active && c.client == ClientId::Codex && c.dumpling)
        );
        drop(b);
        assert!(store.snapshot().iter().all(|c| !c.active));
        // Expiration only affects transient display evidence, not any files.
        for (_, ended) in store.0.lock().unwrap().values_mut() {
            *ended = Some(Instant::now() - Duration::from_secs(6));
        }
        assert!(store.snapshot().is_empty());
    }
}
