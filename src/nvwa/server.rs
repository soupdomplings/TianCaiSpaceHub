use std::{collections::HashMap, path::PathBuf, sync::atomic::Ordering};

use anyhow::{Result, bail, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

use super::{
    NvwaService,
    adapters::{self, AdapterContext, AdapterOperation, AdapterTarget},
    bridge,
    config::validate_profile,
    profile_fingerprint,
    runtime::{constant_eq, now_ms, random_capability},
    types::{AuthOutcome, AuthResult, ClientKind, NvwaProfile, PasswordLoginInput},
};

const BROWSER_TTL_MS: u64 = 10 * 60 * 1000;
const MAX_LOGIN_ATTEMPTS: usize = 4096;

#[derive(Default)]
pub(crate) struct LoginAttempts {
    cancelled: HashMap<(String, String), u64>,
    active: HashMap<String, String>,
}

impl LoginAttempts {
    fn prune(&mut self) {
        self.cancelled.retain(|_, expiry| *expiry > now_ms());
    }
    fn cancel(&mut self, profile_id: &str, attempt_id: &str) -> Result<()> {
        self.prune();
        let key = (profile_id.to_owned(), attempt_id.to_owned());
        ensure!(
            self.cancelled.contains_key(&key) || self.cancelled.len() < MAX_LOGIN_ATTEMPTS,
            "Too many cancelled login attempts; wait before starting another login"
        );
        self.cancelled.insert(key, now_ms() + BROWSER_TTL_MS);
        Ok(())
    }
}

pub(crate) struct BrowserTransaction {
    pub profile: NvwaProfile,
    generation: String,
    app_secret: Vec<u8>,
    remember: bool,
    expires_at: u64,
}

pub(crate) fn router(service: NvwaService) -> Router {
    // Deliberately no shared TraceLayer or access-log middleware: callback
    // paths and query strings carry one-time authorization material.
    Router::new()
        .route("/manage/status", get(status))
        .route("/manage/{*operation}", post(manage))
        .route("/callback/{state}", get(callback))
        .route(
            "/mcp/{profile}/{client}",
            post(bridge::post)
                .delete(bridge::delete)
                .get(bridge::unsupported),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(service)
}

fn answer(value: Value) -> Response {
    let mut response = Json(json!({"ok":true,"data":value})).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    let mut response = (
        status,
        Json(json!({"ok":false,"error":{"code":code,"message":message}})),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

async fn authorized(service: &NvwaService, headers: &HeaderMap) -> bool {
    service.local_request(headers).await
        && bridge::bearer(headers)
            .is_some_and(|value| constant_eq(value, &service.inner.management.capability))
}

async fn status(State(service): State<NvwaService>, headers: HeaderMap) -> Response {
    if !authorized(&service, &headers).await {
        return failure(
            StatusCode::UNAUTHORIZED,
            "authorization_required",
            "NVWA management authorization required",
        );
    }
    match service.inner.store.load() {
        Ok(config) => {
            let profiles = service.inner.runtime.status(&config.profiles).await;
            let mut value = serde_json::to_value(&config).unwrap_or(Value::Null);
            value["revision"] = Value::String(config.revision.clone());
            answer(
                json!({"config":value,"profiles":profiles,"bridge":{"running":service.inner.running.load(Ordering::Acquire),
                "baseUrl":service.inner.management.base_url,"detail":"Independent authenticated loopback MCP bridge"}}),
            )
        }
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "configuration_unavailable",
            "NVWA configuration unavailable",
        ),
    }
}

async fn manage(
    State(service): State<NvwaService>,
    Path(operation): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&service, &headers).await {
        return failure(
            StatusCode::UNAUTHORIZED,
            "authorization_required",
            "NVWA management authorization required",
        );
    }
    if service.closing() {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "shutting_down",
            "Hub is shutting down",
        );
    }
    if !body.is_object() {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "JSON object required",
        );
    }
    match command(&service, &operation, &body).await {
        Ok(data) => answer(data),
        Err(error) => {
            // Providers/adapters use fixed errors and never raw remote bodies.
            // Error text is nevertheless bounded and filtered before UI output.
            let message = safe_message(&error.to_string());
            failure(StatusCode::BAD_REQUEST, "operation_failed", &message)
        }
    }
}

