# 远程原生会话适配

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
- 会话列表用 `thread/list`，历史用 `thread/read` 元数据及 `thread/turns/list(itemsView:full)` 分页。仅明确返回方法不支持时兼容旧版完整历史读取，不因普通接口失败静默切换。
- 首次打开和发送前使用指定原连接的 `thread/resume(excludeTurns:true)` 建立订阅。浏览器请求不改变 IM 的 thread／turn 选择，也不在连接重建后重放写操作。
- 新建复用 Hub 的 `ThreadStartOptions`；模型及思考强度来自 `model/list`。运行权限沿用目标端，不生成另一套权限字段或提升权限。
- 停止先读取当前真实 turn，向相同 thread／turn 发送 `turn/interrupt`，再读回终态；原生尚未确认时只显示“停止中”。
- 审批复用 Hub 既有原生决定映射，保留命令、文件、权限、MCP 授权和结构化追问。回复精确送回原 connection／stream／request，已重建连接的旧确认拒绝提交。
- 事件提供用户／助手文字、思考、命令输出、工具、文件变更和终态。已存在的本机图片沿用原生图片输入；其他上传附件以目标文件引用附入原输入。

App Server 协议依据官方[接口文档](https://learn.chatgpt.com/docs/app-server)；仅源码实现不代表当前用户的桌面版本已完成实机验收。

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
