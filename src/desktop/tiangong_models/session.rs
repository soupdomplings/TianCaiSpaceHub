//! User-authorized TianGong session integration. No install secret is bundled,
//! exported, or persisted. Only the installed built-in adapter's exact endpoint
//! and headers are accepted; JavaScript is never executed.
use super::*;
use reqwest::blocking::Client;
use std::io::{Read, Seek, SeekFrom};

pub(super) const AUTH_BASE: &str = "https://tiangong.jiuqi.com.cn/console";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LoginSession {
    pub access_token: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub auth_base_url: String,
    #[serde(default)]
    pub expires_at: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub revision: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Import the native adapter without making personal-account login a gate.
    #[serde(default)]
    pub native: bool,
}

pub(super) fn local_session(database: &Path) -> Result<Option<LoginSession>> {
    if !database.is_file() {
        return Ok(None);
    }
    let db = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| anyhow::anyhow!("无法读取天工登录状态，请稍后重试"))?;
    db.busy_timeout(Duration::from_millis(300))?;
    use rusqlite::OptionalExtension;
    let value: Option<String> = match db
        .query_row(
            "SELECT value_json FROM app_settings WHERE key = 'claw_oauth_session'",
            [],
            |r| r.get(0),
        )
        .optional()
    {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    value
        .filter(|v| !v.is_empty())
        .map(|v| {
            ensure!(
                v.len() <= 128 * 1024,
                "天工登录状态格式不受支持，请在 Hub 重新登录"
            );
            serde_json::from_str::<LoginSession>(&v)
                .map_err(|_| anyhow::anyhow!("天工登录状态不完整，请在 Hub 重新登录"))
        })
        .transpose()
}

fn http(config: &AppConfig) -> Result<Client> {
    crate::outbound_http::apply_blocking_proxy(
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30)),
        &config.outbound_proxy,
        config.local_listen_port(),
    )?
    .build()
    .map_err(|_| anyhow::anyhow!("无法初始化天工登录连接，请检查网络设置"))
}

fn response_json(response: reqwest::blocking::Response) -> Result<Value> {
    let status = response.status();
    ensure!(
        !matches!(status.as_u16(), 401 | 403),
        "天工登录已失效或账号无权访问，请点击“天工登录”重新登录"
    );
    ensure!(
        status.is_success(),
        "天工登录服务返回 HTTP {}，请稍后重试；未修改原模型",
        status.as_u16()
    );
    ensure!(
        response.content_length().is_none_or(|n| n <= 1024 * 1024),
        "天工登录响应过大，请稍后重试"
    );
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("天工登录响应中断，请重新登录"))?;
    ensure!(bytes.len() <= 1024 * 1024, "天工登录响应过大，请稍后重试");
    serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("天工登录响应格式不兼容，请稍后重试"))
}

pub(super) fn valid_session(session: &LoginSession) -> Result<()> {
    ensure!(
        session.auth_base_url.trim_end_matches('/') == AUTH_BASE,
        "当前天工登录环境不受支持，请使用天工 Work 正式环境账号登录"
    );
    ensure!(
        !session.access_token.is_empty()
            && session.access_token.len() <= 8192
            && session.access_token.is_ascii()
            && !session.access_token.chars().any(char::is_whitespace),
        "天工登录信息不完整，请重新登录"
    );
    ensure!(
        session.expires_at == 0
            || session.expires_at > crate::ai_gateway::request_log::now_ms() as u64,
        "天工登录已过期，请点击“天工登录”重新登录"
    );
    Ok(())
}

