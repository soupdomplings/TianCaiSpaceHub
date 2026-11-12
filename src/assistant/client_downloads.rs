//! Official download guidance only; this tool never downloads or runs software.
use crate::desktop::clients::ClientId;
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    client: Option<String>,
}

pub(super) fn definition() -> Value {
    super::spec(
        "hub_client_downloads",
        "提供当前可见客户端的官方下载安装入口和简短步骤。client省略时列出全部支持的客户端；填写时只返回该端。此工具只给出官网链接，不会下载、运行安装器或打开浏览器，不要声称已经安装。",
        json!({"client":{"type":"string","enum":ClientId::VISIBLE.map(ClientId::key)}}),
        &[],
    )
}

pub(super) fn downloads(args: &Value) -> Result<Value> {
    let request: Request = serde_json::from_value(args.clone())?;
    let clients: Vec<Value> = ClientId::VISIBLE
        .into_iter()
        .filter(|client| {
            request
                .client
                .as_deref()
                .is_none_or(|key| key == client.key())
        })
        .map(entry)
        .collect();
    ensure!(!clients.is_empty(), "当前没有这个客户端的官方下载指引");
    Ok(json!({
        "clients":clients,
        "automaticInstall":false,
        "downloaded":false,
        "opened":false,
        "instructions":[
            "打开对应官网，选择适合当前系统的桌面安装包。",
            "按官网说明完成安装；安装包、商店入口及系统要求以官网为准。",
            "安装完成后回到 Hub，继续为该客户端配置模型。"
        ],
        "message":"可提供官方下载入口与安装指引，安装需要你在本机完成。"
    }))
}

fn entry(client: ClientId) -> Value {
    // Stable official pages, checked 2026-11-12. TianGong reuses the official
    // download source already maintained by the desktop UI since 2026-11-10.
    let (download, windows_guide, macos_guide) = match client {
        ClientId::Codex => ("https://chatgpt.com/download/", None, None),
        ClientId::WorkBuddy => (
            "https://www.workbuddy.cn/",
            Some(
                "https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide",
            ),
            Some(
                "https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Mac-Guide",
            ),
        ),
        ClientId::Tiangong => (
            "https://tiangong.jiuqi.com.cn/tiangongwork/#download",
            None,
            None,
        ),
        ClientId::DeepSeekHarness => ("https://www.deepseek.com/en/download/", None, None),
        ClientId::ZCode => (
            "https://zcode.z.ai/cn",
            Some("https://zcode.z.ai/cn/docs/install"),
            Some("https://zcode.z.ai/cn/docs/install"),
        ),
        ClientId::VsCode | ClientId::WebStorm => unreachable!("only visible clients are listed"),
    };
    json!({
        "client":client.key(),
        "name":client.name(),
        "downloadUrl":download,
        "installationGuides":{"windows":windows_guide,"macos":macos_guide}
    })
}
