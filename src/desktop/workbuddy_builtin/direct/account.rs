//! Public account/credit projection. Refresh is explicit; opening a page is local.
use super::*;

const ACCOUNT_TIMEOUT: Duration = Duration::from_secs(20);
const PERSONAL_PATH: &str = "/billing/meter/get-user-resource-summary";
const ENTERPRISE_PATH: &str = "/v2/billing/meter/get-enterprise-user-usage";

pub(super) struct CreditsCache {
    account_id: String,
    value: Value,
}

impl Runtime {
    pub(crate) async fn public_account_status(&self) -> Value {
        let state = self.state.lock().await;
        account_status(&state)
    }

    /// This is the only balance-network entry; callers must already have enabled
    /// the Runtime. The cache never goes into the protected authorization record.
    pub(crate) async fn refresh_account(&self) -> Result<Value> {
        let mut changed = self.changed.subscribe();
        let deadline = Instant::now() + ACCOUNT_TIMEOUT;
        let (mut auth, mut revision, lifecycle) = {
            let state = self.state.lock().await;
            let stored = state
                .stored
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("请先启用 WorkBuddy 分享并授权，再更新账户积分"))?;
            (
                stored.authorization.clone(),
                state.revision,
                state.lifecycle,
            )
        };
        let account_id = auth.account_id();
        let _guard = tokio::select! {
            biased;
            _ = changed.changed() => return Err(anyhow::anyhow!(CANCELLED)),
            guard = tokio::time::timeout_at(deadline, self.account_gate.lock()) => guard.map_err(|_| anyhow::anyhow!("WorkBuddy 积分更新超时，请稍后重试"))?,
        };
        let result = self
            .fetch_credits(&mut auth, &mut revision, lifecycle, &mut changed, deadline)
            .await;
        let mut state = self.state.lock().await;
        // A refresh can itself advance the token revision. A different capture or
        // account change must not publish an old success or error on the new UI.
        let same_authorization = state.lifecycle == lifecycle
            && state.revision == revision
            && state
                .stored
                .as_ref()
                .is_some_and(|s| s.authorization.account_id() == account_id);
        if !same_authorization {
            return Err(anyhow::anyhow!(
                "WorkBuddy 授权已更新，本次积分结果未保存，请重新更新"
            ));
        }
        match result {
            Ok(value) => {
                state.account_credits = Some(CreditsCache { account_id, value });
                Ok(account_status(&state))
            }
            Err(error) => {
                let detail = account_error(&error);
                let mut value = state
                    .account_credits
                    .as_ref()
                    .filter(|c| c.account_id == account_id)
                    .map(|c| c.value.clone())
                    .unwrap_or_else(unknown_credits);
                value["status"] = json!("error");
                value["detail"] = json!(detail);
                state.account_credits = Some(CreditsCache { account_id, value });
                Err(anyhow::anyhow!(detail))
            }
        }
    }

    async fn fetch_credits(
        &self,
        auth: &mut Authorization,
        revision: &mut u64,
        lifecycle: u64,
        changed: &mut watch::Receiver<u64>,
        deadline: Instant,
    ) -> std::result::Result<Value, GatewayError> {
        {
            let state = self.state.lock().await;
            check_lifecycle(changed, lifecycle)?;
            if state.blocked {
                return Err(GatewayError::bad_request(REAUTHORIZE));
            }
            let stored = state
                .stored
                .as_ref()
                .ok_or_else(|| GatewayError::bad_request(REAUTHORIZE))?;
            if state.revision != *revision {
                *auth = stored.authorization.clone();
                *revision = state.revision;
            }
        }
        let mut refreshed = false;
        if auth.auth.expires_at.is_some_and(|at| at <= now_ms()) {
            (*auth, *revision) = self
                .refresh(*revision, lifecycle, changed, deadline)
                .await?;
            refreshed = true;
        }
        let enterprise = has_enterprise(auth);
        let value = loop {
            let response = self
                .credit_request(auth, enterprise, lifecycle, changed, deadline)
                .await?;
            let status = response.status();
            let value = if status.is_success() {
                Some(read_json(response, changed, lifecycle, deadline).await?)
            } else {
                drop(response);
                None
            };
            let unauthorized = status == StatusCode::UNAUTHORIZED
                || value
                    .as_ref()
                    .and_then(|v| v.get("code"))
                    .and_then(Value::as_i64)
                    == Some(401);
            if unauthorized {
                if refreshed {
                    self.block_if_current(lifecycle, *revision, REAUTHORIZE)
                        .await;
                    return Err(GatewayError::upstream(
                        StatusCode::UNAUTHORIZED,
                        REAUTHORIZE,
                    ));
                }
                (*auth, *revision) = self
                    .refresh(*revision, lifecycle, changed, deadline)
                    .await?;
                refreshed = true;
                continue;
            }
            if !status.is_success() {
                return Err(upstream_error(status));
            }
            break value.unwrap();
        };
        let data = successful_data(&value).ok_or_else(|| {
            GatewayError::upstream(StatusCode::BAD_GATEWAY, "WorkBuddy 没有返回可用积分数据")
        })?;
        let projected = if enterprise {
            enterprise_credits(data)
        } else {
            personal_credits(data)
        }
        .map_err(|_| {
            GatewayError::upstream(StatusCode::BAD_GATEWAY, "WorkBuddy 积分数据格式暂不兼容")
        })?;
        Ok(projected)
    }

    async fn credit_request(
        &self,
        auth: &Authorization,
        enterprise: bool,
        lifecycle: u64,
        changed: &mut watch::Receiver<u64>,
        deadline: Instant,
    ) -> std::result::Result<Response, GatewayError> {
        // These POSTs are the installed desktop application's read-only billing
        // methods. Do not guess GET variants or retry a second billing endpoint.
        let mut url = endpoint(&auth.gateway_endpoint)
            .map_err(|_| GatewayError::bad_request("WorkBuddy 积分接口地址不兼容"))?;
        let base = url.path().trim_end_matches('/');
        let suffix = if enterprise {
            ENTERPRISE_PATH
        } else {
            PERSONAL_PATH
        };
        url.set_path(&format!("{base}{suffix}"));
        let mut headers = auth.headers(&auth.auth.access_token)?;
        headers.insert("accept-language", HeaderValue::from_static("zh"));
        let request = self
            .client
            .post(url)
            .headers(headers)
            .json(&json!({}))
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .build()
            .map_err(|_| GatewayError::bad_request("无法构建 WorkBuddy 积分查询"))?;
        self.execute(request, lifecycle, changed, deadline).await
    }
}

