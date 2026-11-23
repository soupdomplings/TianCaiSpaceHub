use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::Duration,
};
use uuid::Uuid;

pub(super) type Emit<'a> = dyn Fn(&Value) -> Result<()> + 'a;

pub(super) fn supports_form(protocol: &str, capabilities: &Value) -> bool {
    matches!(protocol, "2025-06-18" | "2025-11-25")
        && capabilities["elicitation"]
            .as_object()
            .is_some_and(|e| e.is_empty() || e.get("form").is_some_and(Value::is_object))
}

#[derive(Default)]
struct State {
    cancelled: bool,
    waiting: Option<(String, mpsc::Sender<Value>)>,
    queued: Option<PathBuf>,
}

/// Owned by one incoming tool call on one stdio connection.
#[derive(Default)]
pub(super) struct Request(Mutex<State>);

impl Request {
    pub(super) fn cancelled(&self) -> bool {
        self.0.lock().unwrap().cancelled
    }

    pub(super) fn cancel(&self) {
        let mut state = self.0.lock().unwrap();
        state.cancelled = true;
        state.waiting.take();
        if let Some(path) = state.queued.take() {
            let _ = fs::remove_file(path);
        }
    }

    pub(super) fn publish(&self, path: &Path, ready: tempfile::NamedTempFile) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        ensure!(!state.cancelled, "巡检已取消。");
        ready.persist_noclobber(path).map_err(|e| e.error)?;
        state.queued = Some(path.to_owned());
        Ok(())
    }

    /// Unrelated, repeated and late responses cannot resolve another form.
    pub(super) fn receive(&self, packet: &Value) {
        let mut state = self.0.lock().unwrap();
        if state
            .waiting
            .as_ref()
            .is_some_and(|(id, _)| packet["id"].as_str() == Some(id))
        {
            let (_, sender) = state.waiting.take().unwrap();
            let _ = sender.send(packet.clone());
        }
    }

    pub(super) fn confirm(
        &self,
        message: String,
        timeout: Duration,
        emit: &Emit<'_>,
    ) -> Result<()> {
        let id = format!("xshellb:{}", Uuid::new_v4());
        let (sender, receiver) = mpsc::channel();
        {
            let mut state = self.0.lock().unwrap();
            ensure!(!state.cancelled, "巡检已取消。");
            state.waiting = Some((id.clone(), sender));
        }
        // Omitting mode is form mode in both the June and November revisions.
        // The client, not the model, submits this form over the stdio connection.
        emit(
            &json!({"jsonrpc":"2.0","id":id,"method":"elicitation/create","params":{
                "message":message,
                "requestedSchema":{"type":"object","properties":{"allow":{"type":"boolean","title":"允许这一次巡检"}},"required":["allow"]}
            }}),
        )?;
        let reply = receiver.recv_timeout(timeout);
        self.0.lock().unwrap().waiting.take();
        let reply = match reply {
            Ok(reply) => reply,
            Err(why) => {
                let _ = emit(
                    &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}),
                );
                bail!(match why {
                    mpsc::RecvTimeoutError::Timeout => "等待客户端确认超时，命令未发送。",
                    mpsc::RecvTimeoutError::Disconnected => "客户端已取消巡检，命令未发送。",
                });
            }
        };
        ensure!(
            reply["jsonrpc"] == "2.0"
                && reply.get("error").is_none()
                && reply.get("method").is_none(),
            "客户端未能完成确认，命令未发送。"
        );
        match reply["result"]["action"].as_str() {
            Some("accept") if reply["result"]["content"]["allow"] == true => Ok(()),
            Some("decline" | "cancel" | "accept") => bail!("未同意这次巡检，命令未发送。"),
            _ => bail!("客户端没有返回有效确认，命令未发送。"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{now_ms, read_json, serve_transport};
    use super::*;
    use std::io::{self, BufReader, Write};
    use std::thread;
    use std::time::Instant;

    #[test]
    fn negotiate_only_declared_form_support() {
        assert!(supports_form("2025-06-18", &json!({"elicitation":{}})));
        assert!(supports_form(
            "2025-11-25",
            &json!({"elicitation":{"form":{}}})
        ));
        for caps in [
            json!({}),
            json!({"elicitation":false}),
            json!({"elicitation":{"url":{}}}),
        ] {
            assert!(!supports_form("2025-11-25", &caps));
        }
        assert!(!supports_form("2025-03-26", &json!({"elicitation":{}})));
    }

    #[test]
    fn timeout_withdraws_the_form_and_late_consent_cannot_authorize_another_request() {
        let task = Request::default();
        let first = Mutex::new(Value::Null);
        let cancelled = Mutex::new(Value::Null);
        assert!(
            task.confirm("one".into(), Duration::ZERO, &|v| {
                if v["method"] == "notifications/cancelled" {
                    *cancelled.lock().unwrap() = v["params"]["requestId"].clone();
                } else {
                    *first.lock().unwrap() = v["id"].clone();
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(*first.lock().unwrap(), *cancelled.lock().unwrap());
        let err = task.confirm("two".into(), Duration::from_secs(1), &|v| {
            task.receive(&json!({"jsonrpc":"2.0","id":*first.lock().unwrap(),"result":{"action":"accept","content":{"allow":true}}}));
            task.receive(&json!({"jsonrpc":"2.0","id":v["id"],"result":{"action":"decline"}}));
            Ok(())
        }).unwrap_err();
        assert!(err.to_string().contains("未同意"));
    }

    struct Frames(Vec<u8>, mpsc::Sender<Value>);
    impl Write for Frames {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            let frame = serde_json::from_slice(&self.0)?;
            self.0.clear();
            self.1.send(frame).map_err(io::Error::other)
        }
    }

    #[test]
    fn stdio_client_must_accept_the_exact_check_and_eof_withdraws_queued_work() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_owned();
        let session = Uuid::new_v4().to_string();
        let dir = root.join("sessions").join(&session);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("state.json"), json!({"connected":true,"updated":now_ms(),"clientConfirmation":true,"host":"fixture","port":22}).to_string()).unwrap();
        let (reader, mut writer) = io::pipe().unwrap();
        let (out, frames) = mpsc::channel();
        let server = thread::spawn(move || {
            serve_transport(root, BufReader::new(reader), Frames(vec![], out))
        });
        let recv = || frames.recv_timeout(Duration::from_secs(3)).unwrap();
        let call = |id: i64| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"xshellb_run_check","arguments":{"sessionId":session,"check":"directory","clientApproved":true}}});
        writeln!(&mut writer, "{}", json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"elicitation":{"form":{}}}}})).unwrap();
        assert_eq!(recv()["result"]["protocolVersion"], "2025-11-25");
        // Tool input claims approval on every turn. None of these replies is consent.
        for (index, reply) in [
            json!({"result":{"action":"decline"}}),
            json!({"result":{"action":"cancel"}}),
            json!({"result":{"action":"accept","content":{"allow":false}}}),
            json!({"result":{"action":"accept"}}),
            json!({"error":{"code":-32603,"message":"no UI"}}),
        ]
        .into_iter()
        .enumerate()
        {
            let parent = index as i64 + 2;
            writeln!(&mut writer, "{}", call(parent)).unwrap();
            let form = recv();
            assert_eq!(form["method"], "elicitation/create");
            assert!(
                ["fixture", "22", "pwd", &session]
                    .iter()
                    .all(|s| form["params"]["message"].as_str().unwrap().contains(*s))
            );
            let mut r = json!({"jsonrpc":"2.0","id":form["id"]});
            r.as_object_mut()
                .unwrap()
                .extend(reply.as_object().unwrap().clone());
            writeln!(&mut writer, "{r}").unwrap();
            let denied = recv();
            assert_eq!(denied["id"], parent);
            assert_eq!(denied["result"]["isError"], true);
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        }
        writeln!(&mut writer, "{}", call(9)).unwrap();
        let form = recv();
        let accept = |id: &Value| json!({"jsonrpc":"2.0","id":id,"result":{"action":"accept","content":{"allow":true}}});
        writeln!(&mut writer, "{}", accept(&json!("wrong-id"))).unwrap();
        writeln!(
            &mut writer,
            "{}",
            json!({"jsonrpc":"2.0","id":10,"method":"ping"})
        )
        .unwrap();
        assert_eq!(recv()["id"], 10);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        writeln!(&mut writer, "{}", accept(&form["id"])).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let data = loop {
            if let Some(file) = fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .find(|e| e.file_name().to_string_lossy().starts_with("request-"))
            {
                break read_json(&file.path()).unwrap();
            }
            assert!(Instant::now() < deadline, "missing mailbox request");
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(data["operation"], "directory");
        assert_eq!(data["clientForm"], true);
        assert_eq!(data["clientApproved"], true);
        drop(writer);
        server.join().unwrap().unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }
}