fn safe_message(message: &str) -> String {
    if message.contains("http://")
        || message.contains("https://")
        || message.contains("Bearer ")
        || message.to_ascii_lowercase().contains("authorization-")
    {
        return "NVWA operation failed; check configuration or login again".into();
    }
    message
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}

fn required<'a>(body: &'a Value, key: &str) -> Result<&'a str> {
    let value = body
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Required field missing: {key}"))?;
    ensure!(
        !value.is_empty() && value.len() <= 16_384,
        "Invalid field: {key}"
    );
    Ok(value)
}

fn revision(body: &Value) -> Result<&str> {
    body.get("expectedRevision")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Configuration revision required"))
}

async fn command(service: &NvwaService, operation: &str, body: &Value) -> Result<Value> {
    if operation == "profile/save" {
        return save_profile(service, body).await;
    }
    let profile_id = required(body, "profileId")?;
    let preliminary_profile = service.profile(profile_id)?;
    if matches!(operation, "logout" | "login/cancel") {
        let auth = service.inner.runtime.raw_auth(profile_id).await.ok();
        let generation = {
            // Admission and cancellation share this short critical section.
            // A cancellation which arrives before its login HTTP request is
            // retained, so scheduling cannot resurrect the cancelled attempt.
            let mut attempts = service.inner.login_attempts.lock().await;
            let attempt_id = if operation == "login/cancel" {
                body.get("loginAttemptId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
            } else {
                None
            };
            if let Some(attempt_id) = attempt_id {
                validate_attempt(attempt_id)?;
                attempts.cancel(profile_id, attempt_id)?;
                if attempts
                    .active
                    .get(profile_id)
                    .is_some_and(|active| active != attempt_id)
                {
                    return Ok(
                        json!({"profileId":profile_id,"state":"login_attempt_cancelled",
                        "detail":"The specified earlier login attempt was cancelled"}),
                    );
                }
            } else if let Some(active) = attempts.active.get(profile_id).cloned() {
                attempts.cancel(profile_id, &active)?;
            }
            attempts.active.remove(profile_id);
            service.inner.runtime.begin_auth(profile_id).await
        };
        service.clear_generation(profile_id, &generation).await?;
        service
            .inner
            .runtime
            .set_detail(
                profile_id,
                &generation,
                "Logged out; previous client access revoked",
            )
            .await;
        let remote_logout = if operation == "logout" {
            if let Some((_, auth)) = auth {
                service
                    .inner
                    .auth
                    .logout(&preliminary_profile, &auth)
                    .await
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };
        return Ok(
            json!({"profileId":profile_id,"state":"logged_out","remoteLogoutConfirmed":remote_logout,
            "detail":"Local sessions and previous client credentials revoked"}),
        );
    }
    let gate = service.gate(profile_id).await;
    // No outbound call holds the config/global runtime lock. Each explicit
    // management mutation is serialized only for its own profile.
    let _guard = gate.lock().await;
    let profile = service.profile(profile_id)?;
    match operation {
        "profile/delete" => {
            let context = adapter_context(service, ClientKind::Codex).await?;
            ensure!(
                !adapters::has_managed_profile(&context, profile_id).await?,
                "Remove all managed MCP client entries before deleting this environment"
            );
            let mut config = service.inner.store.load()?;
            config.profiles.retain(|p| p.id != profile_id);
            let saved = service.inner.store.save(&config, revision(body)?)?;
            service.invalidate(profile_id).await?;
            if let Some(reference) = profile.credential_secret_ref {
                service.inner.secrets.delete(&reference)?;
            }
            Ok(json!({"profileId":profile_id,"deleted":true,"config":saved}))
        }
        "login/password" => {
            let password = required(body, "password")?.to_owned();
            let mut ext = body.get("extInfo").cloned().unwrap_or_else(|| json!({}));
            ensure!(
                ext.is_object(),
                "Password verification fields must be an object"
            );
            for key in ["verifyId", "verifyCode", "twofactorSessionId", "validCode"] {
                if let Some(value) = body.get(key).filter(|v| !v.is_null()) {
                    ext[key] = value.clone();
                }
            }
            let input = PasswordLoginInput::from_ext_info(password.clone(), Some(&ext))?;
            let generation = begin_login(service, profile_id, body).await?;
            let outcome = service.inner.auth.password_login(&profile, &input).await;
            match outcome {
                Ok(AuthOutcome::Authenticated(auth)) => {
                    ensure!(
                        service
                            .inner
                            .runtime
                            .generation_matches(profile_id, &generation)
                            .await,
                        "Login was cancelled or superseded"
                    );
                    remember(
                        service,
                        &profile,
                        body.get("rememberPassword")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        password.as_bytes(),
                    )?;
                    commit(service, &profile, &generation, auth).await
                }
                Ok(AuthOutcome::CaptchaRequired { message }) => {
                    service
                        .inner
                        .runtime
                        .set_detail(profile_id, &generation, "Captcha verification required")
                        .await;
                    Ok(json!({"state":"captcha_required","detail":safe_message(&message)}))
                }
                Ok(AuthOutcome::TwoFactorRequired {
                    session_id,
                    channel,
                }) => {
                    service
                        .inner
                        .runtime
                        .set_detail(profile_id, &generation, "Two factor verification required")
                        .await;
                    Ok(
                        json!({"state":"two_factor_required","detail":"Enter the verification code to continue",
                        "twofactorSessionId":session_id,"channel":channel}),
                    )
                }
                Ok(AuthOutcome::PasswordChangeRequired { code, message }) => {
                    service
                        .inner
                        .runtime
                        .set_detail(profile_id, &generation, "Password change required in NVWA")
                        .await;
                    Ok(
                        json!({"state":"password_change_required","detail":safe_message(&message),"code":code}),
                    )
                }
                Err(error) => {
                    service
                        .inner
                        .runtime
                        .set_detail(
                            profile_id,
                            &generation,
                            "Login failed; previous client access remains revoked",
                        )
                        .await;
                    Err(error)
                }
            }
        }
        "login/twofactor/send" => {
            service
                .inner
                .auth
                .send_twofactor(&profile, required(body, "twofactorSessionId")?)
                .await?;
            Ok(json!({"state":"two_factor_required","detail":"Verification code requested"}))
        }
        "login/application" => {
            let secret = application_secret(service, &profile, body)?;
            let generation = begin_login(service, profile_id, body).await?;
            let auth = service
                .inner
                .auth
                .application_login(&profile, &secret)
                .await?;
            ensure!(
                service
                    .inner
                    .runtime
                    .generation_matches(profile_id, &generation)
                    .await,
                "Login was cancelled or superseded"
            );
            remember(
                service,
                &profile,
                body.get("rememberSecret")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                &secret,
            )?;
            commit(service, &profile, &generation, auth).await
        }
        "login/browser" => {
            let secret = application_secret(service, &profile, body)?;
            let generation = begin_login(service, profile_id, body).await?;
            let state = random_capability();
            let callback = format!("{}/callback/{state}", service.inner.management.base_url);
            let authorize = service
                .inner
                .auth
                .begin_browser(&profile, &callback, &state)?;
            let mut transactions = service.inner.browser.lock().await;
            transactions.retain(|_, transaction| transaction.expires_at > now_ms());
            ensure!(
                transactions.len() < 64,
                "Too many browser authorization transactions"
            );
            transactions.insert(
                state,
                BrowserTransaction {
                    profile: profile.clone(),
                    generation: generation.clone(),
                    app_secret: secret,
                    remember: body
                        .get("rememberSecret")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    expires_at: now_ms() + BROWSER_TTL_MS,
                },
            );
            service
                .inner
                .runtime
                .set_detail(
                    profile_id,
                    &generation,
                    "Waiting for one-time browser authorization",
                )
                .await;
            Ok(
                json!({"state":"browser_authorization_pending","authorizeUrl":authorize,"expiresAtMs":now_ms()+BROWSER_TTL_MS,
                "detail":"Complete authorization in NVWA; callback must match this one-time transaction"}),
            )
        }
        "tools/detect" => {
            drop(_guard);
            service.ensure_authenticated(&profile).await?;
            let client = body
                .get("clientKind")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(bridge::client_kind)
                .transpose()?;
            let mut detected = bridge::detect(service, &profile, client).await?;
            if body.get("clientKind").and_then(Value::as_str) == Some("tiangong") {
                detected["tiangongDirectoryUpdated"] = Value::Bool(
                    update_tiangong_catalog(service, &profile, &detected)
                        .await
                        .is_ok(),
                );
            }
            Ok(detected)
        }
        value if value.starts_with("client/") => {
            drop(_guard);
            client_operation(service, &profile, value.trim_start_matches("client/"), body).await
        }
        _ => bail!("Unknown NVWA management operation"),
    }
}

fn validate_attempt(attempt_id: &str) -> Result<()> {
    ensure!(
        uuid::Uuid::parse_str(attempt_id).is_ok(),
        "Login attempt ID must be a UUID"
    );
    Ok(())
}

async fn begin_login(service: &NvwaService, profile_id: &str, body: &Value) -> Result<String> {
    let attempt_id = required(body, "loginAttemptId")?;
    validate_attempt(attempt_id)?;
    let generation = {
        let mut attempts = service.inner.login_attempts.lock().await;
        attempts.prune();
        ensure!(
            !attempts
                .cancelled
                .contains_key(&(profile_id.to_owned(), attempt_id.to_owned())),
            "Login attempt was cancelled"
        );
        if let Some(previous) = attempts.active.get(profile_id).cloned() {
            ensure!(
                previous != attempt_id,
                "Login attempt was already submitted"
            );
            attempts.cancel(profile_id, &previous)?;
        }
        attempts
            .active
            .insert(profile_id.to_owned(), attempt_id.to_owned());
        service.inner.runtime.begin_auth(profile_id).await
    };
    // OS protection/filesystem work and outbound calls are outside the short
    // admission lock. Cleanup refuses to clear a later generation.
    service.clear_generation(profile_id, &generation).await?;
    ensure!(
        service
            .inner
            .runtime
            .generation_matches(profile_id, &generation)
            .await,
        "Login attempt was cancelled"
    );
    Ok(generation)
}

fn application_secret(
    service: &NvwaService,
    profile: &NvwaProfile,
    body: &Value,
) -> Result<Vec<u8>> {
    if let Some(secret) = body
        .get("clientSecret")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        ensure!(secret.len() <= 16_384, "NVWA application secret too large");
        return Ok(secret.as_bytes().to_vec());
    }
    let reference = profile
        .credential_secret_ref
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("NVWA application secret required"))?;
    service
        .inner
        .secrets
        .get(reference)?
        .ok_or_else(|| anyhow::anyhow!("Saved NVWA application secret unavailable"))
}

