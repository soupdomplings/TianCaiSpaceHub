//! Product help shipped with Hub. Looking up a topic does not open a page or run an action.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

struct Topic {
    key: &'static str,
    title: &'static str,
    guidance: &'static [&'static str],
    tool_hint: Option<&'static str>,
}

const TOPICS: &[Topic] = &[
    Topic {
        key: "remote",
        title: "远程连接、实时显示和审批",
        guidance: &[
            "第一次连接时，两台电脑都打开 Hub，在「远程连接 → 设置」启用远程连接并保存。到「配对」，一台获取本机连接码，另一台粘贴并请求连接，在提供码的一端点「允许连接」。在电脑 A 的「设备」选择在线的 B，点「打开会话网页」。B 的 Codex／ChatGPT 或天工 Work 需要已接入。",
            "A 看 B 的对话时，请在 B 打开「远程连接 → 设置 → 显示内容（本机发送）」。思考过程、工具执行、文件变更、产物默认全关，勾选后自动保存，网页和小程序均跟随 B。设置只影响随后实时收到的内容，已显示的内容和上翻位置保持原样。",
            "追问和审批单独显示在输入框上方的待处理卡片。打开卡片，根据目标客户端实际给出的选项提交回答、允许或拒绝。四个勾选均关闭也能处理。只有等待状态或必须在桌面处理的请求，需回 B 的原客户端操作。",
            "升级远程功能时两端更新 Hub，并从 A 重新打开网页。手机还需有可用的小程序及连接服务，在目标 Hub 确认手机配对，目标电脑保持在线。",
        ],
        tool_hint: None,
    },
    Topic {
        key: "downloads",
        title: "下载远程会话生成的文件",
        guidance: &[
            "在远程会话点击答复中的文件链接，或在文件卡、预览页点「下载文件」，文件会保存到访问设备。「查看产物」列出当前会话已读取的文件，即使目标 Hub 关闭了「产物」也可以手动下载。",
            "提示「当前会话未提供可下载链接」时，先确认两端 Hub 已更新，并从访问端重新打开原会话，查看产物清单。若还只有路径，检查目标电脑上的原文件是否存在。已有原生产物可以直接关联。新交付文件也可请原会话给出明确的文件链接，例如工作目录下 target、outputs 或 deliverables 直接存放的 MSI、PDF 或 TXT。",
            "答复中的目标电脑路径只在那台电脑上有效。如果文件已删除，请在目标电脑恢复原文件，或请原会话重新生成并给出链接。",
        ],
        tool_hint: Some("hub_client_downloads 仅用于下载安装客户端。"),
    },
    Topic {
        key: "startup",
        title: "Windows 登录后启动 Hub",
        guidance: &[
            "在 Windows 的 Hub 打开「设置 → 启动与集成」，勾选「开机启动 Hub」。下次以当前 Windows 用户登录时会自动打开 Hub。关闭时回到同一位置取消勾选。如果页面提示已被系统停用，到 Windows 设置 → 应用 → 启动中重新开启。",
        ],
        tool_hint: Some(
            "用户要求打开设置时，用 hub_open_page({page:\"settings\"})，收到 opened=true 再提示用户操作开关。搭档可提供操作路径，开关状态以页面为准。",
        ),
    },
    Topic {
        key: "gateway",
        title: "模型请求出现 502、DNS 或 Windows 11001",
        guidance: &[
            "HTTP 502 / Bad Gateway 表示网关没从上游拿到可用响应。下一步看错误详情里有没有上游域名和具体原因。",
            "详情含 dns error、failed to lookup address 或主机名查找的 Windows 11001 时，发起该上游请求的一端当时无法解析所写的域名。先核对该渠道的上游域名拼写，再在发起解析的电脑或服务器检查网络、代理和 DNS。若是管理员提供的渠道，把报错时间、域名及脱敏关键行交给管理员核对。",
            "若请求 URL 是 127.0.0.1 且已收到 502，客户端已从本机入口收到 HTTP 响应。请沿错误详情里的上游域名排查。",
        ],
        tool_hint: Some(
            "单凭 502 不能判断 DNS、凭据或余额问题。要核对本机失败记录，用 hub_query_requests({source:\"hub\",status:\"failed\",period:\"today\"}) 或按实际时间筛选，返回模型、渠道、时间、状态等元数据。核对普通渠道地址可读 hub_read_configuration。这两项都不能探测网络或证明 DNS 已修好。只有截图且看不到原因时，请用户贴状态码和脱敏错误行即可。",
        ),
    },
    Topic {
        key: "xshell",
        title: "XshellB-MCP 接入与巡检权限",
        guidance: &[
            "Windows Hub 打开「共用能力 → XshellB-MCP」，给使用的客户端配置 XshellB-MCP 后重新打开客户端。先选好 Xshell 中已登录、停在空闲提示符的标签页，再在 Hub 选择对应窗口，点一次「启动脚本」，保持所选标签页，等 Hub 显示已接入。已有旧脚本时这一步也会替换它。",
            "打开「设置巡检权限」，可在「批量自动同意」按中英文分号或换行填写支持的巡检命令，每条独立授权。用「查看支持的命令」核对支持项；新版脚本会从下一条请求开始使用已保存的权限。",
        ],
        tool_hint: None,
    },
];

pub(super) fn definition() -> Value {
    super::spec(
        "hub_help",
        "按 topic 读取 Hub 操作与排错帮助。远程配对、显示、审批用 remote，会话文件下载用 downloads，Windows 开机启动用 startup，502、DNS 或 11001 用 gateway，XshellB-MCP 用 xshell。省略时返回能力和专题目录。",
        json!({"topic":{"type":"string","enum":TOPICS.iter().map(|t| t.key).collect::<Vec<_>>()}}),
        &[],
    )
}

pub(super) fn catalog() -> Value {
    TOPICS
        .iter()
        .map(|t| (t.key.to_owned(), json!(t.title)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[cfg(feature = "gui")]
pub(crate) fn offline_help_topics() -> Vec<(&'static str, String)> {
    TOPICS
        .iter()
        .map(|topic| (topic.title, topic.guidance.join("\n\n")))
        .collect()
}

pub(super) fn lookup(args: &Value) -> Result<Option<Value>> {
    let fields = args.as_object().context("帮助参数应为对象")?;
    ensure!(
        fields.keys().all(|field| field == "topic"),
        "帮助只接受可选的 topic"
    );
    let Some(value) = fields.get("topic") else {
        return Ok(None);
    };
    let key = value.as_str().context("topic 应为专题名称")?;
    let topic = TOPICS
        .iter()
        .find(|t| t.key == key)
        .context("没有这个帮助专题，可省略 topic 查看目录")?;
    Ok(Some(
        json!({"topic":topic.key,"title":topic.title,"guidance":topic.guidance,"toolHint":topic.tool_hint}),
    ))
}
