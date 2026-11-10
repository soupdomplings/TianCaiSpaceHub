//! MCP interoperability. Commands are configuration data, never executed here.
use super::{formats, targets::McpFormat};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub config: Value,
}

impl McpServer {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 128
                && !self.name.chars().any(char::is_control),
            "请填写有效的 MCP 名称"
        );
        let object = self
            .config
            .as_object()
            .context("MCP 配置必须是 JSON 对象")?;
        let command = object
            .get("command")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let url = object
            .get("url")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        ensure!(
            command.is_some() ^ url.is_some(),
            "远程 MCP 请填写地址，本地 MCP 请填写命令，二者只能选一种"
        );
        if let Some(url) = url {
            let url = url::Url::parse(url)
                .context("MCP 地址无效，请填写完整的 http:// 或 https:// 地址")?;
            ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.fragment().is_none(),
                "MCP 地址应为 HTTP/HTTPS；认证请放在认证或 Headers 中"
            );
        }
        for key in ["env", "headers", "http_headers", "env_http_headers"] {
            if let Some(value) = object.get(key) {
                ensure!(
                    value
                        .as_object()
                        .is_some_and(|o| o.iter().all(|(k, v)| !k.is_empty()
                            && !k.chars().any(char::is_control)
                            && v.is_string())),
                    "{key} 应为名称和值组成的对象"
                );
            }
        }
        for key in ["args", "env_vars"] {
            if let Some(value) = object.get(key) {
                ensure!(
                    value
                        .as_array()
                        .is_some_and(|a| a.iter().all(Value::is_string)),
                    "{key} 应为字符串数组"
                );
            }
        }
        for key in ["type", "transport"] {
            if let Some(value) = object.get(key) {
                ensure!(
                    value.as_str().is_some_and(|v| matches!(
                        v,
                        "http" | "streamable-http" | "streamableHttp" | "sse" | "stdio"
                    )),
                    "不识别的 MCP 传输方式，请检查 {key}"
                );
            }
        }
        ensure!(
            serde_json::to_vec(&self.config)?.len() <= 256 * 1024,
            "单个 MCP 配置过大"
        );
        Ok(())
    }
    pub fn transport(&self) -> &str {
        if self.config.get("command").is_some() {
            "stdio"
        } else if self
            .config
            .get("type")
            .or(self.config.get("transport"))
            .and_then(Value::as_str)
            == Some("sse")
        {
            "sse"
        } else {
            "http"
        }
    }
    pub fn native(&self, format: McpFormat) -> Result<Value> {
        self.validate()?;
        let mut value = self.config.clone();
        let t = self.transport();
        if format == McpFormat::Codex {
            ensure!(
                t != "sse",
                "Codex 需要 Streamable HTTP 或本地命令，请改用服务提供的 HTTP 地址"
            );
            let obj = value.as_object_mut().unwrap();
            obj.remove("type");
            obj.remove("transport");
            obj.remove("disabled");
            if let Some(headers) = obj.remove("headers") {
                obj.insert("http_headers".into(), headers);
            }
            obj.insert("enabled".into(), true.into());
        } else if format == McpFormat::Harness {
            ensure!(
                self.name.len() <= 32
                    && self
                        .name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
                "DeepSeek Harness 的名称只能是 1–32 个字母、数字、下划线或连字符"
            );
            ensure!(
                t != "sse",
                "DeepSeek Harness 需要 Streamable HTTP 或本地命令"
            );
            let obj = value.as_object_mut().unwrap();
            obj.remove("type");
            obj.insert("serverName".into(), self.name.clone().into());
            obj.insert(
                "transport".into(),
                if t == "http" { "streamable-http" } else { t }.into(),
            );
        } else if format == McpFormat::OpenCode {
            let obj = value.as_object_mut().unwrap();
            obj.remove("type");
            obj.remove("transport");
            obj.remove("disabled");
            obj.remove("enabled");
            if let Some(headers) = obj.remove("http_headers") {
                obj.entry("headers").or_insert(headers);
            }
            if t == "stdio" {
                ensure!(
                    !obj.contains_key("cwd"),
                    "此 ACP 版本不支持逐 MCP 工作目录，请使用绝对路径或配置文件提供的远程服务"
                );
                let mut command = vec![obj.remove("command").unwrap()];
                if let Some(args) = obj.remove("args").and_then(|a| a.as_array().cloned()) {
                    command.extend(args);
                }
                obj.insert("command".into(), command.into());
                if let Some(env) = obj.remove("env") {
                    obj.insert("environment".into(), env);
                }
                if let Some(names) = obj.remove("env_vars").and_then(|v| v.as_array().cloned()) {
                    let env = obj
                        .entry("environment")
                        .or_insert(json!({}))
                        .as_object_mut()
                        .context("环境变量格式无效")?;
                    for name in names {
                        let name = name.as_str().context("环境变量名无效")?;
                        env.entry(name).or_insert(format!("{{env:{name}}}").into());
                    }
                }
                obj.insert("type".into(), "local".into());
            } else {
                obj.insert("type".into(), "remote".into());
            }
            obj.insert("enabled".into(), true.into());
        } else {
            let obj = value.as_object_mut().unwrap();
            if let Some(headers) = obj.remove("http_headers") {
                obj.entry("headers").or_insert(headers);
            }
            obj.remove("enabled");
            obj.remove("disabled");
            if format == McpFormat::Servers {
                obj.insert("type".into(), t.into());
            } else if matches!(format, McpFormat::McpServers | McpFormat::ZCode) {
                // WorkBuddy's CodeBuddy engine and ZCode read the Claude-style
                // transport names (http/stdio/sse), not streamable-http.
                obj.insert("type".into(), t.into());
                obj.remove("transport");
            } else if format != McpFormat::Tiangong {
                obj.insert(
                    "type".into(),
                    if t == "http" { "streamable-http" } else { t }.into(),
                );
            }
        }
        Ok(value)
    }
}

