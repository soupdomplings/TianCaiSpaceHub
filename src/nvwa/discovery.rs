//! Small, bounded discovery results for the desktop UI. Full tool contracts stay
//! in the MCP transport and, when needed, TianGong's native tool directory.
use std::collections::BTreeMap;

use serde_json::{Value, json};

const MAX_GROUPS_SHOWN: usize = 32;
const MAX_EXAMPLES_PER_GROUP: usize = 3;

pub(crate) fn brief(value: &str, limit: usize) -> String {
    let mut characters = value.trim().chars().filter(|c| !c.is_control());
    let mut result: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() {
        result.push('…');
    }
    result
}

fn group_key(tool: &Value) -> (String, &'static str) {
    if let Some(module) = tool
        .pointer("/_meta/com.jiuqi.nvwa/moduleName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|module| {
            !module.is_empty() && module.len() <= 128 && !module.chars().any(char::is_control)
        })
    {
        return (module.to_owned(), "module");
    }
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
    if let Some((namespace, _)) = name.split_once('.')
        && !namespace.is_empty()
        && namespace.len() <= 128
    {
        return (namespace.to_owned(), "namespace");
    }
    ("other".into(), "other")
}

#[derive(Default)]
struct Group {
    count: usize,
    examples: Vec<Value>,
}

pub(crate) fn summarize_tools(tools: &[Value]) -> Value {
    let mut groups: BTreeMap<(String, &str), Group> = BTreeMap::new();
    for tool in tools {
        let group = groups.entry(group_key(tool)).or_default();
        group.count += 1;
        if group.examples.len() < MAX_EXAMPLES_PER_GROUP {
            let description = tool["description"]
                .as_str()
                .unwrap_or("")
                .split(['\n', '\r'])
                .find(|part| !part.trim().is_empty())
                .unwrap_or("");
            let title = tool["title"]
                .as_str()
                .filter(|title| !title.trim().is_empty())
                .or_else(|| tool.pointer("/annotations/title").and_then(Value::as_str))
                .unwrap_or("");
            group.examples.push(json!({
                "name": brief(tool["name"].as_str().unwrap_or(""), 256),
                "title": brief(title, 80),
                "description": brief(description, 120),
            }));
        }
    }
    let group_count = groups.len();
    let module_count = groups
        .keys()
        .filter(|(_, source)| *source == "module")
        .count();
    let examples_omitted = groups
        .values()
        .any(|group| group.count > group.examples.len());
    let groups: Vec<Value> = groups
        .into_iter()
        .take(MAX_GROUPS_SHOWN)
        .map(|((name, source), group)| {
            json!({"name":name,"source":source,"toolCount":group.count,
                "omittedToolCount":group.count-group.examples.len(),"examples":group.examples})
        })
        .collect();
    json!({"groupCount":group_count,"moduleCount":module_count,"omittedGroupCount":group_count-groups.len(),
        "examplesTruncated":examples_omitted || group_count > groups.len(),"groups":groups})
}

/// Apply this only after consumers that require full contracts (TianGong).
pub(crate) fn compact_detection(mut detected: Value) -> Value {
    let tools = detected
        .as_object_mut()
        .and_then(|object| object.remove("tools"))
        .unwrap_or_else(|| json!([]));
    detected["catalogSummary"] =
        summarize_tools(tools.as_array().map(Vec::as_slice).unwrap_or(&[]));
    // initialize serverInfo is remote input too; never forward arbitrary extras.
    detected["serverInfo"] = json!({
        "name":brief(detected["serverInfo"]["name"].as_str().unwrap_or(""), 96),
        "version":brief(detected["serverInfo"]["version"].as_str().unwrap_or(""), 48),
    });
    detected
}

