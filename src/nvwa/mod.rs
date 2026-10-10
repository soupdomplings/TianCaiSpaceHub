pub mod adapters;
pub mod auth;
mod bridge;
pub mod config;
mod runtime;
pub mod secrets;
mod server;
pub mod types;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, ensure};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore, watch};

use crate::app_state::{AppState, SharedState};
use config::ConfigStore;
use runtime::Runtime;
use secrets::SecretStore;
use types::{AuthMode, AuthOutcome, AuthResult, NvwaProfile, PasswordLoginInput};

const MANAGEMENT_KEY: &str = "management-bootstrap-v1";
const RENEWAL_RETRY_DELAY_MS: u64 = 60_000;

// This is serialized only inside the OS-protected SecretStore. It must never
// appear in dashboards, diagnostics, Debug output or ordinary configuration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagementInfo {
    pub base_url: String,
    pub capability: String,
    pub instance_id: String,
}

#[derive(Clone)]
pub struct NvwaService {
    pub(crate) inner: Arc<ServiceInner>,
}

pub(crate) struct ServiceInner {
    pub store: ConfigStore,
    pub secrets: SecretStore,
    pub auth: auth::AuthClient,
    pub http: reqwest::Client,
    pub runtime: Runtime,
    pub config_path: PathBuf,
    pub hub_state: Weak<AppState>,
    pub management: ManagementInfo,
    pub running: AtomicBool,
    pub stopping: AtomicBool,
    pub gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    pub browser: Mutex<HashMap<String, server::BrowserTransaction>>,
    pub login_attempts: Mutex<server::LoginAttempts>,
    pub identity_checks: Mutex<HashMap<String, u64>>,
    pub renewal_failures: Mutex<HashMap<String, (String, u64)>>,
    pub persistence: Mutex<()>,
    pub inflight: Arc<Semaphore>,
}