pub fn parse_import(text: &str, name: &str) -> Result<Vec<McpServer>> {
    let value = formats::json(text.as_bytes(), Value::Null)?;
    let value = value
        .get("mcpServers")
        .or_else(|| value.get("servers"))
        .unwrap_or(&value);
    let servers = if value.get("command").is_some() || value.get("url").is_some() {
        vec![McpServer {
            name: name.trim().into(),
            config: value.clone(),
        }]
    } else {
        value
            .as_object()
            .context("请粘贴完整 MCP 配置或单个服务配置")?
            .iter()
            .map(|(name, config)| McpServer {
                name: name.clone(),
                config: config.clone(),
            })
            .collect()
    };
    ensure!(
        !servers.is_empty() && servers.len() <= 128,
        "MCP 配置没有服务或条目过多"
    );
    for server in &servers {
        server.validate()?;
    }
    Ok(servers)
}

pub fn equivalent(left: &Value, right: &Value) -> bool {
    fn normal(value: &Value) -> Value {
        let mut value = value.clone();
        if let Some(object) = value.as_object_mut() {
            let transport = object.remove("type").or_else(|| object.remove("transport"));
            object.remove("transport");
            if object.contains_key("url") {
                object.insert(
                    "type".into(),
                    if transport.as_ref().and_then(Value::as_str) == Some("sse") {
                        "sse"
                    } else {
                        "http"
                    }
                    .into(),
                );
            }
            for key in ["enabled", "disabled", "status", "serverName"] {
                object.remove(key);
            }
            if let Some(headers) = object.remove("http_headers") {
                object.entry("headers").or_insert(headers);
            }
        }
        value
    }
    normal(left) == normal(right)
}
pub fn from_native(name: &str, mut config: Value, format: McpFormat) -> Result<McpServer> {
    let object = config.as_object_mut().context("客户端 MCP 配置应为对象")?;
    if format == McpFormat::Codex {
        if let Some(headers) = object.remove("http_headers") {
            object.insert("headers".into(), headers);
        }
        object.remove("enabled");
    }
    if format == McpFormat::Harness {
        object.remove("serverName");
    }
    if format == McpFormat::OpenCode {
        if let Some(command) = object.get("command").and_then(Value::as_array).cloned() {
            ensure!(!command.is_empty(), "ACP 命令为空");
            object.insert("command".into(), command[0].clone());
            object.insert("args".into(), command[1..].to_vec().into());
        }
        if let Some(env) = object.remove("environment") {
            object.insert("env".into(), env);
        }
        let kind = if object.contains_key("command") {
            "stdio"
        } else {
            "http"
        };
        object.insert("type".into(), kind.into());
    }
    object.remove("disabled");
    object.remove("enabled");
    object.remove("status");
    let server = McpServer {
        name: name.into(),
        config,
    };
    server.validate()?;
    Ok(server)
}

