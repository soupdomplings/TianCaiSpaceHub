use anyhow::{Context, Result, ensure};
use jsonc_parser::{
    ParseOptions,
    cst::{CstArray, CstInputValue, CstObject, CstRootNode},
};
use serde_json::Value;

pub fn json(raw: &[u8], empty: Value) -> Result<Value> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(empty);
    }
    let text = std::str::from_utf8(raw).context("配置不是 UTF-8，请检查所选文件")?;
    let root = CstRootNode::parse(
        text.trim_start_matches('\u{feff}'),
        &ParseOptions::default(),
    )
    .map_err(|_| anyhow::anyhow!("配置 JSON 无效，本次未修改"))?;
    root.to_serde_value().context("配置内容为空或无效")
}

pub fn write_json(raw: &[u8], next: &Value) -> Result<Vec<u8>> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(serde_json::to_vec_pretty(next)?);
    }
    let text = std::str::from_utf8(raw).context("配置不是 UTF-8")?;
    let bom = text.starts_with('\u{feff}');
    let root = CstRootNode::parse(
        text.trim_start_matches('\u{feff}'),
        &ParseOptions::default(),
    )
    .map_err(|_| anyhow::anyhow!("配置 JSON 无效，本次未修改"))?;
    if root.to_serde_value().as_ref() == Some(next) {
        return Ok(raw.to_vec());
    }
    if let (Some(object), Some(desired)) = (root.object_value(), next.as_object()) {
        sync_object(&object, desired);
    } else if let (Some(array), Some(desired)) = (root.array_value(), next.as_array()) {
        sync_array(&array, desired);
    } else {
        root.set_value(input(next));
    }
    let result = format!("{}{}", if bom { "\u{feff}" } else { "" }, root);
    ensure!(
        json(result.as_bytes(), Value::Null)? == *next,
        "保存后的 JSON 核对失败"
    );
    Ok(result.into_bytes())
}

fn sync_object(object: &CstObject, desired: &serde_json::Map<String, Value>) {
    let previous = object.to_serde_value().unwrap_or_default();
    if let Some(previous) = previous.as_object() {
        for key in previous.keys().filter(|k| !desired.contains_key(*k)) {
            if let Some(prop) = object.get(key) {
                prop.remove();
            }
        }
    }
    for (key, value) in desired {
        if previous.get(key) == Some(value) {
            continue;
        }
        if let Some(prop) = object.get(key) {
            if let (Some(child), Some(map)) = (prop.object_value(), value.as_object()) {
                sync_object(&child, map);
            } else if let (Some(child), Some(values)) = (prop.array_value(), value.as_array()) {
                sync_array(&child, values);
            } else {
                prop.set_value(input(value));
            }
        } else {
            object.append(key, input(value));
        }
    }
}

fn same_entry(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    for key in ["id", "providerId", "name"] {
        if let (Some(x), Some(y)) = (
            a.get(key).and_then(Value::as_str),
            b.get(key).and_then(Value::as_str),
        ) {
            if x != y {
                return false;
            }
            if key == "providerId" {
                return a.get("modelId") == b.get("modelId");
            }
            return true;
        }
    }
    false
}

// Preserve untouched array elements (including their comments and number spelling).
fn sync_array(array: &CstArray, desired: &[Value]) {
    for node in array.elements().into_iter().rev() {
        if node
            .to_serde_value()
            .is_some_and(|v| !desired.iter().any(|d| same_entry(&v, d)))
        {
            node.remove();
        }
    }
    for (i, value) in desired.iter().enumerate() {
        let elements = array.elements();
        if let Some(node) = elements.get(i) {
            let previous = node.to_serde_value().unwrap_or_default();
            if previous == *value {
                continue;
            }
            if same_entry(&previous, value) {
                if let (Some(object), Some(map)) = (node.as_object(), value.as_object()) {
                    sync_object(&object, map);
                    continue;
                }
                if let (Some(child), Some(values)) = (node.as_array(), value.as_array()) {
                    sync_array(&child, values);
                    continue;
                }
                array.insert(i, input(value));
                node.clone().remove();
            } else {
                array.insert(i, input(value));
            }
        } else {
            array.append(input(value));
        }
    }
    for node in array.elements().into_iter().skip(desired.len()).rev() {
        node.remove();
    }
}

