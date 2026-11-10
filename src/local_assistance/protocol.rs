//! The model cannot approve itself: only verified renderer events reach here.
use super::{MAX_TEXT, Prompt};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};

pub fn supported(method: &str, params: &Value) -> bool {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => true,
        "item/permissions/requestApproval" => params["permissions"].is_object(),
        "item/tool/requestUserInput" => params["questions"].as_array().is_some_and(|a| {
            !a.is_empty()
                && a.len() <= 8
                && a.iter().all(|q| {
                    q["id"].as_str().is_some()
                        && q["question"].as_str().is_some()
                        && q.get("options").is_none_or(|v| {
                            v.is_null() || v.as_array().is_some_and(|a| a.len() <= 12)
                        })
                })
        }),
        "mcpServer/elicitation/request" => {
            matches!(params["mode"].as_str(), Some("form" | "openai/form"))
                && params["requestedSchema"]["properties"]
                    .as_object()
                    .is_some_and(|a| {
                        a.len() <= 8
                            && a.values().all(|v| {
                                matches!(
                                    v["type"].as_str(),
                                    Some("string" | "boolean" | "number" | "integer")
                                )
                            })
                    })
        }
        _ => false,
    }
}
pub fn decision(prompt: &Prompt, input: &Value) -> Result<Value> {
    ensure!(serde_json::to_vec(input)?.len() <= MAX_TEXT, "输入内容过长");
    let allow = input["choice"] == "allow";
    ensure!(
        matches!(input["choice"].as_str(), Some("allow" | "deny")),
        "请选择允许、拒绝或提交回答"
    );
    match prompt.method.as_str() {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            if allow {
                if let Some(options) = prompt.params["availableDecisions"].as_array() {
                    ensure!(
                        options.iter().any(|v| v == "accept"),
                        "此请求不支持单次批准，请拒绝或停止协助"
                    );
                }
            }
            Ok(json!({"decision":if allow{"accept"}else{"decline"}}))
        }
        "item/permissions/requestApproval" => Ok(
            json!({"permissions":if allow {prompt.params["permissions"].clone()}else{json!({})},"scope":"turn"}),
        ),
        "item/tool/requestUserInput" => {
            let mut answers = serde_json::Map::new();
            for q in prompt.params["questions"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("问题格式不兼容"))?
            {
                let id = q["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("问题标识缺失"))?;
                let text = input["answers"][id].as_str().unwrap_or("");
                if allow {
                    ensure!(
                        !text.trim().is_empty() && text.len() <= 8192,
                        "请填写问题的回答"
                    );
                    if let Some(options) = q["options"].as_array().filter(|a| !a.is_empty()) {
                        ensure!(
                            q["isOther"] == true || options.iter().any(|o| o["label"] == text),
                            "请选择本问题提供的选项"
                        );
                    }
                    answers.insert(id.into(), json!({"answers":[text]}));
                } else {
                    answers.insert(id.into(), json!({"answers":[]}));
                }
            }
            Ok(json!({"answers":answers}))
        }
        "mcpServer/elicitation/request" => {
            if !allow {
                return Ok(json!({"action":"decline","content":null}));
            }
            ensure!(
                supported(&prompt.method, &prompt.params),
                "此询问格式尚不支持，请停止协助"
            );
            let schema = &prompt.params["requestedSchema"];
            let mut content = serde_json::Map::new();
            for (key, property) in schema["properties"].as_object().unwrap() {
                let val = &input["answers"][key];
                let required = schema["required"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == key));
                if val.is_null() {
                    ensure!(!required, "请完成必填内容");
                    continue;
                }
                ensure!(
                    match property["type"].as_str() {
                        Some("string") => val.is_string(),
                        Some("boolean") => val.is_boolean(),
                        Some("integer") => val.is_i64() || val.is_u64(),
                        Some("number") => val.is_number(),
                        _ => false,
                    },
                    "输入类型与问题不一致"
                );
                if let Some(allowed) = property["enum"].as_array() {
                    ensure!(allowed.contains(val), "请选择允许的值");
                }
                if let Some(s) = val.as_str() {
                    ensure!(s.len() <= 8192, "回答内容过长");
                    if let Some(min) = property["minLength"].as_u64() {
                        ensure!(s.chars().count() as u64 >= min, "回答长度不足");
                    }
                    if let Some(max) = property["maxLength"].as_u64() {
                        ensure!(s.chars().count() as u64 <= max, "回答内容过长");
                    }
                }
                if let Some(n) = val.as_f64() {
                    if let Some(min) = property["minimum"].as_f64() {
                        ensure!(n >= min, "数值低于允许范围");
                    }
                    if let Some(max) = property["maximum"].as_f64() {
                        ensure!(n <= max, "数值超出允许范围");
                    }
                }
                content.insert(key.clone(), val.clone());
            }
            Ok(json!({"action":"accept","content":content}))
        }
        _ => bail!("此类询问尚不能安全转交，已保留为待处理"),
    }
}

pub fn turn_outcome(value: &Value, thread: &str, turn: Option<&str>) -> Option<super::Phase> {
    let p = &value["params"];
    if value["method"] != "turn/completed"
        || p["threadId"].as_str() != Some(thread)
        || p["turn"]["id"].as_str() != turn
    {
        return None;
    }
    match p["turn"]["status"].as_str() {
        Some("completed") => Some(super::Phase::Completed),
        Some("interrupted") => Some(super::Phase::Cancelled),
        Some("failed") => Some(super::Phase::Failed),
        _ => None,
    }
}
