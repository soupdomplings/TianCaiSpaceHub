# 远程原生会话适配

**V1.0.58 · 共用目标端显示设置**：远程 history 增加 `displayContent:{reasoning,tool,fileChange,artifact}`，每条 SSE 事件的 `data.displayContent` 带同一目标 Hub 在发送时的设置。实时思考、工具、文件变更还由目标端按选择过滤，事件重播仍不补历史过程；已登记文件和原生待办独立保留。网页与小程序据每条事件处理随后生成的内容，改勾选不要求重读历史或重建订阅。写入仅在目标端本机的「远程连接 → 设置」开放。协议见[契约](../development/remote-session-contract.md)，保存见[配置](../../src/remote_network/display.rs)。

**V1.0.57 · 从助手答复下载交付文件**

目标 Hub 在 Codex／ChatGPT 和天工的历史答复、实时完成答复中识别明确的 Markdown 文件链接。匹配到文件后，正文链接指向当前会话的下载地址，同一条消息携带文件名、大小和产物标识。已有附件或产物优先按本会话目录匹配。形如 `sandbox:/mnt/data/报告.pdf` 的链接，仅当当前会话已登记一个名称完全相同的文件时才能下载。

历史答复首次出现尚未登记的成品时，Hub 使用原生会话自身的工作目录。Codex 按需从匹配会话 ID 的原生连接读取目录并在该连接内复用。允许的交付目录为工作目录下的 `target`、`dist`、`artifacts`、`outputs`、`output` 和 `deliverables`，仅接受目录的直接子文件。支持 MSI、ZIP、7Z、DMG、PKG、PDF、DOCX、PPTX、XLSX、CSV、TXT、MD、HTML、PNG、JPG／JPEG、WEBP、GIF、MP4、MP3 和 WAV。链接可使用工作目录相对路径、系统绝对路径、中文、空格和括号。源码行号、代码示例和无法确认的路径保留原文。

文件登记沿用本会话原有目录，新增的真实路径与交付目录字段均可选。网页及手机按下载标识打开文件后，使用该文件句柄核对实际位置，再分别流式读取或分片读取。历史页只登记文件信息。回退目标端会恢复旧版处理方式，原生会话仍保存原文。实现见[交付链接与句柄核对](../../src/remote_sessions/delivery_links.rs)。

**V1.0.56 · 桌面反馈的远程显示**　目标 Hub 在读取 Codex 历史及接收实时用户消息时，将桌面的结构化批注整理为引用、批注意见和请求正文。桌面附带的提问回复按原问题和答案显示。识别入口位于 [desktop_text.rs](../../src/remote_sessions/desktop_text.rs)。该处理仅生成远程显示文本，原生会话保存原文。无法完整识别的数据和用户在正文中举的协议示例按普通文本显示。无配置迁移，回退目标端版本即恢复原文显示。

**V1.0.54 · 可选实时过程**　`/clients` 为目标端客户端增加 `capabilities.liveActivity`。支持的访问端使用现有 SSE `events?after=N&liveActivity=1`。新到的公开过程可附带 `data.liveActivity`，其中 `itemId` 是原条目标识，`category` 为 `reasoning`、`tool` 或 `fileChange`，`mode` 为 `started`、`append`、`entry` 或 `completed`，`text` 是当次可显示文本。访问端在收到事件时按本端偏好选取片段。已过滤事件继续使用原游标，注册文件仍经原条目传输。默认订阅、历史和事件回放不携带过程片段。

Codex 提供可见思考摘要、进度、命令输出及文件变更增量，完成通知只取结束状态，避免之前隐藏的完整输出在切换后出现。天工依现有步骤提供单次摘要、工具名称、状态和变更路径。内部记录、桌面专属弹窗和审批继续沿各自原有规则。实现见[目标端事件投影](../../src/remote_sessions/transcript.rs)。没有持久配置迁移，访问端可退回原 SSE 协议。

[V1.0.53](../releases/v1.0.53.md)：远程历史与实时/重播事件在目标端共用[消息显示规则](../../src/remote_sessions/transcript.rs)，省略过程数据而保留已登记文件和事件位置。天工原任务在等待确认时，打开或断线重连会核对 run 详情中的 pending_calls；错过原 confirm 事件也能恢复仍有效的同一组调用，不重复发卡。该版本未证实天工提供通用的结构化追问协议。

维护日期：2026-11-15。关联 TC-025。源码基线 `main / 7c6d3d5`，功能实现 `e44b497`；用户随后要求统一生成 V1.0.32 Windows 本地测试包，实际构建和包身份见[交付记录](../releases/v1.0.32.md)，各模块不单独打包。

