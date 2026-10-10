# 二开功能总表

维护日期：2026-10-10。当前开发基线为 `v0.4.30-5`，源码只维护 `main`。范围：TianCaiSpaceHub 相对于 CodexHub 上游的定制，以及上游升级时必须保留的兼容衔接。

本轮承接最终 `main` 的 `8d1ccea7…`（v4 产品、CI 修复与核验文档），保留 TC-001～TC-013，新增并简化 [TC-014 NVWA MCP](nvwa-mcp.md)。2026-10-10 的界面、默认值及可编辑 MCP 路径已实现，Windows 本轮 GUI 编译和 debug EXE 构建通过；前阶段通过结果和测试 EXE 保留原身份。macOS 原生构建、本轮真实登录/客户端/业务与用户验收待完成，本轮不创建标签、Release 或安装包。状态见 [v0.4.30-5 开发交付](../releases/v0.4.30-5.md)。

NVWA 普通界面提供账号密码和浏览器个人授权，只填写一个服务地址；认证服务默认同址。折叠高级设置中的 MCP 输入直接显示默认 `/mcp`，用户可改为以单 `/` 开头的路径或完整 HTTP(S) 地址；路径跟随服务地址的部署前缀，完整地址独立覆盖，空值与 `/mcp` 使用同一默认行为。独立认证地址、租户、登录单位及应用注册也在高级设置，旧 `application` 保留为管理员显式勾选，主界面的应用代表用户提示仅在该模式显示，折叠高级设置后仍可见。密码模式不要求应用 ID；浏览器限定授权账号为高级可选项，留空以产品页面实际授权身份为准；空租户请求使用官方默认占位，最终仍绑定真实上下文。切换认证清理旧账号、秘密、应用 ID、挑战等材料并撤销旧授权；双因子只在服务端挑战时出现，无手填图形验证码 ID。保存后的刷新只在完整 profile 一致时保留当前 GUI 内存秘密，不写普通配置。

账号密码使用原始 `Authorization`；换票 token 使用 `authorization-ticket-token`，分别记录期限。每端本机凭据和会话绑定真实用户/租户/认证代次；操作客户端前预览并核对目标指纹，失败或未知工具结果不自动重放。与模型渠道、IM 执行和网页导入分开维护。

天工按实际安装身份、HTML 脚本入口与可写运行能力同步；`/tg`、`/gpt` 共用平台会话入口，`/wb` 外部执行仍未接通。已有模型路由、任务、历史与审批继续保留。历史包、构建和有限反馈只属于 [v4](../releases/v0.4.30-4.md) 等原版本，不替代本轮功能验收。

## 已实现的二开能力

2026-10-03 天工模型/外部消息已保存于 `9b9702e`，上游合并为 `22ee850`。`v0.4.30-2` 保留 TC-001～TC-012，扩展 TC-002 为 WorkBuddy 多模型，并新增 TC-013 接入点用途标识；本次 v0.4.30-3 开发统一 TC-001/TC-007 的产品与可执行文件命名，扩展 TC-007/TC-012 的概览与天工自动连接。2026-10-04 将 TC-012 收敛到各平台共用会话流程，并固定逐会话目录和模型，已移除天工专用目录菜单。2026-10-06 分阶段修复已运行实例接入及模型/日志状态，扩展自动重连、真实桌面任务与完整场景恢复，修复残留 `processing` 误判及原生步数兼容，并新增桌面启动按钮。此前另行修复天工安装发现、每轮等待消息及已打开会话的局部消息同步；前阶段复用平台审批按钮或文字选项，用户已有限确认该次测试；本轮修复退出端口、补齐原生项目目录并新增临时回复状态，各阶段分别登记。构建和预发布结果单独记录，历史产物和测试不作为后续改动的验收。

编号用于后续需求、修复和合并记录引用，已有编号不复用。

