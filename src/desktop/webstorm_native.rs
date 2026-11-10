//! IDE-side bridge owns PasswordSafe. Rust never writes a credential XML file.
use super::{
    models::Binding,
    targets::{self, ClientTarget},
    transaction,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeUndo {
    pub before: Value,
    pub after: Value,
}
#[derive(Deserialize)]
struct Registration {
    version: u32,
    port: u16,
    token: String,
    instance: String,
}
fn registration() -> Result<Registration> {
    let root = targets::home()?.join(".tiancaispace-hub/ide-bridge");
    transaction::reject_links(&root)?;
    let mut live = Vec::new();
    let started = std::time::Instant::now();
    if root.is_dir() {
        for entry in std::fs::read_dir(root)?.take(64) {
            ensure!(
                started.elapsed() < std::time::Duration::from_secs(4),
                "WebStorm 实例探测未完成，请关闭多余实例后重试"
            );
            let path = entry?.path();
            if !path
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("webstorm-"))
                || path.extension().is_none_or(|s| s != "json")
            {
                continue;
            }
            if let Some(raw) = transaction::read(&path)? {
                if let Ok(r) = serde_json::from_slice::<Registration>(&raw) {
                    if r.version == 1
                        && r.port > 0
                        && r.token.len() <= 200
                        && uuid::Uuid::parse_str(&r.instance).is_ok()
                        && request(&r, None).is_ok()
                    {
                        live.push(r);
                    }
                }
            }
        }
    }
    ensure!(
        live.len() <= 1,
        "发现多个 WebStorm 实例，请保留要配置的实例后重新读取"
    );
    live.pop()
        .context("请先在 WebStorm 安装 TianCaiSpaceHub Bridge，重新打开 IDE 中的项目后重试")
}
fn request(reg: &Registration, body: Option<&Value>) -> Result<Value> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_millis(if body.is_some() {
            30_000
        } else {
            700
        }))
        .build()?;
    let url = format!("http://127.0.0.1:{}/v1/state", reg.port);
    let request = if let Some(body) = body {
        client.post(url).json(body)
    } else {
        client.get(url)
    };
    let response = request
        .bearer_auth(&reg.token)
        .send()
        .map_err(|_| anyhow::anyhow!("IDE 未确认操作结果，请重新读取，勿重复保存"))?;
    ensure!(
        response.status().is_success(),
        "WebStorm 配置已变化或 PasswordSafe 未确认，请重新读取后再操作"
    );
    use std::io::Read;
    let mut bytes = Vec::new();
    response.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 256 * 1024, "IDE 返回内容过大");
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn base(target: &ClientTarget, port: u16) -> String {
    format!(
        "http://127.0.0.1:{port}/ai-gateway/clients/webstorm-native/{}/v1",
        target.id
    )
}
pub fn prepare(
    target: &ClientTarget,
    port: u16,
    bindings: &[Binding],
    replace: bool,
) -> Result<NativeUndo> {
    let reg = registration()?;
    let before = request(&reg, None)?;
    let selected = bindings
        .iter()
        .filter(|b| b.target_id == target.id)
        .collect::<Vec<_>>();
    let desired_base = base(target, port);
    ensure!(
        replace
            || before["baseUrl"]
                .as_str()
                .is_none_or(|v| v.is_empty() || v == desired_base),
        "WebStorm 已有自定义来源。核对后勾选替换，将为原配置保留受保护的撤销记录"
    );
    let after = if let Some(first) = selected.first() {
        json!({"baseUrl":desired_base,"apiKey":first.local_token,"enabled":true,"toolEnabled":true})
    } else {
        json!({"baseUrl":desired_base,"apiKey":"","enabled":false,"toolEnabled":true})
    };
    Ok(NativeUndo { before, after })
}
pub fn apply(undo: &NativeUndo, restore: bool) -> Result<()> {
    let reg = registration()?;
    let (expected, desired) = if restore {
        (&undo.after, &undo.before)
    } else {
        (&undo.before, &undo.after)
    };
    let current = request(&reg, None)?;
    if current == *desired {
        return Ok(());
    }
    ensure!(
        current == *expected,
        "WebStorm 的来源后来已被修改，未覆盖。请重新读取并核对"
    );
    match request(
        &reg,
        Some(&json!({"expected":expected,"desired":desired,"restore":restore})),
    ) {
        Ok(actual) => ensure!(actual == *desired, "IDE 保存后的内容未能确认，请重新读取"),
        Err(error) => {
            if request(&reg, None).ok().as_ref() != Some(desired) {
                return Err(error);
            }
        }
    }
    Ok(())
}
pub fn using(binding: &Binding, target: &ClientTarget, port: u16) -> Result<bool> {
    let actual = request(&registration()?, None)?;
    Ok(actual["enabled"] == true
        && actual["baseUrl"] == base(target, port)
        && actual["apiKey"] == binding.local_token)
}
pub fn export_plugin(destination: &Path) -> Result<()> {
    const JAR: &[u8] = include_bytes!("../../resources/webstorm-bridge/tiancai-hub-bridge.jar");
    transaction::atomic_write(destination, JAR)
}
