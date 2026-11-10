//! Bounded, account-generation-scoped memory cache. Never stores writes or errors.
use super::Session;
use serde_json::Value;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    account: Option<(i64, String)>,
    path: String,
    query: Vec<(String, String)>,
}
impl Key {
    pub fn new(session: Option<&Session>, path: &str, query: &[(String, String)]) -> Self {
        Self {
            account: session.map(|s| (s.user_id, s.generation.clone())),
            path: path.into(),
            query: query.to_vec(),
        }
    }
}
struct Entry {
    key: Key,
    at: Instant,
    value: Value,
    bytes: usize,
}
#[derive(Default)]
pub(super) struct ReadCache {
    epoch: u64,
    entries: VecDeque<Entry>,
    bytes: usize,
}
impl ReadCache {
    pub fn read(&mut self, key: &Key, now: Instant) -> (u64, Option<Value>) {
        self.entries
            .retain(|entry| now.saturating_duration_since(entry.at) < lifetime(&entry.key.path));
        self.bytes = self.entries.iter().map(|e| e.bytes).sum();
        (
            self.epoch,
            self.entries
                .iter()
                .find(|e| e.key == *key)
                .map(|e| e.value.clone()),
        )
    }
    pub fn insert(&mut self, key: Key, value: &Value, epoch: u64, now: Instant) {
        if epoch != self.epoch {
            return;
        }
        if lifetime(&key.path).is_zero() {
            return;
        }
        let bytes = serde_json::to_vec(value)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if bytes > 1024 * 1024 {
            return;
        }
        self.entries.retain(|e| e.key != key);
        self.bytes = self.entries.iter().map(|e| e.bytes).sum();
        while self.entries.len() >= 32 || self.bytes + bytes > 4 * 1024 * 1024 {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= old.bytes;
            } else {
                break;
            }
        }
        self.entries.push_back(Entry {
            key,
            at: now,
            value: value.clone(),
            bytes,
        });
        self.bytes += bytes;
    }
    pub fn clear(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.entries.clear();
        self.bytes = 0;
    }
}
fn lifetime(path: &str) -> Duration {
    Duration::from_secs(match path {
        "/settings/public" => 300,
        "/auth/me" | "/user/organization" => 30,
        "/keys"
        | "/usage"
        | "/usage/dashboard/stats"
        | "/usage/dashboard/trend"
        | "/user/profile"
        | "/channels/available"
        | "/organization-admin/business-units" => 15,
        // Order verification, balances used for writes and all detail/action reads stay live.
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn session(generation: &str) -> Session {
        Session {
            user_id: 7,
            generation: generation.into(),
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
        }
    }
    #[test]
    fn cache_separates_logins_accounts_filters_and_expires_without_stale_fallback() {
        let mut cache = ReadCache::default();
        let now = Instant::now();
        let key = Key::new(
            Some(&session("one")),
            "/keys",
            &[("page".into(), "1".into())],
        );
        let (epoch, _) = cache.read(&key, now);
        cache.insert(key.clone(), &json!({"items":[1]}), epoch, now);
        assert!(cache.read(&key, now + Duration::from_secs(14)).1.is_some());
        assert!(
            cache
                .read(&Key::new(Some(&session("two")), "/keys", &key.query), now)
                .1
                .is_none()
        );
        let mut other = session("one");
        other.user_id = 8;
        assert!(
            cache
                .read(&Key::new(Some(&other), "/keys", &key.query), now)
                .1
                .is_none()
        );
        assert!(
            cache
                .read(
                    &Key::new(
                        Some(&session("one")),
                        "/keys",
                        &[("page".into(), "2".into())]
                    ),
                    now
                )
                .1
                .is_none()
        );
        assert!(cache.read(&key, now + Duration::from_secs(15)).1.is_none());
    }
    #[test]
    fn invalidation_rejects_delayed_reads_and_memory_is_bounded() {
        let mut cache = ReadCache::default();
        let now = Instant::now();
        let key = Key::new(None, "/settings/public", &[]);
        let (epoch, _) = cache.read(&key, now);
        cache.clear();
        cache.insert(key.clone(), &json!({"old":true}), epoch, now);
        assert!(cache.read(&key, now).1.is_none());
        let (epoch, _) = cache.read(&key, now);
        for n in 0..40 {
            cache.insert(
                Key::new(None, "/keys", &[("page".into(), n.to_string())]),
                &json!("x".repeat(200_000)),
                epoch,
                now,
            );
        }
        assert!(cache.entries.len() <= 32 && cache.bytes <= 4 * 1024 * 1024);
        assert!(
            cache
                .read(
                    &Key::new(None, "/keys", &[("page".into(), "0".into())]),
                    now
                )
                .1
                .is_none()
        );
        assert!(
            cache
                .read(
                    &Key::new(None, "/keys", &[("page".into(), "39".into())]),
                    now
                )
                .1
                .is_some()
        );
        let order = Key::new(Some(&session("one")), "/payment/orders/9", &[]);
        cache.insert(order.clone(), &json!({"status":"pending"}), epoch, now);
        assert!(cache.read(&order, now).1.is_none());
    }
}