实现状态：Codex／天工原生会话后端已接入，Windows GUI 编译检查及定向离线测试通过；真实双机和 Windows／macOS 桌面往返续聊均待用户验收。组网和网页分别见[远程连接总览](remote-connection.md)、[组网](remote-network.md)及[网页](remote-web.md)。

## 身份与接口

会话键为目标设备、客户端实例和原始会话 ID。`/api/remote/v1` 仅由配对后的隔离网关承载，不合入 Hub 现有管理接口；浏览器请求经访问端 Hub 代理，目标设备运行授权和模型凭据留在目标 Hub。

提供客户端实际能力和模型、项目／会话分页、原生历史、新建、发送、停止、事件续读、审批／追问、附件及关联产物。字段和路径见[接口契约](../development/remote-session-contract.md)。Codex 项目按原生会话目录逐页去重；天工读取原生项目后本机分页，新建时仍核对完整项目目录。

不在网页另存聊天历史，不修改 IM 当前会话。Hub 只在运行期间保留每个已观察会话最近 512 条事件、最近 128 个发送回执及未决审批；没有打开网页时不积累正文，仅有界保留最多 128 项原生 Codex 待确认请求。

发送采用 Hub 自有 worker。浏览器刷新或断开不会中止原任务；相同 `requestId` 返回已知回执，原生执行结果未知时保持待核对状态，不重发消息或工具。审批在校验和提交期间也由 Hub worker 继续，必须使用原连接、原会话和原请求，重复提交只生效一次。确定的原生拒绝允许修正后再试；未确认的非幂等提交保留占用，等待真实状态或原生历史核对。

历史读取前记录事件游标。网页用快照中已有消息 ID 作为边界，再续读事件；`item.updated` 的 `completed:true` 表示可靠完整终项，`false` 不得覆盖已有快照。事件窗口过期返回 `reset`，重新读取原生历史，不重发输入。连接代次变化清理旧事件和审批，旧游标触发快照刷新。

## Codex

- 复用已接入 Hub 的桌面桥接和原 App Server。多连接按 connection epoch、client ID、stream ID 和 thread ID 分流，入口位于现有 IM 事件过滤之前；旧单连接兼容为 `codex:default`。
- 会话列表用 `thread/list`，历史用 `thread/read` 元数据及 `thread/turns/list(itemsView:full)` 分页。如果元数据报告找不到历史文件但同一原生会话的分页仍返回轮次，网页照常展示这些消息；也找不到轮次时显示中文提示。仅分页明确返回方法不支持时兼容旧版完整历史读取，不把其他会话的响应当作本会话。
- 查看已有会话只读取历史；打开实时事件或发消息才在原连接上按需 `thread/resume(excludeTurns:true)`。渠道变化可能让 Codex 拒绝续聊，但不会挡住可读的旧消息。旧版回退读取完整历史时同样核对返回的原会话 ID。
- 本次连接中由网页成功新建的会话已订阅；首条消息前刷新可以显示空会话，首发直接走 `turn/start`，不要求尚未生成的历史文件。首发已接受、结果不确定、桌面已开始执行或连接已更换后，恢复真实历史读取。普通旧会话缺少历史会显示错误，不会冒充新建空会话。以上操作不改变 IM 的会话选择。
- 新建使用目标 Hub 明确添加的目录或当前 Codex 连接实际列出的会话目录，原始路径交给 `thread/start`。模型及思考强度来自 `model/list`，运行权限沿用目标端设置。
- 停止先读取当前真实 turn，向相同 thread／turn 发送 `turn/interrupt`，再读回终态；原生尚未确认时只显示“停止中”。
- 审批复用 Hub 既有原生决定映射，保留命令、文件、权限、MCP 授权和结构化追问。回复精确送回原 connection／stream／request，已重建连接的旧确认拒绝提交。
- 事件提供用户／助手文字、思考、命令输出、工具、文件变更和终态。已存在的本机图片沿用原生图片输入；其他上传附件以目标文件引用附入原输入。