fn account_status(state: &State) -> Value {
    let Some(stored) = &state.stored else {
        return json!({"authorization":{"status":"missing"},"credits":unknown_credits()});
    };
    let auth = &stored.authorization;
    let mut authorization = json!({"status":authorization_status(state, auth)});
    if let Some(at) = auth.auth.expires_at {
        authorization["expiresAt"] = json!(at);
    }
    if let Some(at) = auth.auth.refresh_expires_at {
        authorization["refreshExpiresAt"] = json!(at);
    }
    let credits = state
        .account_credits
        .as_ref()
        .filter(|c| c.account_id == auth.account_id())
        .map(|c| c.value.clone())
        .unwrap_or_else(unknown_credits);
    json!({"accountId":auth.account_id(),"accountName":stored.metadata.get("accountName"),"authorization":authorization,"credits":credits})
}

fn authorization_status(state: &State, auth: &Authorization) -> &'static str {
    if state.blocked {
        return "rejected";
    }
    if auth.auth.expires_at.is_none_or(|at| at > now_ms()) {
        return "available";
    }
    if auth
        .auth
        .refresh_token
        .as_ref()
        .is_some_and(|s| !s.is_empty())
        && auth.auth.refresh_expires_at.is_none_or(|at| at > now_ms())
    {
        return "refreshable";
    }
    "expired"
}

fn has_enterprise(auth: &Authorization) -> bool {
    auth.account
        .enterprise_id
        .as_ref()
        .is_some_and(|v| match v {
            Value::String(s) => !s.is_empty(),
            Value::Number(n) => n.as_u64() != Some(0),
            _ => false,
        })
}

fn unknown_credits() -> Value {
    json!({"status":"unknown","unit":"credits","detail":"尚未更新 WorkBuddy 积分"})
}

// Preserve upstream decimal numbers without introducing display-unit conversion.
// A missing or malformed value is not a zero balance.
fn quantity(value: Option<&Value>) -> Option<Value> {
    let value = value?;
    if let Some(number) = value.as_f64() {
        return (number.is_finite() && number >= 0.0).then(|| value.clone());
    }
    let text = value.as_str()?;
    if text.trim() != text || text.is_empty() {
        return None;
    }
    let number = text.parse::<f64>().ok()?;
    (number.is_finite() && number >= 0.0).then(|| json!(number))
}

