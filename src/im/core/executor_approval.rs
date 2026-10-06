//! Presentation shared by the native TianGong executor's IM approvals.
//! These choices approve or reject this request only; they are not policy changes.

#[derive(Debug, Clone)]
pub(crate) struct GmClawApproval {
    pub request_key: String,
    pub summary: String,
    pub message_id: Option<String>,
    pub legacy_code: String,
}

pub(crate) fn valid_request_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn decision_label(option_index: usize) -> Option<&'static str> {
    match option_index {
        1 => Some("批准全部"),
        2 => Some("拒绝全部"),
        _ => None,
    }
}

pub(crate) fn approval_text(approval: &GmClawApproval) -> String {
    format!(
        "天工 Claw 审批请求\n\n{}\n\n1. 批准本次全部工具\n2. 拒绝本次全部工具\n\n请回复 /1 或 /2；仅适用于本次请求，不会永久授权。",
        approval.summary
    )
}

pub(crate) fn legacy_approval_text(approval: &GmClawApproval) -> String {
    format!(
        "天工 Claw 审批卡片未发送成功，完整工具参数如下。可回复 /1 或 /2，也可使用兼容指令：\n\n{}\n\n批准本次全部工具：/tg approve {}\n拒绝本次全部工具：/tg reject {}\n\n仅适用于本次请求，不会永久授权。",
        approval.summary, approval.legacy_code, approval.legacy_code
    )
}

pub(crate) fn resolved_text(option_index: usize) -> Option<String> {
    decision_label(option_index)
        .map(|label| format!("天工 Claw：已选择{label}，提交处理；结果以随后回复为准。"))
}

/// Split on Unicode boundaries without trimming or dropping tool parameters.
pub(crate) fn text_chunks(value: &str, max_bytes: usize) -> Vec<&str> {
    assert!(max_bytes >= 4);
    let mut rest = value;
    let mut chunks = Vec::new();
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .find_map(|(index, character)| {
                (index + character.len_utf8() > max_bytes).then_some(index)
            })
            .unwrap_or(rest.len());
        chunks.push(&rest[..end]);
        rest = &rest[end..];
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_choices_are_strict_and_details_survive_fallback() {
        assert!(valid_request_key("fixture-key_123"));
        for value in ["", "key:other", "key\nother", "模型"] {
            assert!(!valid_request_key(value));
        }
        assert!(decision_label(0).is_none());
        assert!(decision_label(3).is_none());
        let approval = GmClawApproval {
            request_key: "fixture-key".into(),
            summary: "天工🔧\\\"\n".repeat(4000),
            message_id: None,
            legacy_code: "fixture-code".into(),
        };
        let fallback = legacy_approval_text(&approval);
        assert!(fallback.contains(&approval.summary));
        assert!(fallback.contains("/tg approve fixture-code"));
        let chunks = text_chunks(&fallback, 3500);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 3500));
        assert_eq!(chunks.concat(), fallback);
    }
}
