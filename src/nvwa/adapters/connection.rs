//! Connection-field repair and private validation of the settings a client uses.
use super::*;
use serde_json::json;

fn toml_value(value: toml::Value) -> Value {
    match value {
        toml::Value::String(value) => json!(["string", value]),
        toml::Value::Integer(value) => json!(["integer", value]),
        toml::Value::Float(value) => json!(["float", value.to_string()]),
        toml::Value::Boolean(value) => json!(["boolean", value]),
        toml::Value::Datetime(value) => json!(["datetime", value.to_string()]),
        toml::Value::Array(values) => json!([
            "array",
            values.into_iter().map(toml_value).collect::<Vec<_>>()
        ]),
        toml::Value::Table(values) => json!([
            "table",
            values
                .into_iter()
                .map(|(key, value)| (key, toml_value(value)))
                .collect::<BTreeMap<_, _>>()
        ]),
    }
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(values) => json!(
            values
                .into_iter()
                .map(|(key, value)| (key, sorted(value)))
                .collect::<BTreeMap<_, _>>()
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sorted).collect()),
        value => value,
    }
}

/// Additional ownership evidence for newly saved entries. Legacy raw hashes and
/// all preview/write CAS checks remain available; formatting never rewrites files.
pub(super) fn semantic_fingerprint(
    snapshot: Option<&Snapshot>,
    client: ClientKind,
) -> Result<String> {
    let Some(snapshot) = snapshot else {
        return Ok("missing".into());
    };
    let snapshot = if client == ClientKind::Tiangong {
        tiangong::static_snapshot(snapshot)
    } else {
        snapshot.clone()
    };
    let value = match snapshot {
        Snapshot::Toml(text) => {
            toml_value(toml::from_str(&text).map_err(|_| anyhow::anyhow!("无法核对 MCP 设置内容"))?)
        }
        Snapshot::Json(value) => sorted(value),
    };
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&value)?)))
}

fn has_authorization(headers: &Value, bearer: &str) -> bool {
    let Some(headers) = headers.as_object() else {
        return false;
    };
    let auth: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .collect();
    auth.len() == 1 && auth[0].1.as_str() == Some(format!("Bearer {bearer}").as_str())
}

fn authorization(headers: &mut Value, bearer: &str) {
    if !headers.is_object() {
        *headers = json!({});
    }
    let headers = headers.as_object_mut().expect("header object");
    headers.retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
    headers.insert("Authorization".into(), json!(format!("Bearer {bearer}")));
}

fn object(snapshot: &Snapshot, name: &str) -> Result<Value> {
    match snapshot {
        Snapshot::Toml(text) => {
            let value: toml::Value =
                toml::from_str(text).map_err(|_| anyhow::anyhow!("MCP 设置格式无效"))?;
            serde_json::to_value(
                value
                    .get("mcp_servers")
                    .and_then(|servers| servers.get(name))
                    .ok_or_else(|| anyhow::anyhow!("MCP 设置缺少受管连接"))?,
            )
            .map_err(Into::into)
        }
        Snapshot::Json(value) => Ok(value.clone()),
    }
}

pub(super) fn matches(
    snapshot: &Snapshot,
    target: &AdapterTarget,
    endpoint: &str,
    bearer: &str,
) -> Result<bool> {
    let value = object(snapshot, &target.server_name)?;
    let matches = match target.client {
        ClientKind::Codex => {
            value["url"].as_str() == Some(endpoint)
                && value.get("enabled").is_none_or(|value| value == true)
                && value.get("command").is_none()
                && value.get("type").is_none()
                && value.get("transport").is_none()
                && value.get("bearer_token_env_var").is_none()
                && value.get("env_http_headers").is_none_or(|value| {
                    value.as_object().is_some_and(|headers| {
                        !headers
                            .keys()
                            .any(|name| name.eq_ignore_ascii_case("authorization"))
                    })
                })
                && has_authorization(&value["http_headers"], bearer)
        }
        ClientKind::Workbuddy => {
            value["url"].as_str() == Some(endpoint)
                && value["type"].as_str() == Some("streamableHttp")
                && value.get("disabled").is_none_or(|value| value == false)
                && value.get("enabled").is_none_or(|value| value == true)
                && value.get("command").is_none()
                && value.get("transport").is_none()
                && has_authorization(&value["headers"], bearer)
        }
        ClientKind::Tiangong => {
            value["server_url"].as_str() == Some(endpoint)
                && value["connect_type"].as_i64() == Some(1)
                && value["status"].as_str() == Some("active")
                && value["token_encrypted"].as_str().is_none_or(str::is_empty)
                && value["config_param"]["transport"].as_str() == Some("streamable")
                && has_authorization(&value["header_config"], bearer)
        }
    };
    Ok(matches)
}