fn remember(
    service: &NvwaService,
    profile: &NvwaProfile,
    enabled: bool,
    secret: &[u8],
) -> Result<()> {
    let mut config = service.inner.store.load()?;
    let revision = config.revision.clone();
    let saved = config
        .profiles
        .iter_mut()
        .find(|p| p.id == profile.id)
        .ok_or_else(|| anyhow::anyhow!("NVWA environment changed"))?;
    let key = format!("credential-{}", profile.id);
    if enabled {
        service.inner.secrets.set(&key, secret)?;
        saved.credential_secret_ref = Some(key);
    } else {
        if let Some(reference) = saved.credential_secret_ref.take() {
            service.inner.secrets.delete(&reference)?;
        }
    }
    service.inner.store.save(&config, &revision)?;
    Ok(())
}

async fn commit(
    service: &NvwaService,
    profile: &NvwaProfile,
    generation: &str,
    auth: AuthResult,
) -> Result<Value> {
    let identity = auth.identity.clone();
    service
        .inner
        .runtime
        .commit_auth(&profile.id, generation, auth)
        .await?;
    service
        .inner
        .identity_checks
        .lock()
        .await
        .insert(profile.id.clone(), now_ms());
    if let Err(error) = service.persist_session(&profile.id).await {
        service.invalidate(&profile.id).await?;
        return Err(error);
    }
    Ok(
        json!({"profileId":profile.id,"state":"authenticated","identity":identity,
        "detail":"Verified login; explicitly apply each client to grant access to this identity"}),
    )
}

