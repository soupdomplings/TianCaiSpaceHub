use std::collections::HashMap;

use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::{
    config::resolve_mcp_url,
    types::{
        AuthMode, AuthResult, AuthToken, ClientKind, NvwaProfile, TokenExpiryPolicy,
        VerifiedIdentity,
    },
};

const SESSION_IDLE_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_SESSIONS: usize = 512;
const MAX_ACTIVE_CALLS: usize = 256;

#[derive(Default)]
pub(crate) struct Runtime {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    profiles: HashMap<String, ProfileRuntime>,
    sessions: HashMap<String, Session>,
    calls: HashMap<(String, String), String>,
}

struct ProfileRuntime {
    generation: String,
    auth: Option<AuthResult>,
    capabilities: HashMap<ClientKind, String>,
    tools: Option<Value>,
    detail: String,
    verified: bool,
    client_checks: HashMap<ClientKind, u64>,
}

struct Session {
    profile: String,
    client: ClientKind,
    generation: String,
    capability: String,
    protocol: String,
    last_used: u64,
}

pub(crate) struct RequestLease {
    pub auth: AuthResult,
    pub generation: String,
    pub session_id: Option<String>,
    pub original_id: Option<Value>,
    pub upstream_id: Option<String>,
    pub protocol: Option<String>,
}

pub(crate) fn random_capability() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn constant_eq(left: &str, right: &str) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.as_bytes().get(index).copied().unwrap_or_default()
                ^ right.as_bytes().get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn rpc_key(id: &Value) -> Result<String> {
    match id {
        Value::String(value) if value.len() <= 256 => Ok(format!("s:{value}")),
        Value::Number(value) if value.is_i64() || value.is_u64() => Ok(format!("n:{value}")),
        _ => bail!("MCP request ID must be a string or integer"),
    }
}

fn live_auth(profile: &ProfileRuntime) -> Result<AuthResult> {
    ensure!(profile.verified, "NVWA identity verification required");
    let auth = profile
        .auth
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("NVWA login required"))?;
    ensure!(
        auth.mcp_token
            .expires_at_ms
            .is_none_or(|expiry| expiry > now_ms()),
        "NVWA MCP token expired; login required"
    );
    Ok(auth.clone())
}

