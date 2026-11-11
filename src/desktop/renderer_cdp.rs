//! Small event-preserving CDP transport shared by explicit renderer adapters.
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
                if value["method"].is_string() {
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
            if value["method"].is_string() {
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