async fn save_profile(service: &NvwaService, body: &Value) -> Result<Value> {
    let mut value = body
        .get("profile")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("NVWA profile required"))?;
    ensure!(value.is_object(), "NVWA profile must be an object");
    if value
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        value["id"] = Value::String(uuid::Uuid::new_v4().to_string());
    }
    let id = value["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("NVWA profile ID invalid"))?
        .to_owned();
    let gate = service.gate(&id).await;
    let _guard = gate.lock().await;
    let mut config = service.inner.store.load()?;
    if let Some(existing) = config.profiles.iter().find(|p| p.id == id) {
        let mut merged = serde_json::to_value(existing)?;
        for (key, value) in value.as_object().unwrap() {
            if key != "credentialSecretRef" {
                merged[key] = value.clone();
            }
        }
        value = merged;
    } else {
        value.as_object_mut().unwrap().remove("credentialSecretRef");
    }
    let mut profile: NvwaProfile = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("NVWA profile fields invalid"))?;
    validate_profile(&profile)?;
    let changed = config
        .profiles
        .iter()
        .find(|p| p.id == id)
        .is_none_or(|p| profile_fingerprint(p) != profile_fingerprint(&profile));
    // A saved password/application secret belongs to its old authentication
    // target, user and mode. Never carry it into a changed environment.
    let old_reference = if changed {
        profile.credential_secret_ref.take()
    } else {
        None
    };
    config.profiles.retain(|p| p.id != id);
    config.profiles.push(profile.clone());
    let saved = service.inner.store.save(&config, revision(body)?)?;
    if changed {
        service.invalidate(&id).await?;
        if let Some(reference) = old_reference {
            service.inner.secrets.delete(&reference)?;
        }
    }
    Ok(json!({"profile":profile,"config":saved}))
}