impl Runtime {
    pub(crate) fn restore(records: Vec<(String, AuthMode, Value)>) -> Self {
        let mut inner = Inner::default();
        for (id, mode, record) in records {
            let decoded = (|| -> Result<ProfileRuntime> {
                let generation = record
                    .get("generation")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid protected session"))?
                    .to_owned();
                ensure!(
                    Uuid::parse_str(&generation).is_ok(),
                    "Invalid protected session"
                );
                let identity: VerifiedIdentity =
                    serde_json::from_value(record["identity"].clone())?;
                let token = |value: &Value| -> Result<AuthToken> {
                    let content = value
                        .get("value")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("Invalid protected token"))?
                        .to_owned();
                    let header = value
                        .get("headerName")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("Invalid protected token"))?
                        .to_owned();
                    ensure!(
                        !content.is_empty()
                            && content.len() <= 16_384
                            && !content.chars().any(char::is_control)
                            && matches!(
                                header.to_ascii_lowercase().as_str(),
                                "authorization" | "authorization-ticket-token"
                            ),
                        "Invalid protected token"
                    );
                    let expiry_policy = if mode == AuthMode::Password {
                        TokenExpiryPolicy::SlidingIdle
                    } else {
                        TokenExpiryPolicy::Fixed
                    };
                    let saved_expiry = value.get("expiresAtMs").and_then(Value::as_u64);
                    Ok(AuthToken {
                        value: content,
                        header_name: header,
                        // Older snapshots treated the password idle timeout as a
                        // fixed deadline. Keep only the real rejection sentinel.
                        expires_at_ms: if mode == AuthMode::Password {
                            saved_expiry.filter(|expiry| *expiry == 0)
                        } else {
                            saved_expiry
                        },
                        expiry_policy,
                    })
                };
                let auth = AuthResult {
                    identity,
                    personal_token: if record["personalToken"].is_null() {
                        None
                    } else {
                        Some(token(&record["personalToken"])?)
                    },
                    mcp_token: token(&record["mcpToken"])?,
                };
                let mut capabilities = HashMap::new();
                for client in [
                    ClientKind::Codex,
                    ClientKind::Workbuddy,
                    ClientKind::Tiangong,
                ] {
                    if let Some(capability) = record["capabilities"][client.as_str()].as_str() {
                        ensure!(
                            capability.len() == 64
                                && capability.bytes().all(|v| v.is_ascii_hexdigit()),
                            "Invalid protected client capability"
                        );
                        capabilities.insert(client, capability.to_owned());
                    }
                }
                Ok(ProfileRuntime {
                    generation,
                    auth: Some(auth),
                    capabilities,
                    tools: None,
                    detail: "Restored protected credentials; identity verification pending".into(),
                    verified: false,
                    client_checks: HashMap::new(),
                })
            })();
            if let Ok(profile) = decoded {
                inner.profiles.insert(id, profile);
            }
        }
        Self {
            inner: Mutex::new(inner),
        }
    }

    pub async fn protected_snapshot(&self, id: &str) -> Option<Value> {
        let inner = self.inner.lock().await;
        let profile = inner.profiles.get(id)?;
        let auth = profile.auth.as_ref()?;
        let token = |token: &AuthToken| json!({"value":token.value,"headerName":token.header_name,"expiresAtMs":token.expires_at_ms,"expiryPolicy":token.expiry_policy});
        let mut capabilities = serde_json::Map::new();
        for (kind, capability) in &profile.capabilities {
            capabilities.insert(kind.as_str().to_owned(), Value::String(capability.clone()));
        }
        Some(
            json!({"generation":profile.generation,"identity":auth.identity,"personalToken":auth.personal_token.as_ref().map(token),
            "mcpToken":token(&auth.mcp_token),"capabilities":capabilities}),
        )
    }

    pub async fn raw_auth(&self, profile_id: &str) -> Result<(String, AuthResult)> {
        let inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get(profile_id)
            .ok_or_else(|| anyhow::anyhow!("NVWA login required"))?;
        let auth = profile
            .auth
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("NVWA login required"))?;
        Ok((profile.generation.clone(), auth.clone()))
    }

    pub async fn generation_matches(&self, profile_id: &str, generation: &str) -> bool {
        self.inner
            .lock()
            .await
            .profiles
            .get(profile_id)
            .is_some_and(|p| p.generation == generation)
    }

    pub async fn renew_auth(
        &self,
        profile_id: &str,
        generation: &str,
        auth: AuthResult,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| anyhow::anyhow!("Login changed during renewal"))?;
        ensure!(
            profile.generation == generation,
            "Login changed during renewal"
        );
        let previous = profile
            .auth
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Login changed during renewal"))?;
        ensure!(
            previous.identity.user_id == auth.identity.user_id
                && previous.identity.identity_id == auth.identity.identity_id
                && previous.identity.tenant_id == auth.identity.tenant_id,
            "NVWA identity changed; explicit login required"
        );
        profile.auth = Some(auth);
        profile.verified = true;
        Ok(())
    }

    pub async fn mark_expired(&self, profile_id: &str, generation: &str, rejected_token: &str) {
        let mut inner = self.inner.lock().await;
        if let Some(profile) = inner
            .profiles
            .get_mut(profile_id)
            .filter(|p| p.generation == generation)
        {
            if let Some(auth) = &mut profile.auth
                && constant_eq(&auth.mcp_token.value, rejected_token)
            {
                auth.mcp_token.expires_at_ms = Some(0);
                profile.detail = "NVWA rejected authorization; request was not replayed".into();
            }
        }
    }

    pub async fn begin_auth(&self, profile_id: &str) -> String {
        let mut inner = self.inner.lock().await;
        inner
            .sessions
            .retain(|_, session| session.profile != profile_id);
        let retained: std::collections::HashSet<_> = inner.sessions.keys().cloned().collect();
        inner
            .calls
            .retain(|(session, _), _| retained.contains(session));
        let generation = Uuid::new_v4().to_string();
        inner.profiles.insert(
            profile_id.to_owned(),
            ProfileRuntime {
                generation: generation.clone(),
                auth: None,
                capabilities: HashMap::new(),
                tools: None,
                detail: "Waiting for login".into(),
                verified: false,
                client_checks: HashMap::new(),
            },
        );
        generation
    }

    pub async fn commit_auth(
        &self,
        profile_id: &str,
        generation: &str,
        auth: AuthResult,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| anyhow::anyhow!("Login was superseded"))?;
        ensure!(profile.generation == generation, "Login was superseded");
        profile.auth = Some(auth);
        profile.verified = true;
        profile.detail = "Authenticated; client access must be explicitly applied".into();
        Ok(())
    }

    pub async fn set_detail(&self, profile_id: &str, generation: &str, detail: &str) {
        let mut inner = self.inner.lock().await;
        if let Some(profile) = inner
            .profiles
            .get_mut(profile_id)
            .filter(|p| p.generation == generation)
        {
            profile.detail = detail.to_owned();
        }
    }

    pub async fn logout(&self, profile_id: &str) {
        let generation = self.begin_auth(profile_id).await;
        self.set_detail(
            profile_id,
            &generation,
            "Logged out; previous client access revoked",
        )
        .await;
    }

    pub async fn auth(&self, profile_id: &str) -> Result<(String, AuthResult)> {
        let inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get(profile_id)
            .ok_or_else(|| anyhow::anyhow!("NVWA login required"))?;
        Ok((profile.generation.clone(), live_auth(profile)?))
    }

    pub async fn capability(&self, profile_id: &str, client: ClientKind) -> Result<String> {
        let mut inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| anyhow::anyhow!("NVWA login required"))?;
        live_auth(profile)?;
        Ok(profile
            .capabilities
            .entry(client)
            .or_insert_with(random_capability)
            .clone())
    }

    pub async fn check_capability(
        &self,
        profile_id: &str,
        client: ClientKind,
        bearer: &str,
    ) -> Result<()> {
        let inner = self.inner.lock().await;
        let expected = inner
            .profiles
            .get(profile_id)
            .and_then(|p| p.capabilities.get(&client))
            .ok_or_else(|| anyhow::anyhow!("Client authorization required"))?;
        ensure!(
            constant_eq(expected, bearer),
            "Client authorization invalid"
        );
        Ok(())
    }

    pub async fn existing_capability(
        &self,
        profile_id: &str,
        client: ClientKind,
    ) -> Option<String> {
        self.inner
            .lock()
            .await
            .profiles
            .get(profile_id)
            .and_then(|p| p.capabilities.get(&client))
            .cloned()
    }

    pub async fn revoke_client(&self, profile_id: &str, client: ClientKind) {
        let mut inner = self.inner.lock().await;
        if let Some(profile) = inner.profiles.get_mut(profile_id) {
            profile.capabilities.remove(&client);
            profile.client_checks.remove(&client);
        }
        inner
            .sessions
            .retain(|_, session| session.profile != profile_id || session.client != client);
        let retained: std::collections::HashSet<_> = inner.sessions.keys().cloned().collect();
        inner
            .calls
            .retain(|(session, _), _| retained.contains(session));
    }

    pub async fn admit(
        &self,
        profile_id: &str,
        client: ClientKind,
        bearer: &str,
        session_id: Option<&str>,
        method: &str,
        id: Option<&Value>,
    ) -> Result<RequestLease> {
        let mut inner = self.inner.lock().await;
        let expired: Vec<_> = inner
            .sessions
            .iter()
            .filter(|(_, s)| now_ms().saturating_sub(s.last_used) > SESSION_IDLE_MS)
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            inner.sessions.remove(&key);
            inner.calls.retain(|(s, _), _| s != &key);
        }
        let profile = inner
            .profiles
            .get(profile_id)
            .ok_or_else(|| anyhow::anyhow!("NVWA login required"))?;
        let expected = profile
            .capabilities
            .get(&client)
            .ok_or_else(|| anyhow::anyhow!("Client authorization required"))?;
        ensure!(
            constant_eq(expected, bearer),
            "Client authorization invalid"
        );
        let auth = live_auth(profile)?;
        let generation = profile.generation.clone();
        let protocol = if method == "initialize" {
            ensure!(
                session_id.is_none(),
                "Initialize must not reuse an existing session"
            );
            ensure!(id.is_some(), "Initialize requires a request ID");
            None
        } else {
            let sid = session_id.ok_or_else(|| anyhow::anyhow!("MCP session required"))?;
            let session = inner
                .sessions
                .get_mut(sid)
                .ok_or_else(|| anyhow::anyhow!("MCP session expired"))?;
            ensure!(
                session.profile == profile_id
                    && session.client == client
                    && session.generation == generation
                    && constant_eq(&session.capability, bearer),
                "MCP session owner mismatch"
            );
            session.last_used = now_ms();
            Some(session.protocol.clone())
        };
        let upstream_id = if let Some(id) = id {
            let key = rpc_key(id)?;
            ensure!(
                inner.calls.len() < MAX_ACTIVE_CALLS,
                "Too many active MCP requests"
            );
            let upstream = format!("hub-{}", Uuid::new_v4());
            if let Some(sid) = session_id {
                ensure!(
                    !inner.calls.contains_key(&(sid.to_owned(), key.clone())),
                    "Duplicate active MCP request ID"
                );
                inner.calls.insert((sid.to_owned(), key), upstream.clone());
            }
            Some(upstream)
        } else {
            None
        };
        Ok(RequestLease {
            auth,
            generation,
            session_id: session_id.map(str::to_owned),
            original_id: id.cloned(),
            upstream_id,
            protocol,
        })
    }

    pub async fn create_session(
        &self,
        profile_id: &str,
        client: ClientKind,
        bearer: &str,
        generation: &str,
        protocol: &str,
    ) -> Result<String> {
        let mut inner = self.inner.lock().await;
        ensure!(inner.sessions.len() < MAX_SESSIONS, "Too many MCP sessions");
        let profile = inner
            .profiles
            .get(profile_id)
            .ok_or_else(|| anyhow::anyhow!("Login was superseded"))?;
        ensure!(
            profile.generation == generation
                && profile
                    .capabilities
                    .get(&client)
                    .is_some_and(|v| constant_eq(v, bearer)),
            "Login was superseded"
        );
        let id = Uuid::new_v4().to_string();
        inner.sessions.insert(
            id.clone(),
            Session {
                profile: profile_id.to_owned(),
                client,
                generation: generation.to_owned(),
                capability: bearer.to_owned(),
                protocol: protocol.to_owned(),
                last_used: now_ms(),
            },
        );
        Ok(id)
    }

    pub async fn cancel_id(&self, session_id: &str, original: &Value) -> Result<Option<String>> {
        let inner = self.inner.lock().await;
        Ok(inner
            .calls
            .get(&(session_id.to_owned(), rpc_key(original)?))
            .cloned())
    }

    pub async fn finish(&self, lease: &RequestLease) {
        if let (Some(session), Some(id), Some(upstream)) =
            (&lease.session_id, &lease.original_id, &lease.upstream_id)
            && let Ok(key) = rpc_key(id)
        {
            let mut inner = self.inner.lock().await;
            let identity = (session.clone(), key);
            if inner.calls.get(&identity) == Some(upstream) {
                inner.calls.remove(&identity);
            }
        }
    }

    pub async fn delete_session(&self, session_id: &str) {
        let mut inner = self.inner.lock().await;
        inner.sessions.remove(session_id);
        inner.calls.retain(|(s, _), _| s != session_id);
    }

    pub async fn set_tools(&self, profile_id: &str, generation: &str, tools: Value) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| anyhow::anyhow!("Login was superseded"))?;
        ensure!(profile.generation == generation, "Login was superseded");
        profile.tools = Some(tools);
        profile.detail =
            "MCP initialize and paginated tools/list verified; no tool was invoked".into();
        Ok(())
    }

    pub async fn record_client_check(
        &self,
        profile_id: &str,
        client: ClientKind,
        generation: &str,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| anyhow::anyhow!("Login changed during detection"))?;
        ensure!(
            profile.generation == generation && profile.capabilities.contains_key(&client),
            "Login changed during detection"
        );
        profile.client_checks.insert(client, now_ms());
        Ok(())
    }

    pub async fn client_check(&self, profile_id: &str, client: ClientKind) -> Option<u64> {
        self.inner
            .lock()
            .await
            .profiles
            .get(profile_id)
            .and_then(|p| p.client_checks.get(&client))
            .copied()
    }

    pub async fn tools(&self, profile_id: &str) -> Option<Value> {
        self.inner
            .lock()
            .await
            .profiles
            .get(profile_id)
            .and_then(|p| p.tools.clone())
    }

    pub async fn status(&self, profiles: &[NvwaProfile]) -> Vec<Value> {
        let inner = self.inner.lock().await;
        profiles.iter().map(|profile| {
            let runtime = inner.profiles.get(&profile.id);
            let auth = runtime.and_then(|p| p.auth.as_ref());
            let state = match auth {
                None => "logged_out",
                Some(a) if a.mcp_token.expires_at_ms.is_some_and(|expiry| expiry <= now_ms()) => "expired",
                Some(a) if a.personal_token.as_ref().and_then(|t| t.expires_at_ms).is_some_and(|expiry| expiry <= now_ms()) => "personal_expired",
                Some(_) if runtime.is_some_and(|p| !p.verified) => "identity_verification_pending",
                Some(_) => "authenticated",
            };
            json!({"profileId":profile.id,"state":state,"identity":auth.map(|a| &a.identity),
                "resolvedMcpUrl":resolve_mcp_url(profile).ok().map(|url| url.to_string()),
                "personalTokenPresent":auth.is_some_and(|a| a.personal_token.is_some()),
                "mcpTokenPresent":auth.is_some(),
                "personalExpiresAtMs":auth.and_then(|a| a.personal_token.as_ref()).and_then(|t| t.expires_at_ms),
                "mcpExpiresAtMs":auth.and_then(|a| a.mcp_token.expires_at_ms),
                "personalExpiryPolicy":auth.and_then(|a| a.personal_token.as_ref()).map(|t| t.expiry_policy),
                "mcpExpiryPolicy":auth.map(|a| a.mcp_token.expiry_policy),
                "personalExpired":auth.and_then(|a| a.personal_token.as_ref()).and_then(|t| t.expires_at_ms).is_some_and(|e| e <= now_ms()),
                "toolCount":runtime.and_then(|p| p.tools.as_ref()).and_then(Value::as_array).map(Vec::len),
                "detail":runtime.map(|p| p.detail.as_str()).unwrap_or("Login required"),
                "clientAccess":runtime.map(|p| p.capabilities.keys().map(|k| k.as_str()).collect::<Vec<_>>()).unwrap_or_default()})
        }).collect()
    }
}