/// Called only by an already confirmed Apply/Repair transaction. Preserve the
/// current before-image and all settings outside the bridge/auth/enable fields.
pub(super) fn merge(
    current: &Snapshot,
    target: &AdapterTarget,
    desired: &Snapshot,
) -> Result<Snapshot> {
    if target.client == ClientKind::Codex {
        return codex::merge(current, &target.server_name, desired);
    }
    let Snapshot::Json(mut value) = current.clone() else {
        bail!("MCP 修复设置类型不匹配");
    };
    let Snapshot::Json(desired) = desired else {
        bail!("MCP 修复设置类型不匹配");
    };
    ensure!(
        value.is_object() && desired.is_object(),
        "MCP 修复设置必须是对象"
    );
    let fields: &[&str] = if target.client == ClientKind::Tiangong {
        &[
            "server_id",
            "server_url",
            "connect_type",
            "token_encrypted",
            "status",
            "tools_json",
            "is_connected",
            "conn_last_error",
            "retry_count",
        ]
    } else {
        &["type", "url"]
    };
    for field in fields {
        value[*field] = desired[*field].clone();
    }
    let header_key = if target.client == ClientKind::Tiangong {
        "header_config"
    } else {
        "headers"
    };
    let bearer = desired[header_key]["Authorization"]
        .as_str()
        .and_then(|header| header.strip_prefix("Bearer "))
        .ok_or_else(|| anyhow::anyhow!("MCP 修复缺少本机凭据"))?;
    authorization(&mut value[header_key], bearer);
    if target.client == ClientKind::Tiangong {
        if !value["config_param"].is_object() {
            value["config_param"] = json!({});
        }
        value["config_param"]["transport"] = desired["config_param"]["transport"].clone();
        for key in ["requestTimeout", "maximumTotalTimeout"] {
            if value["config_param"].get(key).is_none() {
                value["config_param"][key] = desired["config_param"][key].clone();
            }
        }
    } else {
        let object = value.as_object_mut().expect("connection object");
        for key in ["command", "args", "env", "cwd", "envFile", "transport"] {
            object.remove(key);
        }
        object.insert("disabled".into(), json!(false));
        if object.contains_key("enabled") {
            object.insert("enabled".into(), json!(true));
        }
    }
    Ok(Snapshot::Json(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(client: ClientKind) -> AdapterTarget {
        AdapterTarget {
            profile_id: "fixture".into(),
            client,
            server_name: "MCP196".into(),
            override_path: None,
        }
    }

    #[test]
    fn codex_inline_settings_repair_conflicting_auth_and_keep_tool_preferences() {
        let current = Snapshot::Toml(
            concat!(
                "[mcp_servers]\n",
                "MCP196 = { url='http://127.0.0.1:9999/old', enabled=false, command='fixture', ",
                "tool_timeout_sec=127, enabled_tools=['fixture.read'], ",
                "http_headers={ authorization='old', X_Fixture='keep' }, ",
                "env_http_headers={ AUTHORIZATION='OLD_AUTH', X_Env='KEEP_ENV' } }\n",
            )
            .into(),
        );
        let target = target(ClientKind::Codex);
        let endpoint = "http://127.0.0.1:3849/mcp/fixture/codex";
        let desired = codex::desired(&target.server_name, endpoint, "fixture-key");
        assert!(!matches(&current, &target, endpoint, "fixture-key").unwrap());
        let repaired = merge(&current, &target, &desired).unwrap();
        assert!(matches(&repaired, &target, endpoint, "fixture-key").unwrap());
        let entry = object(&repaired, &target.server_name).unwrap();
        assert_eq!(entry["tool_timeout_sec"], 127);
        assert_eq!(entry["enabled_tools"], json!(["fixture.read"]));
        assert_eq!(entry["http_headers"]["X_Fixture"], "keep");
        assert_eq!(entry["env_http_headers"]["X_Env"], "KEEP_ENV");
        assert!(entry.get("command").is_none());
        assert_eq!(entry["http_headers"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn tiangong_repair_preserves_native_timeouts_and_ignores_discovery_churn() {
        let target = target(ClientKind::Tiangong);
        let endpoint = "http://127.0.0.1:3849/mcp/fixture/tiangong";
        let desired = tiangong::desired(&target.server_name, endpoint, "fixture-key");
        let Snapshot::Json(mut value) = desired.clone() else {
            unreachable!()
        };
        value["server_url"] = json!("http://127.0.0.1:9999/old");
        value["timeout_ms"] = json!(127000);
        value["token_encrypted"] = json!("old-fixture");
        value["status"] = json!("inactive");
        value["config_param"]["requestTimeout"] = json!(127);
        value["config_param"]["custom"] = json!(true);
        value["header_config"]["authorization"] = json!("old-fixture");
        value["header_config"]["X-Fixture"] = json!("keep");
        let repaired = merge(&Snapshot::Json(value), &target, &desired).unwrap();
        assert!(matches(&repaired, &target, endpoint, "fixture-key").unwrap());
        let entry = object(&repaired, &target.server_name).unwrap();
        assert_eq!(entry["timeout_ms"], 127000);
        assert_eq!(entry["config_param"]["requestTimeout"], 127);
        assert_eq!(entry["config_param"]["custom"], true);
        assert_eq!(entry["header_config"]["X-Fixture"], "keep");
        let original_hash = semantic_fingerprint(Some(&repaired), ClientKind::Tiangong).unwrap();
        let Snapshot::Json(mut value) = repaired else {
            unreachable!()
        };
        value["is_connected"] = json!(1);
        value["tools_json"] = json!([{"name":"fixture.read"}]);
        value["conn_last_error"] = json!("fixture transient");
        value["retry_count"] = json!(3);
        assert_eq!(
            original_hash,
            semantic_fingerprint(Some(&Snapshot::Json(value)), ClientKind::Tiangong).unwrap()
        );
    }

    #[test]
    fn config_validation_rejects_disabled_or_malformed_entries_and_duplicate_auth() {
        let target = target(ClientKind::Workbuddy);
        let endpoint = "http://127.0.0.1:3849/mcp/fixture/workbuddy";
        let Snapshot::Json(original) = workbuddy::desired(endpoint, "fixture-key") else {
            unreachable!()
        };
        assert!(
            matches(
                &Snapshot::Json(original.clone()),
                &target,
                endpoint,
                "fixture-key"
            )
            .unwrap()
        );
        for (field, value) in [
            ("disabled", json!(true)),
            ("disabled", json!("false")),
            ("enabled", json!(false)),
            ("enabled", json!("true")),
            ("type", json!("sse")),
            ("command", json!("fixture")),
            (
                "headers",
                json!({"Authorization":"Bearer fixture-key","authorization":"Bearer fixture-key"}),
            ),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            assert!(!matches(&Snapshot::Json(changed), &target, endpoint, "fixture-key").unwrap());
        }
    }

    #[test]
    fn toml_formatting_and_key_order_are_equivalent_but_values_and_types_are_not() {
        let one = Snapshot::Toml("[mcp_servers.demo]\nenabled=true\nurl='fixture'\n".into());
        let two = Snapshot::Toml(
            "# rewritten\n[mcp_servers.demo]\nurl = \"fixture\"\nenabled = true # comment\n".into(),
        );
        let hash = semantic_fingerprint(Some(&one), ClientKind::Codex).unwrap();
        assert_eq!(
            hash,
            semantic_fingerprint(Some(&two), ClientKind::Codex).unwrap()
        );
        for changed in [
            "[mcp_servers.demo]\nenabled=false\nurl='fixture'",
            "[mcp_servers.demo]\nenabled='true'\nurl='fixture'",
        ] {
            assert_ne!(
                hash,
                semantic_fingerprint(Some(&Snapshot::Toml(changed.into())), ClientKind::Codex)
                    .unwrap()
            );
        }
    }
}
