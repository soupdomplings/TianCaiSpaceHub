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
    pending_access: Option<PendingClientAccess>,
}

struct PendingClientAccess {
    identity: VerifiedIdentity,
    capabilities: HashMap<ClientKind, String>,
    session_generation: String,
}

fn same_identity(left: &VerifiedIdentity, right: &VerifiedIdentity) -> bool {
    left.user_id == right.user_id
        && left.identity_id == right.identity_id
        && left.tenant_id == right.tenant_id
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

fn begin_auth(inner: &mut Inner, profile_id: &str) -> String {
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
            pending_access: None,
        },
    );
    generation
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
                // A pending sign-in keeps the local keys protected, but cannot
                // restore an upstream token or authorize a client before login.
                let pending = record
                    .get("pendingIdentity")
                    .filter(|value| !value.is_null());
                let (auth, pending_access, capabilities) = if let Some(identity) = pending {
                    ensure!(
                        record["mcpToken"].is_null() && record["personalToken"].is_null(),
                        "Invalid protected pending login"
                    );
                    let identity: VerifiedIdentity = serde_json::from_value(identity.clone())?;
                    (
                        None,
                        Some(PendingClientAccess {
                            identity,
                            capabilities,
                            session_generation: generation.clone(),
                        }),
                        HashMap::new(),
                    )
                } else {
                    let auth = AuthResult {
                        identity: serde_json::from_value(record["identity"].clone())?,
                        personal_token: if record["personalToken"].is_null() {
                            None
                        } else {
                            Some(token(&record["personalToken"])?)
                        },
                        mcp_token: token(&record["mcpToken"])?,
                    };
                    (Some(auth), None, capabilities)
                };
                let detail = if auth.is_none() {
                    "Restored client access suspended; explicit login required"
                } else {
                    "Restored protected credentials; identity verification pending"
                }
                .into();
                Ok(ProfileRuntime {
                    generation,
                    auth,
                    capabilities,
                    tools: None,
                    detail,
                    verified: false,
                    client_checks: HashMap::new(),
                    pending_access,
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
        let token = |token: &AuthToken| json!({"value":token.value,"headerName":token.header_name,"expiresAtMs":token.expires_at_ms,"expiryPolicy":token.expiry_policy});
        let mut capabilities = serde_json::Map::new();
        let stored_capabilities = profile
            .pending_access
            .as_ref()
            .map(|pending| &pending.capabilities)
            .unwrap_or(&profile.capabilities);
        for (kind, capability) in stored_capabilities {
            capabilities.insert(kind.as_str().to_owned(), Value::String(capability.clone()));
        }
        if let Some(pending) = &profile.pending_access {
            return Some(json!({"generation":profile.generation,
                "pendingIdentity":pending.identity,"capabilities":capabilities}));
        }
        let auth = profile.auth.as_ref()?;
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
        profile.detail = "Authentication verified; existing client access retained".into();
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
        begin_auth(&mut inner, profile_id)
    }

    pub async fn begin_login(&self, profile_id: &str) -> String {
        let mut inner = self.inner.lock().await;
        let pending_access = inner.profiles.remove(profile_id).and_then(|previous| {
            previous.pending_access.or_else(|| {
                previous.auth.map(|auth| PendingClientAccess {
                    identity: auth.identity,
                    capabilities: previous.capabilities,
                    session_generation: previous.generation,
                })
            })
        });
        // Existing sessions are suspended until the exact identity is verified.
        // In-flight calls are never carried into the new login generation.
        inner.sessions.retain(|_, session| {
            session.profile != profile_id
                || pending_access.as_ref().is_some_and(|pending| {
                    session.generation == pending.session_generation
                        && pending
                            .capabilities
                            .get(&session.client)
                            .is_some_and(|key| constant_eq(key, &session.capability))
                })
        });
        let other_sessions: std::collections::HashSet<_> = inner
            .sessions
            .iter()
            .filter(|(_, session)| session.profile != profile_id)
            .map(|(id, _)| id.clone())
            .collect();
        inner
            .calls
            .retain(|(session, _), _| other_sessions.contains(session));
        let generation = Uuid::new_v4().to_string();
        inner.profiles.insert(
            profile_id.to_owned(),
            ProfileRuntime {
                generation: generation.clone(),
                auth: None,
                capabilities: HashMap::new(),
                tools: None,
                detail: "Waiting for login; existing client access suspended".into(),
                verified: false,
                client_checks: HashMap::new(),
                pending_access,
            },
        );
        generation
    }

    pub async fn invalidate_generation(&self, profile_id: &str, generation: &str) -> bool {
        let mut inner = self.inner.lock().await;
        if !inner
            .profiles
            .get(profile_id)
            .is_some_and(|profile| profile.generation == generation)
        {
            return false;
        }
        begin_auth(&mut inner, profile_id);
        inner.profiles.get_mut(profile_id).unwrap().detail =
            "NVWA identity changed; explicit login and client access required".into();
        true
    }

    pub async fn commit_auth(
        &self,
        profile_id: &str,
        generation: &str,
        auth: AuthResult,
    ) -> Result<bool> {
        let mut inner = self.inner.lock().await;
        let profile = inner
            .profiles
            .get_mut(profile_id)
            .ok_or_else(|| anyhow::anyhow!("Login was superseded"))?;
        ensure!(profile.generation == generation, "Login was superseded");
        let pending = profile
            .pending_access
            .take()
            .filter(|pending| same_identity(&pending.identity, &auth.identity));
        profile.capabilities.clear();
        if let Some(pending) = &pending {
            profile.capabilities = pending.capabilities.clone();
        }
        let retained = !profile.capabilities.is_empty();
        profile.auth = Some(auth);
        profile.verified = true;
        profile.detail = if retained {
            "Authenticated; existing client access retained"
        } else {
            "Authenticated; client access must be explicitly applied"
        }
        .into();
        inner.sessions.retain(|_, session| {
            if session.profile != profile_id {
                return true;
            }
            if pending.as_ref().is_some_and(|pending| {
                session.generation == pending.session_generation
                    && pending
                        .capabilities
                        .get(&session.client)
                        .is_some_and(|key| constant_eq(key, &session.capability))
            }) {
                session.generation = generation.to_owned();
                true
            } else {
                false
            }
        });
        Ok(retained)
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

    pub async fn client_access_suspended(&self, profile_id: &str, client: ClientKind) -> bool {
        self.inner
            .lock()
            .await
            .profiles
            .get(profile_id)
            .and_then(|profile| profile.pending_access.as_ref())
            .is_some_and(|pending| pending.capabilities.contains_key(&client))
    }

    pub async fn revoke_client(&self, profile_id: &str, client: ClientKind) {
        let mut inner = self.inner.lock().await;
        if let Some(profile) = inner.profiles.get_mut(profile_id) {
            profile.capabilities.remove(&client);
            if let Some(pending) = &mut profile.pending_access {
                pending.capabilities.remove(&client);
            }
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

    pub async fn lease_is_current(
        &self,
        profile_id: &str,
        client: ClientKind,
        bearer: &str,
        lease: &RequestLease,
    ) -> bool {
        let inner = self.inner.lock().await;
        inner.profiles.get(profile_id).is_some_and(|profile| {
            profile.generation == lease.generation
                && profile
                    .capabilities
                    .get(&client)
                    .is_some_and(|key| constant_eq(key, bearer))
        }) && lease.session_id.as_ref().is_none_or(|id| {
            inner.sessions.get(id).is_some_and(|session| {
                session.profile == profile_id
                    && session.client == client
                    && session.generation == lease.generation
                    && constant_eq(&session.capability, bearer)
            })
        })
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
                None if runtime.is_some_and(|p| p.pending_access.is_some()) => "login_pending",
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
                "clientAccess":runtime.map(|p| p.capabilities.keys().map(|k| k.as_str()).collect::<Vec<_>>()).unwrap_or_default(),
                "suspendedClientAccess":runtime.and_then(|p| p.pending_access.as_ref()).map(|p| p.capabilities.keys().map(|k| k.as_str()).collect::<Vec<_>>()).unwrap_or_default()})
        }).collect()
    }
}