async fn adapter_context(service: &NvwaService, client: ClientKind) -> Result<AdapterContext> {
    let desktop = if client == ClientKind::Tiangong {
        let state = service
            .inner
            .hub_state
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Hub daemon unavailable"))?;
        let config = state.config.lock().await.gmclaw_bridge.clone();
        let status = crate::gmclaw_runtime::status(&config).await;
        ensure!(
            status.state == crate::gmclaw_runtime::GmClawConnectionState::Connected,
            "TianGong official local connection is not ready; start or authorize TianGong first"
        );
        let authorization = crate::gmclaw_runtime::connection_authorization(&config).await?;
        Some(crate::gmclaw_desktop::DesktopClient::with_authorization(
            &config.endpoint,
            authorization,
        )?)
    } else {
        None
    };
    Ok(AdapterContext {
        config_path: service.inner.config_path.clone(),
        secrets: service.inner.secrets.clone(),
        desktop,
    })
}

async fn client_operation(
    service: &NvwaService,
    profile: &NvwaProfile,
    operation: &str,
    body: &Value,
) -> Result<Value> {
    let client = bridge::client_kind(required(body, "clientKind")?)?;
    let target = AdapterTarget {
        profile_id: profile.id.clone(),
        client,
        server_name: body
            .get("serverName")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .unwrap_or(adapters::managed_server_name(&profile.id)?),
        override_path: body
            .get("overridePath")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from),
    };
    let context = adapter_context(service, client).await?;
    match operation {
        "inspect" => {
            let mut value = serde_json::to_value(adapters::inspect(&context, &target).await?)?;
            value["bridgeCheckedAtMs"] = service
                .inner
                .runtime
                .client_check(&profile.id, client)
                .await
                .map(Value::from)
                .unwrap_or(Value::Null);
            if !value["bridgeCheckedAtMs"].is_null() {
                value["bridgeState"] = "tools_discovered".into();
            } else {
                value["bridgeState"] = "unknown".into();
            }
            Ok(value)
        }
        "preview" => {
            let operation = match body
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("apply")
            {
                "apply" => AdapterOperation::Apply,
                "remove" => AdapterOperation::Remove,
                "restore" => AdapterOperation::Restore,
                _ => bail!("Unknown client operation"),
            };
            Ok(serde_json::to_value(
                adapters::preview(&context, &target, operation).await?,
            )?)
        }
        "apply" => {
            service.ensure_authenticated(profile).await?;
            let gate = service.gate(&profile.id).await;
            let _guard = gate.lock().await;
            let credential = service
                .inner
                .runtime
                .capability(&profile.id, client)
                .await?;
            service.persist_session(&profile.id).await?;
            let endpoint = format!(
                "{}/mcp/{}/{}",
                service.inner.management.base_url,
                profile.id,
                client.as_str()
            );
            let outcome = adapters::apply(
                &context,
                &target,
                required(body, "expectedFingerprint")?,
                &endpoint,
                &credential,
            )
            .await?;
            Ok(serde_json::to_value(outcome)?)
        }
        "remove" => {
            let gate = service.gate(&profile.id).await;
            let _guard = gate.lock().await;
            let outcome =
                adapters::remove(&context, &target, required(body, "expectedFingerprint")?).await?;
            service
                .inner
                .runtime
                .revoke_client(&profile.id, client)
                .await;
            service.persist_session(&profile.id).await?;
            Ok(serde_json::to_value(outcome)?)
        }
        "restore" => {
            let gate = service.gate(&profile.id).await;
            let _guard = gate.lock().await;
            let outcome = adapters::restore(
                &context,
                &target,
                required(body, "expectedFingerprint")?,
                required(body, "backupRef")?,
            )
            .await?;
            service
                .inner
                .runtime
                .revoke_client(&profile.id, client)
                .await;
            service.persist_session(&profile.id).await?;
            Ok(serde_json::to_value(outcome)?)
        }
        _ => bail!("Unknown client operation"),
    }
}

