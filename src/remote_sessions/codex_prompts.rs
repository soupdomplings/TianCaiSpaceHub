use super::ApprovalDecision;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};

fn desktop() -> Value {
    json!({"type":"approval","title":"请在目标电脑的 Codex 中处理","message":"这项请求需要原客户端的界面。处理后，网页会自动继续。","desktopOnly":true,"choices":[]})
}

fn field_key(key: &str) -> bool {
    !key.is_empty() && !matches!(key, "__proto__" | "constructor" | "prototype")
}

pub(super) fn input(method: &str) -> bool {
    matches!(
        method,
        "item/tool/requestUserInput" | "tool/requestUserInput"
    )
}
fn input_questions(params: &Value) -> Option<Value> {
    let mut questions = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for q in params["questions"].as_array()? {
        let id = q["id"].as_str()?;
        if !field_key(id) || !ids.insert(id) || q["isSecret"] == true {
            return None;
        }
        let prompt = q["question"].as_str()?;
        let options = q["options"]
            .as_array()
            .map(|opts| {
                opts.iter()
                    .map(|o| {
                        let label = o["label"].as_str()?;
                        Some(json!({"value":label,"label":label,"description":o["description"]}))
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .unwrap_or_else(|| Some(Vec::new()))?;
        questions.push(json!({"id":id,"prompt":prompt,"required":true,"options":options,"allowOther":q["isOther"]==true}));
    }
    (!questions.is_empty())
        .then(|| json!({"type":"question","title":"需要你补充","questions":questions}))
}

fn form_questions(params: &Value) -> Option<Vec<Value>> {
    if !(matches!(params["mode"].as_str(), Some("form" | "openai/form"))
        || params.get("mode").is_none())
        || params.get("url").is_some_and(|v| !v.is_null())
    {
        return None;
    }
    let schema = &params["requestedSchema"];
    if schema["type"] != "object" {
        return None;
    }
    let properties = schema["properties"].as_object()?;
    if properties.len() > 32
        || schema.as_object()?.keys().any(|k| {
            !matches!(
                k.as_str(),
                "$schema"
                    | "type"
                    | "properties"
                    | "required"
                    | "title"
                    | "description"
                    | "additionalProperties"
            )
        })
        || schema
            .get("additionalProperties")
            .is_some_and(|v| !v.is_boolean())
    {
        return None;
    }
    let required = match schema.get("required") {
        Some(v) => v
            .as_array()?
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()?,
        None => vec![],
    };
    if required.iter().any(|key| !properties.contains_key(*key)) {
        return None;
    }
    let mut fields = Vec::new();
    for (id, spec) in properties {
        if !field_key(id)
            || spec.as_object()?.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "type"
                        | "title"
                        | "description"
                        | "enum"
                        | "enumNames"
                        | "oneOf"
                        | "minimum"
                        | "maximum"
                        | "minLength"
                        | "maxLength"
                        | "default"
                )
            })
        {
            return None;
        }
        let kind = spec["type"].as_str()?;
        let options = match kind {
            "boolean" => vec![json!({"value":"true","label":"是"}),json!({"value":"false","label":"否"})],
            "string" if spec.get("oneOf").is_some() => spec["oneOf"].as_array()?.iter().map(|v| {
                let val = v["const"].as_str()?;
                Some(json!({"value":val,"label":v["title"].as_str().unwrap_or(val)}))
            }).collect::<Option<Vec<_>>>()?,
            "string" if spec.get("enum").is_some() => spec["enum"].as_array()?.iter().enumerate().map(|(i,v)| {
                let val = v.as_str()?;
                Some(json!({"value":val,"label":spec["enumNames"].get(i).and_then(Value::as_str).unwrap_or(val)}))
            }).collect::<Option<Vec<_>>>()?,
            "string"|"number"|"integer" if spec.get("enum").is_none() && spec.get("oneOf").is_none() => vec![],
            _ => return None,
        };
        fields.push(json!({"id":id,"prompt":spec["title"].as_str().unwrap_or(id),"description":spec["description"],
          "inputType":if kind=="integer"||kind=="number"{kind}else{"text"},"required":required.contains(&id.as_str()),"options":options}));
    }
    Some(fields)
}

pub(super) fn view(method: &str, params: &Value) -> Option<Value> {
    if input(method) {
        return Some(input_questions(params).unwrap_or_else(desktop));
    }
    if method != "mcpServer/elicitation/request" {
        return None;
    }
    Some(form_questions(params).map(|questions|json!({"type":"question","canDecline":true,
        "title":params["serverName"].as_str().unwrap_or("MCP 确认"),"message":params["message"],"questions":questions})).unwrap_or_else(desktop))
}

pub(super) fn response(method: &str, params: &Value, decision: &ApprovalDecision) -> Result<Value> {
    let card = view(method, params).context("此请求需要目标客户端处理")?;
    ensure!(
        card["desktopOnly"] != true,
        "请在目标电脑的 Codex 中处理这项请求"
    );
    let submit = match decision.decision.as_str() {
        "accept" => true,
        "decline" | "cancel" => false,
        _ => bail!("确认选项无效"),
    };
    let is_input = input(method);
    if !submit && !is_input {
        return Ok(json!({"action":decision.decision,"content":null}));
    }
    let mut answers = Map::new();
    for q in card["questions"].as_array().context("问题格式不兼容")? {
        let key = q["id"].as_str().unwrap();
        if !submit {
            answers.insert(key.into(), json!({"answers":[]}));
            continue;
        }
        let raw = &decision.answers[key];
        let text = match raw {
            Value::String(s) => Some(s.as_str()),
            Value::Array(list) if is_input && list.len() == 1 => list[0].as_str(),
            Value::Null => None,
            _ => bail!("回答格式不正确"),
        };
        let Some(text) = text.filter(|s| !s.trim().is_empty()) else {
            ensure!(q["required"] != true, "请填写所有必填项");
            continue;
        };
        ensure!(text.len() <= 16384, "回答太长");
        if let Some(opts) = q["options"].as_array().filter(|opts| !opts.is_empty()) {
            ensure!(
                q["allowOther"] == true || opts.iter().any(|v| v["value"] == text),
                "请选择原请求提供的选项"
            );
        }
        let value = if is_input {
            json!({"answers":[text]})
        } else {
            let prop = &params["requestedSchema"]["properties"][key];
            let value = match prop["type"].as_str() {
                Some("boolean") => json!(text.parse::<bool>().context("请选择是或否")?),
                Some("number" | "integer") => {
                    let value: Value = serde_json::from_str(text).context("请填写有效数值")?;
                    ensure!(
                        value.is_number()
                            && (prop["type"] != "integer" || value.is_i64() || value.is_u64()),
                        "数值类型不正确"
                    );
                    let n = value.as_f64().context("数值无效")?;
                    ensure!(
                        prop["minimum"].as_f64().is_none_or(|min| n >= min)
                            && prop["maximum"].as_f64().is_none_or(|max| n <= max),
                        "数值超出范围"
                    );
                    value
                }
                _ => {
                    let len = text.chars().count() as u64;
                    ensure!(
                        prop["minLength"].as_u64().is_none_or(|min| len >= min)
                            && prop["maxLength"].as_u64().is_none_or(|max| len <= max),
                        "回答长度超出范围"
                    );
                    json!(text)
                }
            };
            value
        };
        answers.insert(key.to_owned(), value);
    }
    Ok(if is_input {
        json!({"answers":answers})
    } else {
        json!({"action":"accept","content":answers})
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn answer(decision: &str, answers: Value) -> ApprovalDecision {
        ApprovalDecision {
            decision: decision.into(),
            answers,
        }
    }

    #[test]
    fn elicitation_replies_with_the_users_typed_fields_and_never_fills_consent_itself() {
        let method = "mcpServer/elicitation/request";
        let form = json!({"mode":"form","requestedSchema":{"type":"object","required":["allow","count","name"],"properties":{
            "allow":{"type":"boolean","default":true},"count":{"type":"integer","minimum":2},"name":{"type":"string","oneOf":[{"const":"one","title":"第一"}]}
        }}});
        assert_eq!(view(method, &form).unwrap()["type"], "question");
        let mut legacy = form.clone();
        legacy.as_object_mut().unwrap().remove("mode");
        assert_eq!(view(method, &legacy).unwrap()["type"], "question");
        legacy["url"] = "https://example.invalid".into();
        assert_eq!(view(method, &legacy).unwrap()["desktopOnly"], true);
        assert!(
            response(
                method,
                &form,
                &answer("accept", json!({"count":"2","name":"one"}))
            )
            .is_err()
        );
        for invalid in [
            json!({"allow":"true","count":"1","name":"one"}),
            json!({"allow":"yes","count":"2","name":"one"}),
            json!({"allow":"true","count":"2","name":"outside"}),
        ] {
            assert!(response(method, &form, &answer("accept", invalid)).is_err());
        }
        assert_eq!(
            response(
                method,
                &form,
                &answer("accept", json!({"allow":"false","count":"2","name":"one"}))
            )
            .unwrap(),
            json!({"action":"accept","content":{"allow":false,"count":2,"name":"one"}})
        );
        assert_eq!(
            response(method, &form, &answer("cancel", Value::Null)).unwrap(),
            json!({"action":"cancel","content":null})
        );
        for params in [
            json!({"mode":"url","url":"https://example.invalid"}),
            json!({"mode":"form","requestedSchema":{"type":"object","properties":{"records":{"type":"array"}}}}),
        ] {
            assert_eq!(view(method, &params).unwrap()["desktopOnly"], true);
            assert!(response(method, &params, &answer("accept", Value::Null)).is_err());
        }
    }

    #[test]
    fn user_input_options_enforce_original_constraints_and_keep_custom_answers() {
        let method = "item/tool/requestUserInput";
        let mut p =
            json!({"questions":[{"id":"q","question":"Which?","options":[{"label":"Listed"}]}]});
        let custom = answer("accept", json!({"q":"A different choice"}));
        assert!(response(method, &p, &custom).is_err());
        p["questions"][0]["isOther"] = true.into();
        assert_eq!(
            view(method, &p).unwrap()["questions"][0]["allowOther"],
            true
        );
        assert_eq!(
            response(method, &p, &custom).unwrap(),
            json!({"answers":{"q":{"answers":["A different choice"]}}})
        );
        assert_eq!(
            response(method, &p, &answer("cancel", custom.answers)).unwrap(),
            json!({"answers":{"q":{"answers":[]}}})
        );
        p["questions"][0]["isSecret"] = true.into();
        assert_eq!(view(method, &p).unwrap()["desktopOnly"], true);
    }
}