pub fn read_entries(raw: &[u8], format: McpFormat) -> Result<BTreeMap<String, Value>> {
    if raw.is_empty() {
        return Ok(BTreeMap::new());
    }
    if format == McpFormat::Codex {
        let doc: toml::Value = std::str::from_utf8(raw)?
            .parse()
            .context("Codex TOML 无效，本次未修改")?;
        return match doc.get("mcp_servers") {
            Some(v) => Ok(serde_json::from_value(serde_json::to_value(v)?)?),
            None => Ok(BTreeMap::new()),
        };
    }
    if format == McpFormat::Harness {
        let blocks = patch_blocks(raw)?;
        let mut result = BTreeMap::new();
        for block in blocks {
            let row: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&block).context("Harness YAML 无效，本次未修改")?;
            let Some(items) = row.as_sequence() else {
                continue;
            };
            for patch in items {
                if let Some(rows) = patch
                    .get("insert")
                    .and_then(serde_yaml_ng::Value::as_sequence)
                {
                    for item in rows {
                        if item.get("name").and_then(serde_yaml_ng::Value::as_str)
                            != Some("@deepseek-ai/dsh-mcp-client")
                        {
                            continue;
                        }
                        if let Some(config) = item.get("config") {
                            let name = config
                                .get("serverName")
                                .and_then(serde_yaml_ng::Value::as_str)
                                .context("Harness MCP 缺少 serverName")?;
                            result.insert(
                                name.into(),
                                serde_json::to_value(config)
                                    .context("Harness MCP 使用了不可跨端复制的 YAML 表达式")?,
                            );
                        }
                    }
                }
            }
        }
        return Ok(result);
    }
    ensure!(
        format != McpFormat::Tiangong,
        "天工 MCP 通过已授权的本机服务读取"
    );
    let doc = formats::json(raw, json!({}))?;
    let entry = match format {
        McpFormat::Servers => doc.get("servers"),
        McpFormat::ZCode => doc.get("mcp").and_then(|m| m.get("servers")),
        McpFormat::OpenCode => doc.get("mcp"),
        _ => doc.get("mcpServers"),
    };
    match entry {
        Some(v) => serde_json::from_value(v.clone()).context("MCP 服务列表格式无效"),
        None => Ok(BTreeMap::new()),
    }
}

