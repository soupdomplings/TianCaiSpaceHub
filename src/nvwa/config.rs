use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use url::Url;

use super::types::{NvwaConfig, NvwaProfile};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
    root: PathBuf,
    lock_path: PathBuf,
}

impl ConfigStore {
    pub fn new(config_path: &Path) -> Result<Self> {
        let parent = config_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).context("无法创建 NVWA 配置目录")?;
        let stem = config_path
            .file_stem()
            .context("Hub 配置路径缺少文件名")?
            .to_string_lossy();
        let path = parent.join(format!("{stem}.nvwa.json"));
        let root = parent.join(format!("{stem}.nvwa-secrets"));
        let lock_path = parent.join(format!("{stem}.nvwa.lock"));
        Ok(Self {
            path,
            root,
            lock_path,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load(&self) -> Result<NvwaConfig> {
        let _lock = self.lock()?;
        self.load_unlocked()
    }

    pub fn save(&self, config: &NvwaConfig, expected_revision: &str) -> Result<NvwaConfig> {
        validate_config(config)?;
        let _lock = self.lock()?;
        let current = self.load_unlocked()?;
        ensure!(
            current.revision == expected_revision,
            "NVWA 配置已被其他操作修改，请刷新后重试"
        );
        let mut stored = config.clone();
        stored.revision.clear();
        let bytes = serde_json::to_vec_pretty(&stored).context("无法编码 NVWA 配置")?;
        ensure!(
            bytes.len() as u64 <= MAX_CONFIG_BYTES,
            "NVWA 配置超出大小限制"
        );
        let mut temp =
            tempfile::NamedTempFile::new_in(self.path.parent().unwrap_or_else(|| Path::new(".")))
                .context("无法创建 NVWA 配置临时文件")?;
        temp.write_all(&bytes).context("无法写入 NVWA 配置")?;
        temp.as_file().sync_all().context("无法保存 NVWA 配置")?;
        temp.persist(&self.path)
            .map_err(|_| anyhow::anyhow!("无法原子替换 NVWA 配置"))?;
        stored.revision = hex::encode(Sha256::digest(&bytes));
        Ok(stored)
    }

    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .context("无法打开 NVWA 配置锁")?;
        file.lock_exclusive().context("无法锁定 NVWA 配置")?;
        Ok(file)
    }

    fn load_unlocked(&self) -> Result<NvwaConfig> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(NvwaConfig::default()),
            Err(_) => bail!("无法读取 NVWA 配置"),
        };
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("无法读取 NVWA 配置")?;
        ensure!(
            bytes.len() as u64 <= MAX_CONFIG_BYTES,
            "NVWA 配置超出大小限制"
        );
        let mut config: NvwaConfig = serde_json::from_slice(&bytes).context("NVWA 配置格式无效")?;
        validate_config(&config)?;
        config.revision = hex::encode(Sha256::digest(&bytes));
        Ok(config)
    }
}

pub fn validate_config(config: &NvwaConfig) -> Result<()> {
    validate_extensions(
        &config.extra,
        &["version", "bridgePort", "profiles", "_revision", "revision"],
    )?;
    ensure!(config.version == 1, "不支持此 NVWA 配置版本");
    ensure!(config.bridge_port != 0, "NVWA 桥端口不能为 0");
    ensure!(config.profiles.len() <= 64, "NVWA 环境最多 64 个");
    let mut ids = HashSet::new();
    for profile in &config.profiles {
        validate_profile(profile)?;
        ensure!(ids.insert(&profile.id), "NVWA 环境标识重复");
    }
    Ok(())
}

pub fn validate_profile(profile: &NvwaProfile) -> Result<()> {
    validate_extensions(
        &profile.extra,
        &[
            "id",
            "name",
            "productBaseUrl",
            "certificationBaseUrl",
            "mcpUrl",
            "authMode",
            "clientId",
            "username",
            "tenant",
            "loginUnit",
            "mcpAuthHeader",
            "signatureAlgorithm",
            "credentialSecretRef",
        ],
    )?;
    ensure!(
        !profile.id.is_empty()
            && profile.id.len() <= 80
            && profile
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "NVWA 环境标识无效"
    );
    ensure!(
        !profile.name.trim().is_empty() && profile.name.len() <= 256,
        "NVWA 环境名称无效"
    );
    validate_url(&profile.product_base_url)?;
    if !profile.certification_base_url.is_empty() {
        validate_url(&profile.certification_base_url)?;
    }
    if !profile.mcp_url.is_empty() {
        validate_url(&profile.mcp_url)?;
    }
    if let Some(header) = &profile.mcp_auth_header {
        ensure!(
            matches!(
                header.to_ascii_lowercase().as_str(),
                "authorization" | "authorization-ticket-token"
            ),
            "NVWA MCP 认证头只支持 Authorization 或 authorization-ticket-token"
        );
    }
    for value in [&profile.client_id, &profile.username, &profile.tenant] {
        ensure!(
            value.len() <= 1024 && !value.chars().any(char::is_control),
            "NVWA 环境字段无效"
        );
    }
    if let Some(reference) = &profile.credential_secret_ref {
        ensure!(
            !reference.is_empty() && reference.len() <= 256,
            "NVWA 凭据引用无效"
        );
    }
    Ok(())
}

fn validate_extensions(
    fields: &serde_json::Map<String, serde_json::Value>,
    reserved: &[&str],
) -> Result<()> {
    ensure!(
        !fields.keys().any(|key| reserved.contains(&key.as_str())),
        "NVWA 扩展配置不能覆盖标准字段"
    );
    let mut count = 0;
    for (key, value) in fields {
        validate_extension_key(key)?;
        validate_extension_value(value, 0, &mut count)?;
    }
    Ok(())
}

fn validate_extension_key(key: &str) -> Result<()> {
    ensure!(
        !key.is_empty() && key.len() <= 256 && !key.chars().any(char::is_control),
        "NVWA 扩展配置字段无效"
    );
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let sensitive = matches!(
        normalized.as_str(),
        "pwd"
            | "psw"
            | "pass"
            | "authorization"
            | "authorizationticket"
            | "authorizationticketid"
            | "authorizationticketvalue"
            | "apikey"
            | "keymaterial"
            | "bearer"
            | "credential"
            | "credentials"
            | "cookie"
            | "cookies"
            | "ticket"
            | "ticketid"
            | "twofactorsessionid"
            | "twofactorsession"
            | "verifycode"
            | "validcode"
            | "captcha"
            | "privatekey"
    ) || normalized.ends_with("password")
        || normalized.ends_with("secret")
        || normalized.ends_with("token")
        || normalized.starts_with("authorization");
    ensure!(
        !sensitive,
        "NVWA 普通扩展配置不能保存密码、密钥、令牌或登录挑战"
    );
    Ok(())
}

fn validate_extension_value(
    value: &serde_json::Value,
    depth: usize,
    count: &mut usize,
) -> Result<()> {
    ensure!(depth <= 16 && *count < 10_000, "NVWA 扩展配置超出结构限制");
    *count += 1;
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                validate_extension_key(key)?;
                validate_extension_value(value, depth + 1, count)?;
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                validate_extension_value(value, depth + 1, count)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_url(value: &str) -> Result<Url> {
    ensure!(value.len() <= 4096, "NVWA 地址超出长度限制");
    let url = Url::parse(value).map_err(|_| anyhow::anyhow!("NVWA 地址格式无效"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "NVWA 地址必须为 HTTP(S)"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "NVWA 地址不能包含用户信息、查询或片段"
    );
    Ok(url)
}
