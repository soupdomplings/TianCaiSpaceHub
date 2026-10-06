use serde_json::Value as JsonValue;

use crate::im::core::executor_approval::{GmClawApproval, decision_label};
use crate::im::core::i18n::ImText;
use crate::im_runtime::ApprovalDecisionOption;

use super::APPROVAL_CARD_TEMPLATE;
use super::common::build_markdown_card;
use super::markdown::normalize_card_markdown;

pub fn build_approval_card(
    kind_label: &str,
    summary: &str,
    decisions: &[ApprovalDecisionOption],
    request_key: &str,
    text: ImText,
) -> serde_json::Value {
    build_approval_card_with_callback(
        kind_label,
        summary,
        decisions,
        request_key,
        "codex_approval_decision",
        text,
    )
}

pub(crate) fn build_gmclaw_approval_card(
    approval: &GmClawApproval,
    text: ImText,
) -> serde_json::Value {
    let decisions = [
        ApprovalDecisionOption {
            label: "批准全部".into(),
            decision: serde_json::json!("accept"),
        },
        ApprovalDecisionOption {
            label: "拒绝全部".into(),
            decision: serde_json::json!("decline"),
        },
    ];
    build_approval_card_with_callback(
        "天工 Claw · 仅本次工具请求",
        &approval.summary,
        &decisions,
        &approval.request_key,
        "gmclaw_approval_decision",
        text,
    )
}

pub(crate) fn build_resolved_gmclaw_approval_card(
    approval: &GmClawApproval,
    option_index: usize,
    text: ImText,
) -> Option<serde_json::Value> {
    let label = decision_label(option_index)?;
    let mut card = build_resolved_approval_card(
        "天工 Claw · 仅本次工具请求",
        &approval.summary,
        &format!("已选择{label}，提交处理；结果以随后回复为准"),
        option_index,
        text,
    );
    card["body"]["elements"][1]["content"] = serde_json::json!(approval.summary);
    if card.to_string().len() > 24 * 1024 {
        card["body"]["elements"][1]["content"] =
            serde_json::json!("完整工具参数见本请求此前发送的消息，仅适用于本次请求。");
    }
    Some(card)
}

fn build_approval_card_with_callback(
    kind_label: &str,
    summary: &str,
    decisions: &[ApprovalDecisionOption],
    request_key: &str,
    callback_kind: &str,
    text: ImText,
) -> serde_json::Value {
    let content = if callback_kind == "gmclaw_approval_decision" {
        summary.to_owned()
    } else {
        normalize_card_markdown(summary)
    };
    let mut elements = vec![
        {
            serde_json::json!({
                "tag": "markdown",
                "content": format!(
                    "**{}: `{}`**",
                    normalize_card_markdown(text.approval_request_heading()),
                    normalize_card_markdown(kind_label)
                )
            })
        },
        {
            serde_json::json!({
                "tag": "markdown",
                "content": content
            })
        },
    ];
    if !decisions.is_empty() {
        elements.push(serde_json::json!({
            "tag": "hr"
        }));
        elements.push(build_approval_button_row(
            decisions,
            request_key,
            callback_kind,
        ));
    }
    let mut card = build_markdown_card(
        "",
        Some(text.approval_pending_title()),
        Some(APPROVAL_CARD_TEMPLATE),
    );
    card["body"]["padding"] = serde_json::json!("8px 8px 8px 8px");
    card["body"]["vertical_spacing"] = serde_json::json!("8px");
    card["body"]["elements"] = serde_json::Value::Array(elements);
    card
}

pub fn build_resolved_approval_card(
    kind_label: &str,
    summary: &str,
    decision_label: &str,
    option_index: usize,
    text: ImText,
) -> serde_json::Value {
    let content = normalize_card_markdown(summary);
    let selected = normalize_card_markdown(decision_label.trim());
    let elements = vec![
        serde_json::json!({
            "tag": "markdown",
            "content": format!(
                "**{}: `{}`**",
                normalize_card_markdown(text.approval_request_heading()),
                normalize_card_markdown(kind_label)
            )
        }),
        serde_json::json!({
            "tag": "markdown",
            "content": content
        }),
        serde_json::json!({
            "tag": "hr"
        }),
        serde_json::json!({
            "tag": "markdown",
            "content": format!(
                "**{}**",
                normalize_card_markdown(&text.approval_selected_label(option_index, &selected))
            )
        }),
    ];
    let mut card = build_markdown_card("", Some(text.approval_resolved_title()), Some("green"));
    card["body"]["padding"] = serde_json::json!("8px 8px 8px 8px");
    card["body"]["vertical_spacing"] = serde_json::json!("8px");
    card["body"]["elements"] = serde_json::Value::Array(elements);
    card
}

fn build_approval_button_row(
    decisions: &[ApprovalDecisionOption],
    request_key: &str,
    callback_kind: &str,
) -> serde_json::Value {
    let columns = decisions
        .iter()
        .enumerate()
        .map(|(index, decision)| {
            let option_index = index + 1;
            let primary = index == 0 && !decision_is_negative(&decision.decision);
            serde_json::json!({
                "tag": "column",
                "width": "auto",
                "padding": "0px 0px 0px 0px",
                "vertical_spacing": "0px",
                "elements": [
                    {
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": decision.label.trim()
                        },
                        "type": if primary { "primary_filled" } else { "default" },
                        "width": "default",
                        "behaviors": [
                            {
                                "type": "callback",
                                "value": {
                                    "kind": callback_kind,
                                    "option": option_index,
                                    "requestKey": request_key
                                }
                            }
                        ]
                    }
                ]
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "tag": "column_set",
        "flex_mode": "flow",
        "horizontal_spacing": "8px",
        "horizontal_align": "left",
        "columns": columns
    })
}

fn decision_is_negative(decision: &JsonValue) -> bool {
    decision
        .as_str()
        .is_some_and(|value| matches!(value, "decline" | "cancel" | "denied"))
        || decision
            .get("applyNetworkPolicyAmendment")
            .and_then(|value| value.get("network_policy_amendment"))
            .and_then(|value| value.get("action"))
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "deny")
}

#[cfg(test)]
mod gmclaw_tests {
    use super::*;

    #[test]
    fn native_approval_preserves_details_and_has_separate_current_request_buttons() {
        let approval = GmClawApproval {
            request_key: "fixture-key".into(),
            summary: "工具参数：```json\n{\"content\": \"![原值](unchanged)\"}\n```".into(),
            message_id: None,
            legacy_code: "fixture-code".into(),
        };
        let card = build_gmclaw_approval_card(&approval, ImText::zh_cn());
        assert_eq!(
            card.pointer("/body/elements/1/content")
                .and_then(JsonValue::as_str),
            Some(approval.summary.as_str())
        );
        let encoded = card.to_string();
        assert_eq!(encoded.matches("gmclaw_approval_decision").count(), 2);
        assert!(!encoded.contains("codex_approval_decision"));
        assert!(!encoded.contains("fixture-code"));
        let resolved = build_resolved_gmclaw_approval_card(&approval, 2, ImText::zh_cn())
            .unwrap()
            .to_string();
        assert!(!resolved.contains("behaviors"));
        assert!(resolved.contains("结果以随后回复为准"));
        assert!(build_resolved_gmclaw_approval_card(&approval, 0, ImText::zh_cn()).is_none());
    }
}
