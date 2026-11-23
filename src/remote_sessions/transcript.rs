//! Project the target machine's native activity onto the remote chat protocol.
//! This boundary is shared by live/replayed events and works with older browsers.
use super::types::RemoteEvent;
use serde_json::{Value, json};

fn activity(tag: &str) -> bool {
    matches!(
        tag,
        "tool"
            | "reasoning"
            | "thinking"
            | "think"
            | "progress"
            | "internal"
            | "fileChange"
            | "file_change"
            | "commandExecution"
            | "mcpToolCall"
            | "dynamicToolCall"
            | "collabAgentToolCall"
            | "webSearch"
            | "tool_start"
            | "tool_input"
            | "tool_result"
            | "file_write"
    )
}

fn hidden(item: &Value) -> bool {
    // Never inspect displayed text: a user can legitimately discuss these items.
    [
        &item["type"],
        &item["role"],
        &item["nativeType"],
        &item["kind"],
        &item["data"]["nativeType"],
        &item["data"]["type"],
        &item["data"]["kind"],
    ]
    .into_iter()
    .filter_map(Value::as_str)
    .any(activity)
}

fn visible_item(item: &mut Value) -> bool {
    if !hidden(item) {
        return true;
    }
    if !item["files"]
        .as_array()
        .is_some_and(|files| !files.is_empty())
    {
        return false;
    }
    // Retain registered file handles for preview/download without tool output.
    *item = json!({"id":item["id"].take(),"role":"assistant","type":"message","text":"","files":item["files"].take()});
    true
}

pub(super) fn history(snapshot: &mut Value) {
    if let Some(items) = snapshot["items"].as_array_mut() {
        items.retain_mut(visible_item);
    }
}