| 编号 | 内容 | 当前行为与必须保留的约束 | 详细说明 |
| --- | --- | --- | --- |
| TC-001 | 天才空间品牌与打包 | v0.4.30-3：产品/CLI/可执行文件/App 统一 `TianCaiSpaceHub`，Cargo package 为 `tiancaispacehub`；保留配置目录、协议与安装升级身份，历史包名不回写 | [品牌、桌面与交付](desktop-and-packaging.md) |
| TC-002 | WorkBuddy 多模型独立接入 | 页签逐条新增/编辑/删除/撤销；专用 `workbuddy` 与 `workbuddy:<entryId>`、独立地址、上游选择、数组/未知字段保留和版本校验；旧单模型入口兼容 | [WorkBuddy](../workbuddy.md) |
| TC-003 | WorkBuddy 错误重试 | 上游 HTTP 502/503 最多额外重试两次，等待 1 秒、2 秒；与传输错误共用预算，同一上游和请求，流已建立后不重发 | [WorkBuddy](../workbuddy.md) |
| TC-004 | WorkBuddy 思考强度与缓存 | 模型、别名和协议联动；Claude 五档；保留有效默认值；OpenAI 缓存键按优先级回退，Anthropic 用原生 `cache_control` | [WorkBuddy](../workbuddy.md) |
| TC-005 | 通用 Chat Completions | 新建通用渠道使用 `compatibility=openai_chat`；保留旧 DeepSeek Chat 行为；按渠道关闭推理，不改全局 | [Chat Completions](../openai-chat-completions.md) |
| TC-006 | 动态 Codex 模型 | 手动新增、同步渠道、远端获取、补齐选定渠道路由；未知模型按家族继承能力，手工覆盖优先；新增目录后 `gpt-6-next` 默认继承 `gpt-6.1-sol`；可见模型与路由分开管理 | [动态模型](../dynamic-codex-models.zh-CN.md) |
| TC-007 | 桌面启动与升级方式 | 本轮 Windows 监听和天工子进程禁止句柄继承，正常退出只等待/必要时结束 Hub 自有后台，保留天工任务；旧已继承实例需正常退出一次迁移。启动自动最大化；帮助菜单、托盘及启动自动检查更新均取消；v0.4.30-3：改名同步安装目录/程序/快捷方式并保留升级身份；概览随接入页签切换，公共页保留视角，不修改 IM 选择。天工 Claw/WorkBuddy 接入页各有显式启动按钮；天工自动准备启用与内置授权、复用启动互斥，并从 App Paths/卸载元数据定位正式程序，兼容仅登记图标的非系统盘安装，保留完整资源校验。天工按钮下方持续显示同步状态，页面未出现、完整地址不匹配、多页面及标识异常分别显示固定原因；App/Panel 作用域、引用/类型以及 IPC、身份、消息/摘要结构以类型化错误提供固定诊断；采用最近 30 秒的脱敏诊断及经过时间，区分完整 renderer Runtime/字段能力已准备、实际上次更新、延期与失败，旧结果过期后不作为当前刷新证据。WorkBuddy 仅启动桌面，用户本次反馈该按钮可启动，仅覆盖该次本机操作。按钮不切换 IM 或发送任务，`/wb` 仍暂不支持外部会话；前阶段窗口同步多次失败，用户随后确认 `fe48829e…` 修复后桌面消息能更新，仅覆盖该次操作；其他机器/macOS、分页及重启恢复待验收。天工模型连接由当前本地请求、配置地址与桌面进程证明，与 Harness 授权及窗口同步分别核对 | [品牌、桌面与交付](desktop-and-packaging.md)、[天工模型](gmclaw.md)、[WorkBuddy](../workbuddy.md) |
| TC-008 | 上游升级的二开衔接 | 保留全部二开；沿用 Kimi、ChatGPT 账号凭证和模型发现，合入 Windows 官方桌面识别、1455/1457 登录回调及 GPT-6.1-Sol；生产检查更新入口继续关闭 | [v0.4.30 整合](../upstream-v0.4.30-integration.md)，历史 [v0.4.28](../upstream-v0.4.28-integration.md)、[v0.4.29](../upstream-v0.4.29-integration.md) |
| TC-009 | Sub2API 网页渠道导入 | Windows 协议唤起；macOS App 协议声明、URL 事件与同用户 Unix socket 转交已随本版成功构建打包，实机待验收；共用一次性码兑换、模型查询、HTTP/HTTPS 兼容和预览保存；默认禁用，一次一个渠道 | [网页导入](../hub-external-import.md)、[契约 v1](../HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) |
| TC-010 | 导入带来的配置并发保护 | 保存重读最新配置，只合并目标；目标指纹防止覆盖预览期间修改；全量 API 保存带版本，文件锁及原子替换；空模型导入渠道不能事后直接启用 | [网页导入](../hub-external-import.md)、[配置](../configuration.md) |
| TC-011 | 天工 Claw 模型接入 | 多条目独立 ID/渠道/地址，同模型可绑定不同渠道；保存、删除、默认切换及撤销；集合版本校验、厂商参数、JSON/SSE 聚合与推理状态。v0.4.30-3：模型概览按本轮专用入口请求、当前配置 URL 与桌面进程显示等待/已连接；请求日志新增可空 `upstreamStream`，保留 `stream` 的客户端语义，聚合 JSON 行为保持。旧单模型无需重启已有有限反馈；已发布版 Windows/macOS 构建通过，新增行为待用户验收 | [天工 Claw 模型](gmclaw.md)、[请求日志](../ai-gateway-request-log-detail-patch.zh-CN.md) |
| TC-012 | IM 执行端选择与天工会话 | v0.4.30-3：目录读取官方全场景 work_dir，含无任务的手工项目，与默认及当前发送者目录按顺序去重、失败说明未完整读取；飞书20项分页保留草稿，企微10项下拉超限走共同8项文字分页，微信8项；独立随机回合与有界worker显示/终态收尾临时回复状态，Feishu同卡、Wecom仅有效Message callback同stream，Wechat无永久等待句，异常Drop报未知不重放。飞书/企微审批复用批准全部、拒绝全部按钮；企微先发完整参数文字再发按钮，微信用 `/1`/`/2` 文字选项（兼容 `1`/`2`、`y`/`n`），长命令作兼容/投递失败备用。审批独立于 Codex，以发送者平台/账号/聊天/ID、会话/配置/运行身份与随机请求标识隔离、15 分钟有效；非法选项、非本人、重复/过期、切端或重启旧请求不执行、不落 Codex，预检失败保留待审批、未知执行不重放，不新增永久或单工具授权。`/tg` 首次启用/切端，启用后每 5 秒自动识别同用户官方天工运行授权，重启不需重复命令；后台不启动桌面或重放任务，必要启动仍显式执行。复用飞书/微信/企微原会话交互，新建真实桌面场景/任务并保存消息，全部桌面任务供现有允许名单获准发送者浏览/恢复，保持原目录、精确模型与记忆身份。Hub 新建步数默认 `30`、配置范围 `1..200`；原生历史恢复、元数据登记与执行请求兼容 `1..1000` 并透传。`/q` 释放会话认领，`/gpt` 成功切离保留会话但释放认领，`/tg` 返回重新认领；被他人占用可用 `/tg new` 新建。`/wb` 暂不可用，`/s` 不伪造取消。认领、运行/审批/未知状态和旧卡片保护生效；残留 `processing` 仅在当前轮完整 `harness_sidecar user→over`、无该轮审批、更新时间不晚于终态且含 `chat_id` 的状态/事件至少间隔 1 秒双读稳定时允许恢复/普通执行；缺少终态、实际运行或未知继续阻止，不回写状态、不重放、不锁住桌面。关联状态持久保存，不补造旧内存历史。普通天工消息移除无条件固定等待回复；v0.4.30-4 标签源码的局部同步改按已安装天工身份、renderer HTML 脚本入口与运行时字段能力核对，不比较固定版本或资源指纹，仅刷新匹配任务/会话的消息及缓存；Windows 页面路径逐段拼接、按完整组件兼容分隔符及 ASCII 大小写，拒绝不同页面/query/hash，修复旧全字符串误判；首次正常退出天工后从 Hub 启动启用 `127.0.0.1:18769` 通道，后续 Hub 重启可重连。展示队列最多 128 项；独立来源缓存最多 128 个任务/会话键、每键 128 个回复行 ID，出队/完成不丢来源。版本 `1` 关联新增可选 `context.owned_reply_ids`，缺失默认空、每关联最多 128 个本 Hub 成功创建的回复行 ID；恢复先核对原任务/会话/记忆身份，额外来源保存失败不阻止已授权模型执行。组件按根视图及完整任务/会话/活动属性定位，不依赖编译后的 __name；App/ChatPanel 均有界检查 Closure/Block，每组件最多 8 个候选且完整所需字段来自同一作用域、只读 setup schema 通过且有效候选唯一；前阶段已在核验本机身份/资源后只读检查页面列表和 Runtime，确认 App 4 个引用与 Panel 11 个值完整、schema ready，仅证明引用能力。完成 Runtime/引用/字段核对后才标为能力已准备，按钮下方与概览区分 30 秒内实际更新和失败。实际写入的 scenarios/messages/showWelcome/hasOlderMessages Ref 与实例缓存须可写，纯读 Ref 不额外限制，异步读取后再次核对；保留草稿/模型/当前选择，真实桌面执行、审批或未保存信息存在时暂缓，不整页重载、切会话或重放。普通手动实例仍可自动授权及使用 IM，无通道时不能即时同步并提示首次从 Hub 启动。旧无来源占位不猜补；升级遇到前阶段卡住窗口需正常退出天工后从 Hub 重开一次读取已保存记录。用户已有限反馈自动重连、飞书选取历史并对话成功，但 e407305f… 仍因页面路径误判未同步，后续 9cdc7398… 又因遗漏 Block 引用未同步，这两个前阶段即时显示未通过用户验收；用户随后确认 `fe48829e…` 程序桌面消息能更新，仅登记该次操作，用户随后确认 `71c3043d…` 审批测试通过，仅覆盖当次操作，其余保护仍待验收；前作用域修复迁移无需再退出天工，仅更换 Hub 后重新保存/恢复触发展示；本轮若旧实例已继承3847，需核对任务后正常退出旧天工一次、再从新Hub启动，后续不要求每次关闭；连续多轮、Hub 重启恢复及其他场景待用户验收。实现与构建见品牌专题；开发方在前阶段仅执行页面定位/Runtime 结构诊断，本轮未运行程序或真实业务，没有执行消息同步函数、窗口更新、交互测试或 IM/模型/Harness/DataServer 业务请求，v0.4.30-3 macOS 原生构建已通过；本版 v0.4.30-4 的 macOS 首次 DMG 失败保留历史，修复后同标签重建与包核验已通过，实机行为待用户验收 | [天工外部消息](gmclaw-im.md)、[WorkBuddy 边界](../workbuddy.md#外部消息执行端边界) |
| TC-013 | 接入点用途标识与隔离 | 大模型渠道列表/编辑器明确普通与专用用途，专用身份不可误改；整个 WorkBuddy/天工命名空间排除普通请求与导入，条目地址精确选渠道，不跨客户端或条目回退 | [用途与隔离](client-channel-scope.md) |
| TC-014 | NVWA MCP 认证与三端接入 | v0.4.30-5：普通密码/浏览器个人授权、单服务地址；高级 MCP 默认 `/mcp` 可编辑为路径或完整 URL，路径跟随部署前缀，空值与 `/mcp` 同义；显式管理员应用模式；切认证清旧材料并撤销授权、挑战式双因子，无手填 captcha ID；一致 profile 刷新仅保留 GUI 内存秘密；两类 token/真实身份、独立桥及三端预览/指纹/备份/请求隔离，不重放工具；Windows 本轮编译及 debug EXE 构建通过，待用户验收，未发布 | [NVWA MCP](nvwa-mcp.md) |

## 代码与配置定位

| 范围 | 入口 | 配置或兼容注意点 |
| --- | --- | --- |
| TC-014 | [界面](../../src/gui/nvwa.rs)、[认证](../../src/nvwa/auth.rs)、[配置](../../src/nvwa/config.rs)、[凭据](../../src/nvwa/secrets.rs)、[服务](../../src/nvwa/mod.rs)、[桥](../../src/nvwa/bridge.rs)、[管理](../../src/nvwa/server.rs)、[适配器](../../src/nvwa/adapters/mod.rs) | 独立 version 1 环境集合、`_revision`，默认 3849；旧完整地址和 application 兼容；空 mcpUrl 与 `/mcp` 同默认，新配置将默认 `/mcp` 规范为空保存，旧 `/mcp` 无改动保存保留原值；其他单 `/` 路径原样保存并保留部署前缀，完整 URL 独立覆盖，集中解析及 `resolvedMcpUrl`；DPAPI/Keychain；profile/client/代次身份隔离；只管自身 MCP 项，天工不用 SQLite；回退先移除或恢复各端 |
| TC-001 / TC-007 | [不可继承监听](../../src/main.rs)、[Windows 天工启动](../../src/gmclaw_runtime/windows_start.rs)、[自有后台退出](../../src/gui/daemon.rs)、[GUI](../../src/gui.rs)、[托盘](../../src/gui/tray.rs)、[天工启动与持续状态](../../src/gui/gmclaw.rs)、[运行与同步诊断](../../src/gmclaw_runtime.rs)、[页签概览](../../src/gui/client_overview.rs)、[Windows MSI](../../packaging/windows/TianCaiSpaceHub.wxs)、[macOS 信息](../../packaging/macos/Info.plist)、[打包脚本](../../scripts/package-hub-import.ps1) | 产品名与编译/安装输出同步；协议、状态目录、环境变量及升级身份保持兼容；历史更新模块不代表存在用户入口。同步诊断最多采纳最近 30 秒结果，四类页面识别失败及 App/Panel、IPC、身份/数据结构失败固定脱敏显示；能力准备、实际更新及失败独立于模型连通/Harness 授权，不新增 TOML 字段 |
| TC-002 / TC-004 | [WorkBuddy UI](../../src/gui/workbuddy.rs)、[配置与备份](../../src/workbuddy_config.rs)、[协议转换](../../src/ai_gateway/workbuddy.rs)、[请求分发](../../src/ai_gateway/handler.rs) | `WORKBUDDY_CONFIG_PATH`；保留完整数组，兼容旧对象；独立 entryId、集合版本与备份；切换协议清理不适用缓存字段 |
| TC-003 | [上游请求及重试](../../src/ai_gateway/providers/mod.rs) | 重试不能叠加预算、切换渠道或重放成功建立的流 |
| TC-005 | [渠道配置](../../src/ai_gateway/config.rs)、[Chat 转换](../../src/ai_gateway/providers/deepseek_chat.rs)、[GUI](../../src/gui.rs) | `providerType=chat_completions`、`compatibility=openai_chat`、`chatDisableReasoning` |
| TC-006 | [模型目录](../../src/ai_gateway/model.rs)、[网关配置](../../src/ai_gateway/config.rs)、[Codex 页签](../../src/gui/codex_tab.rs)、[GUI](../../src/gui.rs) | `codexVisibleModels`、`codexModelProfiles`、`modelAliases`；继承不证明上游真实能力 |
| TC-008 | [账号登录](../../src/ai_gateway/chatgpt_auth.rs)、[WorkBuddy 配置](../../src/workbuddy_config.rs)、[浏览器启动](../../src/gui/browser.rs) | 保留账号引用和刷新，不把账号令牌变成普通 API Key；不能因移除更新入口破坏 OAuth |
| TC-009 | [导入模块](../../src/external_import.rs)、[网络](../../src/external_import/client.rs)、[IPC](../../src/external_import/ipc.rs)、[关联注册](../../src/external_import/registration.rs)、[导入 UI](../../src/gui/external_import.rs)、[macOS 事件](../../src/gui/external_import/macos.rs)、[macOS IPC](../../src/external_import/ipc/macos.rs)、[CLI](../../src/cli.rs) | `tiancaispacehub://import/v1`；`importSource` 持久保存来源；ticket 和待保存 Key 仅在内存；macOS 关联由 App 与 Launch Services 管理 |
| TC-010 | [配置读写](../../src/config.rs)、[本地 API](../../src/web.rs) | `_revision` 仅用于 API，不写 TOML；导入更新保留用户权重、超时、缓存及默认原有映射 |
| TC-011 | [模型配置](../../src/gmclaw_config.rs)、[页签](../../src/gui/gmclaw.rs)、[参数](../../src/ai_gateway/gmclaw.rs)、[推理状态](../../src/ai_gateway/gmclaw_replay.rs)、[流式聚合](../../src/ai_gateway/gmclaw_stream.rs)、[模型连接证据](../../src/client_overview.rs)、[请求日志](../../src/ai_gateway/request_log.rs)、[日志界面](../../src/gui/request_logs.rs)、[API](../../src/web.rs)、[路由](../../src/ai_gateway/router.rs) | `GMCLAW_CONFIG_PATH`；只管理既有数据库的旧 `tiancaispacehub` 与新 `tiancaispacehub-<entryId>`；`gmclawParameters` 仅存 Hub；`gmclaw`/`gmclaw:<entryId>` 全部排除普通路由与导入；备份元数据 v2 兼容 v1。模型活动只存内存，旧日志不能证明当前连接；`upstreamStream` 省略/空表示上游模式未知，SQLite 兼容新增可空列 |
| TC-012 | [临时回合与终态](../../src/im/core/executor_turn.rs)、[天工审批展示结构](../../src/im/core/executor_approval.rs)、[独立审批投递载荷](../../src/im/core/outbound.rs)、[共用会话后端](../../src/im/core/session_backend.rs)、[天工会话数据与操作](../../src/gmclaw_im/sessions.rs)、[桌面关联与恢复](../../src/gmclaw_im/desktop.rs)、[官方桌面数据接口](../../src/gmclaw_desktop.rs)、[共用表单与能力](../../src/im/core/thread.rs)、[飞书表单](../../src/im/feishu/renderer/threads/create.rs)、[微信流程](../../src/im/wechat/flow.rs)、[企微卡片](../../src/im/wecom/adapter.rs)、[只读模型列表](../../src/gmclaw_config.rs)、[自动授权、来源缓存与诊断](../../src/gmclaw_runtime.rs)、[已有实例授权识别](../../src/gmclaw_runtime/credentials.rs)、[局部消息同步](../../src/gmclaw_runtime/display.rs)、[renderer 定位](../../src/gmclaw_runtime/display-target.js)、[setup 能力核对](../../src/gmclaw_runtime/display-setup-schema.js)、[能力字段核对](../../src/gmclaw_runtime/display-schema.js)、[renderer 适配](../../src/gmclaw_runtime/display-sync.js)、[IM 分派与审批](../../src/gmclaw_im.rs)、[Harness 客户端](../../src/gmclaw_executor.rs)、[配置](../../src/config.rs) | `gmclawBridge` 默认未启用；启用后每 5 秒检查，状态 worker 同配置共享且最多 18 秒、发现失败 5 秒限流，概览缓存不超过 15 秒；运行授权只留内存，后台不启动。显式接入最多等待 30 秒。桌面数据固定 `127.0.0.1:18768`、同运行授权；默认原生场景项目 ID 为 `0`，其他用真实场景 ID。`<config stem>.gmclaw-sessions.json` 版本 `1` 原子保存精确模型/任务/会话/记忆身份及未知/待审批标志，可选 `context.owned_reply_ids` 缺失默认空、每关联最多 128 个互不重复的正安全整数行 ID，仅登记本 Hub 官方 `append_message(system)` 成功返回行；整文件最多 20000 关联/8 MiB，不存口令或正文。旧版重写关联会丢新增字段，旧无来源占位需一次正常重开加载。独立内存来源缓存最多 128 键×128 行，展示队列出队不丢来源；App/Panel 均有界检查 Closure/Block，每组件最多 8 个同作用域完整候选、只读 setup schema 通过且有效候选唯一，不绑定组件 __name，忽略 Module/Global/Script；前阶段本机只读结构/schema 核对仅属于当时实现，不替代本轮实际能力或消息更新验证。完整 Runtime/schema 能力通过后才显示准备。恢复列表最多 20000 任务/8 MiB。Hub 重启不恢复发送者当前选择或审批控件，真实桌面历史仍可读；旧默认目录配置兼容，新建/恢复均固定目录与模型；`GmClawApproval` 展示与 `GmClawApprovalDecision` 动作独立，callback/pending 不交给 Codex，审批交互不新增 TOML 或持久字段 |
| TC-013 | [渠道身份](../../src/ai_gateway/config.rs)、[列表用途](../../src/gui/ai_gateway.rs)、[路由](../../src/ai_gateway/router.rs)、[WebSocket](../../src/ai_gateway/websocket/mod.rs) | 用途由保留名称派生，原始名称与展示标签分开；权重和粘性不能突破入口隔离，旧版不理解新 WorkBuddy 命名空间 |

## 沿用的上游能力

本地二开继续依赖 AI Gateway、IM remote-control、多执行端路由、Codex 初始化与恢复、增强启动、插件兼容、各厂商适配器及请求日志。Kimi K3、ChatGPT 账号登录、Responses WebSocket 和新版内置 GPT 目录来自上游合并；二开记录描述其与定制功能的衔接，不能把上游整项功能记为本地新开发。

相应专题均列在 [文档总入口](../README.md)。对这些模块做新的本地修复时，更新原专题并在本表新增或关联条目，而不是另建重复说明。

## 当前边界与后续衔接

- 仓库只保留 `main`，当前开发统一 `v0.4.30-5`；本地与 live origin 无其他分支。版本通过完整二开标签及 GitHub Releases 管理，源码、编译、用户验收和发布分别记录；此前 v4 包保持历史身份，分支对齐见 [仓库清理记录](../development/repository-cleanup.md)。
- Windows 优先，macOS 次之；Linux 不纳入新增需求。保留上游 Linux 工作流不代表本次开发和验收覆盖 Linux。
- `0.4.29-3` 用户反馈 Windows 本地导入正常；后续流程调整、其他站点及安装/升级/卸载待用户验收。`0.4.29-5` 当时只有 macOS 接入源码，未完成 Mac 构建/验收；前版 `v0.4.30-2` 当时已通过 macOS 原生 Actions 构建并交付 universal 包；本版 `v0.4.30-3` 双架构包亦已构建发布，实机导入仍待用户验收。
- 模型发现与推理调用分开；远端返回模型 ID 不等于证明所有协议和能力可用。导入不发起计费模型测试。
- Chrome 插件完整兼容、按模型选择 Remote Compact V2、Agent Manager 等研究资料不属于本表已交付功能。用户授权新任务后再更新状态。
- WorkBuddy 支持模型接入，`v0.4.30-3` 提供显式启动桌面按钮；用户本次反馈按钮可启动，仅覆盖该次本机操作，不代表其他机器/macOS 或外部会话接口验收。`/wb` 为已发布的暂不支持提示入口。本机 5.6.2 程序资源已只读核对，尚未取得能管理现有桌面会话的稳定授权接口，不能用独立 CLI 冒充桌面接入。
- Codex 会话归入 AI Gateway 的操作只涉及 Codex `rollout/threads.model_provider`，不改天工的任务、会话或模型元数据；天工原生历史的步数兼容在其独立恢复/请求链路处理，不依赖 Codex 归档操作。
- 当前测试由用户负责；已有历史测试数量仅属于对应版本，不能用于宣称后续变更已通过测试。
- 用户已确认上一轮天工模型保存成功，且无需完全退出再重开；该有限反馈不覆盖本轮多模型、流式聚合和外部消息执行端。历史调试 EXE 的大小与哈希保留在专题，不能作为本轮产物身份。

每次功能变更同时更新对应专题和 [变更记录](CHANGELOG.md)。新增能力扩充本表及 [索引](../README.md)，执行步骤见 [维护规范](../development/documentation.md)。