App Server 协议依据官方[接口文档](https://learn.chatgpt.com/docs/app-server)；仅源码实现不代表当前用户的桌面版本已完成实机验收。

**本轮远程网页确认与正文（待统一验证）**：Codex 命令、文件和权限审批沿用原生选项；结构化追问保留原来的选项及“其他”填写。MCP 普通表单（含省略 mode 的旧格式）逐字段展示文字、布尔、数值和枚举，按用户实际选择与原类型回复，不代填允许项。URL 流程、敏感追问及网页不能完整呈现的复杂表单请回目标 Codex 处理。授权只回复实际收到请求的原连接、会话及请求 ID；浏览器重开可恢复 Hub 已观察到的未决卡片。桌面私有弹窗不会自动共享：App Server 只上报等待标记而未把原请求送到 Hub 时，网页只提示回目标桌面处理，不凭状态重造确认，也不自动批准。本轮不更改 IM。

原生历史与实时内容使用相同的分类：明确标记为 `agentMessage.phase=commentary` 的是进度；无阶段、`partial_answer` 与 `final_answer` 均保留为答复。Hook、上下文压缩及原生显式隐藏的内部记录不显示为聊天正文，不按用户输入的字词猜测。代码见[Codex 适配](../../src/remote_sessions/codex.rs)、[原生问答映射](../../src/remote_sessions/codex_prompts.rs)。依据[官方 App Server 审批与表单](https://learn.chatgpt.com/docs/app-server#approvals)，已只读核对本机桌面 26.1103.7773.0 的等待标记与消息阶段。新增合成表单、撤销、原流回复和消息类型回归；实机桌面与网页交叉操作仍待用户测试。

## 天工 Work

- 复用现有已启用、已就绪的天工连接授权和官方 DataServer，读取真实任务、项目、消息、`files` 和会话元数据，执行采用原生 `/v2/chat/runs` 的 create／events／stop／resume。
- 续聊保持原 `session_id`、`task_id`、`metadata.user_id`、项目、模型及已有模型参数。不会将普通账号会话迁移到 AI Gateway，也不猜测账号归属。
- 网页新建仅在 Hub 确认 AI Gateway 增强工作区仍活动时开放，归属 `ai-gateway`，必须选取目标端真实配置的模型。普通登录已有会话仍可续聊。
- 当前续聊沿用原任务模型；切换模型须先在目标天工操作，再重新读取。暂不提供独立思考强度，能力字段如实关闭；新建仍显示可选模型。
- 运行 worker 与 IM 共用既有执行信号量。自有新回合的用户消息、助手内容、工具步骤及附件／产物写回原 DataServer，保留桌面可继续的原始会话。
- 首次观察桌面正在运行的任务时按原 `last_seq` 建立回放边界；旧事件仅恢复内存前缀和仍未决的主任务确认，不重复追加正文，不改写桌面原消息。断流仅按 `after_seq` 续读，不再次 create 或 resume。
- 停止／审批校验同一 run 和 session，审批还绑定原运行授权身份。子 Agent 的独立确认仍需在天工桌面处理，不把子运行审批误发给主任务；结构化追问能力暂不宣称支持。
- 旧天工版本缺少完整 run 管理能力时关闭执行、停止和审批能力，已有历史仍通过 DataServer 读取。不能以断开事件订阅冒充真实停止。

### Work 1.1.0 兼容更新

2026-11-16 基于 `main / 27568b5` 只读核对 Work 1.1.0 安装包主进程及 Harness 源码后更新以下原生字段与运行状态。源码已集成，本模块 8 项离线回归与 Windows GUI 测试目标编译通过，统一构建见[V33](../releases/v1.0.33.md)；真实双机和 macOS 尚未验收。

- 原生 DataServer 请求固定当前会话账号头：复用 `/data/claw/session` 的脱敏真实登录状态，增强启动优先读取独立 `hubConversationOwner: ai-gateway`。原任务 `owner_id` 与会话元数据不一致时停止当前操作；账号变化不自动换账号重试，也不另建替代历史。网页新建仍限于实际增强工作区，普通账号已有会话保持原归属。
- 默认任务使用原生分配的 `task_work_dir`，Harness 同时携带原 `task_id`、`agent_id`、对应模型的 `model_options[model_id]`、`conversation_revision` 及适用的 `project.task_workspace_id`。用户消息写入与执行使用同一 `chat_id`；附件继续回写原生 `files`。编辑重发产生的新版会话 revision 沿原生版本规则处理，不覆盖或重放旧消息。
- 新建显式 `projectId` 优先于目录，DataServer 按该原场景 ID 创建任务；同目录多个历史项目壳不取首项，没有显式 ID 时目录匹配必须唯一。保存的记忆项目路径沿新任务返回的实际目录，项目 ID 与 `metadata.project_id` 保持一致。执行已有原生 `project_id: default` 会话时，顶层值保留 `default`，仅嵌套 `project.id` 映射为 `0`，与桌面及主进程的默认工作区身份一致。
- 历史优先沿用旧 `thinking_steps`，为空时读取新版 `execution.blocks` 中的工具／思考块。子 Agent 帧可能沿用主运行的 `run_id/session_id/seq`，必须另识别 `subagent_run_id` 与 `child_session_id`；其序号正常消费，但不把子回复、结束或审批覆盖到主消息。子审批仍提示用户在目标天工处理。
- 新 `waiting` 状态展示“继续／停止”卡片。继续只向同一原 run 提交 `resume_action: continue`，停止调用原 stop；不会新建一轮任务。该状态在观察开始、流式处理、断线核对和审批回执中均保持为待处理状态，刷新后由原事件恢复。工具审批另核对当前 `pending_calls`，只对仍待处理的原 call ID 提交 `approval_scope: operation`；过期卡片不转成另一项工具执行。

兼容与回滚：账号／revision 路由明确 404 的旧版仍沿旧协议处理；不迁移原生数据库，不改变会话归属或模型配置，不新存运行令牌。停用远程或回退前完成／停止网页任务，保留已发送附件及原生历史；新版原生账号隔离不因回退 Hub 而放宽。关键入口为 `src/gmclaw_desktop.rs`、`src/remote_sessions/tiangong.rs`；增强账号来源由共用主进程适配提供，详见[Work 兼容专题](tiangong-work-compatibility-assessment.md)。

本轮新增四项 `remote_sessions::tiangong::tests::work_v110_*` 合成用例，覆盖原检查点接续／停止与旧工具拒绝、新版历史块、包装子帧隔离及等待卡片、项目 ID 优先与目录歧义；共用 DataServer／IM 另增六项相关用例，含指定同目录项目创建及默认任务独立目录。数据与服务均为临时夹具，不调用真实客户端／模型。定向 rustfmt、本模块 8 项及共用 `gmclaw` 141 项回归通过；两设备桌面 → 网页 → 桌面、真实附件／工具／审批和 Windows／macOS 交叉组合仍待用户验收。

## 附件与产物

上传字节及关联索引集中在目标端 `<Hub 配置文件名>.remote-files/<会话范围标识>/`。该目录独立于天才搭档临时目录；真正退出和搭档清理不删除已发送附件、原生产物或用户原文件。

每个附件上限 32 MB，每轮上限 20 个。未发送的 Hub 上传可经 DELETE 回收；发送后及原生产物不可经此接口删除。Codex 原生明确拒绝发送时，只将这次原本未发送的附件恢复为可回收；结果未知仍保留，天工已写入原始用户消息后也保留。

文件读取只接受当前会话关联 ID，不接受浏览器任意路径。原生产物按原生消息／文件事件建立引用，不复制整份历史；原文件已被用户移除时返回可读错误。图片、纯文本和 PDF 可预览，HTML／SVG 等主动内容强制下载，并设置 `nosniff`、沙箱 CSP 和 `no-store`。

## 维护、兼容与回滚

主要代码：`src/remote_sessions/{mod,types,service,codex,tiangong,files}.rs`；原连接请求和事件分流位于 `src/remote_control_backend/`；天工 files 写回位于 `src/gmclaw_desktop.rs`，共享执行限额位于 `src/gmclaw_im.rs`。

不迁移 Codex／天工已有会话数据库，不改模型配置，不保存目标运行令牌。停用远程功能或回退旧 Hub 后原客户端与 IM 仍沿原路径使用；回退前停止网页任务。保留独立远程附件目录以免已有原生历史的引用失效；原生执行由原客户端持有，Hub 退出不主动关闭原客户端。

## 验证与边界

2026-11-15 本模块 13 项合成／离线用例全部通过，覆盖源及会话隔离、旧事件清理、未知结果不重放、审批恢复与一次提交、浏览器掉线后发送／审批 worker、协议级真实 interrupt 后读回终态、输入边界、附件范围与回收、主动内容下载、天工 seq 和子运行隔离、工具凭据字段清理、原生在途回放及待确认恢复。测试数据全部使用 TempDir 下的 Hub 配置、状态和请求日志，不调用真实运行端或模型。

同批既有 Codex 桥接与天工 DataServer／IM 定向回归亦通过，合计 105 项不同 Rust 测试；Windows GUI `cargo check` 通过。准确命令和范围见[集成验证](remote-connection.md#验证记录)，不将历史或网页测试计入本模块 13 项。

首版尚未完成两设备桌面 → 网页 → 桌面续聊、真实附件／工具／审批、长历史和两平台实机验收。Windows／macOS 共用此后端源码不等于已在 macOS 编译或测试；构建、离线检查、用户验收和发布状态分别记录。