async fn update_tiangong_catalog(
    service: &NvwaService,
    profile: &NvwaProfile,
    detected: &Value,
) -> Result<()> {
    // Explicit detect updates only an enabled entry still owned by Hub and
    // carrying this generation's local bearer. Native test flags are not proof.
    let gate = service.gate(&profile.id).await;
    let _guard = gate.lock().await;
    let (generation, _) = service.inner.runtime.auth(&profile.id).await?;
    ensure!(
        detected.get("authGeneration").and_then(Value::as_str) == Some(generation.as_str()),
        "Login changed after bridge detection"
    );
    let bearer = service
        .inner
        .runtime
        .existing_capability(&profile.id, ClientKind::Tiangong)
        .await
        .ok_or_else(|| anyhow::anyhow!("Apply TianGong client access before detection"))?;
    let context = adapter_context(service, ClientKind::Tiangong).await?;
    let name = adapters::managed_server_name(&profile.id)?;
    let target = AdapterTarget {
        profile_id: profile.id.clone(),
        client: ClientKind::Tiangong,
        server_name: name.clone(),
        override_path: None,
    };
    let status = adapters::inspect(&context, &target).await?;
    ensure!(
        status.owned && !status.modified && status.enabled == Some(true),
        "TianGong entry is disabled or has changed"
    );
    let desktop = context
        .desktop
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("TianGong connection unavailable"))?;
    let entry = desktop
        .mcp_connection(&name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("TianGong entry unavailable"))?;
    ensure!(
        entry.get("status").and_then(Value::as_str) == Some("active")
            && adapters::tiangong_target_fingerprint(&entry)? == status.target_fingerprint,
        "TianGong entry changed after inspection; inspect and detect again"
    );
    let headers = entry.get("header_config").cloned().unwrap_or(Value::Null);
    let headers: Value = if let Some(text) = headers.as_str() {
        serde_json::from_str(text).unwrap_or(Value::Null)
    } else {
        headers
    };
    ensure!(
        headers
            .get("Authorization")
            .and_then(Value::as_str)
            .is_some_and(|value| constant_eq(value, &format!("Bearer {bearer}"))),
        "TianGong entry belongs to a previous login"
    );
    ensure!(
        service
            .inner
            .runtime
            .generation_matches(&profile.id, &generation)
            .await,
        "Login changed during detection"
    );
    desktop.mcp_update_connection(&name,json!({"is_connected":1,"tools_json":detected.get("tools").cloned().unwrap_or_else(|| json!([])),
        "conn_last_error":"","retry_count":0})).await
}