#[cfg(test)]
mod login_access_tests {
    use super::*;

    fn auth() -> AuthResult {
        AuthResult {
            identity: VerifiedIdentity {
                user_id: "fixture-user".into(),
                identity_id: "fixture-identity".into(),
                tenant_id: "fixture-tenant".into(),
                username: "fixture".into(),
            },
            personal_token: None,
            mcp_token: AuthToken {
                value: "fixture-token".into(),
                header_name: "authorization-ticket-token".into(),
                expires_at_ms: None,
                expiry_policy: TokenExpiryPolicy::Fixed,
            },
        }
    }

    async fn connected() -> (Runtime, String, String, String, RequestLease) {
        let runtime = Runtime::default();
        let generation = runtime.begin_auth("fixture-profile").await;
        runtime
            .commit_auth("fixture-profile", &generation, auth())
            .await
            .unwrap();
        let capability = runtime
            .capability("fixture-profile", ClientKind::Codex)
            .await
            .unwrap();
        let session = runtime
            .create_session(
                "fixture-profile",
                ClientKind::Codex,
                &capability,
                &generation,
                "2025-03-26",
            )
            .await
            .unwrap();
        let lease = runtime
            .admit(
                "fixture-profile",
                ClientKind::Codex,
                &capability,
                Some(&session),
                "tools/list",
                Some(&json!(1)),
            )
            .await
            .unwrap();
        (runtime, generation, capability, session, lease)
    }

