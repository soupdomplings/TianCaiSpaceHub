# 二开功能总表

维护日期：2026-10-03。范围：TianCaiSpace Hub 相对于 CodexHub 上游的定制，以及上游升级时必须保留的兼容衔接。

当前版本 `v0.4.30-2` 已发布 GitHub 预发布版，基于 `ba3be07` 增加 WorkBuddy 多模型与渠道用途标识，标签源码为 `5114002fceee61c7db6cc7709cbd99b0fecf672c`。Windows/macOS Actions 原生构建和打包均通过；Windows 包未签名，macOS universal 包为 ad-hoc 签名、未公证。以下“已实现”仅指代码存在，功能、安装升级、实机和真实调用仍待用户验收；当前交付见 [版本记录](../releases/v0.4.30-2.md) 和 [变更记录](CHANGELOG.md)。上游来源见 [v0.4.30 整合](../upstream-v0.4.30-integration.md)；macOS 导入历史见 [0.4.29-5](../releases/v0.4.29-5.md)，早期 GitHub 交付见 [v0.4.29-2](../releases/v0.4.29-2.md)。

## 已实现的二开能力

2026-10-03 天工模型/外部消息已保存于 `9b9702e`，上游合并为 `22ee850`。本轮继续保留 TC-001～TC-012，扩展 TC-002 为 WorkBuddy 多模型，并新增 TC-013 接入点用途标识；本轮构建和预发布结果在版本交付记录单独登记，历史产物和测试不作为当前验收。

编号用于后续需求、修复和合并记录引用，已有编号不复用。

| 编号 | 内容 | 当前行为与必须保留的约束 | 详细说明 |
| --- | --- | --- | --- |
| TC-001 | 天才空间品牌与打包 | 产品显示名和图标使用 TianCaiSpace Hub；保留 `codexhub` 内部标识及安装升级身份；支持平台产物及未签名测试包 | [品牌、桌面与交付](desktop-and-packaging.md) |
| TC-002 | WorkBuddy 多模型独立接入 | 页签逐条新增/编辑/删除/撤销；专用 `workbuddy` 与 `workbuddy:<entryId>`、独立地址、上游选择、数组/未知字段保留和版本校验；旧单模型入口兼容 | [WorkBuddy](../workbuddy.md) |
| TC-003 | WorkBuddy 错误重试 | 上游 HTTP 502/503 最多额外重试两次，等待 1 秒、2 秒；与传输错误共用预算，同一上游和请求，流已建立后不重发 | [WorkBuddy](../workbuddy.md) |
| TC-004 | WorkBuddy 思考强度与缓存 | 模型、别名和协议联动；Claude 五档；保留有效默认值；OpenAI 缓存键按优先级回退，Anthropic 用原生 `cache_control` | [WorkBuddy](../workbuddy.md) |
| TC-005 | 通用 Chat Completions | 新建通用渠道使用 `compatibility=openai_chat`；保留旧 DeepSeek Chat 行为；按渠道关闭推理，不改全局 | [Chat Completions](../openai-chat-completions.md) |
| TC-006 | 动态 Codex 模型 | 手动新增、同步渠道、远端获取、补齐选定渠道路由；未知模型按家族继承能力，手工覆盖优先；新增目录后 `gpt-6-next` 默认继承 `gpt-6.1-sol`；可见模型与路由分开管理 | [动态模型](../dynamic-codex-models.zh-CN.md) |
| TC-007 | 桌面启动与升级方式 | 启动自动最大化；帮助菜单、托盘及启动自动检查更新均取消；保留主动安装新版的打包能力 | [品牌、桌面与交付](desktop-and-packaging.md) |
| TC-008 | 上游升级的二开衔接 | 保留全部二开；沿用 Kimi、ChatGPT 账号凭证和模型发现，合入 Windows 官方桌面识别、1455/1457 登录回调及 GPT-6.1-Sol；生产检查更新入口继续关闭 | [v0.4.30 整合](../upstream-v0.4.30-integration.md)，历史 [v0.4.28](../upstream-v0.4.28-integration.md)、[v0.4.29](../upstream-v0.4.29-integration.md) |
| TC-009 | Sub2API 网页渠道导入 | Windows 协议唤起；macOS App 协议声明、URL 事件与同用户 Unix socket 转交已随本版成功构建打包，实机待验收；共用一次性码兑换、模型查询、HTTP/HTTPS 兼容和预览保存；默认禁用，一次一个渠道 | [网页导入](../hub-external-import.md)、[契约 v1](../HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) |
| TC-010 | 导入带来的配置并发保护 | 保存重读最新配置，只合并目标；目标指纹防止覆盖预览期间修改；全量 API 保存带版本，文件锁及原子替换；空模型导入渠道不能事后直接启用 | [网页导入](../hub-external-import.md)、[配置](../configuration.md) |
| TC-011 | 天工 Claw 模型接入 | 多条目独立 ID/渠道/地址，同模型可绑定不同渠道；保存、删除、默认切换及撤销；集合版本校验、厂商参数、JSON/SSE 聚合与推理状态；旧单模型无需重启已有有限反馈，本版 Windows/macOS 构建通过并预发布，功能待用户验收 | [天工 Claw 模型](gmclaw.md) |
| TC-012 | 天工外部消息执行端 | 飞书/微信/企微显式 `/gmclaw`，本机 Harness 授权、发送者隔离、串行等待、文本和父会话工具审批；Windows/macOS 构建通过并预发布，功能待用户验收；附件、主动取消及 MCP CRUD 不在首版范围 | [天工外部消息](gmclaw-im.md) |
| TC-013 | 接入点用途标识与隔离 | 大模型渠道列表/编辑器明确普通与专用用途，专用身份不可误改；整个 WorkBuddy/天工命名空间排除普通请求与导入，条目地址精确选渠道，不跨客户端或条目回退 | [用途与隔离](client-channel-scope.md) |