pub(crate) fn catalog_result(
    version: &str,
    server_info: Option<Value>,
    generation: &str,
    local: bool,
    tools: Vec<Value>,
    limit: Option<&str>,
) -> Value {
    json!({
        "state":if limit.is_some() { "connected_catalog_limited" } else { "tools_discovered" },
        "connectionVerified":true,"catalogComplete":limit.is_none(),"catalogLimitReason":limit,
        "protocolVersion":version,"serverInfo":server_info,
        "toolCount":limit.is_none().then_some(tools.len()),"observedToolCount":tools.len(),
        "tools":tools,"checkedAtMs":super::runtime::now_ms(),"authGeneration":generation,
        "localBridgeVerified":local,
        "detail":if limit.is_some() { "MCP connected; tool catalog exceeds read limit" }
            else { "initialize and paginated tools/list verified; no tool invoked" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, module: Option<&str>) -> Value {
        let mut tool = json!({"name":name,"description":"用途说明\nvery long contract",
            "inputSchema":{"type":"object"},"outputSchema":{"type":"object"}});
        if let Some(module) = module {
            tool["_meta"] = json!({"com.jiuqi.nvwa":{"moduleName":module}});
        }
        tool
    }

    #[test]
    fn multi_megabyte_contracts_become_a_small_summary_with_exact_counts() {
        let mut tools: Vec<_> = (0..156)
            .map(|index| tool(&format!("analysis.tool_{index}"), Some("mcp-analysis")))
            .collect();
        tools[0]["inputSchema"]["properties"] =
            json!({"large":{"description":"x".repeat(3*1024*1024)}});
        tools[0]["_meta"]["com.jiuqi.nvwa"]["operationalContract"] =
            json!({"large":"x".repeat(1024*1024)});
        let original = catalog_result("fixture", None, "fixture-generation", true, tools, None);
        assert!(serde_json::to_vec(&original).unwrap().len() > 2 * 1024 * 1024);
        let compact = compact_detection(original);
        assert_eq!(compact["catalogComplete"], true);
        assert_eq!(compact["toolCount"], 156);
        assert_eq!(compact["catalogSummary"]["groups"][0]["toolCount"], 156);
        assert_eq!(
            compact["catalogSummary"]["groups"][0]["omittedToolCount"],
            153
        );
        assert!(compact.get("tools").is_none());
        assert!(serde_json::to_vec(&compact).unwrap().len() < 8 * 1024);
        let encoded = compact.to_string();
        for excluded in [
            "inputSchema",
            "outputSchema",
            "operationalContract",
            "_meta",
        ] {
            assert!(!encoded.contains(excluded));
        }
    }

    #[test]
    fn real_module_metadata_takes_precedence_and_namespaces_are_explicit_fallbacks() {
        let tools = vec![
            tool("knowledge.read", Some("mcp-core")),
            tool("knowledge.search", Some("mcp-core")),
            tool("knowledge.resolve_standards", Some("mcp-knowledge")),
            tool("analysis.inspect", None),
            tool("unclassified", None),
        ];
        let summary = summarize_tools(&tools);
        let groups = summary["groups"].as_array().unwrap();
        assert_eq!(summary["groupCount"], 4);
        assert!(
            groups
                .iter()
                .any(|group| group["name"] == "mcp-core" && group["toolCount"] == 2)
        );
        assert!(
            groups
                .iter()
                .any(|group| group["name"] == "analysis" && group["source"] == "namespace")
        );
    }

    #[test]
    fn large_number_of_groups_and_long_unicode_text_cannot_overflow_the_ui_response() {
        let mut tools: Vec<_> = (0..10_000)
            .map(|index| tool(&format!("group_{index}.tool"), None))
            .collect();
        tools[0]["title"] = json!("标题".repeat(20_000));
        tools[0]["description"] = json!("用法".repeat(20_000));
        let mut result = catalog_result("fixture", None, "fixture-generation", false, tools, None);
        result["serverInfo"] = json!({"name":"服务".repeat(20_000),"version":"v".repeat(20_000),"extra":"x".repeat(3*1024*1024)});
        let compact = compact_detection(result);
        assert_eq!(compact["toolCount"], 10_000);
        assert_eq!(compact["catalogSummary"]["groupCount"], 10_000);
        assert_eq!(
            compact["catalogSummary"]["groups"]
                .as_array()
                .unwrap()
                .len(),
            32
        );
        assert_eq!(compact["catalogSummary"]["omittedGroupCount"], 9968);
        assert!(serde_json::to_vec(&compact).unwrap().len() < 128 * 1024);
        assert!(compact["serverInfo"].get("extra").is_none());
    }

    #[test]
    fn incomplete_catalog_has_unknown_total_and_is_distinct_from_a_complete_empty_catalog() {
        let partial = compact_detection(catalog_result(
            "fixture",
            None,
            "fixture-generation",
            true,
            vec![tool("analysis.inspect", None)],
            Some("response_size"),
        ));
        assert_eq!(partial["connectionVerified"], true);
        assert_eq!(partial["catalogComplete"], false);
        assert!(partial["toolCount"].is_null());
        assert_eq!(partial["observedToolCount"], 1);
        let empty = compact_detection(catalog_result(
            "fixture",
            None,
            "fixture-generation",
            false,
            vec![],
            None,
        ));
        assert_eq!(empty["catalogComplete"], true);
        assert_eq!(empty["toolCount"], 0);
    }
}