pub fn replace(
    raw: &[u8],
    format: McpFormat,
    name: &str,
    value: Option<&Value>,
) -> Result<Vec<u8>> {
    if format == McpFormat::Codex {
        let mut doc = std::str::from_utf8(raw)?
            .parse::<toml_edit::DocumentMut>()
            .context("Codex TOML 无效，本次未修改")?;
        if doc.get("mcp_servers").is_none() {
            doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let servers = doc["mcp_servers"]
            .as_table_like_mut()
            .context("Codex mcp_servers 格式无效")?;
        if let Some(value) = value {
            let single = toml::to_string(&json!({"entry":value}))
                .context("MCP 配置包含无法写入 TOML 的字段，请核对 JSON")?
                .parse::<toml_edit::DocumentMut>()?;
            servers.insert(name, single["entry"].clone());
        } else {
            servers.remove(name);
        }
        return Ok(doc.to_string().into_bytes());
    }
    if format == McpFormat::Harness {
        return replace_harness(raw, name, value);
    }
    ensure!(
        format != McpFormat::Tiangong,
        "天工 MCP 通过已授权的本机服务保存"
    );
    let mut doc = formats::json(raw, json!({}))?;
    let path: &[&str] = match format {
        McpFormat::Servers => &["servers"],
        McpFormat::ZCode => &["mcp", "servers"],
        McpFormat::OpenCode => &["mcp"],
        _ => &["mcpServers"],
    };
    let servers = formats::object_at_mut(&mut doc, path)?;
    if let Some(value) = value {
        servers.insert(name.into(), value.clone());
    } else {
        servers.remove(name);
    }
    formats::write_json(raw, &doc)
}

/// Keep unrelated YAML blocks byte-for-byte, including !!js expressions.
pub fn patch_blocks(raw: &[u8]) -> Result<Vec<String>> {
    let text = std::str::from_utf8(raw).context("Harness 配置不是 UTF-8")?;
    if text.trim().is_empty() || text.trim() == "[]" {
        return Ok(Vec::new());
    }
    if text.trim_start().starts_with('[') {
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(text).context("Harness YAML 无效")?;
        ensure!(value.is_sequence(), "Harness 补丁必须是列表");
        return Ok(vec![text.into()]);
    }
    let mut blocks = vec![String::new()];
    for line in text.split_inclusive('\n') {
        if line.starts_with("- ") || line.trim_end() == "-" {
            blocks.push(String::new());
        } else if !line.starts_with([' ', '\t', '#', '\r', '\n'])
            && !line.trim().is_empty()
            && line.trim_end() != "---"
        {
            bail!("Harness 配置不是可编辑的补丁列表，已保留原文件");
        }
        blocks.last_mut().unwrap().push_str(line);
    }
    Ok(blocks)
}

fn replace_harness(raw: &[u8], name: &str, value: Option<&Value>) -> Result<Vec<u8>> {
    let mut blocks = patch_blocks(raw)?;
    for block in &mut blocks {
        if block.trim().is_empty() || block.trim_start().starts_with('#') || block.trim() == "---" {
            continue;
        }
        let mut row: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(block).context("Harness YAML 无效，本次未修改")?;
        let mut changed = false;
        if let Some(patches) = row.as_sequence_mut() {
            for patch in patches {
                if let Some(insert) = patch
                    .get_mut("insert")
                    .and_then(serde_yaml_ng::Value::as_sequence_mut)
                {
                    let before = insert.len();
                    insert.retain(|v| {
                        !(v.get("name").and_then(serde_yaml_ng::Value::as_str)
                            == Some("@deepseek-ai/dsh-mcp-client")
                            && v.get("config")
                                .and_then(|v| v.get("serverName"))
                                .and_then(serde_yaml_ng::Value::as_str)
                                == Some(name))
                    });
                    changed |= insert.len() != before;
                }
            }
        }
        if changed {
            *block = serde_yaml_ng::to_string(&row)?;
        }
    }
    if let Some(value) = value {
        let row = json!([{"insert":[{"id":format!("tiancai-mcp-{name}"),"name":"@deepseek-ai/dsh-mcp-client","config":value}]}]);
        blocks.push(serde_yaml_ng::to_string(&row)?);
    }
    let mut text = blocks
        .into_iter()
        .filter(|b| !b.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        text = "[]\n".into();
    }
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workbuddy_and_zcode_use_native_http_transport_and_preserve_other_entries() {
        let remote = McpServer {
            name: "search".into(),
            config: json!({"url":"https://example.invalid/mcp","headers":{"X-Fixture":"fixture"}}),
        };
        for format in [McpFormat::McpServers, McpFormat::ZCode] {
            let native = remote.native(format).unwrap();
            assert_eq!(native["type"], "http");
            let raw = replace(b"", format, "search", Some(&native)).unwrap();
            let entries = read_entries(&raw, format).unwrap();
            assert_eq!(entries["search"]["headers"]["X-Fixture"], "fixture");
            let local = McpServer {
                name: "local".into(),
                config: json!({"command":"never-execute","args":["--fixture"],"env":{"FIXTURE":"value"}}),
            };
            let next =
                replace(&raw, format, "local", Some(&local.native(format).unwrap())).unwrap();
            let entries = read_entries(&next, format).unwrap();
            assert_eq!(entries["local"]["type"], "stdio");
            assert_eq!(entries["local"]["args"], json!(["--fixture"]));
            assert_eq!(entries["search"], native);
        }
    }
    #[test]
    fn roundtrip_mcp_preserves_custom_fields_and_other_servers() {
        let server = McpServer {
            name: "search".into(),
            config: json!({"url":"https://example.invalid/mcp","headers":{"X-Key":"fixture"},"futureSetting":true}),
        };
        for format in [
            McpFormat::Codex,
            McpFormat::Servers,
            McpFormat::McpServers,
            McpFormat::ZCode,
            McpFormat::Harness,
        ] {
            let native = server.native(format).unwrap();
            let other = McpServer {
                name: "other".into(),
                config: json!({"command":"fixture-tool"}),
            }
            .native(format)
            .unwrap();
            let first = replace(b"", format, "other", Some(&other)).unwrap();
            let next = replace(&first, format, &server.name, Some(&native)).unwrap();
            let entries = read_entries(&next, format).unwrap();
            assert_eq!(entries["other"]["command"], "fixture-tool");
            assert_eq!(entries["search"]["futureSetting"], true);
            let removed = replace(&next, format, "search", None).unwrap();
            assert!(
                !read_entries(&removed, format)
                    .unwrap()
                    .contains_key("search")
            );
        }
    }
    #[test]
    fn form_json_and_capability_validation() {
        assert!(
            parse_import(
                r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","fixture"]}}}"#,
                "unused"
            )
            .unwrap()[0]
                .validate()
                .is_ok()
        );
        assert!(
            parse_import(
                r#"{"command":"npx","url":"https://example.invalid"}"#,
                "bad"
            )
            .is_err()
        );
        assert!(
            McpServer {
                name: "s".into(),
                config: json!({"type":"sse","url":"https://example.invalid/sse"})
            }
            .native(McpFormat::Codex)
            .is_err()
        );
    }
}