## 代码与配置定位

| 范围 | 入口 | 配置或兼容注意点 |
| --- | --- | --- |
| TC-001 / TC-007 | [GUI](../../src/gui.rs)、[托盘](../../src/gui/tray.rs)、[Windows MSI](../../packaging/windows/CodexHub.wxs)、[macOS 信息](../../packaging/macos/Info.plist)、[打包脚本](../../scripts/package-hub-import.ps1) | 不把产品显示名替换扩散到协议、状态目录或升级身份；历史更新模块不代表存在用户入口 |
| TC-002 / TC-004 | [WorkBuddy UI](../../src/gui/workbuddy.rs)、[配置与备份](../../src/workbuddy_config.rs)、[协议转换](../../src/ai_gateway/workbuddy.rs)、[请求分发](../../src/ai_gateway/handler.rs) | `WORKBUDDY_CONFIG_PATH`；保留完整数组，兼容旧对象；独立 entryId、集合版本与备份；切换协议清理不适用缓存字段 |
| TC-003 | [上游请求及重试](../../src/ai_gateway/providers/mod.rs) | 重试不能叠加预算、切换渠道或重放成功建立的流 |
| TC-005 | [渠道配置](../../src/ai_gateway/config.rs)、[Chat 转换](../../src/ai_gateway/providers/deepseek_chat.rs)、[GUI](../../src/gui.rs) | `providerType=chat_completions`、`compatibility=openai_chat`、`chatDisableReasoning` |
| TC-006 | [模型目录](../../src/ai_gateway/model.rs)、[网关配置](../../src/ai_gateway/config.rs)、[Codex 页签](../../src/gui/codex_tab.rs)、[GUI](../../src/gui.rs) | `codexVisibleModels`、`codexModelProfiles`、`modelAliases`；继承不证明上游真实能力 |
| TC-008 | [账号登录](../../src/ai_gateway/chatgpt_auth.rs)、[WorkBuddy 配置](../../src/workbuddy_config.rs)、[浏览器启动](../../src/gui/browser.rs) | 保留账号引用和刷新，不把账号令牌变成普通 API Key；不能因移除更新入口破坏 OAuth |
| TC-009 | [导入模块](../../src/external_import.rs)、[网络](../../src/external_import/client.rs)、[IPC](../../src/external_import/ipc.rs)、[关联注册](../../src/external_import/registration.rs)、[导入 UI](../../src/gui/external_import.rs)、[macOS 事件](../../src/gui/external_import/macos.rs)、[macOS IPC](../../src/external_import/ipc/macos.rs)、[CLI](../../src/cli.rs) | `tiancaispacehub://import/v1`；`importSource` 持久保存来源；ticket 和待保存 Key 仅在内存；macOS 关联由 App 与 Launch Services 管理 |
| TC-010 | [配置读写](../../src/config.rs)、[本地 API](../../src/web.rs) | `_revision` 仅用于 API，不写 TOML；导入更新保留用户权重、超时、缓存及默认原有映射 |
| TC-011 | [模型配置](../../src/gmclaw_config.rs)、[页签](../../src/gui/gmclaw.rs)、[参数](../../src/ai_gateway/gmclaw.rs)、[推理状态](../../src/ai_gateway/gmclaw_replay.rs)、[流式聚合](../../src/ai_gateway/gmclaw_stream.rs)、[API](../../src/web.rs)、[路由](../../src/ai_gateway/router.rs) | `GMCLAW_CONFIG_PATH`；只管理既有数据库的旧 `tiancaispacehub` 与新 `tiancaispacehub-<entryId>`；`gmclawParameters` 仅存 Hub；`gmclaw`/`gmclaw:<entryId>` 全部排除普通路由与导入；备份元数据 v2 兼容 v1 |
| TC-012 | [Harness 客户端](../../src/gmclaw_executor.rs)、[IM 会话与审批](../../src/gmclaw_im.rs)、[分派](../../src/bridge.rs)、[状态](../../src/app_state.rs)、[配置](../../src/config.rs)、[页签](../../src/gui/gmclaw.rs) | `gmclawBridge` 默认禁用；用户设置天工启动环境 `GMCLAW_AUTH_TOKEN`；模型 ID 使用数据库行 ID；共享工作目录不等于文件隔离；不自动重试执行、不将断流视为取消 |
| TC-013 | [渠道身份](../../src/ai_gateway/config.rs)、[列表用途](../../src/gui/ai_gateway.rs)、[路由](../../src/ai_gateway/router.rs)、[WebSocket](../../src/ai_gateway/websocket/mod.rs) | 用途由保留名称派生，原始名称与展示标签分开；权重和粘性不能突破入口隔离，旧版不理解新 WorkBuddy 命名空间 |