    #[tokio::test]
    async fn expiry_and_same_identity_sign_in_keep_client_connection() {
        let (runtime, generation, capability, session, lease) = connected().await;
        runtime
            .mark_expired("fixture-profile", &generation, "fixture-token")
            .await;
        assert!(runtime.auth("fixture-profile").await.is_err());
        runtime
            .renew_auth("fixture-profile", &generation, auth())
            .await
            .unwrap();
        assert!(
            runtime
                .lease_is_current("fixture-profile", ClientKind::Codex, &capability, &lease)
                .await
        );
        let next = runtime.begin_login("fixture-profile").await;
        assert_ne!(next, generation);
        assert!(runtime.raw_auth("fixture-profile").await.is_err());
        assert!(
            runtime
                .check_capability("fixture-profile", ClientKind::Codex, &capability)
                .await
                .is_err()
        );
        assert!(
            runtime
                .client_access_suspended("fixture-profile", ClientKind::Codex)
                .await
        );
        assert!(
            runtime
                .commit_auth("fixture-profile", &next, auth())
                .await
                .unwrap()
        );
        assert_eq!(
            runtime
                .existing_capability("fixture-profile", ClientKind::Codex)
                .await,
            Some(capability.clone())
        );
        assert!(
            !runtime
                .lease_is_current("fixture-profile", ClientKind::Codex, &capability, &lease)
                .await
        );
        let current = runtime
            .admit(
                "fixture-profile",
                ClientKind::Codex,
                &capability,
                Some(&session),
                "tools/list",
                Some(&json!(1)),
            )
            .await
            .unwrap();
        runtime.finish(&lease).await;
        assert!(
            runtime
                .cancel_id(&session, &json!(1))
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            runtime
                .lease_is_current("fixture-profile", ClientKind::Codex, &capability, &current)
                .await
        );
    }