async fn callback(
    State(service): State<NvwaService>,
    Path(state): Path<String>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    if !service.local_request(&headers).await || state.len() != 64 {
        return callback_page(false);
    }
    let mut ticket = None;
    let mut returned_state = None;
    let query = query.unwrap_or_default();
    if query.len() > 8192 {
        return callback_page(false);
    }
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "code" | "ticket" | "ticketId" => {
                if ticket.is_some() {
                    return callback_page(false);
                }
                ticket = Some(value.into_owned());
            }
            "state" => {
                if returned_state.is_some() {
                    return callback_page(false);
                }
                returned_state = Some(value.into_owned());
            }
            _ => {}
        }
    }
    if returned_state
        .as_ref()
        .is_some_and(|value| !constant_eq(value, &state))
    {
        return callback_page(false);
    }
    let Some(ticket) =
        ticket.filter(|v| !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
    else {
        return callback_page(false);
    };
    // Remove before network exchange: valid callback states are single use,
    // including failed exchanges. Cancellation/new login removes old states.
    let Some(transaction) = service.inner.browser.lock().await.remove(&state) else {
        return callback_page(false);
    };
    if transaction.expires_at <= now_ms() || service.closing() {
        return callback_page(false);
    }
    let gate = service.gate(&transaction.profile.id).await;
    let _guard = gate.lock().await;
    let success = async {
        ensure!(
            service
                .inner
                .runtime
                .generation_matches(&transaction.profile.id, &transaction.generation)
                .await,
            "NVWA authorization was superseded"
        );
        ensure!(
            profile_fingerprint(&service.profile(&transaction.profile.id)?)
                == profile_fingerprint(&transaction.profile),
            "NVWA profile changed during authorization"
        );
        let auth = service
            .inner
            .auth
            .complete_browser(&transaction.profile, &ticket, &transaction.app_secret)
            .await?;
        ensure!(
            service
                .inner
                .runtime
                .generation_matches(&transaction.profile.id, &transaction.generation)
                .await,
            "NVWA authorization was cancelled"
        );
        remember(
            &service,
            &transaction.profile,
            transaction.remember,
            &transaction.app_secret,
        )?;
        commit(
            &service,
            &transaction.profile,
            &transaction.generation,
            auth,
        )
        .await?;
        Ok::<_, anyhow::Error>(())
    }
    .await
    .is_ok();
    if !success {
        service
            .inner
            .runtime
            .set_detail(
                &transaction.profile.id,
                &transaction.generation,
                "Browser authorization failed or superseded; start again",
            )
            .await;
    }
    callback_page(success)
}

fn callback_page(success: bool) -> Response {
    let message = if success {
        "NVWA authorization completed. Return to TianCaiSpaceHub."
    } else {
        "NVWA authorization could not be completed. Return to TianCaiSpaceHub and start again."
    };
    Response::builder()
        .status(if success {
            StatusCode::OK
        } else {
            StatusCode::BAD_REQUEST
        })
        .header("content-type", "text/html; charset=utf-8")
        .header("cache-control", "no-store")
        .header("referrer-policy", "no-referrer")
        .header(
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        )
        .body(axum::body::Body::from(format!(
            "<!doctype html><meta charset=utf-8><title>NVWA authorization</title><p>{message}</p>"
        )))
        .unwrap()
}
