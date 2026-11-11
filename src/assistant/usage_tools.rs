//! Reuse Hub's existing usage readers and current-user squad APIs.
use super::*;
use crate::ai_gateway::request_log::desktop::LogFilter;
use serde::Deserialize;

pub(super) fn definitions() -> Vec<Value> {
    let filters = json!({
        "source":{"type":"string","enum":["squad","hub"],"description":"默认squad：当前小分队账户实际用量/扣费；hub：本机转发记录，不能当账单"},
        "period":{"type":"string","enum":["today","week","month","custom"],"description":"默认今天；week/month为所选来源的近一周/近一月，自定日期两端均包含"},
        "startDate":{"type":"string","description":"自定开始日期YYYY-MM-DD，与endDate一起传"},
        "endDate":{"type":"string","description":"自定结束日期YYYY-MM-DD"},
        "model":{"type":"string"},
        "client":{"type":"string","description":"仅hub：codex/workbuddy/tiangong/zcode/deepseek-harness/genius-buddy等实际clientId"},
        "channel":{"type":"string","description":"仅hub：实际渠道名称"},
        "apiKeyId":{"type":"integer","minimum":1,"description":"仅squad：指定当前用户密钥"},
        "groupId":{"type":"integer","minimum":1,"description":"仅squad：渠道分组"},
        "status":{"type":"string","description":"仅hub：success/failed/cancelled/running"}
    });
    let mut usage = filters.clone();
    usage["breakdown"] = json!({"type":"string","enum":["models","days","clients","channels"],"description":"按需返回明细分组；squad支持models/days，hub支持全部"});
    let mut requests = filters;
    requests["page"] = json!({"type":"integer","minimum":1});
    requests["pageSize"] = json!({"type":"integer","minimum":1,"maximum":100});
    vec![
        tools::spec(
            "hub_query_usage",
            "直接查询Hub内已有的Token/请求统计或当前登录小分队账户的实际扣费。用户问今天花多少钱、用了多少Token时直接调用，默认squad+today，不只打开页面。支持日期、模型、密钥和分组筛选及按需分组；缺失数据不是0，Hub本机记录不是实际账单，管理员承担搭档费用。",
            usage,
            &[],
        ),
        tools::spec(
            "hub_query_requests",
            "查询Hub本机请求记录或当前小分队账户使用记录，按时间/模型等筛选并分页。只返回用量、耗时、状态等必要元数据，不读取请求正文、密钥、账号私密信息。可配合usage工具排查或比较，不把当前页相加当总统计。",
            requests,
            &[],
        ),
    ]
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Source {
    #[default]
    Squad,
    Hub,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Period {
    #[default]
    Today,
    Week,
    Month,
    Custom,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct Query {
    source: Source,
    period: Period,
    start_date: Option<String>,
    end_date: Option<String>,
    model: Option<String>,
    client: Option<String>,
    channel: Option<String>,
    api_key_id: Option<u64>,
    group_id: Option<u64>,
    status: Option<String>,
    breakdown: Option<String>,
    page: Option<u64>,
    page_size: Option<u64>,
}
impl Query {
    fn validate(&self) -> Result<()> {
        if self.period == Period::Custom || self.start_date.is_some() || self.end_date.is_some() {
            let (Some(start), Some(end)) = (&self.start_date, &self.end_date) else {
                anyhow::bail!("自定日期需要同时提供开始和结束日期");
            };
            ensure!(
                crate::ai_gateway::request_log::desktop::valid_date(start)
                    && crate::ai_gateway::request_log::desktop::valid_date(end)
                    && start <= end,
                "请输入有效的起止日期YYYY-MM-DD，结束日期不能早于开始日期"
            );
        }
        ensure!(
            self.api_key_id.is_none_or(|id| id > 0) && self.group_id.is_none_or(|id| id > 0),
            "密钥或分组ID无效"
        );
        ensure!(
            self.page.unwrap_or(1) > 0 && (1..=100).contains(&self.page_size.unwrap_or(20)),
            "分页参数无效"
        );
        match self.source {
            Source::Squad => ensure!(
                self.client.is_none() && self.channel.is_none() && self.status.is_none(),
                "小分队账单按模型、密钥或分组筛选；按客户端/本机渠道/请求状态请使用hub来源"
            ),
            Source::Hub => ensure!(
                self.api_key_id.is_none() && self.group_id.is_none(),
                "小分队密钥/分组筛选请使用squad来源"
            ),
        }
        if let Some(breakdown) = self.breakdown.as_deref() {
            ensure!(
                ["models", "days", "clients", "channels"].contains(&breakdown),
                "不支持此统计分组"
            );
            ensure!(
                self.source == Source::Hub || matches!(breakdown, "models" | "days"),
                "小分队统计支持按模型或日期分组；本机客户端/渠道分组请使用hub来源"
            );
        }
        Ok(())
    }
    fn range(&self) -> Value {
        let period = if self.start_date.is_some() {
            "custom"
        } else {
            match self.period {
                Period::Today => "today",
                Period::Week => "week",
                Period::Month => "month",
                Period::Custom => "custom",
            }
        };
        json!({"period":period,"startDate":self.start_date,"endDate":self.end_date,
            "calendar":if self.source == Source::Hub {"system_local"} else {"site_default"}})
    }
    fn squad_query(&self) -> Vec<(String, String)> {
        let mut query = Vec::new();
        if let (Some(start), Some(end)) = (&self.start_date, &self.end_date) {
            query.push(("start_date".into(), start.clone()));
            query.push(("end_date".into(), end.clone()));
        } else {
            query.push((
                "period".into(),
                self.range()["period"].as_str().unwrap().into(),
            ));
        }
        if let Some(value) = &self.model {
            query.push(("model".into(), value.clone()));
        }
        if let Some(value) = self.api_key_id {
            query.push(("api_key_id".into(), value.to_string()));
        }
        if let Some(value) = self.group_id {
            query.push(("group_id".into(), value.to_string()));
        }
        query
    }
    fn local_filter(&self) -> Result<LogFilter> {
        let mut filter = LogFilter {
            client: self.client.clone(),
            channel: self.channel.clone(),
            model: self.model.clone(),
            status: self.status.clone(),
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
            bucket_minutes: 1440,
            ..Default::default()
        };
        let db = rusqlite::Connection::open_in_memory()?;
        if self.start_date.is_none() {
            let modifier = match self.period {
                Period::Today => "start of day",
                Period::Week => "-7 days",
                Period::Month => "-1 month",
                Period::Custom => unreachable!(),
            };
            filter.from = db.query_row(
                "SELECT CAST(strftime('%s','now','localtime',?1,'utc') AS INTEGER)*1000",
                [modifier],
                |r| r.get(0),
            )?;
            filter.to = crate::ai_gateway::request_log::now_ms();
        }
        let page = self.page.unwrap_or(1);
        let size = self.page_size.unwrap_or(20);
        filter.limit = size as usize;
        filter.offset = page
            .checked_sub(1)
            .and_then(|n| n.checked_mul(size))
            .and_then(|n| usize::try_from(n).ok())
            .context("分页范围过大")?;
        Ok(filter)
    }
}

fn pick(value: &Value, fields: &[&str]) -> Value {
    Value::Object(
        fields
            .iter()
            .filter_map(|field| value.get(*field).map(|v| ((*field).into(), v.clone())))
            .collect(),
    )
}
fn squad_summary(value: &Value) -> Value {
    json!({"requests":value["total_requests"],"inputTokens":value["total_input_tokens"],
        "outputTokens":value["total_output_tokens"],"cacheReadTokens":value["total_cache_read_tokens"],
        "cacheWriteTokens":value["total_cache_creation_tokens"],"totalTokens":value["total_tokens"],
        "actualCost":value["total_actual_cost"],"currency":"CNY","billing":"actual_account_charge"})
}

pub(super) async fn usage(state: &SharedState, args: Value) -> Result<Value> {
    let args: Query = serde_json::from_value(args).context("统计筛选参数无效")?;
    args.validate()?;
    match args.source {
        Source::Squad => {
            let (squad, account) = super::tool_account(state).await?;
            let query = args.squad_query();
            let value = squad
                .request_checked("GET", "/usage/stats", query.clone(), None, Some(&account))
                .await?;
            let mut result = json!({"source":"squad","range":args.range(),"summary":squad_summary(&value),
                "scope":"当前登录用户的小分队账户，与站点用量接口相同；金额为人民币原值，不含管理员承担的搭档对话费"});
            if let Some(group) = args.breakdown.as_deref() {
                let path = if group == "models" {
                    "/usage/dashboard/models"
                } else {
                    "/usage/dashboard/trend"
                };
                let mut query = query;
                if group == "days" {
                    query.push(("granularity".into(), "day".into()));
                }
                let value = squad
                    .request_checked("GET", path, query, None, Some(&account))
                    .await?;
                let key = if group == "days" { "trend" } else { "models" };
                let rows = value[key].as_array().context("站点统计分组格式无效")?;
                result["breakdown"] = json!({"by":group,"rows":rows.iter().take(100).map(|row| pick(row, &["date","model","requests","input_tokens","output_tokens","cache_creation_tokens","cache_read_tokens","total_tokens","actual_cost"])).collect::<Vec<_>>(),"totalRows":rows.len(),"truncated":rows.len()>100});
            }
            Ok(result)
        }
        Source::Hub => {
            let filter = args.local_filter()?;
            let logs = state.ai_gateway_request_logs.clone();
            let value = tokio::task::spawn_blocking(move || logs.desktop_usage(&filter)).await??;
            let summary = &value["summary"];
            let reported_total = summary["input"]
                .as_i64()
                .zip(summary["output"].as_i64())
                .map(|(i, o)| i.saturating_add(o));
            let complete = summary["inputReported"] == summary["requests"]
                && summary["outputReported"] == summary["requests"];
            let total = if summary["requests"] == 0 {
                Some(0)
            } else if complete {
                reported_total
            } else {
                None
            };
            let mut result = json!({"source":"hub","range":args.range(),"from":value["from"],"to":value["to"],
                "summary":{"requests":summary["requests"],"successful":summary["successful"],"inputTokens":summary["input"],"outputTokens":summary["output"],"totalTokens":total,"reportedTotalTokens":reported_total,"tokensComplete":complete,"inputReported":summary["inputReported"],"outputReported":summary["outputReported"],"actualCost":null},
                "scope":"仅本机Hub请求记录；未上报Token为未知，输入已按原接口包含缓存，不重复相加；不据此推断小分队实际扣费"});
            if let Some(group) = args.breakdown.as_deref() {
                result["breakdown"] = json!({"by":group,"rows":if group == "days" {value["trend"].clone()} else {value["breakdown"][group].clone()},"limit":if group=="days"{1002}else{50}});
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, response::IntoResponse};
    use std::sync::Mutex as StdMutex;

    struct Fixture {
        _dir: tempfile::TempDir,
        state: SharedState,
        squad: Arc<Squad>,
        calls: Arc<StdMutex<Vec<String>>>,
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
    }
    async fn fixture() -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let capture = calls.clone();
        let app = Router::new().fallback(move |request: axum::extract::Request| {
            let capture = capture.clone();
            async move {
                assert_eq!(request.method(), "GET");
                assert_eq!(request.headers()["authorization"], "Bearer synthetic-assistant-token");
                capture.lock().unwrap().push(request.uri().to_string());
                let data = match request.uri().path() {
                    "/api/v1/usage/stats" => json!({"total_requests":40,"total_input_tokens":100,"total_output_tokens":30,"total_cache_read_tokens":70,"total_cache_creation_tokens":10,"total_tokens":210,"total_cost":99,"total_actual_cost":0.012345,"user":{"email":"private@example.invalid"},"api_key":"synthetic-private-key"}),
                    "/api/v1/usage/dashboard/models" => json!({"models":[{"model":"test-model","requests":40,"total_tokens":210,"actual_cost":0.012345,"account_cost":88,"api_key":"synthetic-private-key"}]}),
                    "/api/v1/usage" => json!({"total":40,"items":[{"id":7,"model":"test-model","input_tokens":100,"output_tokens":30,"actual_cost":0.012345,"created_at":"2026-11-11T12:00:00Z","api_key":{"key":"synthetic-private-key"},"user":{"email":"private@example.invalid"},"ip_address":"private-ip","request_json":"private-prompt"}]}),
                    _ => return (StatusCode::NOT_FOUND, axum::Json(json!({"message":"not found"}))).into_response(),
                };
                axum::Json(json!({"code":0,"data":data})).into_response()
            }
        });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = crate::config::AppConfig {
            state_path: dir.path().join("state.json"),
            ..Default::default()
        };
        config.save(&path).unwrap();
        let squad = Arc::new(
            Squad::synthetic(
                dir.path().join("squad"),
                reqwest::Client::builder().no_proxy().build().unwrap(),
                origin,
            )
            .unwrap(),
        );
        let account = squad.assistant_test_identity("usage-tools-test").unwrap();
        let state = crate::app_state::AppState::new(path, config, None, None);
        state.assistant.inner.lock().await.bridge = Some(super::super::proxy::Bridge::new(
            "usage-test-epoch".into(),
            "synthetic-bridge".into(),
            squad.clone(),
            account,
            "fixture-model".into(),
        ));
        Fixture {
            _dir: dir,
            state,
            squad,
            calls,
            server,
        }
    }
    fn public_only(value: &Value) {
        let text = value.to_string();
        for private in [
            "synthetic-private-key",
            "private@example.invalid",
            "private-ip",
            "private-prompt",
            "account_cost",
        ] {
            assert!(!text.contains(private));
        }
    }
    #[tokio::test]
    async fn squad_usage_queries_real_reader_keeps_actual_cost_and_missing_tokens() {
        let fixture = fixture().await;
        let value = fixture
            .call("hub_query_usage", json!({"breakdown":"models"}))
            .await
            .unwrap();
        assert_eq!(value["summary"]["actualCost"], 0.012345);
        assert_eq!(value["summary"]["currency"], "CNY");
        assert_eq!(value["summary"]["totalTokens"], 210);
        assert_eq!(value["breakdown"]["rows"][0]["model"], "test-model");
        assert_eq!(
            fixture.calls.lock().unwrap()[0],
            "/api/v1/usage/stats?period=today"
        );
        public_only(&value);
        let missing = squad_summary(&json!({"total_requests":2}));
        assert!(missing["actualCost"].is_null());
        assert!(missing["totalTokens"].is_null());
    }
    #[tokio::test]
    async fn squad_dates_filters_pagination_projection_and_account_switch() {
        let fixture = fixture().await;
        let value = fixture.call("hub_query_requests", json!({"startDate":"2026-11-01","endDate":"2026-11-02","model":"test-model","apiKeyId":12,"page":2,"pageSize":3})).await.unwrap();
        assert_eq!(value["total"], 40);
        assert_eq!(value["rows"].as_array().unwrap().len(), 1);
        assert_eq!(value["page"], 2);
        public_only(&value);
        let calls = fixture.calls.lock().unwrap().clone();
        for text in [
            "start_date=2026-11-01",
            "end_date=2026-11-02",
            "api_key_id=12",
            "page_size=3",
            "page=2",
        ] {
            assert!(calls[0].contains(text));
        }
        fixture
            .squad
            .assistant_test_identity("changed-account")
            .unwrap();
        assert!(fixture.call("hub_query_usage", json!({})).await.is_err());
        assert_eq!(fixture.calls.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn local_usage_aggregates_all_rows_and_does_not_double_count_cache_or_guess_bill() {
        use crate::ai_gateway::request_log::{LogUsage, RequestLogRecord};
        let fixture = fixture().await;
        for i in 0..27 {
            let record = RequestLogRecord {
                request_id: format!("request-{i}"),
                model_id: "local-model".into(),
                stream: true,
                channel: "local-channel".into(),
                provider_type: "test".into(),
                status: "completed".into(),
                usage: LogUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                    read_cache_tokens: Some(70),
                    ..Default::default()
                },
                cost_usd: Some(123.),
                latency_ms: Some(10),
                ttft_ms: None,
                created_at_ms: crate::ai_gateway::request_log::now_ms(),
                error_message: None,
                request_headers_json: Some("private-headers".into()),
                request_json: Some("private-prompt".into()),
                upstream_request_body_bytes: None,
                upstream_request_headers_json: None,
                upstream_request_json: None,
                upstream_response_sse: None,
                response_json: None,
            };
            let id = fixture
                .state
                .ai_gateway_request_logs
                .insert_record(&record)
                .unwrap();
            fixture.state.ai_gateway_request_logs.bind_identity(
                id,
                Some("workbuddy".into()),
                None,
                None,
            );
        }
        let value = fixture
            .call(
                "hub_query_usage",
                json!({"source":"hub","client":"workbuddy","breakdown":"models"}),
            )
            .await
            .unwrap();
        assert_eq!(value["summary"]["requests"], 27);
        assert_eq!(value["summary"]["totalTokens"], 3240);
        assert!(value["summary"]["actualCost"].is_null());
        assert_eq!(value["breakdown"]["rows"][0]["name"], "local-model");
        let page = fixture
            .call(
                "hub_query_requests",
                json!({"source":"hub","client":"workbuddy","page":2,"pageSize":3}),
            )
            .await
            .unwrap();
        assert_eq!(page["total"], 27);
        assert_eq!(page["rows"].as_array().unwrap().len(), 3);
        public_only(&page);
        assert!(
            fixture.calls.lock().unwrap().is_empty(),
            "local statistics must not call the account service"
        );
    }
    #[test]
    fn filters_reject_invalid_dates_and_cross_source_parameters() {
        for value in [
            json!({"startDate":"2026-02-30","endDate":"2026-03-01"}),
            json!({"period":"custom"}),
            json!({"pageSize":101}),
            json!({"source":"hub","apiKeyId":12}),
            json!({"source":"squad","client":"workbuddy"}),
        ] {
            assert!(
                serde_json::from_value::<Query>(value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
    }
}

pub(super) async fn requests(state: &SharedState, args: Value) -> Result<Value> {
    let args: Query = serde_json::from_value(args).context("请求记录筛选参数无效")?;
    args.validate()?;
    let page = args.page.unwrap_or(1);
    let size = args.page_size.unwrap_or(20);
    let (total, rows) = match args.source {
        Source::Squad => {
            let (squad, account) = super::tool_account(state).await?;
            let mut query = args.squad_query();
            // The list endpoint does not apply period presets; supply the same
            // local calendar dates as Hub's existing account filter controls.
            query.retain(|(key, _)| key != "period");
            if args.start_date.is_none() {
                let db = rusqlite::Connection::open_in_memory()?;
                let modifier = match args.period {
                    Period::Today => "+0 days",
                    Period::Week => "-7 days",
                    Period::Month => "-1 month",
                    Period::Custom => unreachable!(),
                };
                let (start, end): (String, String) = db.query_row(
                    "SELECT date('now','localtime',?1),date('now','localtime')",
                    [modifier],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                query.extend([("start_date".into(), start), ("end_date".into(), end)]);
            }
            query.extend([
                ("page".into(), page.to_string()),
                ("page_size".into(), size.to_string()),
            ]);
            let value = squad
                .request_checked("GET", "/usage", query, None, Some(&account))
                .await?;
            let rows = value["items"].as_array().context("站点使用记录格式无效")?;
            (
                value["total"].clone(),
                rows.iter()
                    .map(|row| {
                        pick(
                            row,
                            &[
                                "id",
                                "created_at",
                                "model",
                                "request_id",
                                "input_tokens",
                                "output_tokens",
                                "cache_creation_tokens",
                                "cache_read_tokens",
                                "actual_cost",
                                "duration_ms",
                                "stream",
                            ],
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        }
        Source::Hub => {
            let filter = args.local_filter()?;
            let logs = state.ai_gateway_request_logs.clone();
            let value = tokio::task::spawn_blocking(move || logs.desktop_page(&filter)).await??;
            let rows = value["rows"].as_array().context("Hub请求记录格式无效")?;
            (
                value["total"].clone(),
                rows.iter()
                    .map(|row| {
                        pick(
                            row,
                            &[
                                "id",
                                "createdAt",
                                "modelId",
                                "channel",
                                "clientId",
                                "status",
                                "inputTokens",
                                "outputTokens",
                                "latencyMs",
                                "stream",
                            ],
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        }
    };
    Ok(
        json!({"source":if args.source==Source::Hub {"hub"}else{"squad"},"range":args.range(),"page":page,"pageSize":size,"total":total,"rows":rows,"currency":if args.source==Source::Squad {Some("CNY")}else{None},"message":"仅返回元数据；分页列表不能当作完整总统计，汇总使用hub_query_usage"}),
    )
}
