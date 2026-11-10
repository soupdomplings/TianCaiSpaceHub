//! Adapt an actual unary Chat result to the downstream streaming contract.
//! Used only when the source model explicitly disables upstream streaming.
use crate::ai_gateway::error::GatewayError;
use axum::body::Bytes;
use serde_json::{Value, json};
pub(in crate::ai_gateway) fn chunks(value: &Value) -> Result<Vec<Bytes>, GatewayError> {
    let choices = value["choices"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| GatewayError::bad_request("上游没有返回有效的 Chat 结果"))?;
    let mut delta = Vec::new();
    let mut finished = Vec::new();
    for (i, choice) in choices.iter().enumerate() {
        let mut message = choice["message"].clone();
        if !message.is_object() {
            return Err(GatewayError::bad_request("上游 Chat 消息格式不兼容"));
        }
        if let Some(tools) = message["tool_calls"].as_array_mut() {
            for (i, tool) in tools.iter_mut().enumerate() {
                tool["index"] = json!(i);
            }
        }
        let index = choice.get("index").cloned().unwrap_or(json!(i));
        delta.push(json!({"index":index,"delta":message,"finish_reason":null}));
        finished.push(json!({"index":index,"delta":{},"finish_reason":choice["finish_reason"]}));
    }
    let payload = |choices, usage| json!({"id":value["id"],"object":"chat.completion.chunk","created":value["created"],"model":value["model"],"choices":choices,"usage":usage});
    Ok(vec![
        Bytes::from(format!("data: {}\n\n", payload(delta, Value::Null))),
        Bytes::from(format!(
            "data: {}\n\n",
            payload(finished, value["usage"].clone())
        )),
        Bytes::from_static(b"data: [DONE]\n\n"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    #[tokio::test]
    async fn tiangong_unary_to_stream_retains_tools_arguments_usage_and_finish_reason() {
        let upstream = json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call-a","type":"function","function":{"name":"run","arguments":"{\"path\":\"test.txt\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":120,"completion_tokens":9,"total_tokens":129,"prompt_tokens_details":{"cached_tokens":40}}});
        let stream_chunks = chunks(&upstream).unwrap();
        let first: Value = serde_json::from_str(
            std::str::from_utf8(&stream_chunks[0])
                .unwrap()
                .trim()
                .trim_start_matches("data:")
                .trim(),
        )
        .unwrap();
        assert_eq!(first["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
        let events = crate::ai_gateway::transform::responses_stream::ChatSseToResponsesSse::new(
            futures_util::stream::iter(stream_chunks.into_iter().map(Ok::<_, std::io::Error>)),
            "local-alias".into(),
        )
        .collect::<Vec<_>>()
        .await;
        let text = events
            .into_iter()
            .map(|v| String::from_utf8(v.unwrap().to_vec()).unwrap())
            .collect::<String>();
        assert!(text.contains("response.completed"));
        assert!(text.contains("call-a"));
        assert!(text.contains("test.txt"));
        assert!(text.contains("cached_tokens\":40"));
        assert!(text.contains("input_tokens\":120"));
        assert!(chunks(&json!({"error":{"message":"bad"}})).is_err());
    }
}
