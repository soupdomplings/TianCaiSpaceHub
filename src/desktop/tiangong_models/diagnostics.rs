//! Explicit user-triggered server diagnostics. Test identities are ephemeral;
//! normal requests never accept a caller-supplied or random account identity.
use super::*;
use std::io::Read;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IdentityMode {
    Native,
    Account,
    Random,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityTestRequest {
    pub revision: String,
    pub model_id: String,
    pub mode: IdentityMode,
}

fn test_user(mode: IdentityMode, credential: Option<&Credential>) -> Result<String> {
    Ok(match mode {
        IdentityMode::Native => String::new(),
        IdentityMode::Random => uuid::Uuid::new_v4().to_string(),
        IdentityMode::Account => credential
            .and_then(|c| c.session.as_ref())
            .filter(|s| session_valid(s))
            .map(|s| s.user_id.clone())
            .filter(|s| !s.is_empty())
            .context("当前没有有效的个人登录，请先读取天工登录或登录后再测试")?,
    })
}
fn session_valid(s: &session::LoginSession) -> bool {
    session::valid_session(s).is_ok()
}

fn prepare(
    url: &str,
    model: &str,
    p: &Parameters,
    pat: String,
    user_id: String,
) -> Result<reqwest::Request> {
    p.validate()?;
    let mut body = json!({"model":model,"messages":[{"role":"user","content":"Reply with OK only."}],"stream":p.stream});
    p.apply(&mut body);
    if p.stream {
        body["stream_options"] = json!({"include_usage":true});
    }
    let mut request = reqwest::Request::new(reqwest::Method::POST, reqwest::Url::parse(url)?);
    request.headers_mut().insert(
        "content-type",
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    *request.body_mut() = Some(serde_json::to_vec(&body)?.into());
    apply_credential(
        &mut request,
        &Credential {
            pat,
            user_id,
            session: None,
        },
    )?;
    Ok(request)
}

pub fn test_identity(path: &Path, request: IdentityTestRequest) -> Result<Value> {
    let config = AppConfig::load_or_default(&path.to_path_buf())?;
    ensure!(
        config.revision.as_deref() == Some(&request.revision),
        "配置已变化，请重新打开接入测试"
    );
    let provider = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| {
            p.enabled
                && !p.is_client_reserved()
                && p.desktop_binding.is_none()
                && p.tiangong_builtin
                    .as_ref()
                    .is_some_and(|b| b.model_id == request.model_id)
        })
        .context("请先在天工 Work 的模型页读取内置模型")?;
    validate_provider(provider)?;
    let p = parameters_for_provider(provider).context("内置模型参数尚未读取，请先重新读取")?;
    let credential = saved_credential(path, &config).ok();
    let user_id = test_user(request.mode, credential.as_ref())?;
    let pat = session::installed_headers(&crate::gmclaw_runtime::model_archive(
        &config.gmclaw_bridge,
    )?)?;
    let outbound = prepare(
        ENDPOINT,
        &provider.tiangong_builtin.as_ref().unwrap().upstream_model,
        &p,
        pat,
        user_id.clone(),
    )?;
    let http = crate::outbound_http::apply_blocking_proxy(
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(p.timeout_secs.min(30))),
        &config.outbound_proxy,
        config.local_listen_port(),
    )?
    .build()?;
    // Recheck immediately before the single network operation; this function
    // has no persistence or retry path, and never changes normal credentials.
    ensure!(
        AppConfig::load_or_default(&path.to_path_buf())?.revision == config.revision,
        "配置已变化，测试尚未发送，请重新打开"
    );
    let probe = http
        .request(outbound.method().clone(), outbound.url().clone())
        .headers(outbound.headers().clone())
        .body(
            outbound
                .body()
                .and_then(|b| b.as_bytes())
                .unwrap_or_default()
                .to_vec(),
        )
        .build()
        .map_err(|_| anyhow::anyhow!("无法建立天工测试请求"))?;
    let response = http
        .execute(probe)
        .map_err(|_| anyhow::anyhow!("测试请求超时或连接中断；未重发，可查看服务端后手动重试"))?;
    let status = response.status();
    let message = if !status.is_success() {
        "服务端未接受这组身份；请核对服务端授权或稍后重试"
    } else {
        "HTTP 成功，但没有收到完整有效的模型回复；请核对服务端响应格式后重试"
    };
    let mut bytes = Vec::new();
    // An upstream failure may echo account/installation headers. Do not return
    // or log its raw body, even in the debugging UI.
    let valid = if status.is_success() {
        response
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("测试响应中断，未重发"))?;
        ensure!(
            bytes.len() <= 256 * 1024,
            "测试响应过大，已停止读取，未重发"
        );
        valid_response(&bytes)
    } else {
        false
    };
    Ok(
        json!({"ok":valid,"httpStatus":status.as_u16(),"mode":request.mode,"testUserId":if request.mode==IdentityMode::Random {Some(user_id)}else{None},"message":if valid{"已收到有效模型回复"}else{message},"saved":false}),
    )
}
fn valid_response(bytes: &[u8]) -> bool {
    let has_text = |value: &Value| {
        value["choices"].as_array().is_some_and(|choices| {
            choices.iter().any(|c| {
                c["message"]["content"]
                    .as_str()
                    .or_else(|| c["delta"]["content"].as_str())
                    .is_some_and(|s| !s.is_empty())
            })
        })
    };
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        return has_text(&value);
    }
    let text = String::from_utf8_lossy(bytes);
    let mut seen = false;
    let mut done = false;
    for line in text
        .lines()
        .filter_map(|s| s.strip_prefix("data:").map(str::trim))
    {
        if line == "[DONE]" {
            done = true;
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        if value.get("error").is_some_and(|v| !v.is_null()) {
            return false;
        }
        seen |= has_text(&value);
    }
    seen && done
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_account_and_random_tests_only_change_ephemeral_identity_header() {
        let native = test_user(IdentityMode::Native, None).unwrap();
        assert!(native.is_empty());
        assert!(test_user(IdentityMode::Account, None).is_err());
        let credential = Credential {
            pat: String::new(),
            user_id: "fixture-user".into(),
            session: Some(session::LoginSession {
                access_token: "fixture-account-token".into(),
                user_id: "fixture-user".into(),
                display_name: "fixture".into(),
                auth_base_url: session::AUTH_BASE.into(),
                expires_at: 0,
            }),
        };
        let account = test_user(IdentityMode::Account, Some(&credential)).unwrap();
        let random = test_user(IdentityMode::Random, None).unwrap();
        assert!(uuid::Uuid::parse_str(&random).is_ok());
        assert_ne!(random, test_user(IdentityMode::Random, None).unwrap());
        let (_temp, db, _) = super::super::tests::fixture();
        let p = Parameters::read(&Connection::open(&db).unwrap(), IDS[0].0).unwrap();
        for id in [native, account, random] {
            let request = prepare(
                ENDPOINT,
                "fixture-model",
                &p,
                "fixture-pat".into(),
                id.clone(),
            )
            .unwrap();
            assert_eq!(
                request
                    .headers()
                    .get("x-tiangong-end-user-id")
                    .map(|v| v.to_str().unwrap()),
                (!id.is_empty()).then_some(id.as_str())
            );
            assert!(!format!("{request:?}").contains("fixture-pat"));
            assert!(request.headers().get("authorization").is_none());
        }
    }
}
