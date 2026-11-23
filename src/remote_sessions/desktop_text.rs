//! Readable projection of desktop-generated user messages for remote clients.
use serde::Deserialize;

pub(super) fn user_message(text: String) -> String {
    annotations(&text)
        .or_else(|| question_reply(&text))
        .unwrap_or(text)
}

#[derive(Deserialize)]
struct Selection {
    text: String,
    #[serde(default, alias = "userComment")]
    comment: Option<String>,
}

fn annotations(text: &str) -> Option<String> {
    let (heading, rest) = text.trim_start().split_once('\n')?;
    if heading.trim_end() != "# Response annotations:" {
        return None;
    }
    let (intro, payload) = rest.split_once("<response-annotations>")?;
    if !intro.trim().is_empty()
        && !intro
            .trim_start()
            .starts_with("Each item contains text selected from an earlier Codex response")
    {
        return None;
    }
    // Use the JSON boundary; quoted passages may themselves describe the closing tag.
    let mut values = serde_json::Deserializer::from_str(payload).into_iter::<Vec<Selection>>();
    let rows = values.next()?.ok()?;
    let rest = payload[values.byte_offset()..]
        .trim_start()
        .strip_prefix("</response-annotations>")?
        .trim_start();
    let request = if rest.is_empty() {
        ""
    } else {
        let body = rest.strip_prefix("## My request:")?;
        if !body.is_empty() && !body.starts_with(['\r', '\n']) {
            return None;
        }
        body.trim_start_matches(['\r', '\n'])
    };
    let mut blocks = Vec::new();
    for row in rows {
        if !row.text.trim().is_empty() {
            blocks.push(format!("引用「{}」", row.text.trim()));
        }
        if let Some(comment) = row.comment.filter(|s| !s.trim().is_empty()) {
            blocks.push(comment);
        }
    }
    if !request.is_empty() {
        blocks.push(request.to_owned());
    }
    (!blocks.is_empty()).then(|| blocks.join("\n\n"))
}

#[derive(Deserialize)]
struct Reply {
    question: String,
    answer: String,
}

fn question_reply(text: &str) -> Option<String> {
    let body = text
        .trim()
        .strip_prefix("<send_user_message_question_reply>")?
        .strip_suffix("</send_user_message_question_reply>")?;
    let replies: Vec<Reply> = serde_json::from_str(body).ok()?;
    if replies.is_empty() || replies.iter().any(|r| r.question.trim().is_empty()) {
        return None;
    }
    Some(
        replies
            .into_iter()
            .map(|r| format!("{}\n你的回答：{}", r.question, r.answer))
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn displays_selected_text_comments_and_actual_request() {
        let input = "# Response annotations:\r\nEach item contains text selected from an earlier Codex response and may include a user comment.\r\n<response-annotations>\r\n[{\"text\":\"开关\",\"source\":{\"messageId\":\"private-id\",\"startOffset\":16}},{\"text\":\"</response-annotations>\",\"comment\":\"保留此引用\"}]\r\n</response-annotations>\r\n\r\n## My request:\r\n那三项开关还要加一项产物";
        assert_eq!(
            user_message(input.into()),
            "引用「开关」\n\n引用「</response-annotations>」\n\n保留此引用\n\n那三项开关还要加一项产物"
        );
    }
    #[test]
    fn displays_question_and_answer_without_desktop_routing_metadata() {
        let raw = r#"<send_user_message_question_reply>
[{"questionItemId":"opaque","question":"是否已经注册？","answer":"暂时还没有注册"}]
</send_user_message_question_reply>"#;
        assert_eq!(
            user_message(raw.into()),
            "是否已经注册？\n你的回答：暂时还没有注册"
        );
    }
    #[test]
    fn retains_examples_and_unrecognized_or_broken_messages_verbatim() {
        for raw in [
            "说明中有 <response-annotations> 标记",
            "# Response annotations:\n<response-annotations>\n[{broken",
            "普通用户回答：拒绝",
            "```xml\n<send_user_message_question_reply>[]</send_user_message_question_reply>\n```",
        ] {
            assert_eq!(user_message(raw.into()), raw);
        }
    }
}
