//! Phone requests end at the existing session router. There is no network proxy.
use super::envelope::Call;
use crate::{app_state::SharedState, remote_sessions};
use anyhow::{Context, Result, bail, ensure};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, Uri},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tower::ServiceExt;

#[derive(Default)]
pub(super) struct Streams(Mutex<HashMap<(String, String), Slot>>);
struct Slot {
    touched: Instant,
    stream: Arc<Mutex<Feed>>,
}
#[derive(Default)]
struct Feed {
    body: Option<axum::body::BodyDataStream>,
    cursor: u64,
    bytes: Vec<u8>,
}
impl Streams {
    pub async fn clear(&self) {
        self.0.lock().await.clear();
    }
    pub async fn revoke(&self, binding: &str) {
        self.0.lock().await.retain(|(b, _), _| b != binding);
    }
}
fn decode_segment(value: &str) -> Result<String> {
    let arg = format!("v={}", value.replace('+', "%2B"));
    let decoded = url::form_urlencoded::parse(arg.as_bytes())
        .next()
        .context("无效会话标识")?
        .1
        .into_owned();
    ensure!(
        !decoded.is_empty()
            && !decoded.contains(['\0', '/', '\\'])
            && decoded != "."
            && decoded != "..",
        "无效会话标识"
    );
    Ok(decoded)
}
pub(super) async fn execute(state: &SharedState, binding: &str, call: Call) -> Result<Value> {
    ensure!(
        call.path.len() <= 8192 && call.path.starts_with("/clients") && !call.path.contains('#'),
        "无效的远程会话路径"
    );
    let uri: Uri = call.path.parse()?;
    ensure!(
        uri.authority().is_none() && uri.scheme().is_none(),
        "仅支持本机会话"
    );
    let parts: Vec<_> = uri.path().trim_start_matches('/').split('/').collect();
    let method: Method = call.method.parse()?;
    if method == Method::GET
        && let ["clients", client, "sessions", session, "files", file] = parts.as_slice()
    {
        let chunk = remote_sessions::mobile_download(
            state,
            &decode_segment(client)?,
            &decode_segment(session)?,
            &decode_segment(file)?,
            call.offset,
        )
        .await?;
        return Ok(json!({"status":200,"body":chunk}));
    }
    let events = matches!(parts.as_slice(), ["clients", _, "sessions", _, "events"]);
    let allowed = match method {
        Method::GET => matches!(
            parts.as_slice(),
            ["clients"]
                | ["clients", _, "models" | "projects" | "sessions"]
                | ["clients", _, "sessions", _, "history" | "events"]
        ),
        Method::POST => matches!(
            parts.as_slice(),
            ["clients", _, "sessions"]
                | ["clients", _, "sessions", _, "turns" | "stop"]
                | ["clients", _, "sessions", _, "approvals", _]
        ),
        _ => false,
    };
    ensure!(allowed, "该操作不属于远程会话");
    if events {
        let after = url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
            .find(|(k, _)| k == "after")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0);
        return read_events(state, binding, &uri, after).await;
    }
    let body = if method == Method::POST {
        Body::from(serde_json::to_vec(&call.body)?)
    } else {
        Body::empty()
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)?;
    let response = remote_sessions::router()
        .with_state(state.clone())
        .oneshot(request)
        .await?;
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 3 * 1024 * 1024).await?;
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"error":{"message":"无法读取目标会话，请检查客户端状态"}}));
    Ok(json!({"status":status,"body":body}))
}

async fn read_events(state: &SharedState, binding: &str, uri: &Uri, after: u64) -> Result<Value> {
    let feeds = &state.remote_network.mobile.streams;
    let key = (binding.to_owned(), uri.path().to_owned());
    let feed = {
        let mut map = feeds.0.lock().await;
        map.retain(|_, slot| slot.touched.elapsed() < Duration::from_secs(60));
        if map.len() >= 64 && !map.contains_key(&key) {
            bail!("正在查看的实时会话较多，请稍后重试")
        }
        let slot = map.entry(key).or_insert_with(|| Slot {
            touched: Instant::now(),
            stream: Default::default(),
        });
        slot.touched = Instant::now();
        slot.stream.clone()
    };
    let mut feed = feed.lock().await;
    if feed.body.is_none() || feed.cursor != after {
        let request = Request::get(uri.clone()).body(Body::empty())?;
        let response = remote_sessions::router()
            .with_state(state.clone())
            .oneshot(request)
            .await?;
        ensure!(response.status().is_success(), "目标会话暂时无法订阅");
        *feed = Feed {
            body: Some(response.into_body().into_data_stream()),
            cursor: after,
            bytes: vec![],
        };
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let mut out = vec![];
    let mut size = 0;
    loop {
        while let Some(end) = feed.bytes.windows(2).position(|x| x == b"\n\n") {
            let frame: Vec<_> = feed.bytes.drain(..end + 2).collect();
            let data = std::str::from_utf8(&frame)?
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() {
                let event: Value = serde_json::from_str(&data)?;
                let id = event["id"].as_u64().context("事件缺少游标")?;
                feed.cursor = id;
                size += data.len();
                out.push(event);
                if size >= 384 * 1024 || out.len() >= 48 {
                    break;
                }
            }
        }
        if !out.is_empty() {
            break;
        }
        let next =
            tokio::time::timeout_at(deadline, feed.body.as_mut().context("订阅已关闭")?.next())
                .await;
        match next {
            Ok(Some(Ok(chunk))) => {
                ensure!(
                    feed.bytes.len() + chunk.len() < 3 * 1024 * 1024,
                    "事件超过手机接收大小"
                );
                feed.bytes.extend_from_slice(&chunk);
            }
            Ok(_) => {
                feed.body = None;
                bail!("订阅断开，请重新读取当前会话")
            }
            Err(_) => break,
        }
    }
    Ok(json!({"status":200,"body":{"events":out}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cannot_address_management_other_hosts_or_unregistered_files() {
        let (_dir, state) = super::super::super::tests::fixture();
        for (method, path) in [
            ("POST", "/api/remote-network/settings"),
            ("GET", "//example.invalid/clients"),
            ("GET", "/clients/codex%3At/sessions/abc/files/absent"),
            ("GET", "/clients/..%2Fconfig/sessions/x/files/bad"),
            ("POST", "/clients/t/sessions/x/files"),
        ] {
            let call = Call {
                issued_at: 1,
                method: method.into(),
                path: path.into(),
                body: Value::Null,
                offset: 0,
            };
            assert!(execute(&state, "paired", call).await.is_err(), "{path}");
        }
    }
}