## 沿用的上游能力

本地二开继续依赖 AI Gateway、IM remote-control、多执行端路由、Codex 初始化与恢复、增强启动、插件兼容、各厂商适配器及请求日志。Kimi K3、ChatGPT 账号登录、Responses WebSocket 和新版内置 GPT 目录来自上游合并；二开记录描述其与定制功能的衔接，不能把上游整项功能记为本地新开发。

相应专题均列在 [文档总入口](../README.md)。对这些模块做新的本地修复时，更新原专题并在本表新增或关联条目，而不是另建重复说明。

## 当前边界与后续衔接

- 仓库只保留 `main`，二开内容和上游整合统一进入主分支；版本通过完整二开版本标签及 GitHub Releases 管理，不再为版本建立分支。当前 `v0.4.30-2` 已发布 GitHub 预发布版，交付与验收分别记录；分支整理见 [仓库清理记录](../development/repository-cleanup.md)。
- Windows 优先，macOS 次之；Linux 不纳入新增需求。保留上游 Linux 工作流不代表本次开发和验收覆盖 Linux。
- `0.4.29-3` 用户反馈 Windows 本地导入正常；后续流程调整、其他站点及安装/升级/卸载待用户验收。`0.4.29-5` 当时只有 macOS 接入源码，未完成 Mac 构建/验收；当前 `v0.4.30-2` 已通过 macOS 原生 Actions 构建并交付 universal 包，实机导入仍待用户验收。
- 模型发现与推理调用分开；远端返回模型 ID 不等于证明所有协议和能力可用。导入不发起计费模型测试。
- Chrome 插件完整兼容、按模型选择 Remote Compact V2、Agent Manager 等研究资料不属于本表已交付功能。用户授权新任务后再更新状态。
- 当前测试由用户负责；已有历史测试数量仅属于对应版本，不能用于宣称后续变更已通过测试。
- 用户已确认上一轮天工模型保存成功，且无需完全退出再重开；该有限反馈不覆盖本轮多模型、流式聚合和外部消息执行端。历史调试 EXE 的大小与哈希保留在专题，不能作为本轮产物身份。

每次功能变更同时更新对应专题和 [变更记录](CHANGELOG.md)。新增能力扩充本表及 [索引](../README.md)，执行步骤见 [维护规范](../development/documentation.md)。
