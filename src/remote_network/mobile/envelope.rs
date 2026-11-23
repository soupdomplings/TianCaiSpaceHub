use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) fn new_key() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    B64.encode(bytes)
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct Envelope {
    pub binding_id: String,
    pub request_id: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Call {
    pub issued_at: u64,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub offset: u64,
}

impl Envelope {
    fn cipher(&self, key: &str) -> Result<Aes256Gcm> {
        ensure!(
            !self.binding_id.contains('|') && uuid::Uuid::parse_str(&self.request_id).is_ok(),
            "无效的手机请求标识"
        );
        Aes256Gcm::new_from_slice(&B64.decode(key)?).map_err(|_| anyhow!("配对密钥无效"))
    }
    fn aad(&self, dir: &str) -> String {
        format!("tc-hub-v1|{}|{}|{dir}", self.binding_id, self.request_id)
    }
    pub fn open(&self, key: &str) -> Result<Call> {
        let nonce: [u8; 12] = B64
            .decode(&self.nonce)?
            .try_into()
            .map_err(|_| anyhow!("无效的加密请求"))?;
        ensure!(self.ciphertext.len() <= 1024 * 1024, "手机请求过大");
        let clear = self
            .cipher(key)?
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &B64.decode(&self.ciphertext)?,
                    aad: self.aad("req").as_bytes(),
                },
            )
            .map_err(|_| anyhow!("手机请求认证失败"))?;
        Ok(serde_json::from_slice(&clear)?)
    }
    pub fn reply(&self, key: &str, value: &Value) -> Result<Self> {
        let mut nonce = [0; 12];
        OsRng.fill_bytes(&mut nonce);
        let cipher = self
            .cipher(key)?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &serde_json::to_vec(value)?,
                    aad: self.aad("res").as_bytes(),
                },
            )
            .map_err(|_| anyhow!("无法加密响应"))?;
        Ok(Self {
            binding_id: self.binding_id.clone(),
            request_id: self.request_id.clone(),
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(cipher),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opens_packet_from_wechat_javascript_implementation() {
        let packet: Envelope = serde_json::from_value(serde_json::json!({
            "bindingId":"bind-test","requestId":"00000000-0000-4000-8000-000000000002",
            "nonce":"CQkJCQkJCQkJCQkJ","ciphertext":"XKft582FpAXhFukC18aBsrHEC5xQzM3iKOsShPlWcRunjcX5wH1JfBbWy2I3Fp75b4uH1TCsqhGLhbP5Me75"
        })).unwrap();
        let call = packet.open(&B64.encode([7; 32])).unwrap();
        assert_eq!(
            (call.issued_at, call.method.as_str(), call.path.as_str()),
            (1, "GET", "/clients")
        );
    }
    #[test]
    fn request_authenticates_direction_and_both_identifiers() {
        let key = new_key();
        let nonce = [7u8; 12];
        let mut env = Envelope {
            binding_id: "phone".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            nonce: B64.encode(nonce),
            ciphertext: String::new(),
        };
        let bytes = env
            .cipher(&key)
            .unwrap()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: br#"{"issuedAt":1,"method":"GET","path":"/clients"}"#,
                    aad: env.aad("req").as_bytes(),
                },
            )
            .unwrap();
        env.ciphertext = B64.encode(bytes);
        assert_eq!(env.open(&key).unwrap().path, "/clients");
        assert!(env.reply(&key, &Value::Null).unwrap().open(&key).is_err());
        env.binding_id.push('x');
        assert!(env.open(&key).is_err());
    }
}