// Only the event currently being received can add activity to the transcript.
// Completed Codex items contain the full accumulated output and may have
// started before a viewer enabled the category. Never republish that prefix.
fn live_detail(e: &RemoteEvent) -> Option<Value> {
    let (item, delta) = match e.kind.as_str() {
        "item.updated" => (e.data.get("item")?, false),
        "message.delta" => (&e.data, true),
        _ => return None,
    };
    let category = match item["type"].as_str()? {
        "reasoning" | "progress" => "reasoning",
        "fileChange" => "fileChange",
        "tool" => "tool",
        _ => return None,
    };
    let meta = &item["data"];
    let (mode, text) = if delta {
        ("append", item["text"].as_str()?.to_owned())
    } else if let Some(tag) = e.data["activityStep"].as_str() {
        // A Tiangong step is a single native notification. Its end marker can
        // include a whole tool transcript, so expose only its current status.
        let text = match tag {
            "think" => item["text"].as_str().unwrap_or_default(),
            "file_write" | "file_read" => meta["path"].as_str().unwrap_or(tag),
            "tool_start" | "tool_input" => meta["tool_name"].as_str().unwrap_or("调用工具"),
            "tool_error" => "工具执行失败",
            _ => "工具执行结束",
        };
        ("entry", text.to_owned())
    } else if e.data["completed"] == true {
        let label = match category {
            "tool" => format!(
                "执行结束{}",
                meta["exitCode"]
                    .as_i64()
                    .map(|n| format!("（退出码 {n}）"))
                    .unwrap_or_default()
            ),
            "fileChange" => "文件变更结束".into(),
            _ => String::new(),
        };
        ("completed", label)
    } else {
        let label = match category {
            "tool" => meta["command"]
                .as_str()
                .or(meta["tool"].as_str())
                .or(meta["nativeType"].as_str())
                .unwrap_or("工具")
                .to_owned(),
            "fileChange" => meta["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["path"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        ("started", label)
    };
    Some(json!({"category":category,"mode":mode,"text":text,"itemId":e.item_id.as_deref()?}))
}

pub(super) fn event(mut e: RemoteEvent, live: bool) -> RemoteEvent {
    let detail = live.then(|| live_detail(&e)).flatten();
    let omit = match e.kind.as_str() {
        "item.updated" => e
            .data
            .get_mut("item")
            .is_some_and(|item| !visible_item(item)),
        "message.delta" => hidden(&e.data),
        _ => false,
    };
    if omit {
        e.kind = "cursor.advanced".into();
        e.item_id = None;
        e.data = json!({});
    }
    if let Some(detail) = detail {
        e.data["liveActivity"] = detail;
    }
    e
}

pub(super) fn event_with_display(
    e: RemoteEvent,
    live: bool,
    choices: crate::remote_network::DisplayContent,
) -> RemoteEvent {
    let mut e = event(e, live);
    if e.data["liveActivity"]["category"]
        .as_str()
        .is_some_and(|category| !choices.shows(category))
    {
        e.data.as_object_mut().unwrap().remove("liveActivity");
    }
    e.data["displayContent"] = json!(choices);
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_content_gates_fresh_activity_and_keeps_file_handles_and_approvals() {
        let packet = |kind: &str, data: Value| RemoteEvent {
            id: 9,
            kind: kind.into(),
            client_id: "codex:c".into(),
            session_id: "s".into(),
            turn_id: None,
            item_id: Some("i".into()),
            data,
        };
        let off = crate::remote_network::DisplayContent::default();
        for (kind, category) in [("reasoning", "reasoning"), ("tool", "tool"), ("fileChange", "fileChange")] {
            let frame = packet("message.delta", json!({"type":kind,"text":"new output"}));
            let hidden = event_with_display(frame.clone(), true, off);
            assert!(hidden.data.get("liveActivity").is_none());
            assert_eq!(hidden.data["displayContent"][category], false);
            let on = crate::remote_network::DisplayContent { reasoning:true,tool:true,file_change:true,artifact:true };
            let visible = event_with_display(frame.clone(), true, on);
            assert_eq!(visible.data["liveActivity"]["text"], "new output");
            assert_eq!(visible.data["displayContent"][category], true);
            assert!(event_with_display(frame, false, on).data.get("liveActivity").is_none());
        }
        let file = event_with_display(packet("item.updated", json!({"item":{"id":"i","type":"tool","text":"internal","files":[{"id":"manual-download"}]}})),true,off);
        assert_eq!(file.data["item"]["files"][0]["id"], "manual-download");
        let approval = event_with_display(packet("approval.requested", json!({"approval":{"id":"original"}})),true,off);
        assert_eq!(approval.data["approval"]["id"], "original");
    }
    #[test]
    fn snapshot_keeps_question_cursor_message_and_artifact_without_tool_payload() {
        let mut page = json!({"items":[{"id":"a","type":"reasoning","text":"private progress"},{"id":"t","role":"tool","text":"internal"},
            {"id":"f","type":"fileChange","data":{"diff":"patch"},"files":[{"id":"file"}]}, {"id":"m","type":"message","role":"user","text":"文件变更"}],
            "cursor":22,"nextCursor":"p","approvals":[{"id":"q"}]});
        history(&mut page);
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert_eq!(
            page["items"][0],
            json!({"id":"f","role":"assistant","type":"message","text":"","files":[{"id":"file"}]})
        );
        assert_eq!(page["items"][1]["text"], "文件变更");
        assert_eq!(
            (
                &page["cursor"],
                &page["nextCursor"],
                &page["approvals"][0]["id"]
            ),
            (&json!(22), &json!("p"), &json!("q"))
        );
    }
    #[test]
    fn opted_in_stream_projects_only_fresh_display_fields() {
        let packet = |data| RemoteEvent {
            id: 9,
            kind: "item.updated".into(),
            client_id: "codex:c".into(),
            session_id: "s".into(),
            item_id: Some("i".into()),
            turn_id: None,
            data,
        };
        for (kind, category) in [
            ("reasoning", "reasoning"),
            ("progress", "reasoning"),
            ("tool", "tool"),
            ("fileChange", "fileChange"),
        ] {
            let frame = packet(
                json!({"completed":true,"item":{"id":"i","type":kind,"text":"earlier output","files":[{"id":"artifact"}],"data":{"input":"private"}}}),
            );
            let fresh = event(frame.clone(), true);
            assert_eq!(fresh.data["liveActivity"]["category"], category);
            assert_eq!(fresh.data["liveActivity"]["itemId"], "i");
            assert_eq!(fresh.data["item"]["files"][0]["id"], "artifact");
            assert!(!fresh.data.to_string().contains("earlier output"));
            assert!(!fresh.data.to_string().contains("private"));
            assert!(event(frame, false).data.get("liveActivity").is_none());
        }
        let internal = event(
            packet(json!({"item":{"type":"internal","text":"private"}})),
            true,
        );
        assert_eq!(internal.kind, "cursor.advanced");
        assert_eq!(internal.data, json!({}));
    }
    #[test]
    fn hidden_stream_events_advance_cursor_but_do_not_hide_real_requests() {
        let packet = |kind: &str, data: Value| RemoteEvent {
            id: 3,
            kind: kind.into(),
            client_id: "c".into(),
            session_id: "s".into(),
            turn_id: None,
            item_id: Some("i".into()),
            data,
        };
        for e in [
            packet("message.delta", json!({"type":"tool","text":"internal"})),
            packet(
                "item.updated",
                json!({"item":{"type":"reasoning","text":"internal"}}),
            ),
        ] {
            let e = event(e, false);
            assert_eq!(
                (e.kind.as_str(), e.id, e.data),
                ("cursor.advanced", 3, json!({}))
            );
        }
        let e = event(
            packet(
                "approval.requested",
                json!({"approval":{"id":"q","type":"tool"}}),
            ),
            true,
        );
        assert_eq!(e.data["approval"]["id"], "q");
    }
}