impl NvwaService {
    pub fn new(config_path: &Path, hub_state: SharedState) -> Result<Self> {
        let store = ConfigStore::new(config_path)?;
        let config = store.load()?;
        let secrets = SecretStore::new(store.root().to_path_buf())?;
        let mut records = Vec::new();
        for profile in &config.profiles {
            if let Some(bytes) = secrets.get(&session_key(&profile.id))? {
                if let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes)
                    && record
                        .get("profileFingerprint")
                        .and_then(serde_json::Value::as_str)
                        == Some(profile_fingerprint(profile).as_str())
                {
                    records.push((profile.id.clone(), profile.auth_mode, record));
                } else {
                    secrets.delete(&session_key(&profile.id))?;
                }
            }
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(300))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|_| anyhow::anyhow!("Cannot prepare NVWA MCP connection"))?;
        let management = ManagementInfo {
            base_url: format!("http://127.0.0.1:{}", config.bridge_port),
            capability: runtime::random_capability(),
            instance_id: uuid::Uuid::new_v4().to_string(),
        };
        Ok(Self {
            inner: Arc::new(ServiceInner {
                store,
                secrets,
                auth: auth::AuthClient::new()?,
                http,
                runtime: Runtime::restore(records),
                config_path: config_path.to_path_buf(),
                hub_state: Arc::downgrade(&hub_state),
                management,
                running: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
                gates: Mutex::new(HashMap::new()),
                browser: Mutex::new(HashMap::new()),
                login_attempts: Mutex::new(server::LoginAttempts::default()),
                identity_checks: Mutex::new(HashMap::new()),
                renewal_failures: Mutex::new(HashMap::new()),
                persistence: Mutex::new(()),
                inflight: Arc::new(Semaphore::new(256)),
            }),
        })
    }

    pub fn load_management_info(config_path: &Path) -> Result<Option<ManagementInfo>> {
        let store = ConfigStore::new(config_path)?;
        let secrets = SecretStore::new(store.root().to_path_buf())?;
        let Some(bytes) = secrets.get(MANAGEMENT_KEY)? else {
            return Ok(None);
        };
        let info: ManagementInfo = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("NVWA management bootstrap invalid"))?;
        let url = url::Url::parse(&info.base_url)
            .map_err(|_| anyhow::anyhow!("NVWA management address invalid"))?;
        ensure!(
            url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none(),
            "NVWA management address must be local"
        );
        ensure!(
            info.capability.len() == 64 && info.capability.bytes().all(|v| v.is_ascii_hexdigit()),
            "NVWA management capability invalid"
        );
        ensure!(
            uuid::Uuid::parse_str(&info.instance_id).is_ok(),
            "NVWA management instance invalid"
        );
        Ok(Some(info))
    }

    pub async fn serve(self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let port = self.inner.store.load()?.bridge_port;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let listener = bind_listener(address).await.map_err(|_| {
            anyhow::anyhow!("NVWA bridge port unavailable; check the configured local port")
        })?;
        let bootstrap = serde_json::to_vec(&self.inner.management)
            .map_err(|_| anyhow::anyhow!("Cannot prepare protected NVWA management bootstrap"))?;
        self.inner.secrets.set(MANAGEMENT_KEY, &bootstrap)?;
        self.inner.running.store(true, Ordering::Release);
        let service = self.clone();
        let result = axum::serve(listener, server::router(self.clone()))
            .with_graceful_shutdown(async move {
                if !*shutdown.borrow() {
                    while shutdown.changed().await.is_ok() {
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                }
                service.inner.stopping.store(true, Ordering::Release);
                service.inner.running.store(false, Ordering::Release);
                service.clear_bootstrap();
                service.inner.browser.lock().await.clear();
            })
            .await;
        self.inner.running.store(false, Ordering::Release);
        self.inner.stopping.store(true, Ordering::Release);
        self.clear_bootstrap();
        result.map_err(|_| anyhow::anyhow!("NVWA bridge stopped unexpectedly"))
    }

    pub(crate) fn closing(&self) -> bool {
        self.inner.stopping.load(Ordering::Acquire)
    }

    fn clear_bootstrap(&self) {
        // Clear at the shutdown signal, before graceful HTTP draining. The
        // parent may stop waiting while an unknown remote write is still live.
        // Never remove a replacement daemon's newer bootstrap.
        if let Ok(Some(bytes)) = self.inner.secrets.get(MANAGEMENT_KEY)
            && serde_json::from_slice::<ManagementInfo>(&bytes)
                .ok()
                .is_some_and(|info| info.instance_id == self.inner.management.instance_id)
        {
            let _ = self.inner.secrets.delete(MANAGEMENT_KEY);
        }
    }

    pub(crate) fn profile(&self, id: &str) -> Result<NvwaProfile> {
        self.inner
            .store
            .load()?
            .profiles
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("NVWA profile unavailable"))
    }

    pub(crate) async fn gate(&self, id: &str) -> Arc<Mutex<()>> {
        self.inner
            .gates
            .lock()
            .await
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub(crate) async fn persist_session(&self, profile_id: &str) -> Result<()> {
        let _guard = self.inner.persistence.lock().await;
        if let Some(mut snapshot) = self.inner.runtime.protected_snapshot(profile_id).await {
            snapshot["profileFingerprint"] =
                serde_json::Value::String(profile_fingerprint(&self.profile(profile_id)?));
            let bytes = serde_json::to_vec(&snapshot)
                .map_err(|_| anyhow::anyhow!("Cannot protect NVWA authentication state"))?;
            self.inner.secrets.set(&session_key(profile_id), &bytes)
        } else {
            self.inner.secrets.delete(&session_key(profile_id))
        }
    }

    pub(crate) async fn invalidate(&self, profile_id: &str) -> Result<()> {
        // Order snapshot writes and explicit revocation so a late writer
        // cannot resurrect a logged-out generation after daemon restart.
        let _guard = self.inner.persistence.lock().await;
        self.inner.runtime.logout(profile_id).await;
        self.inner.identity_checks.lock().await.remove(profile_id);
        self.inner.renewal_failures.lock().await.remove(profile_id);
        self.inner
            .browser
            .lock()
            .await
            .retain(|_, transaction| transaction.profile.id != profile_id);
        self.inner.secrets.delete(&session_key(profile_id))
    }

    pub(crate) async fn clear_generation(&self, profile_id: &str, generation: &str) -> Result<()> {
        let _guard = self.inner.persistence.lock().await;
        if !self
            .inner
            .runtime
            .generation_matches(profile_id, generation)
            .await
        {
            return Ok(());
        }
        self.inner.identity_checks.lock().await.remove(profile_id);
        self.inner
            .browser
            .lock()
            .await
            .retain(|_, transaction| transaction.profile.id != profile_id);
        self.inner.secrets.delete(&session_key(profile_id))
    }

    pub(crate) async fn ensure_authenticated(&self, profile: &NvwaProfile) -> Result<()> {
        let gate = self.gate(&profile.id).await;
        let _guard = gate.lock().await;
        ensure!(
            profile_fingerprint(profile) == profile_fingerprint(&self.profile(&profile.id)?),
            "NVWA profile changed; explicit login required"
        );
        let (generation, previous) = self.inner.runtime.raw_auth(&profile.id).await?;
        if let Some(bytes) = self.inner.secrets.get(&session_key(&profile.id))? {
            let matches = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .is_some_and(|snapshot| {
                    snapshot
                        .get("profileFingerprint")
                        .and_then(serde_json::Value::as_str)
                        == Some(profile_fingerprint(profile).as_str())
                });
            if !matches {
                self.invalidate_generation(&profile.id, &generation).await?;
                anyhow::bail!("NVWA profile changed; explicit login required");
            }
        }
        let recent = self
            .inner
            .identity_checks
            .lock()
            .await
            .get(&profile.id)
            .is_some_and(|checked| runtime::now_ms().saturating_sub(*checked) < 60_000);
        let personal_expired = previous
            .personal_token
            .as_ref()
            .and_then(|token| token.expires_at_ms)
            .is_some_and(|expiry| expiry <= runtime::now_ms());
        if recent && !personal_expired && self.inner.runtime.auth(&profile.id).await.is_ok() {
            return Ok(());
        }
        let renewed = if personal_expired
            || previous
                .mcp_token
                .expires_at_ms
                .is_some_and(|expiry| expiry <= runtime::now_ms())
        {
            self.renew_saved_auth(profile, &generation).await?
        } else {
            match self.inner.auth.verify_session(profile, &previous).await {
                Ok(_) => previous,
                Err(error) => match error.downcast_ref::<auth::AuthError>() {
                    Some(auth::AuthError::SessionUnauthorized) => {
                        self.inner
                            .runtime
                            .mark_expired(&profile.id, &generation, &previous.mcp_token.value)
                            .await;
                        self.persist_session(&profile.id).await?;
                        self.renew_saved_auth(profile, &generation).await?
                    }
                    Some(auth::AuthError::IdentityMismatch(_)) => {
                        self.invalidate_generation(&profile.id, &generation).await?;
                        return Err(error);
                    }
                    // A timeout, HTTP 403 or malformed response does not prove
                    // expiry or a changed identity. Fail this request without
                    // discarding the saved login or resubmitting credentials.
                    None => return Err(error),
                },
            }
        };
        if let Err(error) = self
            .inner
            .runtime
            .renew_auth(&profile.id, &generation, renewed)
            .await
        {
            // Cancellation can change the generation during the outbound
            // login. A late result must never revoke its replacement.
            self.invalidate_generation(&profile.id, &generation).await?;
            return Err(error);
        }
        self.inner.renewal_failures.lock().await.remove(&profile.id);
        self.inner
            .identity_checks
            .lock()
            .await
            .insert(profile.id.clone(), runtime::now_ms());
        self.persist_session(&profile.id).await
    }

    async fn invalidate_generation(&self, profile_id: &str, generation: &str) -> Result<()> {
        let _guard = self.inner.persistence.lock().await;
        if !self
            .inner
            .runtime
            .invalidate_generation(profile_id, generation)
            .await
        {
            return Ok(());
        }
        self.inner.identity_checks.lock().await.remove(profile_id);
        self.inner.renewal_failures.lock().await.remove(profile_id);
        self.inner
            .browser
            .lock()
            .await
            .retain(|_, transaction| transaction.profile.id != profile_id);
        self.inner.secrets.delete(&session_key(profile_id))
    }

    async fn renew_saved_auth(
        &self,
        profile: &NvwaProfile,
        generation: &str,
    ) -> Result<AuthResult> {
        if profile.auth_mode == AuthMode::Browser {
            anyhow::bail!("NVWA browser authorization expired; authorize again");
        }
        ensure!(
            self.inner
                .runtime
                .generation_matches(&profile.id, generation)
                .await,
            "Login changed during renewal"
        );
        let reference = profile
            .credential_secret_ref
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("NVWA token expired; explicit login required"))?;
        let secret =
            self.inner.secrets.get(reference)?.ok_or_else(|| {
                anyhow::anyhow!("Saved NVWA credential unavailable; login required")
            })?;
        ensure!(
            !self
                .inner
                .renewal_failures
                .lock()
                .await
                .get(&profile.id)
                .is_some_and(
                    |(failed_generation, retry_at)| failed_generation == generation
                        && *retry_at > runtime::now_ms()
                ),
            "NVWA automatic renewal failed recently; retry after one minute or sign in manually"
        );
        let result = match profile.auth_mode {
            AuthMode::Password => {
                let password = String::from_utf8(secret)
                    .map_err(|_| anyhow::anyhow!("Saved NVWA password invalid"))?;
                match self
                    .inner
                    .auth
                    .password_login(
                        profile,
                        &PasswordLoginInput {
                            password,
                            ..Default::default()
                        },
                    )
                    .await
                {
                    Ok(AuthOutcome::Authenticated(auth)) => Ok(auth),
                    Ok(_) => Err(anyhow::anyhow!(
                        "NVWA requires interactive login verification"
                    )),
                    Err(error) => Err(error),
                }
            }
            AuthMode::Application => self.inner.auth.application_login(profile, &secret).await,
            AuthMode::Browser => unreachable!(),
        };
        if let Err(error) = &result {
            if matches!(
                error.downcast_ref::<auth::AuthError>(),
                Some(auth::AuthError::IdentityMismatch(_))
            ) {
                self.invalidate_generation(&profile.id, generation).await?;
            } else if self
                .inner
                .runtime
                .generation_matches(&profile.id, generation)
                .await
            {
                self.inner.renewal_failures.lock().await.insert(
                    profile.id.clone(),
                    (
                        generation.to_owned(),
                        runtime::now_ms().saturating_add(RENEWAL_RETRY_DELAY_MS),
                    ),
                );
                self.inner
                    .runtime
                    .set_detail(
                        &profile.id,
                        generation,
                        "Automatic renewal failed; retry after one minute or sign in manually",
                    )
                    .await;
            }
        }
        result
    }

    pub(crate) async fn local_request(&self, headers: &HeaderMap) -> bool {
        let authority = self
            .inner
            .management
            .base_url
            .strip_prefix("http://")
            .unwrap_or_default();
        if headers.get("host").and_then(|v| v.to_str().ok()) != Some(authority) {
            return false;
        }
        if let Some(origin) = headers.get("origin") {
            return origin.to_str().ok() == Some(self.inner.management.base_url.as_str());
        }
        true
    }
}

fn session_key(profile_id: &str) -> String {
    format!("runtime-session-{profile_id}")
}

pub(crate) fn profile_fingerprint(profile: &NvwaProfile) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(profile).unwrap_or_default(),
    ))
}

async fn bind_listener(addr: std::net::SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};
        let listener = std::net::TcpListener::bind(addr)?;
        if unsafe {
            SetHandleInformation(listener.as_raw_socket() as HANDLE, HANDLE_FLAG_INHERIT, 0)
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        listener.set_nonblocking(true)?;
        tokio::net::TcpListener::from_std(listener)
    }
    #[cfg(not(windows))]
    {
        tokio::net::TcpListener::bind(addr).await
    }
}
