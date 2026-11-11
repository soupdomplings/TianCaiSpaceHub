//! Small CDP transport shared by explicit renderer adapters. Only notifications
//! actually consumed by those adapters survive command waits.
use std::{collections::VecDeque, time::Duration};

use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use url::Url;

pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(8);

fn workspace_event(value: &Value) -> bool {
    // Both ZCode and TianGong consume only paused script responses. Enabling
    // Network also emits notifications for every preload and resource chunk;
    // none of those are used, and retaining them can fill the queue before a
    // reload or evaluation replies. A paused response must keep its full
    // payload and FIFO position so the adapter can fulfill or release it.
    value["method"] == "Fetch.requestPaused"
}

pub(crate) struct Cdp {
    pub(crate) socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u32,
    events: VecDeque<Value>,
    label: &'static str,
}
impl Cdp {
    pub(crate) async fn connect(socket: &Url, label: &'static str) -> Result<Self> {
        let config = WebSocketConfig {
            max_message_size: Some(MAX_BYTES),
            max_frame_size: Some(MAX_BYTES),
            ..WebSocketConfig::default()
        };
        let (socket, _) = connect_async_with_config(socket.as_str(), Some(config), false)
            .await
            .map_err(|_| anyhow::anyhow!("{label}增强启动通道未启用，请正常退出后从 Hub 启动"))?;
        Ok(Self {
            socket,
            next_id: 0,
            events: VecDeque::new(),
            label,
        })
    }
    async fn receive(&mut self) -> Result<Value> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text)
                        .map_err(|_| anyhow::anyhow!("{}增强启动通信格式不兼容", self.label));
                }
                Some(Ok(Message::Ping(bytes))) => {
                    self.socket.send(Message::Pong(bytes)).await?;
                }
                Some(Ok(Message::Close(_))) | None => {
                    anyhow::bail!("{}增强启动连接已关闭", self.label)
                }
                Some(Err(_)) => anyhow::bail!("{}增强启动通信中断", self.label),
                _ => {}
            }
        }
    }
    pub(crate) async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let request = json!({"id":id,"method":method,"params":params}).to_string();
        ensure!(
            request.len() <= MAX_BYTES,
            "{}增强启动请求超过支持大小",
            self.label
        );
        tokio::time::timeout(CALL_TIMEOUT, async {
            self.socket.send(Message::Text(request)).await?;
            loop {
                let value = self.receive().await?;
                if value["id"].as_u64() == Some(id as u64) {
                    ensure!(
                        value.get("error").is_none(),
                        "{}增强启动协议暂不兼容",
                        self.label
                    );
                    return Ok(value["result"].clone());
                }
                if workspace_event(&value) {
                    ensure!(
                        self.events.len() < 256,
                        "{}增强启动事件过多，请稍后重试",
                        self.label
                    );
                    self.events.push_back(value);
                }
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("{}增强启动通信超时", self.label))?
    }
    pub(crate) async fn event(&mut self) -> Result<Value> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        loop {
            let value = self.receive().await?;
            if workspace_event(&value) {
                return Ok(value);
            }
        }
    }
    pub(crate) async fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"silent":true}),
            )
            .await?;
        ensure!(
            result.get("exceptionDetails").is_none(),
            "{}增强启动界面脚本未完成",
            self.label
        );
        result["result"]
            .get("value")
            .cloned()
            .with_context(|| format!("{}增强启动界面响应缺失", self.label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task::JoinHandle;

    async fn read_request(
        socket: &mut WebSocketStream<TcpStream>,
        method: &str,
        request_id: Option<&str>,
    ) -> Value {
        let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
            panic!("expected a CDP request");
        };
        let request: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(request["method"], method);
        if let Some(id) = request_id {
            assert_eq!(request["params"]["requestId"], id);
        }
        request
    }

    async fn notifications(
        socket: &mut WebSocketStream<TcpStream>,
        count: usize,
        pauses: &[(usize, &str)],
    ) {
        for index in 0..count {
            let method = [
                "Network.requestWillBeSent",
                "Network.responseReceived",
                "Network.dataReceived",
                "Network.loadingFinished",
                "Page.domContentEventFired",
                "Runtime.consoleAPICalled",
            ][index % 6];
            socket
                .send(Message::Text(
                    json!({"method":method,"params":{"sequence":index}}).to_string(),
                ))
                .await
                .unwrap();
            for (_, id) in pauses.iter().filter(|(at, _)| *at == index) {
                socket
                    .send(Message::Text(
                        json!({"method":"Fetch.requestPaused","params":{
                            "requestId":id,"responseStatusCode":200,
                            "responseHeaders":[{"name":"Content-Type","value":"text/javascript"}],
                            "request":{"url":format!("file:///fixture/{id}.js")}
                        }})
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            }
        }
    }

    async fn reply(socket: &mut WebSocketStream<TcpStream>, request: &Value, result: Value) {
        socket
            .send(Message::Text(
                json!({"id":request["id"],"result":result}).to_string(),
            ))
            .await
            .unwrap();
    }

    async fn flood_fixture(label: &'static str) -> (Cdp, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request = read_request(&mut socket, "Page.reload", None).await;
            notifications(&mut socket, 768, &[(31, "first"), (511, "second")]).await;
            reply(&mut socket, &request, json!({"reloaded":true})).await;

            let request = read_request(&mut socket, "Fetch.getResponseBody", Some("first")).await;
            notifications(&mut socket, 512, &[(100, "third")]).await;
            reply(
                &mut socket,
                &request,
                json!({"body":"fixture script","base64Encoded":false}),
            )
            .await;
            let request = read_request(&mut socket, "Fetch.fulfillRequest", Some("first")).await;
            assert_eq!(request["params"]["responseCode"], 200);
            reply(&mut socket, &request, json!({})).await;

            for id in ["second", "third"] {
                let request = read_request(&mut socket, "Fetch.continueRequest", Some(id)).await;
                reply(&mut socket, &request, json!({})).await;
            }
            // This flood is consumed by event(), rather than by call().
            notifications(&mut socket, 512, &[(511, "direct")]).await;
            let request = read_request(&mut socket, "Fetch.continueRequest", Some("direct")).await;
            reply(&mut socket, &request, json!({})).await;
        });
        (
            Cdp::connect(&Url::parse(&format!("ws://{address}")).unwrap(), label)
                .await
                .unwrap(),
            server,
        )
    }

    #[tokio::test]
    async fn noisy_workspace_notifications_preserve_fetch_pauses_across_calls_and_events() {
        for label in ["ZCode", "天工"] {
            let (mut cdp, server) = flood_fixture(label).await;
            assert_eq!(
                cdp.call("Page.reload", json!({"ignoreCache":true}))
                    .await
                    .unwrap(),
                json!({"reloaded":true})
            );
            assert_eq!(cdp.events.len(), 2, "noise must not occupy the pause queue");
            let first = cdp.event().await.unwrap();
            assert_eq!(first["method"], "Fetch.requestPaused");
            assert_eq!(first["params"]["requestId"], "first");
            assert_eq!(
                first["params"]["responseHeaders"],
                json!([{"name":"Content-Type","value":"text/javascript"}])
            );
            assert_eq!(
                first["params"]["request"]["url"],
                "file:///fixture/first.js"
            );
            let body = cdp
                .call("Fetch.getResponseBody", json!({"requestId":"first"}))
                .await
                .unwrap();
            assert_eq!(body["body"], "fixture script");
            assert_eq!(cdp.events.len(), 2);
            cdp.call(
                "Fetch.fulfillRequest",
                json!({"requestId":"first","responseCode":200,"body":""}),
            )
            .await
            .unwrap();
            for id in ["second", "third", "direct"] {
                let event = tokio::time::timeout(Duration::from_secs(3), cdp.event())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(event["method"], "Fetch.requestPaused");
                assert_eq!(event["params"]["requestId"], id);
                cdp.call("Fetch.continueRequest", json!({"requestId":id}))
                    .await
                    .unwrap();
            }
            assert!(cdp.events.is_empty());
            server.await.unwrap();
        }
    }
}