fn enterprise_credits(data: &Value) -> Result<Value> {
    let unlimited = data.get("limit_num").and_then(Value::as_f64) == Some(-1.0)
        || data.get("limit_num").and_then(Value::as_str) == Some("-1");
    let mut result =
        json!({"status":"ready","unit":"credits","scope":"enterpriseMember","updatedAt":now_ms()});
    if unlimited {
        result["unlimited"] = json!(true);
        if let Some(used) = quantity(data.get("used_num")) {
            result["used"] = used;
        }
        result["detail"] = json!("当前组织未设置成员积分上限");
        return Ok(result);
    }
    if let Some(total) = quantity(data.get("limit_num")) {
        result["total"] = total;
    }
    if let Some(used) = quantity(data.get("used_num")) {
        result["used"] = used;
    }
    if let (Some(total), Some(used)) = (result["total"].as_f64(), result["used"].as_f64()) {
        result["remaining"] = json!(total - used);
        result["remainingBasis"] = json!("limitMinusUsed");
    }
    ensure!(
        result.get("total").is_some() || result.get("used").is_some(),
        "missing enterprise credits"
    );
    result["detail"] = json!("当前组织为该成员分配的积分额度");
    Ok(result)
}

fn personal_credits(data: &Value) -> Result<Value> {
    let packages = data
        .get("Packages")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing personal credit packages"))?;
    let mut items = Vec::new();
    for (index, package) in packages.iter().enumerate() {
        let name = package
            .get("PackageName")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let code = package
            .get("PackageCode")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let label = name
            .map(str::to_owned)
            .or_else(|| code.map(str::to_owned))
            .unwrap_or_else(|| format!("积分包 {}", index + 1));
        let mut item = json!({"label":label});
        if let Some(code) = code {
            item["packageCode"] = json!(code);
        }
        for (public, field) in [
            ("total", "CycleTotalCapacity"),
            ("remaining", "CycleRemainCapacity"),
            ("used", "CycleUsedCapacity"),
        ] {
            if let Some(value) = quantity(package.get(field)) {
                item[public] = value;
            }
        }
        items.push(item);
    }
    let mut result = json!({"status":"ready","unit":"credits","scope":"personalPackages","updatedAt":now_ms(),"quotaItems":items});
    if packages.is_empty() {
        result["status"] = json!("unknown");
        result["detail"] = json!("WorkBuddy 未返回当前可用积分包");
        return Ok(result);
    }
    let items = result["quotaItems"].as_array().unwrap();
    let sums = ["total", "remaining", "used"].map(|field| {
        let sum = items.iter().try_fold(0.0, |total, item| {
            item.get(field)
                .and_then(Value::as_f64)
                .map(|value| total + value)
        });
        (field, sum.filter(|s| s.is_finite()))
    });
    for (field, value) in sums {
        if let Some(value) = value {
            result[field] = json!(value);
        }
    }
    if !result["quotaItems"].as_array().unwrap().iter().any(|item| {
        ["remaining", "total", "used"]
            .iter()
            .any(|field| item.get(*field).is_some())
    }) {
        result["status"] = json!("unknown");
        result["detail"] = json!("WorkBuddy 未提供这些积分包的余额数值");
        return Ok(result);
    }
    if let Some(name) = data
        .get("SubscriptionPackageName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        result["planName"] = json!(name);
    }
    result["detail"] = json!("按 WorkBuddy 当前积分包周期汇总；各积分包明细分别保留");
    Ok(result)
}

fn account_error(error: &GatewayError) -> &'static str {
    if matches!(error.status.as_u16(), 401 | 403) || error.message == REAUTHORIZE {
        return "WorkBuddy 账户授权不可用，请重新授权后更新积分";
    }
    if error.code == "upstream_timeout" {
        return "WorkBuddy 积分更新超时，请稍后重试";
    }
    match error.status.as_u16() {
        429 => "WorkBuddy 积分查询暂时受限，请稍后重试",
        404 => "当前 WorkBuddy 版本的积分接口不可用",
        _ => "WorkBuddy 积分未更新，请稍后重试；已有数值仍为上次查询结果",
    }
}

#[cfg(test)]
mod tests;
