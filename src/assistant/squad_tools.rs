//! Current-user key selection and import. Only allowlisted metadata reaches the model.
use super::{tools, *};
use serde::Deserialize;

pub(super) fn definitions() -> Vec<Value> {
    vec![
        tools::spec(
            "hub_squad_list_api_keys",
            "读取当前已登录小分队用户的 API 密钥列表，仅返回 keyId、名称、掩码、状态和分组等元数据。按用户指定的密钥调用 prepare 工具；不返回明文密钥。可分页或按名称搜索，不创建密钥。",
            json!({"page":{"type":"integer","minimum":1},"pageSize":{"type":"integer","minimum":1,"maximum":100},"search":{"type":"string"}}),
            &[],
        ),
        tools::spec(
            "hub_squad_key_options",
            "读取当前用户可绑定 API 密钥的真实渠道分组。新建前读取；用户未指定分组时只有唯一可用组可直接选择，多组有歧义则询问。模型和API地址在prepare后读取，不猜测。",
            json!({}),
            &[],
        ),
        tools::spec(
            "hub_squad_create_api_key",
            "按用户明确要求，为当前已登录小分队用户创建一个 API 密钥，返回 keyId 与掩码。仅此用户原有权限；不操作余额/组织/支付。结果未知不可重复创建，应先列表核对。随后用 keyId prepare，取得凭据引用和真实地址/模型，再保存普通渠道与客户端。",
            json!({"name":{"type":"string"},"groupId":{"type":["integer","null"],"minimum":1,"description":"从key_options取得；null为用户明确要求不指定分组"},"quota":{"type":"number","minimum":0,"description":"密钥额度上限，0为不限额；沿站点原始数值"},"expiresInDays":{"type":"integer","minimum":1,"description":"有效天数；省略为不过期"}}),
            &["name"],
        ),
        tools::spec(
            "hub_squad_prepare_api_key",
            "为用户指定的当前账户 keyId 准备导入。复用一次性导入合同与模型目录，返回 credentialReference、mask、suggestedChannel、settings（真实协议、地址、models、modelAliases）。尚未保存；将返回的settings及引用交hub_save_channel，按用户要求enabled=true后配置客户端。禁止把密钥明文交给模型。",
            json!({"keyId":{"type":"integer","minimum":1}}),
            &["keyId"],
        ),
    ]
}

