//! Whole-database filtering and aggregation. No recent-row sampling.
use super::{RequestLogStore, RequestLogWrite, try_lock_maintenance};
use rusqlite::{params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LogFilter {
    pub from: i64,
    pub to: i64,
    pub client: Option<String>,
    pub entry: Option<String>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub channel: Option<String>,
    pub status: Option<String>,
    pub query: Option<String>,
    pub offset: usize,
    pub limit: usize,
    pub bucket_minutes: i64,
    pub calendar_days: i64,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}
impl LogFilter {
    pub fn validate(&self) -> Result<(), String> {
        for value in [&self.start_date, &self.end_date].into_iter().flatten() {
            if !value.is_empty() && !valid_date(value) {
                return Err("日期请填写有效的 YYYY-MM-DD".into());
            }
        }
        if let (Some(start), Some(end)) = (&self.start_date, &self.end_date) {
            if !start.is_empty() && !end.is_empty() && start > end {
                return Err("开始日期不能晚于结束日期".into());
            }
        }
        if self.from < 0 || self.to < 0 || (self.to > 0 && self.from > self.to) {
            return Err("时间范围无效".into());
        }
        Ok(())
    }
    fn sql(&self) -> (String, Vec<SqlValue>) {
        let mut clauses = vec![
            if self.calendar_days > 0 {
                format!(
                    "l.created_at_ms >= CAST(strftime('%s','now','localtime','start of day','-{} days','utc') AS INTEGER)*1000 AND l.created_at_ms >= ?",
                    self.calendar_days.clamp(1, 3660) - 1
                )
            } else {
                "l.created_at_ms >= ?".to_string()
            },
            "l.created_at_ms < ?".to_string(),
        ];
        let mut args = vec![
            SqlValue::Integer(self.from.max(0)),
            SqlValue::Integer(if self.to > 0 { self.to } else { i64::MAX }),
        ];
        for (date, clause) in [
            (
                &self.start_date,
                "l.created_at_ms >= CAST(strftime('%s',?,'utc') AS INTEGER)*1000",
            ),
            (
                &self.end_date,
                "l.created_at_ms < CAST(strftime('%s',?,'+1 day','utc') AS INTEGER)*1000",
            ),
        ] {
            if let Some(date) = date.as_ref().filter(|d| !d.is_empty()) {
                clauses.push(clause.into());
                args.push(SqlValue::Text(date.clone()));
            }
        }
        for (field, value) in [
            ("client_id", &self.client),
            ("client_entry", &self.entry),
            ("route_id", &self.route),
            ("model_id", &self.model),
            ("channel", &self.channel),
        ] {
            if let Some(value) = value.as_ref().filter(|s| !s.is_empty()) {
                if value == "__unknown__" && matches!(field, "client_id" | "route_id") {
                    clauses.push(format!("l.{field} IS NULL"));
                    continue;
                }
                clauses.push(format!("l.{field} = ?"));
                args.push(SqlValue::Text(value.clone()));
            }
        }
        if let Some(status) = self.status.as_deref().filter(|s| !s.is_empty()) {
            match status {
                "completed" | "success" => {
                    clauses.push("l.status IN ('completed','success')".into())
                }
                "failed" | "error" => clauses.push("l.status IN ('failed','error')".into()),
                "cancelled" | "canceled" => {
                    clauses.push("l.status IN ('cancelled','canceled')".into())
                }
                _ => {
                    clauses.push("l.status = ?".into());
                    args.push(SqlValue::Text(status.into()));
                }
            }
        }
        if let Some(query) = self.query.as_ref().filter(|s| !s.is_empty()) {
            clauses.push("(instr(lower(l.model_id),lower(?)) > 0 OR instr(lower(l.channel),lower(?)) > 0 OR instr(lower(l.request_id),lower(?)) > 0)".into());
            for _ in 0..3 {
                args.push(SqlValue::Text(query.chars().take(200).collect()));
            }
        }
        (clauses.join(" AND "), args)
    }
}
pub fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return false;
    }
    let year: u32 = value[..4].parse().unwrap_or(0);
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..].parse().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= max
}
impl RequestLogStore {
    pub fn bind_identity(
        &self,
        id: i64,
        client: Option<String>,
        entry: Option<String>,
        route: Option<String>,
    ) {
        self.enqueue_write(RequestLogWrite::Identity {
            id,
            client,
            entry,
            route,
        });
    }
    pub fn desktop_page(&self, filter: &LogFilter) -> rusqlite::Result<Value> {
        filter
            .validate()
            .map_err(|e| rusqlite::Error::InvalidParameterName(e))?;
        let _maintenance = try_lock_maintenance(&self.inner.maintenance)?;
        self.flush_pending_writes()?;
        self.with_conn(|conn|{
            let(clause,mut args)=filter.sql();
            let total:i64=conn.query_row(&format!("SELECT COUNT(*) FROM ai_gateway_request_logs l WHERE {clause}"),params_from_iter(args.iter()),|r|r.get(0))?;
            args.push(SqlValue::Integer(if filter.limit==0{50}else{filter.limit.min(200)}as i64));args.push(SqlValue::Integer(filter.offset.min(i64::MAX as usize)as i64));
            let mut stmt=conn.prepare(&format!("SELECT l.id,l.request_id,l.model_id,l.channel,l.status,l.input_tokens,l.output_tokens,l.latency_ms,l.created_at_ms,l.error_message,l.client_id,l.client_entry,l.route_id,l.stream,l.upstream_stream,datetime(l.created_at_ms / 1000, 'unixepoch', 'localtime') FROM ai_gateway_request_logs l WHERE {clause} ORDER BY l.created_at_ms DESC,l.id DESC LIMIT ? OFFSET ?"))?;
            let rows=stmt.query_map(params_from_iter(args.iter()),|r|Ok(json!({
                "id":r.get::<_,i64>(0)?,"requestId":r.get::<_,String>(1)?,"modelId":r.get::<_,String>(2)?,"channel":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,
                "inputTokens":r.get::<_,Option<i64>>(5)?,"outputTokens":r.get::<_,Option<i64>>(6)?,"latencyMs":r.get::<_,Option<i64>>(7)?,"createdAtMs":r.get::<_,i64>(8)?,
                "errorMessage":r.get::<_,Option<String>>(9)?,"clientId":r.get::<_,Option<String>>(10)?,"entryId":r.get::<_,Option<String>>(11)?,"routeId":r.get::<_,Option<String>>(12)?,
                "createdAt":r.get::<_,String>(15)?,"stream":r.get::<_,i64>(13)?!=0,"upstreamStream":r.get::<_,Option<i64>>(14)?.map(|v|v!=0)
            })))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(json!({"total":total,"offset":filter.offset,"rows":rows}))
        })
    }
    pub fn desktop_usage(&self, filter: &LogFilter) -> rusqlite::Result<Value> {
        filter
            .validate()
            .map_err(|e| rusqlite::Error::InvalidParameterName(e))?;
        let _maintenance = try_lock_maintenance(&self.inner.maintenance)?;
        self.flush_pending_writes()?;
        self.with_conn(|conn|{
            let(clause,args)=filter.sql();
            // input_tokens has already been normalised by each upstream adapter.
            // Adding read/write cache columns here would double count Anthropic.
            let summary:Value=conn.query_row(&format!("SELECT COUNT(*),SUM(CASE WHEN status IN ('completed','success') THEN 1 ELSE 0 END),SUM(input_tokens),SUM(output_tokens),COUNT(input_tokens),COUNT(output_tokens) FROM ai_gateway_request_logs l WHERE {clause}"),params_from_iter(args.iter()),|r|Ok(json!({"requests":r.get::<_,i64>(0)?,"successful":r.get::<_,Option<i64>>(1)?.unwrap_or(0),"input":r.get::<_,Option<i64>>(2)?,"output":r.get::<_,Option<i64>>(3)?,"inputReported":r.get::<_,i64>(4)?,"outputReported":r.get::<_,i64>(5)?})))?;
            let mut to=if filter.to>0{filter.to}else{super::now_ms()};
            let mut from=if filter.calendar_days>0{conn.query_row(&format!("SELECT CAST(strftime('%s','now','localtime','start of day','-{} days','utc') AS INTEGER)*1000",filter.calendar_days.clamp(1,3660)-1),[],|r|r.get::<_,i64>(0))?.max(filter.from)}else if filter.from>0{filter.from}else{conn.query_row(&format!("SELECT MIN(l.created_at_ms) FROM ai_gateway_request_logs l WHERE {clause}"),params_from_iter(args.iter()),|r|r.get::<_,Option<i64>>(0))?.unwrap_or(to)};
            if let Some(date)=filter.start_date.as_deref().filter(|s|!s.is_empty()) {
                let start=conn.query_row("SELECT CAST(strftime('%s',?1,'utc') AS INTEGER)*1000",params![date],|r|r.get::<_,i64>(0))?;
                // The requested calendar period includes idle time before the first record.
                from=if filter.from>0||filter.calendar_days>0{from.max(start)}else{start.max(0)};
            }
            if let Some(date)=filter.end_date.as_deref().filter(|s|!s.is_empty()) {
                let end=conn.query_row("SELECT CAST(strftime('%s',?1,'+1 day','utc') AS INTEGER)*1000",params![date],|r|r.get::<_,i64>(0))?;
                to=to.min(end);
            }
            // Increase the bucket instead of truncating history. All matching
            // rows contribute, including time ranges containing long idle gaps.
            let span=to.saturating_sub(from).max(1);
            let requested=filter.bucket_minutes.clamp(1,1440)*60_000;
            let bucket=requested.max((span.saturating_add(999)/1000).saturating_add(59_999)/60_000*60_000);
            let mut trend_stmt=conn.prepare(&format!("SELECT ((l.created_at_ms-{from})/{bucket})*{bucket}+{from},COUNT(*),SUM(input_tokens),SUM(output_tokens),SUM(CASE WHEN status IN ('failed','error') THEN 1 ELSE 0 END) FROM ai_gateway_request_logs l WHERE {clause} GROUP BY 1 ORDER BY 1"))?;
            let found=trend_stmt.query_map(params_from_iter(args.iter()),|r|Ok((r.get::<_,i64>(0)?,json!({"at":r.get::<_,i64>(0)?,"requests":r.get::<_,i64>(1)?,"input":r.get::<_,Option<i64>>(2)?,"output":r.get::<_,Option<i64>>(3)?,"failed":r.get::<_,i64>(4)?}))))?.collect::<rusqlite::Result<std::collections::BTreeMap<_,_>>>()?;
            let mut label=conn.prepare("SELECT strftime('%m-%d %H:%M',?1/1000,'unixepoch','localtime')")?;
            let mut trend=Vec::new();let mut at=from;
            while at<to&&trend.len()<1002{let mut row=found.get(&at).cloned().unwrap_or_else(||json!({"at":at,"requests":0,"input":0,"output":0,"failed":0}));row["label"]=label.query_row(params![at],|r|r.get::<_,String>(0))?.into();trend.push(row);at=at.saturating_add(bucket);}
            let mut dimensions=serde_json::Map::new();
            for(field,key)in [("client_id","clients"),("model_id","models"),("channel","channels")]{
                let (extra,group)=if key=="channels"{("route_id", "channel,route_id")}else{("NULL",field)};
                let mut stmt=conn.prepare(&format!("SELECT {field},COUNT(*),SUM(input_tokens),SUM(output_tokens),{extra} FROM ai_gateway_request_logs l WHERE {clause} GROUP BY {group} ORDER BY COUNT(*) DESC LIMIT 50"))?;
                let rows=stmt.query_map(params_from_iter(args.iter()),|r|Ok(json!({"name":r.get::<_,Option<String>>(0)?,"requests":r.get::<_,i64>(1)?,"input":r.get::<_,Option<i64>>(2)?,"output":r.get::<_,Option<i64>>(3)?,"route":r.get::<_,Option<String>>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
                dimensions.insert(key.into(),rows.into());
            }
            // Picker facets come from all matching history, before applying
            // model/channel selection. They must not collapse to the selected
            // value or be sampled from the first page/top-50 breakdown.
            let (facet_clause,facet_args)=LogFilter{model:None,channel:None,route:None,..filter.clone()}.sql();
            let mut facets=serde_json::Map::new();
            for (field,key) in [("model_id","models"),("channel","channels")] {
                let mut stmt=conn.prepare(&format!("SELECT DISTINCT l.{field} FROM ai_gateway_request_logs l WHERE {facet_clause} AND l.{field} IS NOT NULL AND l.{field} != '' ORDER BY l.{field} COLLATE NOCASE"))?;
                let values=stmt.query_map(params_from_iter(facet_args.iter()),|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                facets.insert(key.into(),json!(values));
            }
            Ok(json!({"summary":summary,"trend":trend,"breakdown":dimensions,"filters":facets,"bucketMs":bucket,"from":from,"to":to}))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_facets_include_all_matching_history_and_remain_changeable_after_selection() {
        let dir = tempfile::tempdir().unwrap();
        let store = RequestLogStore::new(dir.path().join("facets.sqlite"));
        store.with_conn(|conn|{
            for i in 0..73 {
                conn.execute("INSERT INTO ai_gateway_request_logs(request_id,model_id,channel,client_id,stream,provider_type,status,created_at_ms)VALUES('fixture',?1,?2,'workbuddy',0,'test','completed',100)",params![format!("model-{i:02}"),format!("channel-{i:02}")])?;
            }
            conn.execute("INSERT INTO ai_gateway_request_logs(request_id,model_id,channel,client_id,stream,provider_type,status,created_at_ms)VALUES('other','excluded','excluded','codex',0,'test','completed',100)",[])?;
            Ok(())
        }).unwrap();
        let data = store
            .desktop_usage(&LogFilter {
                from: 50,
                to: 200,
                client: Some("workbuddy".into()),
                model: Some("model-05".into()),
                channel: Some("channel-05".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(data["summary"]["requests"], 1);
        for key in ["models", "channels"] {
            assert_eq!(data["filters"][key].as_array().unwrap().len(), 73);
        }
        assert!(
            data["filters"]["models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "model-72")
        );
        assert!(
            !data["filters"]["models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "excluded")
        );
    }
    #[test]
    fn calendar_validation_and_status_aliases_do_not_hide_requests() {
        for d in [
            "2026-02-29",
            "2026-13-01",
            "2026-04-31",
            "0000-01-01",
            "2026-11-1",
            "2026-01-01x",
        ] {
            assert!(!valid_date(d), "{d}");
        }
        for d in ["2024-02-29", "2000-02-29", "2026-11-08"] {
            assert!(valid_date(d));
        }
        assert!(
            LogFilter {
                start_date: Some("2026-11-09".into()),
                end_date: Some("2026-11-08".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let dir = tempfile::tempdir().unwrap();
        let store = RequestLogStore::new(dir.path().join("logs.sqlite"));
        store.with_conn(|c|{for(i,status)in ["success","completed","error","failed","running"].into_iter().enumerate(){c.execute("INSERT INTO ai_gateway_request_logs(id,request_id,model_id,stream,channel,provider_type,status,created_at_ms)VALUES(?1,'test','same',0,'same','test',?2,1)",params![i as i64+1,status])?;}Ok(())}).unwrap();
        for status in ["completed", "success", "failed", "error"] {
            assert_eq!(
                store
                    .desktop_page(&LogFilter {
                        status: Some(status.into()),
                        ..Default::default()
                    })
                    .unwrap()["total"],
                2
            );
        }
        assert_eq!(
            store
                .desktop_page(&LogFilter {
                    client: Some("__unknown__".into()),
                    ..Default::default()
                })
                .unwrap()["total"],
            5
        );
    }
    #[test]
    fn long_range_includes_idle_gaps_and_distinguishes_equal_channel_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = RequestLogStore::new(dir.path().join("logs.sqlite"));
        let end = 20_000 * 3_600_000i64;
        store.with_conn(|c|{for(id,at,route)in [(1,1,"route-a"),(2,end-1,"route-b")]{c.execute("INSERT INTO ai_gateway_request_logs(id,request_id,model_id,stream,channel,provider_type,status,input_tokens,output_tokens,route_id,created_at_ms)VALUES(?1,'test','equal',0,'equal','test','completed',100,10,?2,?3)",params![id,route,at])?;}Ok(())}).unwrap();
        let filter = LogFilter {
            from: 1,
            to: end,
            bucket_minutes: 60,
            ..Default::default()
        };
        let data = store.desktop_usage(&filter).unwrap();
        let trend = data["trend"].as_array().unwrap();
        assert!(trend.len() <= 1002);
        assert_eq!(
            trend
                .iter()
                .filter_map(|v| v["requests"].as_i64())
                .sum::<i64>(),
            2
        );
        assert!(trend.iter().any(|v| v["requests"] == 0));
        assert_eq!(data["breakdown"]["channels"].as_array().unwrap().len(), 2);
        assert_eq!(
            store
                .desktop_usage(&LogFilter {
                    route: Some("route-b".into()),
                    ..filter
                })
                .unwrap()["summary"]["input"],
            100
        );
        assert!(
            store
                .desktop_page(&LogFilter {
                    start_date: Some("2026-02-30".into()),
                    ..Default::default()
                })
                .is_err()
        );
    }
    #[test]
    fn whole_database_totals_paging_and_unknown_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = RequestLogStore::new(dir.path().join("fixture.sqlite"));
        store.with_conn(|conn|{for i in 1..=650{conn.execute("INSERT INTO ai_gateway_request_logs(id,request_id,model_id,stream,channel,provider_type,status,input_tokens,output_tokens,read_cache_tokens,created_at_ms) VALUES (?1,'synthetic','equal-name',0,'channel','test','completed',100,10,60,?1)",params![i])?;}Ok(())}).unwrap();
        let filter = LogFilter {
            bucket_minutes: 1,
            ..Default::default()
        };
        let usage = store.desktop_usage(&filter).unwrap();
        assert_eq!(usage["summary"]["requests"], 650);
        assert_eq!(usage["summary"]["input"], 65000);
        assert_eq!(usage["summary"]["output"], 6500);
        let page = store
            .desktop_page(&LogFilter {
                offset: 625,
                limit: 50,
                ..filter.clone()
            })
            .unwrap();
        assert_eq!(page["rows"].as_array().unwrap().len(), 25);
        assert!(page["rows"][0]["clientId"].is_null());
        store.bind_identity(
            1,
            Some("vscode".into()),
            Some("entry".into()),
            Some("route".into()),
        );
        let page = store
            .desktop_page(&LogFilter {
                client: Some("vscode".into()),
                ..filter
            })
            .unwrap();
        assert_eq!(page["total"], 1);
        assert_eq!(page["rows"][0]["routeId"], "route");
    }
}