fn verify_with(http: &Client, base: &str, session: &mut LoginSession) -> Result<()> {
    valid_session(session)?;
    let mut token = reqwest::header::HeaderValue::from_str(&session.access_token)
        .map_err(|_| anyhow::anyhow!("天工登录信息格式无效，请重新登录"))?;
    token.set_sensitive(true);
    let payload = response_json(
        http.get(format!("{base}/nvwa/getLoginContext"))
            .header("Authorization", token)
            .send()
            .map_err(|_| anyhow::anyhow!("无法验证天工登录，请检查网络后重试"))?,
    )?;
    ensure!(
        payload
            .get("code")
            .is_none_or(|v| v.is_null() || v.as_i64() == Some(0) || v.as_str() == Some("0")),
        "天工登录未通过验证，请点击“天工登录”重新登录"
    );
    let context = &payload["context"];
    let id = context["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("天工未返回账号信息，请重新登录")?;
    validate_credential("session-placeholder", id)
        .map_err(|_| anyhow::anyhow!("天工账号身份格式不兼容，请重新登录"))?;
    ensure!(
        session.user_id.is_empty() || session.user_id == id,
        "天工账号已变化，请重新登录，原模型未修改"
    );
    session.user_id = id.to_string();
    session.display_name = ["name", "displayName", "username", "loginName"]
        .iter()
        .find_map(|key| context[*key].as_str())
        .unwrap_or("天工账号")
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    Ok(())
}

fn password_login(
    http: &Client,
    base: &str,
    username: &str,
    password: &str,
) -> Result<LoginSession> {
    ensure!(
        !username.trim().is_empty()
            && username.len() <= 256
            && !password.is_empty()
            && password.len() <= 4096,
        "请填写天工账号和密码"
    );
    let key = response_json(
        http.get(format!("{base}/anon/framework/api/encrypt/key"))
            .send()
            .map_err(|_| anyhow::anyhow!("无法获取天工登录公钥，请检查网络后重试"))?,
    )?;
    let public = key["text"]
        .as_str()
        .filter(|s| s.len() <= 16 * 1024)
        .context("天工登录公钥格式不兼容，请稍后重试")?;
    let encrypt = |value: &str| crate::nvwa::auth::encrypt_login_field(public, "3", value);
    let payload = response_json(http.post(format!("{base}/nvwa/login"))
        .json(&json!({"username":encrypt(username.trim())?,"pwd":encrypt(password)?,"tenant":"__default_tenant__","extInfo":{},"encryptType":"3L"}))
        .send().map_err(|_| anyhow::anyhow!("天工登录未完成，请检查网络；不会自动重试登录"))?)?;
    ensure!(
        payload["code"].as_i64() == Some(0) || payload["code"].as_str() == Some("0"),
        "天工未接受本次登录，请检查账号密码；需要验证码或企业授权时请先在天工完成登录，再点击“读取天工登录”"
    );
    let mut session = LoginSession {
        access_token: payload["token"]
            .as_str()
            .context("天工未返回登录信息，请重新登录")?
            .into(),
        user_id: String::new(),
        display_name: username.trim().into(),
        auth_base_url: AUTH_BASE.into(),
        expires_at: 0,
    };
    verify_with(http, base, &mut session)?;
    Ok(session)
}

pub fn connect(path: &Path, request: ConnectRequest) -> Result<Value> {
    connect_at(path, &crate::gmclaw_config::config_path(), request, None)
}

fn connect_at(
    path: &Path,
    database: &Path,
    request: ConnectRequest,
    fixture: Option<(&str, &Path)>,
) -> Result<Value> {
    let config = AppConfig::load_or_default(&path.to_path_buf())?;
    ensure!(
        config.revision.as_deref() == Some(request.revision.as_str()),
        "Hub 配置已变化，请重新读取后登录"
    );
    let archive = if let Some((_, archive)) = fixture {
        archive.to_path_buf()
    } else {
        crate::gmclaw_runtime::model_archive(&config.gmclaw_bridge)?
    };
    // Reject unsupported native adapters before sending account credentials.
    let _adapter = installed_headers(&archive)?;
    if request.native {
        ensure!(
            request.username.is_empty() && request.password.is_empty(),
            "原生接入不能同时提交账号密码"
        );
        let models = discover_at(database)?;
        let credential = saved_credential(path, &config).ok().unwrap_or_else(|| {
            let session = local_session(database)
                .ok()
                .flatten()
                .filter(|s| valid_session(s).is_ok());
            Credential {
                pat: String::new(),
                user_id: session
                    .as_ref()
                    .map(|s| s.user_id.clone())
                    .unwrap_or_default(),
                session,
            }
        });
        let imported = enable_authorized_at(
            path,
            database,
            EnableRequest {
                revision: request.revision,
                catalog_revision: catalog_revision(&models)?,
                model_ids: models.iter().map(|m| m.id.clone()).collect(),
                pat: String::new(),
                user_id: String::new(),
            },
            Some(credential),
        )?;
        let mut view = account_view(
            path,
            &AppConfig::load_or_default(&path.to_path_buf())?,
            database,
        );
        view["parameterSyncErrors"] = imported["parameterSyncErrors"].clone();
        return Ok(view);
    }
    let http = http(&config)?;
    let base = fixture.map(|(base, _)| base).unwrap_or(AUTH_BASE);
    let mut session = if request.username.is_empty() && request.password.is_empty() {
        let local = local_session(database)?.filter(|s| valid_session(s).is_ok());
        let saved = saved_credential(path, &config).ok().and_then(|c| c.session);
        let mut session = local
            .or(saved)
            .context("没有有效的天工登录，请填写账号密码重新登录")?;
        verify_with(&http, base, &mut session)?;
        session
    } else {
        password_login(&http, base, &request.username, &request.password)?
    };
    session.auth_base_url = AUTH_BASE.into();
    let models = discover_at(database)?;
    let request = EnableRequest {
        revision: request.revision,
        catalog_revision: catalog_revision(&models)?,
        model_ids: models.iter().map(|m| m.id.clone()).collect(),
        pat: String::new(),
        user_id: String::new(),
    };
    let imported = enable_authorized_at(
        path,
        database,
        request,
        Some(Credential {
            pat: String::new(),
            user_id: session.user_id.clone(),
            session: Some(session),
        }),
    )?;
    let mut view = account_view(
        path,
        &AppConfig::load_or_default(&path.to_path_buf())?,
        database,
    );
    view["parameterSyncErrors"] = imported["parameterSyncErrors"].clone();
    Ok(view)
}

pub(super) fn account_view(path: &Path, config: &AppConfig, database: &Path) -> Value {
    let mut view = view_at(config, database);
    let saved = saved_credential(path, config).ok();
    let session = saved
        .and_then(|c| c.session)
        .filter(|s| valid_session(s).is_ok());
    view["loggedIn"] = session.is_some().into();
    view["accountName"] = session
        .as_ref()
        .map(|s| s.display_name.clone())
        .unwrap_or_default()
        .into();
    view["localSessionAvailable"] = local_session(database)
        .ok()
        .flatten()
        .is_some_and(|s| valid_session(&s).is_ok())
        .into();
    view["parametersCurrent"] = discover_at(database)
        .ok()
        .is_some_and(|models| {
            models.iter().all(|m| {
                config.ai_gateway.providers.iter().any(|p| {
                    p.desktop_binding.is_none()
                        && !p.is_client_reserved()
                        && p.tiangong_builtin.as_ref().is_some_and(|b| {
                            b.model_id == m.id
                                && b.upstream_model == m.upstream_model
                                && b.parameters.as_ref() == Some(&m.parameters)
                        })
                })
            })
        })
        .into();
    view["parameterSyncErrors"] = config
        .ai_gateway
        .providers
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| {
            let parameters = p.tiangong_builtin.as_ref()?.parameters.as_ref()?;
            let binding = p.desktop_binding.as_ref()?;
            (binding.capabilities != parameters.capabilities()).then(|| {
                format!(
                    "{} 的来源参数尚未同步，请在该端模型页核对后保存；外部修改不会被覆盖",
                    binding.client.name()
                )
            })
        })
        .collect::<Vec<_>>()
        .into();
    let shares=config.ai_gateway.providers.iter().filter(|p|p.enabled && (!p.name.starts_with("Codex · ") || p.model_aliases.keys().any(|m|config.ai_gateway.codex_visible_models.contains(m)))).filter_map(|p|{
        let binding=p.tiangong_builtin.as_ref()?;
        let client=p.desktop_binding.as_ref().map(|b|b.client).or_else(||{
            if p.is_workbuddy(){Some(super::super::clients::ClientId::WorkBuddy)}
            else if p.name.starts_with("Codex · "){Some(super::super::clients::ClientId::Codex)}else{None}
        })?;
        Some(json!({"modelId":binding.model_id,"client":client,"targetId":p.desktop_binding.as_ref().map(|b|b.target_id.as_str())}))
    }).collect::<Vec<_>>();
    view["shares"] = shares.into();
    view["message"] = "内置模型沿用天工配置；个人登录可选，服务端决定实际使用权限。".into();
    view
}