fn rows(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .or_else(|| value["items"].as_array())
        .context("小分队返回的列表格式无效，请在账户页重新读取")
}
fn group_view(group: &Value) -> Value {
    json!({"groupId":group["id"],"name":group["name"],"platform":group["platform"],"status":group["status"]})
}
fn key_view(key: &Value) -> Value {
    json!({"keyId":key["id"],"name":key["name"],"status":key["status"],
        "mask":key["key"].as_str().map(tools::mask),"groupId":key["group_id"],
        "group":key.get("group").filter(|g|g.is_object()).map(group_view),
        "quota":key["quota"],"expiresAt":key["expires_at"],"createdAt":key["created_at"]})
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListKeys {
    page: Option<u64>,
    page_size: Option<u64>,
    search: Option<String>,
}
pub(super) async fn list_keys(state: &SharedState, args: Value) -> Result<Value> {
    let args: ListKeys = serde_json::from_value(args).context("密钥列表参数无效")?;
    let page = args.page.unwrap_or(1);
    let page_size = args.page_size.unwrap_or(20);
    ensure!(page > 0 && (1..=100).contains(&page_size), "分页参数无效");
    let mut query = vec![
        ("page".into(), page.to_string()),
        ("page_size".into(), page_size.to_string()),
    ];
    if let Some(search) = args.search {
        ensure!(search.len() <= 256, "搜索内容过长");
        query.push(("search".into(), search));
    }
    let (squad, account) = super::tool_account(state).await?;
    let value = squad
        .request_checked("GET", "/keys", query, None, Some(&account))
        .await?;
    super::ensure_current_account(state).await?;
    Ok(
        json!({"keys":rows(&value)?.iter().map(key_view).collect::<Vec<_>>(),
        "page":value.get("page").cloned().unwrap_or(json!(page)),
        "pageSize":value.get("page_size").cloned().unwrap_or(json!(page_size)),
        "total":value["total"],"pages":value["pages"],
        "message":"仅列当前账户密钥元数据；使用选定keyId准备导入，不需要用户输入密钥"}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
pub(super) async fn key_options(state: &SharedState, args: Value) -> Result<Value> {
    let _: Empty = serde_json::from_value(args).context("可用分组参数无效")?;
    let (squad, account) = super::tool_account(state).await?;
    let value = squad
        .request_checked("GET", "/groups/available", vec![], None, Some(&account))
        .await?;
    super::ensure_current_account(state).await?;
    Ok(
        json!({"groups":rows(&value)?.iter().map(group_view).collect::<Vec<_>>(),
        "groupCanBeNull":true,"message":"选择真实groupId；多组有歧义时询问用户。可用模型和地址在选定密钥prepare后获取"}),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateKey {
    name: String,
    group_id: Option<u64>,
    quota: Option<f64>,
    expires_in_days: Option<u64>,
}
impl CreateKey {
    fn body(&self) -> Result<Value> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 256,
            "密钥名称应为1～256字节"
        );
        ensure!(self.group_id.is_none_or(|id| id > 0), "渠道分组无效");
        ensure!(
            self.quota.is_none_or(|n| n.is_finite() && n >= 0.),
            "额度上限无效"
        );
        ensure!(
            self.expires_in_days
                .is_none_or(|days| days > 0 && days <= i32::MAX as u64),
            "有效天数无效"
        );
        let mut body = json!({"name":self.name.trim(),"group_id":self.group_id});
        if let Some(quota) = self.quota {
            body["quota"] = json!(quota);
        }
        if let Some(days) = self.expires_in_days {
            body["expires_in_days"] = json!(days);
        }
        Ok(body)
    }
}
pub(super) async fn create_key(state: &SharedState, args: Value) -> Result<Value> {
    let args: CreateKey = serde_json::from_value(args).context("创建密钥参数无效")?;
    let body = args.body()?;
    let (squad, account) = super::tool_account(state).await?;
    // request_checked retains the existing unresolved-write record. Never replay on timeout/5xx.
    let key = squad
        .request_checked("POST", "/keys", vec![], Some(body), Some(&account))
        .await?;
    super::ensure_current_account(state).await?;
    ensure!(
        key["id"].as_u64().is_some_and(|id| id > 0),
        "创建结果缺少密钥ID，请先读取列表核对，不要重复创建"
    );
    let mut view = key_view(&key);
    view["created"] = true.into();
    view["message"] = "密钥已创建；尚未保存到Hub渠道。下一步用keyId准备导入".into();
    Ok(view)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareKey {
    key_id: u64,
}
pub(super) async fn prepare_key(state: &SharedState, args: Value) -> Result<Value> {
    let args: PrepareKey = serde_json::from_value(args).context("准备密钥参数无效")?;
    ensure!(args.key_id > 0, "请选择有效密钥");
    let generation = tools::generation(state).await;
    let (squad, account) = super::tool_account(state).await?;
    let draft = squad.prepare_import_checked(args.key_id, &account).await?;
    tools::remember_import(state, draft, generation).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, response::IntoResponse};
    use std::sync::{Mutex as StdMutex, atomic::AtomicBool};

    const KEY: &str = "synthetic-user-model-key-not-for-assistant";
    #[derive(Default)]
    struct Site {
        calls: StdMutex<Vec<(String, String)>>,
        fail_create: AtomicBool,
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        state: SharedState,
        squad: Arc<Squad>,
        site: Arc<Site>,
        server: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }
    impl Fixture {
        async fn call(&self, name: &str, args: Value) -> Result<Value> {
            super::super::test_tool_scope(&self.state, tools::call(&self.state, name, args)).await
        }
        fn count(&self, method: &str, path: &str) -> usize {
            self.site
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, p)| m == method && p == path)
                .count()
        }
    }
    async fn fixture() -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let site = Arc::new(Site::default());
        let (seen, url) = (site.clone(), origin.clone());
        let app = Router::new().fallback(move |request: axum::extract::Request| {
            let (site,origin)=(seen.clone(),url.clone());
            async move {
                let path=request.uri().path().to_string();
                let method=request.method().as_str().to_string();
                site.calls.lock().unwrap().push((method.clone(),path.clone()));
                let auth=request.headers().get("authorization").and_then(|v|v.to_str().ok()).unwrap_or("").to_string();
                if path=="/api/v1/external-import/resolve" {
                    assert!(auth.is_empty(),"login JWT cannot reach ticket redemption");
                    return axum::Json(json!({"code":0,"data":{"schema_version":1,"target":"tiancaispace-hub",
                        "source":{"origin":origin,"key_id":"12","key_name":"合成用户密钥","site_name":"合成站点"},
                        "provider":{"name":"合成导入渠道","protocol":"openai_responses","base_url":format!("{origin}/v1"),
                            "api_key":KEY,"models":["model-a"],"model_aliases":{}}}})).into_response();
                }
                if path=="/v1/models" {
                    assert_eq!(auth,format!("Bearer {KEY}"));
                    return axum::Json(json!({"data":[{"id":"model-a"},{"id":"other-family"}]})).into_response();
                }
                assert_eq!(auth,"Bearer synthetic-assistant-token");
                if path=="/api/v1/keys/12/external-import" {
                    assert_eq!(request.headers().get("origin").unwrap(),origin.as_str());
                    let bytes=axum::body::to_bytes(request.into_body(),4096).await.unwrap();
                    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(),json!({"target":"tiancaispace-hub","schema_version":1}));
                    let mut link=url::Url::parse("tiancaispacehub://import/v1").unwrap();
                    link.query_pairs_mut().append_pair("origin",&origin).append_pair("ticket",&"a".repeat(43));
                    return axum::Json(json!({"code":0,"data":{"deeplink":link.as_str()}})).into_response();
                }
                let key=json!({"id":12,"name":"合成用户密钥","key":KEY,"status":"active","group_id":3,
                    "user":{"email":"private@example.invalid","token":"private-login-token"},
                    "group":{"id":3,"name":"合成组","platform":"openai","status":"active","api_key":KEY,"rate_multiplier":9},
                    "quota":0,"quota_used":123,"today_actual_cost":4});
                if path=="/api/v1/keys" && method=="POST" {
                    if site.fail_create.load(Ordering::SeqCst) {
                        return (StatusCode::SERVICE_UNAVAILABLE,axum::Json(json!({"code":503,"message":"合成暂不可用"}))).into_response();
                    }
                    let bytes=axum::body::to_bytes(request.into_body(),4096).await.unwrap();
                    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(),json!({"name":"合成用户密钥","group_id":3,"quota":0.0,"expires_in_days":1}));
                    return axum::Json(json!({"code":0,"data":key})).into_response();
                }
                let data=match path.as_str() {
                    "/api/v1/keys"=>json!({"items":[key],"total":1,"page":1,"page_size":20,"pages":1}),
                    "/api/v1/groups/available"=>json!([{"id":3,"name":"合成组","platform":"openai","status":"active","api_key":KEY,"rate_multiplier":9}]),
                    "/api/v1/usage/dashboard/api-keys-usage"=>json!({"stats":{"12":{"api_key_id":12,"today_actual_cost":4,"total_actual_cost":123}}}),
                    _=>return (StatusCode::NOT_FOUND,axum::Json(json!({"code":404}))).into_response(),
                };
                axum::Json(json!({"code":0,"data":data})).into_response()
            }
        });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let mut config = crate::config::AppConfig {
            state_path: dir.path().join("state.json"),
            ..Default::default()
        };
        config.save(&config_path).unwrap();
        let squad = Arc::new(
            Squad::synthetic(
                dir.path().join("squad"),
                reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                origin,
            )
            .unwrap(),
        );
        let account = squad
            .assistant_test_identity("synthetic-key-tools")
            .unwrap();
        let state = crate::app_state::AppState::new(config_path, config, None, None);
        state.assistant.inner.lock().await.bridge = Some(super::super::proxy::Bridge::new(
            "synthetic-key-tools".into(),
            "synthetic-bridge".into(),
            squad.clone(),
            account,
            "fixture-model".into(),
        ));
        Fixture {
            _dir: dir,
            state,
            squad,
            site,
            server,
        }
    }
    fn assert_public(value: &Value) {
        let text = value.to_string();
        for private in [
            KEY,
            "private-login-token",
            "private@example.invalid",
            "api_key",
            "rate_multiplier",
            "quota_used",
            "actual_cost",
        ] {
            assert!(
                !text.contains(private),
                "unexpected private field in projected tool result"
            );
        }
    }
    #[tokio::test]
    async fn created_or_selected_key_uses_import_reference_and_native_channel_writer() {
        let fixture = fixture().await;
        let groups = fixture
            .call("hub_squad_key_options", json!({}))
            .await
            .unwrap();
        assert_public(&groups);
        assert_eq!(groups["groups"][0]["groupId"], 3);
        let created = fixture
            .call(
                "hub_squad_create_api_key",
                json!({"name":"合成用户密钥","groupId":3,"quota":0,"expiresInDays":1}),
            )
            .await
            .unwrap();
        assert_public(&created);
        assert_eq!(created["keyId"], 12);
        let list = fixture
            .call("hub_squad_list_api_keys", json!({}))
            .await
            .unwrap();
        assert_public(&list);
        assert_eq!(list["keys"][0]["keyId"], created["keyId"]);
        assert!(!list["keys"][0]["mask"].is_null());
        let prepared = fixture
            .call(
                "hub_squad_prepare_api_key",
                json!({"keyId":list["keys"][0]["keyId"]}),
            )
            .await
            .unwrap();
        assert_public(&prepared);
        assert_eq!(prepared["settings"]["providerType"], "open_ai_responses");
        assert_eq!(prepared["settings"]["models"], json!(["model-a"]));
        let mut settings = prepared["settings"].clone();
        settings["enabled"] = true.into();
        fixture.call("hub_save_channel",json!({"name":"合成普通渠道","settings":settings,"credentialReference":prepared["credentialReference"]})).await.unwrap();
        let config = crate::config::AppConfig::load_or_default(&fixture.state.config_path).unwrap();
        let saved = &config.ai_gateway.providers[0];
        assert_eq!(saved.api_key, KEY);
        assert_eq!(saved.import_source.as_ref().unwrap().key_id, "12");
        assert!(saved.enabled);
        assert_eq!(fixture.count("POST", "/api/v1/keys"), 1);
        assert_eq!(fixture.count("POST", "/api/v1/keys/12/external-import"), 1);
        assert_eq!(fixture.count("POST", "/api/v1/external-import/resolve"), 1);
        tools::clear(&fixture.state).await;
        assert!(
            tools::public_status(&fixture.state).await["credentialReferences"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            crate::config::AppConfig::load_or_default(&fixture.state.config_path)
                .unwrap()
                .ai_gateway
                .providers[0]
                .api_key,
            KEY
        );
    }
    #[tokio::test]
    async fn uncertain_create_is_not_replayed_and_old_account_cannot_prepare() {
        let fixture = fixture().await;
        fixture.site.fail_create.store(true, Ordering::SeqCst);
        let input = json!({"name":"合成用户密钥","groupId":3});
        assert!(
            fixture
                .call("hub_squad_create_api_key", input.clone())
                .await
                .is_err()
        );
        assert!(
            fixture
                .call("hub_squad_create_api_key", input)
                .await
                .is_err()
        );
        assert_eq!(fixture.count("POST", "/api/v1/keys"), 1);
        let account = fixture.squad.assistant_identity().unwrap().unwrap();
        fixture
            .squad
            .assistant_test_identity("new-account-generation")
            .unwrap();
        assert!(
            fixture
                .squad
                .prepare_import_checked(12, &account)
                .await
                .is_err()
        );
        assert_eq!(fixture.count("POST", "/api/v1/keys/12/external-import"), 0);
    }
}
