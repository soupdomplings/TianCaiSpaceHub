use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    Password,
    Browser,
    Application,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureAlgorithm {
    #[default]
    Sha256,
    Sm3,
    Md5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    Codex,
    Workbuddy,
    Tiangong,
}

impl ClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Workbuddy => "workbuddy",
            Self::Tiangong => "tiangong",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NvwaProfile {
    pub id: String,
    pub name: String,
    pub product_base_url: String,
    pub certification_base_url: String,
    pub mcp_url: String,
    pub auth_mode: AuthMode,
    pub client_id: String,
    pub username: String,
    pub tenant: String,
    pub login_unit: Option<String>,
    pub mcp_auth_header: Option<String>,
    pub signature_algorithm: SignatureAlgorithm,
    /// Only an opaque reference is stored in the ordinary profile JSON.
    pub credential_secret_ref: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for NvwaProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            product_base_url: String::new(),
            certification_base_url: String::new(),
            mcp_url: String::new(),
            auth_mode: AuthMode::Password,
            client_id: String::new(),
            username: String::new(),
            tenant: String::new(),
            login_unit: None,
            mcp_auth_header: None,
            signature_algorithm: SignatureAlgorithm::Sha256,
            credential_secret_ref: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NvwaConfig {
    pub version: u32,
    pub bridge_port: u16,
    pub profiles: Vec<NvwaProfile>,
    #[serde(rename = "_revision", skip_serializing_if = "String::is_empty")]
    pub revision: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for NvwaConfig {
    fn default() -> Self {
        Self {
            version: 1,
            bridge_port: 3849,
            profiles: Vec::new(),
            revision: String::new(),
            extra: serde_json::Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedIdentity {
    pub user_id: String,
    pub identity_id: String,
    pub tenant_id: String,
    pub username: String,
}

/// Public metadata describing the service's expiration rule.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenExpiryPolicy {
    #[default]
    Unknown,
    Fixed,
    SlidingIdle,
}

/// Authentication material is deliberately neither Debug nor Serialize.
#[derive(Clone)]
pub struct AuthToken {
    pub value: String,
    pub header_name: String,
    pub expires_at_ms: Option<u64>,
    pub expiry_policy: TokenExpiryPolicy,
}

#[derive(Clone)]
pub struct AuthResult {
    pub identity: VerifiedIdentity,
    pub personal_token: Option<AuthToken>,
    pub mcp_token: AuthToken,
}

#[derive(Default)]
pub struct PasswordLoginInput {
    pub password: String,
    pub verify_id: Option<String>,
    pub verify_code: Option<String>,
    pub twofactor_session_id: Option<String>,
    pub valid_code: Option<String>,
}

impl PasswordLoginInput {
    pub fn from_ext_info(
        password: String,
        ext_info: Option<&serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let mut input = Self {
            password,
            ..Self::default()
        };
        if let Some(value) = ext_info {
            let fields = value
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("NVWA 登录附加信息必须为对象"))?;
            for (key, value) in fields {
                let text = value
                    .as_str()
                    .filter(|s| {
                        !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control)
                    })
                    .ok_or_else(|| anyhow::anyhow!("NVWA 登录挑战字段无效"))?
                    .to_owned();
                match key.as_str() {
                    "verifyId" => input.verify_id = Some(text),
                    "verifyCode" => input.verify_code = Some(text),
                    "twofactorSessionId" => input.twofactor_session_id = Some(text),
                    "validCode" => input.valid_code = Some(text),
                    _ => anyhow::bail!("NVWA 登录附加信息含不支持的字段"),
                }
            }
        }
        Ok(input)
    }
}

pub enum AuthOutcome {
    Authenticated(AuthResult),
    PasswordChangeRequired { code: i64, message: String },
    CaptchaRequired { message: String },
    TwoFactorRequired { session_id: String, channel: String },
}
