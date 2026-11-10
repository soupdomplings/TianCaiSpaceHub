//! Site-reported key costs. Called while SESSION_GATE is held, alongside the
//! keys read; no synthetic costs, quota-counter fallback or write transaction.
use super::*;

impl Squad {
    pub(super) async fn enrich_key_usage(&self, session: &Session, list: &mut Value) {
        let Some(rows) = list["items"].as_array() else {
            return;
        };
        let mut ids: Vec<i64> = rows
            .iter()
            .filter_map(|r| r["id"].as_i64().filter(|id| *id > 0))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return;
        }
        let mut stats = serde_json::Map::new();
        let mut failed = false;
        for chunk in ids.chunks(100) {
            let result = self
                .send(
                    Method::POST,
                    "/usage/dashboard/api-keys-usage",
                    &[],
                    Some(&json!({"api_key_ids":chunk})),
                    Some(&session.access_token),
                )
                .await
                .and_then(|(status, data)| Self::unwrap(status, data));
            match result {
                Ok(value) if value["stats"].is_object() => {
                    stats.extend(value["stats"].as_object().unwrap().clone());
                }
                _ => {
                    failed = true;
                    break;
                }
            }
        }
        let complete = merge(list, &stats);
        if failed || !complete {
            list["usageLoadError"] = "部分密钥用量未读取完成，点击右上角刷新重试".into();
        }
    }
}

/// Key identities must match both the map key and the site's api_key_id.
/// Missing or malformed usage is unknown, never a fabricated zero.
fn merge(list: &mut Value, stats: &serde_json::Map<String, Value>) -> bool {
    let mut complete = true;
    let Some(rows) = list["items"].as_array_mut() else {
        return true;
    };
    for row in rows {
        let id = row["id"].as_i64().filter(|id| *id > 0);
        let stat = id.and_then(|id| {
            stats
                .get(&id.to_string())
                .filter(|s| s["api_key_id"].as_i64() == Some(id))
        });
        for field in ["today_actual_cost", "total_actual_cost"] {
            let amount = stat
                .map(|s| &s[field])
                .filter(|v| v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.));
            row[field] = amount.cloned().unwrap_or(Value::Null);
            complete &= amount.is_some();
        }
    }
    complete
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn costs_use_site_usage_without_quota_counter_or_currency_conversion() {
        let mut list = json!({"items":[{"id":7,"quota":0,"quota_used":0},{"id":8,"quota":20,"quota_used":3}],"total":2});
        let stats = json!({"7":{"api_key_id":7,"today_actual_cost":0.000057,"total_actual_cost":12.3456},"8":{"api_key_id":8,"today_actual_cost":0,"total_actual_cost":0}});
        assert!(merge(&mut list, stats.as_object().unwrap()));
        assert_eq!(list["items"][0]["total_actual_cost"], 12.3456);
        assert_eq!(list["items"][0]["quota_used"], 0);
        assert_eq!(list["items"][1]["quota_used"], 3);
        assert_eq!(list["total"], 2);
        // A returned zero is meaningful, distinct from an unread amount.
        assert_eq!(list["items"][1]["total_actual_cost"], 0.);
    }
    #[test]
    fn missing_malformed_and_mismatched_usage_does_not_borrow_another_keys_cost() {
        let mut list = json!({"items":[{"id":7},{"id":8},{"id":9},{"id":10}]});
        let stats = json!({"7":{"api_key_id":8,"today_actual_cost":4,"total_actual_cost":8},"8":{"api_key_id":8,"today_actual_cost":"invalid","total_actual_cost":-2},"10":{"api_key_id":10,"today_actual_cost":1,"total_actual_cost":2}});
        assert!(!merge(&mut list, stats.as_object().unwrap()));
        for i in 0..3 {
            assert!(list["items"][i]["total_actual_cost"].is_null());
        }
        assert_eq!(list["items"][3]["total_actual_cost"], 2.);
    }
    #[tokio::test]
    async fn keys_fetch_usage_once_cache_together_and_keep_rows_on_failure() {
        use axum::{Router, extract::Request, response::IntoResponse};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().fallback(move |req: Request| {
            let count = count.clone();
            async move {
                assert_eq!(req.headers()["authorization"], "Bearer fixture-access");
                let path = req.uri().path().to_string();
                let data = if path.ends_with("/keys") {
                    assert_eq!(req.method(), Method::GET);
                    json!({"items":[{"id":12,"quota":0,"quota_used":0}],"total":1})
                } else {
                    assert!(path.ends_with("/usage/dashboard/api-keys-usage"));
                    assert_eq!(req.method(), Method::POST);
                    let bytes = axum::body::to_bytes(req.into_body(), 4096).await.unwrap();
                    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), json!({"api_key_ids":[12]}));
                    if count.fetch_add(1, Ordering::SeqCst) > 0 {
                        return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"code":503}))).into_response();
                    }
                    json!({"stats":{"12":{"api_key_id":12,"today_actual_cost":0.1234,"total_actual_cost":15.75}}})
                };
                axum::Json(json!({"code":0,"data":data})).into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let squad = Squad::synthetic(
            dir.path().join("squad"),
            Client::builder().no_proxy().build().unwrap(),
            origin,
        )
        .unwrap();
        squad
            .save(&Session {
                access_token: "fixture-access".into(),
                refresh_token: String::new(),
                expires_at: 0,
                generation: "fixture-only".into(),
                user_id: 1,
            })
            .unwrap();
        let result = squad.request("GET", "/keys", vec![], None).await.unwrap();
        assert_eq!(result["items"][0]["total_actual_cost"], 15.75);
        assert_eq!(
            squad.request("GET", "/keys", vec![], None).await.unwrap(),
            result
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        squad.invalidate();
        let failed = squad.request("GET", "/keys", vec![], None).await.unwrap();
        assert_eq!(failed["items"][0]["id"], 12);
        assert!(failed["items"][0]["total_actual_cost"].is_null());
        assert!(failed["usageLoadError"].is_string());
        assert!(squad.pending().unwrap().is_empty());
        server.abort();
    }
}