    #[tokio::test]
    async fn pending_retry_snapshot_and_single_client_revocation_remain_isolated() {
        let (runtime, _, capability, old_session, _) = connected().await;
        runtime
            .capability("fixture-profile", ClientKind::Workbuddy)
            .await
            .unwrap();
        runtime
            .capability("fixture-profile", ClientKind::Tiangong)
            .await
            .unwrap();
        runtime.begin_login("fixture-profile").await;
        runtime.begin_login("fixture-profile").await;
        runtime
            .revoke_client("fixture-profile", ClientKind::Workbuddy)
            .await;
        let snapshot = runtime.protected_snapshot("fixture-profile").await.unwrap();
        assert!(snapshot["mcpToken"].is_null() && snapshot["personalToken"].is_null());
        assert!(snapshot["capabilities"]["workbuddy"].is_null());
        let restored = Runtime::restore(vec![(
            "fixture-profile".into(),
            AuthMode::Application,
            snapshot,
        )]);
        assert!(restored.raw_auth("fixture-profile").await.is_err());
        assert!(
            restored
                .existing_capability("fixture-profile", ClientKind::Codex)
                .await
                .is_none()
        );
        assert!(
            restored
                .client_access_suspended("fixture-profile", ClientKind::Codex)
                .await
        );
        let next = restored.begin_login("fixture-profile").await;
        assert!(
            restored
                .commit_auth("fixture-profile", &next, auth())
                .await
                .unwrap()
        );
        assert_eq!(
            restored
                .existing_capability("fixture-profile", ClientKind::Codex)
                .await,
            Some(capability.clone())
        );
        assert!(
            restored
                .existing_capability("fixture-profile", ClientKind::Workbuddy)
                .await
                .is_none()
        );
        assert!(
            restored
                .existing_capability("fixture-profile", ClientKind::Tiangong)
                .await
                .is_some()
        );
        assert!(
            restored
                .admit(
                    "fixture-profile",
                    ClientKind::Codex,
                    &capability,
                    Some(&old_session),
                    "tools/list",
                    Some(&json!(1))
                )
                .await
                .is_err()
        );
        assert!(
            restored
                .admit(
                    "fixture-profile",
                    ClientKind::Codex,
                    &capability,
                    None,
                    "initialize",
                    Some(&json!(1))
                )
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn identity_changes_and_explicit_revocation_never_restore_old_access() {
        for field in 0..3 {
            let (runtime, _, capability, session, _) = connected().await;
            let next = runtime.begin_login("fixture-profile").await;
            let mut changed = auth();
            match field {
                0 => changed.identity.user_id = "other-user".into(),
                1 => changed.identity.identity_id = "other-identity".into(),
                _ => changed.identity.tenant_id = "other-tenant".into(),
            }
            assert!(
                !runtime
                    .commit_auth("fixture-profile", &next, changed)
                    .await
                    .unwrap()
            );
            assert!(
                runtime
                    .existing_capability("fixture-profile", ClientKind::Codex)
                    .await
                    .is_none()
            );
            assert!(
                runtime
                    .check_capability("fixture-profile", ClientKind::Codex, &capability)
                    .await
                    .is_err()
            );
            assert!(!runtime.inner.lock().await.sessions.contains_key(&session));
        }
        let (runtime, _, _, _, _) = connected().await;
        let cancelled = runtime.begin_login("fixture-profile").await;
        runtime.logout("fixture-profile").await;
        assert!(
            runtime
                .protected_snapshot("fixture-profile")
                .await
                .is_none()
        );
        assert!(
            runtime
                .commit_auth("fixture-profile", &cancelled, auth())
                .await
                .is_err()
        );
        let next = runtime.begin_login("fixture-profile").await;
        assert!(
            !runtime
                .commit_auth("fixture-profile", &next, auth())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn existing_active_snapshot_still_requires_verification_after_restore() {
        let (runtime, generation, capability, _, _) = connected().await;
        let snapshot = runtime.protected_snapshot("fixture-profile").await.unwrap();
        assert!(snapshot.get("pendingIdentity").is_none());
        let restored = Runtime::restore(vec![(
            "fixture-profile".into(),
            AuthMode::Application,
            snapshot,
        )]);
        assert!(restored.auth("fixture-profile").await.is_err());
        assert_eq!(
            restored
                .existing_capability("fixture-profile", ClientKind::Codex)
                .await,
            Some(capability)
        );
        restored
            .renew_auth("fixture-profile", &generation, auth())
            .await
            .unwrap();
        assert!(restored.auth("fixture-profile").await.is_ok());
    }
}