pub(super) fn authorize_session(
    path: &Path,
    request: &mut reqwest::Request,
    provider: &ProviderConfig,
    session: &LoginSession,
) -> Result<()> {
    let config = AppConfig::load_or_default(&path.to_path_buf())?;
    // The native adapter permits the personal ID to be absent. A model request
    // never sends a login token or performs a separate account network call.
    let user_id = if valid_session(session).is_ok() {
        session.user_id.clone()
    } else {
        String::new()
    };
    let headers = installed_headers(&crate::gmclaw_runtime::model_archive(
        &config.gmclaw_bridge,
    )?)?;
    apply_credential(
        request,
        &Credential {
            pat: headers,
            user_id,
            session: None,
        },
    )?;
    validate_provider(provider)
}

pub(super) fn authorize_native(path: &Path, request: &mut reqwest::Request) -> Result<()> {
    let config = AppConfig::load_or_default(&path.to_path_buf())?;
    let pat = installed_headers(&crate::gmclaw_runtime::model_archive(
        &config.gmclaw_bridge,
    )?)?;
    apply_credential(
        request,
        &Credential {
            pat,
            user_id: String::new(),
            session: None,
        },
    )
}

fn entry(archive: &Path, names: &[&str], maximum: usize) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(archive).context("无法读取天工安装资源，请检查安装位置")?;
    let file_size = file.metadata()?.len();
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix)
        .context("天工安装资源格式不受支持")?;
    let n = |at| u32::from_le_bytes(prefix[at..at + 4].try_into().unwrap()) as u64;
    let (header_size, json_size) = (n(4), n(12));
    ensure!(
        n(0) == 4
            && json_size <= 8 * 1024 * 1024
            && json_size + 8 <= header_size
            && header_size + 8 <= file_size,
        "天工安装资源索引无效"
    );
    let mut bytes = vec![0; json_size as usize];
    file.read_exact(&mut bytes)?;
    let tree: Value = serde_json::from_slice(&bytes).context("天工安装资源索引不兼容")?;
    let mut node = &tree;
    for name in names {
        node = node
            .get("files")
            .and_then(|v| v.get(*name))
            .context("天工模型适配资源缺失，请检查安装版本")?;
    }
    ensure!(
        node.get("link").is_none() && node.get("unpacked").is_none(),
        "天工模型适配资源格式不受支持"
    );
    let size = node["size"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= maximum as u64)
        .context("天工模型适配资源大小无效")?;
    let start = node["offset"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_add(8 + header_size))
        .context("天工模型适配资源位置无效")?;
    ensure!(
        start.checked_add(size).is_some_and(|end| end <= file_size),
        "天工模型适配资源范围无效"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; size as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn installed_headers(archive: &Path) -> Result<String> {
    let package: Value = serde_json::from_slice(&entry(archive, &["package.json"], 64 * 1024)?)
        .context("天工安装包身份无效")?;
    ensure!(
        package["name"] == "tiangong-desktop",
        "所选程序不是天工 Work"
    );
    let source = String::from_utf8(entry(
        archive,
        &["out", "main", "index.js"],
        16 * 1024 * 1024,
    )?)
    .context("天工模型适配资源格式无效")?;
    parse_installed_headers(&source)
}

fn parse_installed_headers(source: &str) -> Result<String> {
    ensure!(
        source.contains(ENDPOINT),
        "天工内置模型接口已变化，请更新 Hub 后重试"
    );
    let marker = "function builtinModelRequestHeaders(model, userId) {";
    ensure!(
        source.matches(marker).count() == 1,
        "天工模型认证适配已变化，请更新 Hub 后重试"
    );
    let body = source
        .split_once(marker)
        .unwrap()
        .1
        .split_once("\n}")
        .map(|(body, _)| body)
        .filter(|s| s.len() <= 2048)
        .context("天工模型认证适配不兼容")?;
    ensure!(
        body.contains("if (!isBuiltinModel(model)) return {};")
            && body.contains("X-Tiangong-End-User-Id")
            && body.contains("tiangong-prod"),
        "天工模型认证适配不兼容，请更新 Hub 后重试"
    );
    let re = regex::Regex::new(r#""authorization-cer-pat"\s*:\s*("[^"\\\r\n]+")"#)?;
    let captures = re.captures_iter(body).collect::<Vec<_>>();
    ensure!(
        captures.len() == 1,
        "无法识别天工模型认证适配，请更新 Hub 后重试"
    );
    let pat: String = serde_json::from_str(&captures[0][1]).context("天工模型认证格式不兼容")?;
    validate_credential(&pat, "")
        .map_err(|_| anyhow::anyhow!("天工安装认证格式已变化，请更新 Hub 后重试"))?;
    Ok(pat)
}

#[cfg(test)]
mod tests;
