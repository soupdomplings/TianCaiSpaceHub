//! Connection codes bind a private network invitation to one target's TLS identity.
//! Tokens and network secrets are never included in normal status responses.
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use super::{DeviceIdentity, NetworkConfig, NetworkSecrets};

pub(super) const INVITATION_LIFETIME_MS: u128 = 10 * 60 * 1000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConnectionCode {
    pub version: u32,
    pub network_name: String,
    pub network_secret: String,
    pub relay_url: String,
    pub target: DeviceIdentity,
    pub invitation_token: String,
    pub expires_at: u128,
}

impl ConnectionCode {
    pub fn issue(config: &NetworkConfig, secrets: &NetworkSecrets) -> Result<Self> {
        ensure!(!config.network_name.is_empty(), "请先创建个人网络");
        ensure!(
            !secrets.network_secret.is_empty(),
            "个人网络凭据不可用，请重新加入网络"
        );
        Ok(Self {
            version: 1,
            network_name: config.network_name.clone(),
            network_secret: secrets.network_secret.clone(),
            relay_url: config.relay_url.clone(),
            target: config.identity(),
            invitation_token: super::random_token(),
            expires_at: crate::types::now_ms() + INVITATION_LIFETIME_MS,
        })
    }

    pub fn encode(&self) -> Result<String> {
        Ok(format!(
            "TCH1.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?)
        ))
    }

    pub fn decode(code: &str, now: u128) -> Result<Self> {
        let code = code.trim();
        ensure!(code.len() < 16 * 1024, "连接码过长，请重新复制");
        let payload = code
            .strip_prefix("TCH1.")
            .ok_or_else(|| anyhow::anyhow!("连接码格式不正确，请从目标 Hub 重新复制"))?;
        let value: Self = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(payload)
                .map_err(|_| anyhow::anyhow!("连接码内容不完整，请重新复制"))?,
        )
        .map_err(|_| anyhow::anyhow!("连接码内容不完整，请重新复制"))?;
        ensure!(value.version == 1, "此连接码版本暂不支持，请更新 Hub");
        ensure!(
            value.expires_at > now && value.expires_at <= now + INVITATION_LIFETIME_MS + 60_000,
            "连接码已过期或两台电脑时间不同，请校准时间并重新复制"
        );
        ensure!(
            valid_network_name(&value.network_name),
            "连接码中的网络名称无效"
        );
        ensure!(
            (32..=128).contains(&value.network_secret.len()) && value.network_secret.is_ascii(),
            "连接码中的网络凭据无效"
        );
        ensure!(
            (32..=128).contains(&value.invitation_token.len()) && value.invitation_token.is_ascii(),
            "连接码中的配对凭据无效"
        );
        value.target.validate()?;
        validate_relay(&value.relay_url)?;
        Ok(value)
    }
}

pub(super) fn valid_network_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(super) fn validate_relay(raw: &str) -> Result<()> {
    let url = url::Url::parse(raw)
        .map_err(|_| anyhow::anyhow!("节点地址格式不正确，例如 tcp://节点地址:11010"))?;
    ensure!(
        matches!(url.scheme(), "tcp" | "udp" | "ws" | "wss" | "quic")
            && url.host_str().is_some()
            && url.port().is_some(),
        "节点地址需要协议、主机和端口"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "节点地址不能包含账号或附加参数"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_expired_codes_have_readable_errors() {
        assert!(
            ConnectionCode::decode("not-a-code", 0)
                .err()
                .unwrap()
                .to_string()
                .contains("格式")
        );
        let value = serde_json::json!({"version":1,"networkName":"hub-test","networkSecret":"x".repeat(64),"relayUrl":"tcp://public.easytier.cn:11010","target":{"id":"fixture","name":"Fixture","ipv4":"10.203.0.2","certificatePem":"fixture"},"invitationToken":"y".repeat(64),"expiresAt":1});
        let code = format!(
            "TCH1.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).unwrap())
        );
        assert!(
            ConnectionCode::decode(&code, 2)
                .err()
                .unwrap()
                .to_string()
                .contains("过期")
        );
    }

    #[test]
    fn relay_requires_explicit_protocol_and_port_without_credentials() {
        assert!(validate_relay("tcp://public.easytier.cn:11010").is_ok());
        assert!(validate_relay("tcp://user:pass@example.test:11010").is_err());
        assert!(validate_relay("http://example.test:11010").is_err());
        assert!(validate_relay("tcp://example.test").is_err());
    }
}
