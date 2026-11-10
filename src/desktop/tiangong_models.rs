//! Read-only discovery and authenticated reuse of TianGong built-in models.
use std::{collections::BTreeMap, path::Path, time::Duration};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    ai_gateway::config::{ProviderConfig, ProviderType},
    config::AppConfig,
    nvwa::secrets::SecretStore,
};

pub const ENDPOINT: &str = "https://tiangong.jiuqi.com.cn/api/v2/tg/chat/completions";
const BASE_URL: &str = "https://tiangong.jiuqi.com.cn/api/v2/tg";
const IDS: [(&str, &str); 2] = [
    ("tiangong-builtin", "天工内置-外网"),
    ("tiangong-builtin-intranet", "天工内置-内网"),
];
mod diagnostics;
mod parameters;
mod session;
pub use diagnostics::{IdentityTestRequest, test_identity};
pub use parameters::{Parameters, for_provider as parameters_for_provider};
pub use session::{ConnectRequest, connect};

fn source_name(id: &str) -> String {
    format!(
        "天工内置 · {}",
        if id == "tiangong-builtin-intranet" {
            "内网"
        } else {
            "外网"
        }
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinBinding {
    pub model_id: String,
    pub upstream_model: String,
    pub credential_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Parameters>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinModel {
    pub id: String,
    pub name: String,
    pub upstream_model: String,
    pub endpoint: String,
    pub parameters: Parameters,
}

// Intentionally neither Debug nor Serialize in the public view.
#[derive(Deserialize, Serialize)]
struct Credential {
    pat: String,
    user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<session::LoginSession>,
}

fn saved_credential(path: &Path, config: &AppConfig) -> Result<Credential> {
    let key = config
        .ai_gateway
        .providers
        .iter()
        .find_map(|p| p.tiangong_builtin.as_ref())
        .context("尚未登录天工")?
        .credential_ref
        .clone();
    let bytes = secrets(path)?
        .get(&key)?
        .context("天工登录信息不可用，请重新登录")?;
    serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("天工登录信息格式不兼容，请重新登录"))
}

pub fn account_view(path: &Path, config: &AppConfig) -> Value {
    session::account_view(path, config, &crate::gmclaw_config::config_path())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableRequest {
    pub revision: String,
    pub catalog_revision: String,
    pub model_ids: Vec<String>,
    #[serde(default)]
    pub pat: String,
    #[serde(default)]
    pub user_id: String,
}

fn secrets(path: &Path) -> Result<SecretStore> {
    SecretStore::new(
        super::state_root(path)
            .join("tiangong-models")
            .join("credentials"),
    )
    .map_err(|_| anyhow::anyhow!("无法打开天工模型授权存储，请检查 Hub 配置目录权限"))
}

/// Query only identity/endpoint columns. Opening a missing DB must not create it.
pub fn discover_at(path: &Path) -> Result<Vec<BuiltinModel>> {
    ensure!(
        path.is_file(),
        "未找到天工 Work 模型配置。请先安装并在天工中完成首次配置，再重新读取"
    );
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("无法只读打开天工模型配置，请关闭占用该文件的配置工具后重试")?;
    db.busy_timeout(Duration::from_millis(300))?;
    let mut query = db
        .prepare(
            "SELECT model_id, model_name, base_url FROM model_configs
         WHERE model_id IN ('tiangong-builtin', 'tiangong-builtin-intranet') ORDER BY model_id",
        )
        .context("天工模型配置格式不受支持，请检查天工版本后重试")?;
    let mut models = Vec::new();
    for row in query.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (id, model, endpoint) =
            row.context("天工内置模型条目不完整，请先在天工中重新读取模型")?;
        let name = IDS
            .iter()
            .find(|(key, _)| *key == id)
            .context("未知天工内置模型")?
            .1;
        ensure!(
            endpoint.trim_end_matches('/') == ENDPOINT,
            "天工内置模型地址已变化，暂未启用跨端复用。请核对天工版本及接口地址"
        );
        ensure!(
            !model.trim().is_empty() && model.len() <= 256 && !model.chars().any(char::is_control),
            "天工内置模型名称无效，请在天工中检查配置"
        );
        ensure!(
            !models.iter().any(|m: &BuiltinModel| m.id == id),
            "天工内置模型身份重复，请先修复天工配置"
        );
        models.push(BuiltinModel {
            parameters: Parameters::read(&db, &id)?,
            id,
            name: name.into(),
            upstream_model: model,
            endpoint: ENDPOINT.into(),
        });
    }
    ensure!(
        !models.is_empty(),
        "当前天工配置中没有这两个内置模型，请在天工中完成首次配置后重新读取"
    );
    Ok(models)
}

fn catalog_revision(models: &[BuiltinModel]) -> Result<String> {
    Ok(super::transaction::fingerprint(Some(&serde_json::to_vec(
        models,
    )?)))
}

fn view_at(config: &AppConfig, database: &Path) -> Value {
    match discover_at(database) {
        Ok(models) => {
            let rows = models.iter().map(|m| {
                let imported = config.ai_gateway.providers.iter().find(|p| p.name == source_name(&m.id)
                    && p.desktop_binding.is_none() && !p.is_client_reserved()
                    && p.tiangong_builtin.as_ref().is_some_and(|b| b.model_id == m.id));
                json!({"id":m.id,"name":m.name,"upstreamModel":m.upstream_model,"parameters":m.parameters,"configured":imported.is_some(),"enabled":imported.is_some_and(|p|p.enabled)})
            }).collect::<Vec<_>>();
            json!({"models":rows,"revision":config.revision,"catalogRevision":catalog_revision(&models).ok(),"hasAuthorization":config.ai_gateway.providers.iter().any(|p|p.tiangong_builtin.is_some()),"message":"登录天工后自动加入内置模型，可在天工 Work 的模型页分享给其他客户端。"})
        }
        Err(error) => json!({"models":[],"revision":config.revision,"error":error.to_string()}),
    }
}

fn validate_credential(pat: &str, user_id: &str) -> Result<()> {
    ensure!(
        !pat.trim().is_empty()
            && pat.len() <= 8192
            && pat.is_ascii()
            && !pat.chars().any(|c| c.is_control() || c.is_whitespace()),
        "请填写有效的天工个人 PAT；不要填写 Hub 本机密钥或天工运行时令牌"
    );
    ensure!(
        user_id.len() <= 256
            && user_id.is_ascii()
            && !user_id.chars().any(|c| c.is_control() || c.is_whitespace()),
        "天工用户编号格式无效，可留空；仅在授权方要求时填写"
    );
    Ok(())
}

/// The config's existing revision/lock/atomic writer remains authoritative.
/// A new protected secret is staged before save and discarded on save failure.
pub fn enable(path: &Path, request: EnableRequest) -> Result<Value> {
    enable_at(path, &crate::gmclaw_config::config_path(), request)
}

fn enable_at(path: &Path, database: &Path, request: EnableRequest) -> Result<Value> {
    enable_authorized_at(path, database, request, None)
}

fn enable_authorized_at(
    path: &Path,
    database: &Path,
    request: EnableRequest,
    credential: Option<Credential>,
) -> Result<Value> {
    let mut config = AppConfig::load_or_default(&path.to_path_buf())?;
    ensure!(
        config.revision.as_deref() == Some(request.revision.as_str()),
        "Hub 配置已变化，请重新读取后再启用复用"
    );
    let models = discover_at(database)?;
    ensure!(
        catalog_revision(&models)? == request.catalog_revision,
        "天工模型配置已变化，请重新读取后再启用复用"
    );
    ensure!(
        !request.model_ids.is_empty() && request.model_ids.len() <= 2,
        "请选择要复用的天工内置模型"
    );
    for (i, id) in request.model_ids.iter().enumerate() {
        ensure!(
            models.iter().any(|m| m.id == *id) && !request.model_ids[..i].contains(id),
            "所选内置模型已变化，请重新读取"
        );
    }
    let store = secrets(path)?;
    let previous = config.ai_gateway.providers.iter().find_map(|p| {
        p.tiangong_builtin
            .as_ref()
            .map(|b| b.credential_ref.clone())
    });
    let (credential_ref, staged) = if let Some(credential) = credential {
        let key = uuid::Uuid::new_v4().to_string();
        store
            .set(&key, &serde_json::to_vec(&credential)?)
            .map_err(|_| anyhow::anyhow!("无法保存天工登录，请检查系统凭据存储后重试"))?;
        (key, true)
    } else if request.pat.is_empty() {
        let key = previous.context("尚未配置跨端授权。请填写你获授权使用的天工个人 PAT")?;
        ensure!(
            request.user_id.is_empty(),
            "更新用户编号时请同时填写对应的个人 PAT"
        );
        let bytes = store
            .get(&key)?
            .context("已保存的天工授权无法读取，请重新填写个人 PAT")?;
        let saved: Credential = serde_json::from_slice(&bytes)
            .context("已保存的天工授权无法读取，请重新填写个人 PAT")?;
        if saved.session.is_none() && !saved.pat.is_empty() {
            validate_credential(&saved.pat, &saved.user_id)?;
        }
        (key, false)
    } else {
        validate_credential(&request.pat, &request.user_id)?;
        let key = uuid::Uuid::new_v4().to_string();
        store
            .set(
                &key,
                &serde_json::to_vec(&Credential {
                    pat: request.pat,
                    user_id: request.user_id,
                    session: None,
                })?,
            )
            .map_err(|_| anyhow::anyhow!("保存天工授权失败，请检查系统凭据存储后重试"))?;
        (key, true)
    };
    let result = (|| -> Result<()> {
        if staged {
            // Account authorization and native model defaults share a source;
            // route aliases and client-owned identities remain unchanged.
            for provider in &mut config.ai_gateway.providers {
                if let Some(binding) = &mut provider.tiangong_builtin {
                    binding.credential_ref = credential_ref.clone();
                }
            }
        }
        for model in models.iter().filter(|m| request.model_ids.contains(&m.id)) {
            for provider in &mut config.ai_gateway.providers {
                if let Some(binding) = &mut provider.tiangong_builtin
                    && binding.model_id == model.id
                    && binding.upstream_model == model.upstream_model
                {
                    binding.parameters = Some(model.parameters.clone());
                    provider.timeout_secs = model.parameters.timeout_secs;
                }
            }
            let existing = config.ai_gateway.providers.iter().position(|p| {
                p.name == source_name(&model.id)
                    && p.desktop_binding.is_none()
                    && !p.is_client_reserved()
                    && p.tiangong_builtin
                        .as_ref()
                        .is_some_and(|b| b.model_id == model.id)
            });
            let mut provider = ProviderConfig {
                name: source_name(&model.id),
                enabled: true,
                provider_type: ProviderType::ChatCompletions,
                base_url: BASE_URL.into(),
                tiangong_builtin: Some(BuiltinBinding {
                    model_id: model.id.clone(),
                    upstream_model: model.upstream_model.clone(),
                    credential_ref: credential_ref.clone(),
                    parameters: Some(model.parameters.clone()),
                }),
                timeout_secs: model.parameters.timeout_secs,
                model_aliases: BTreeMap::from([(model.name.clone(), model.upstream_model.clone())]),
                ..Default::default()
            };
            if let Some(index) = existing {
                provider.name = config.ai_gateway.providers[index].name.clone();
                provider.weight = config.ai_gateway.providers[index].weight;
                config.ai_gateway.providers[index] = provider;
            } else {
                ensure!(
                    !config
                        .ai_gateway
                        .providers
                        .iter()
                        .any(|p| p.name == provider.name),
                    "已有同名渠道，请先重命名后再启用天工内置模型复用"
                );
                config.ai_gateway.providers.push(provider);
            }
        }
        ensure!(
            catalog_revision(&discover_at(database)?)? == request.catalog_revision,
            "天工模型在保存前发生变化，请重新读取；未覆盖原配置"
        );
        config.ai_gateway.enabled = true;
        config.save(&path.to_path_buf())?;
        Ok(())
    })();
    if result.is_err() && staged {
        let _ = store.delete(&credential_ref);
    }
    result?;
    let parameter_sync_errors = parameters::sync_copies(path);
    Ok(
        json!({"configured":true,"parameterSyncErrors":parameter_sync_errors,"message":"已保存天工模型来源。到目标客户端的模型页面添加并保存即可使用；首次调用由上游核验授权。"}),
    )
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<()> {
    if let Some(binding) = &provider.tiangong_builtin {
        ensure!(
            provider.provider_type == ProviderType::ChatCompletions
                && crate::ai_gateway::config::provider_endpoint(
                    &provider.base_url,
                    "/v1/chat/completions"
                ) == ENDPOINT,
            "天工内置来源仅支持其原始聊天接口，请到“天工 Work → 模型”重新读取"
        );
        ensure!(
            IDS.iter().any(|(id, _)| *id == binding.model_id)
                && !binding.upstream_model.is_empty()
                && provider.models.iter().all(|m| m == &binding.upstream_model)
                && provider
                    .model_aliases
                    .values()
                    .all(|m| m == &binding.upstream_model)
                && (!provider.models.is_empty() || !provider.model_aliases.is_empty()),
            "天工内置来源模型身份不一致，请重新选择来源，不能借用其他条目的授权"
        );
        ensure!(
            uuid::Uuid::parse_str(&binding.credential_ref).is_ok(),
            "天工模型授权引用无效，请到“天工 Work → 模型”重新登录"
        );
        if let Some(parameters) = &binding.parameters {
            parameters.validate()?;
        }
    }
    Ok(())
}

pub fn authorize(
    path: &Path,
    request: &mut reqwest::Request,
    provider: &ProviderConfig,
) -> Result<()> {
    validate_provider(provider)?;
    let binding = provider
        .tiangong_builtin
        .as_ref()
        .context("天工模型来源丢失")?;
    ensure!(
        request.url().as_str() == ENDPOINT,
        "天工模型请求地址不匹配，已停止发送授权信息，请重新读取来源"
    );
    let bytes = secrets(path)?
        .get(&binding.credential_ref)?
        .context("天工模型授权不可用，请到“天工 Work → 模型”重新登录")?;
    let credential: Credential = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("天工模型授权无法读取，请重新登录"))?;
    if let Some(session) = &credential.session {
        session::authorize_session(path, request, provider, session)
    } else if credential.pat.is_empty() {
        session::authorize_native(path, request)
    } else {
        apply_credential(request, &credential)
    }
}

fn apply_credential(request: &mut reqwest::Request, credential: &Credential) -> Result<()> {
    validate_credential(&credential.pat, &credential.user_id)?;
    let headers = request.headers_mut();
    for key in [
        "authorization",
        "authorization-cer-pat",
        "x-tiangong-end-user-id",
        "x-tiangong-end-user-source",
    ] {
        headers.remove(key);
    }
    let mut pat =
        reqwest::header::HeaderValue::from_str(&credential.pat).context("天工 PAT 格式无效")?;
    pat.set_sensitive(true);
    headers.insert("authorization-cer-pat", pat);
    headers.insert(
        "x-tiangong-end-user-source",
        reqwest::header::HeaderValue::from_static("tiangong-prod"),
    );
    if !credential.user_id.is_empty() {
        let mut id = reqwest::header::HeaderValue::from_str(&credential.user_id)
            .context("天工用户编号格式无效")?;
        id.set_sensitive(true);
        headers.insert("x-tiangong-end-user-id", id);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