fn input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(v) => CstInputValue::Bool(*v),
        Value::Number(v) => CstInputValue::Number(v.to_string()),
        Value::String(v) => CstInputValue::String(v.clone()),
        Value::Array(v) => CstInputValue::Array(v.iter().map(input).collect()),
        Value::Object(v) => {
            CstInputValue::Object(v.iter().map(|(k, v)| (k.clone(), input(v))).collect())
        }
    }
}

pub fn object_at_mut<'a>(
    value: &'a mut Value,
    path: &[&str],
) -> Result<&'a mut serde_json::Map<String, Value>> {
    let mut current = value;
    for key in path {
        let object = current
            .as_object_mut()
            .context("配置应为对象，已保留原文件")?;
        current = object.entry(*key).or_insert_with(|| serde_json::json!({}));
    }
    current
        .as_object_mut()
        .context("配置字段应为对象，已保留原文件")
}

pub fn write_yaml_section(raw: &[u8], key: &str, section: &Value) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(raw).context("YAML 需要 UTF-8")?;
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    // Flow documents cannot be edited by line ranges; retain all semantic fields.
    if text.trim_start().starts_with('{') {
        let mut doc: Value = serde_yaml_ng::from_slice(raw)?;
        object_at_mut(&mut doc, &[])?.insert(key.into(), section.clone());
        return Ok(serde_yaml_ng::to_string(&doc)?.into_bytes());
    }
    let prefix = format!("{key}:");
    let start = lines.iter().position(|l| l.starts_with(&prefix));
    let replacement = serde_yaml_ng::to_string(&serde_json::json!({key:section}))?;
    if let Some(start) = start {
        let end = (start + 1..lines.len())
            .find(|i| {
                let line = lines[*i];
                !line.starts_with([' ', '\t', '#', '\n', '\r']) && !line.trim().is_empty()
            })
            .unwrap_or(lines.len());
        Ok(format!(
            "{}{}{}",
            lines[..start].concat(),
            replacement,
            lines[end..].concat()
        )
        .into_bytes())
    } else {
        Ok(format!(
            "{text}{}{}",
            if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            },
            replacement
        )
        .into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targeted_jsonc_preserves_unrelated_comments_and_unknown_fields() {
        let before = b"{\n // keep this\n \"other\": {\"raw\":1.0},\n \"servers\": { /* retain */ \"mine\": {\"url\":\"old\",\"future\":true}},\n}";
        let mut next = json(before, Value::Null).unwrap();
        next["servers"]["mine"]["url"] = "new".into();
        let result = write_json(before, &next).unwrap();
        let text = std::str::from_utf8(&result).unwrap();
        assert!(
            text.contains("// keep this")
                && text.contains("/* retain */")
                && text.contains("\"raw\":1.0")
        );
        assert_eq!(
            json(&result, Value::Null).unwrap()["servers"]["mine"]["future"],
            true
        );
    }
    #[test]
    fn arrays_retain_unrelated_entries_comments_and_compound_model_identity() {
        let raw = br#"[
          {"providerId":"p","modelId":"a","n":1.0}, // preserve-a
          {"providerId":"p","modelId":"b","n":2},
          {"id":"unrelated","n":3.00} /* untouched */
        ]"#;
        let next = serde_json::json!([{"providerId":"p","modelId":"a","n":1.0},{"providerId":"p","modelId":"b","n":4},{"id":"unrelated","n":3.0},{"id":"added"}]);
        let result = write_json(raw, &next).unwrap();
        let text = String::from_utf8(result.clone()).unwrap();
        assert!(text.contains("preserve-a") && text.contains("untouched") && text.contains("3.00"));
        assert_eq!(json(&result, Value::Null).unwrap(), next);
        let removed = serde_json::json!([next[2].clone(), next[0].clone()]);
        assert_eq!(
            json(&write_json(&result, &removed).unwrap(), Value::Null).unwrap(),
            removed
        );
    }
}
