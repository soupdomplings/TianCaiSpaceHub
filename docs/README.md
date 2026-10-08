# TianCaiSpaceHub 文档总入口

维护日期：2026-10-08。当前开发基线统一为 `v0.4.30-5`，只维护唯一 `main`，上游来源仍为 CodexHub `v0.4.30`。本轮新增 [NVWA MCP 接入（TC-014）](customizations/nvwa-mcp.md)：Hub 账号密码登录、浏览器授权、应用代表用户、本机桥和 Codex/WorkBuddy/天工受管 MCP 接入。功能源码已实现，Windows 必要 GUI 编译已通过，最终复核另记；真实测试由用户负责，尚未发布本轮 Release。源码、编译、安装包和验收分别见 [v0.4.30-5 开发交付](releases/v0.4.30-5.md)。

此前最终 `v0.4.30-4` 产品、CI 修复和发布核验已整体并入当前基线；桌面能力识别、专用模型入口、IM 会话与审批、天工任务/历史/消息同步及全部既有二开继续保留。历史标签、包和用户反馈保持原身份，见 [v4 交付](releases/v0.4.30-4.md)，不作为新增 NVWA 功能的验收证据。Windows 优先、macOS 次之，不扩展 Linux；本地不生成发布安装包。

工作区已保存对齐前快照并恢复最终 `main`。本地与 live `origin` 当前仅有 `main`，没有其他待删除/合并分支；完整版本通过标签和 GitHub Releases 管理，不建立版本分支。详情见 [仓库清理记录](development/repository-cleanup.md)。

## 从哪里开始

| 目的 | 文档 |
| --- | --- |
| 了解全部二开内容、代码入口与保留项 | [二开功能总表](customizations/README.md) |
| 查看每次二开开发变更 | [二开变更记录](customizations/CHANGELOG.md) |
| 开发时知道必须更新哪些文档 | [文档规划与维护规范](development/documentation.md) |
| 登录 NVWA、检测 MCP、分别接入三客户端 | [NVWA MCP](customizations/nvwa-mcp.md)、[资料索引](nvwa/README.md) |
| 查看当前 v5 基线、编译与用户待验收内容 | [v0.4.30-5 开发交付](releases/v0.4.30-5.md) |
| 配置 WorkBuddy 模型、启动桌面及了解 `/wb` 边界 | [WorkBuddy 使用说明](workbuddy.md) |
| 区分普通渠道与 WorkBuddy/天工专用渠道 | [接入点用途与隔离](customizations/client-channel-scope.md) |
| 获取当前 GitHub 交付、两平台安装包与验收边界 | [v0.4.30-4 发布交付](releases/v0.4.30-4.md) |
| 查看此前已发布包及原版本身份 | [v0.4.30-3 发布交付](releases/v0.4.30-3.md) |
| 管理天工 Claw 多模型、参数、备份及上游流式兼容 | [天工 Claw 模型接入](customizations/gmclaw.md) |
| 启停 Hub/天工、选择完整项目目录、回复状态、会话同步与审批 | [天工外部消息执行端](customizations/gmclaw-im.md) |
| 区分天工模型配置、本地模型连接、IM 授权与上游流式 | [天工 Claw 模型接入](customizations/gmclaw.md)、[请求日志](ai-gateway-request-log-detail-patch.zh-CN.md) |
| 设置自定义模型与能力继承 | [动态 Codex 可见模型](dynamic-codex-models.zh-CN.md) |
| 从 Sub2API 网页导入渠道 | [网页导入](hub-external-import.md)、[两端接口约定](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) |
| 查看品牌、桌面行为和打包约定 | [品牌、桌面与交付](customizations/desktop-and-packaging.md) |
| 了解最新上游合并及待验收项 | [v0.4.30 整合记录](upstream-v0.4.30-integration.md) |
| 查看 macOS 网页导入的历史开发状态 | [0.4.29-5 开发记录](releases/v0.4.29-5.md)，当前交付见 [v0.4.30-3](releases/v0.4.30-3.md) |
| 获取上一版 Windows 导入流程调整的本地包信息 | [v0.4.29-4 Windows 交付版](releases/v0.4.29-4.md) |
| 查看前一版本 HTTP 导入修复及验收记录 | [v0.4.29-3 Windows 修复版](releases/v0.4.29-3.md) |
| 追溯早期 GitHub 版本交付信息 | [v0.4.29-2 Windows 历史预发布](releases/v0.4.29-2.md) |
| 查看过时文档及分支清理结果 | [仓库清理记录](development/repository-cleanup.md) |

二开总表描述当前实现，变更记录描述版本演进，专题文档保存细节。下方技术资料同时包含上游已有实现、历史方案和调试记录；文档标题包含“计划”不代表已经实现。发生冲突时，先核对当前代码和用户最新决定，再修正文档，不能用旧方案覆盖现行行为。

当前平台重点为 Windows，macOS 次之，Linux 不纳入新增需求。当前开发与测试分工见 [维护规范](development/documentation.md)；构建成功和历史测试通过不能替代本次用户验收。

## 当前二开专题

| 文档 | 用途与状态 |
| --- | --- |
| [NVWA MCP](customizations/nvwa-mcp.md) | TC-014 / v0.4.30-5：Hub 账号密码、浏览器授权、应用代表用户；独立本机桥、系统保护凭据、三端受管配置与恢复；Windows 必要编译已通过，真实测试待用户，本轮未发布 |
| [WorkBuddy](workbuddy.md) | 模型多条目接入已预发布；v0.4.30-3：接入页提供显式启动桌面按钮，用户本次反馈该按钮可启动，仅覆盖该次本机操作；其他机器及 macOS 待验收。`/wb` 仍提示不可用，外部任务执行尚未接通 |
| [接入点用途与隔离](customizations/client-channel-scope.md) | 普通/专用渠道标识、各接入点及同模型条目的精确路由、保留名称与回滚注意项 |
| [天工 Claw 模型](customizations/gmclaw.md) | 多条目与独立路由、保存/删除/默认切换恢复、厂商参数及上游流式聚合已预发布；v0.4.30-3：概览按当前专用模型请求、配置 URL 和桌面进程区分等待/已连接，与上游成功和 IM 授权独立。旧单模型保存无需重启已有有限反馈；新增行为待用户验收 |
| [天工外部消息执行端](customizations/gmclaw-im.md) | v0.4.30-3：Windows 监听/启动禁止句柄继承，退出只结束自有后台，旧继承实例需一次正常迁移；目录候选含无任务的原生项目并完整分页，飞书/有效企微回调显示临时准备状态并终态收尾、微信直接文字；启用后每 5 秒后台恢复同用户官方天工运行授权，不自行启动或重放；复用原平台会话交互，创建真实任务、保存消息及恢复全部场景任务。原记忆、精确模型、认领及运行/审批/未知保护保留，原生步数 `1..1000` 透传。`/gpt` 释放认领但保留会话，`/tg` 返回重新认领；`/s` 不伪造取消。本轮修复图标注册导致的安装误判，移除每轮固定等待回复，并加入局部消息同步；v0.4.30-4 标签源码取消固定版本/资源门槛，改核对已安装天工身份、renderer HTML 脚本入口及运行时字段能力；首次从 Hub 启动启用本机通道，普通手动实例仍可 IM 但无通道不能即时刷新。用户已有限反馈自动重连、飞书历史对话及 `fe48829e…` 桌面消息更新；用户已确认 `71c3043d…` 审批测试通过，仅属于该次操作，其他平台、失败回退和身份隔离待验收；本轮修复端口释放、补齐含无任务项目的目录候选，并新增按平台能力收尾的临时回复状态，这些新增行为待验收，不新增永久或单工具授权。原 `/gmclaw` 首版已预发布，本轮状态见品牌专题，开发方未执行测试 |
| [请求日志详情](ai-gateway-request-log-detail-patch.zh-CN.md) | v0.4.30-3：保留 `stream` 的客户端语义，新增可空 `upstreamStream` 记录实际最终上游请求；天工非流式客户端与上游流式聚合分别展示，旧日志上游模式未知 |
| [OpenAI Chat Completions](openai-chat-completions.md) | 通用兼容渠道与按渠道关闭推理；已实现 |
| [动态模型](dynamic-codex-models.zh-CN.md) | 手填、远端获取、路由补齐和能力继承；已实现 |
| [网页导入](hub-external-import.md) | Windows/macOS 系统接入已随 v0.4.30-2 成功构建打包并预发布；本版实机导入待用户验收 |
| [导入协议 v1](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) | Hub 与 Sub2API 联调契约；不表示主站已上线 |
| [品牌、桌面与交付](customizations/desktop-and-packaging.md) | v0.4.30-3：产品与程序统一 TianCaiSpaceHub；配置与升级身份兼容、改名后迁移和回滚；接入页签概览与天工/WorkBuddy 显式启动按钮不改变 IM 选择；窗口最大化、取消检查更新及平台产物规则 |
| [v0.4.28 整合](upstream-v0.4.28-integration.md) | 历史上游整合记录 |
| [v0.4.29 整合](upstream-v0.4.29-integration.md) | 历史上游整合记录 |
| [v0.4.30 整合](upstream-v0.4.30-integration.md) | 当前上游基线、12 项二开保留矩阵、冲突取舍和验收边界 |

## 通用架构与配置

- [架构](architecture.md)、[配置](configuration.md)、[问题排查](troubleshooting.md)。
- [鉴权说明（中文）](auth-notes.zh-CN.md)、[鉴权说明（英文）](auth-notes.md)。
- [Remote-Control 协议审计](remote-control-protocol-audit.zh-CN.md)、[多执行端路由](remote-control-multi-endpoint-routing.zh-CN.md)、[IM 事件流隔离](im-remote-control-stream-isolation.zh-CN.md)。
- [Telegram 接入设计](telegram-integration-plan.zh-CN.md)、[微信接入设计](wechat-integration-plan.zh-CN.md)。
- [发布检查表](release-checklist.md)；对外摘要仍由根目录 [RELEASE_NOTES.md](../RELEASE_NOTES.md) 和 [UPDATE_NOTES.md](../UPDATE_NOTES.md) 提供。
- [GUI 主题](gui-theme.zh-CN.md)、[GUI 与 wxDragon 维护](gui-runtime-maintenance.zh-CN.md)。

## AI Gateway 技术资料

这些资料主要来自上游积累，二开沿用和维护，不能全部标为本地原创功能。对应当前定制见二开总表。

| 主题 | 文档 |
| --- | --- |
| 总体架构与实现 | [网关架构](ai-gateway-architecture.zh-CN.md)、[实现记录](ai-gateway-impl.zh-CN.md)、[Provider 适配器设计](ai-gateway-provider-adapter-design.zh-CN.md) |
| Anthropic / Claude | [Messages 转换](ai-gateway-anthropic-messages.zh-CN.md)、[缓存](ai-gateway-anthropic-cache-control.zh-CN.md)、[流式和 GLM 缓冲](ai-gateway-anthropic-streaming-glm-buffering.zh-CN.md)、[历史实施路线](ai-gateway-anthropic-first-roadmap.zh-CN.md) |
| 智谱 / GLM | [接入](ai-gateway-glm-anthropic-integration.zh-CN.md)、[搜索](ai-gateway-glm-anthropic-web-search.zh-CN.md) |
| DeepSeek / Kimi | [DeepSeek Responses](ai-gateway-deepseek-responses.zh-CN.md)、[Kimi Responses](ai-gateway-kimi-responses.zh-CN.md) |
| Grok | [Responses 工具](ai-gateway-grok-responses-tools.zh-CN.md)、[协议转换研究](ai-gateway-grok-build-protocol-conversion-reference.zh-CN.md) |
| ChatGPT 账号与 WebSocket | [账号登录](ai-gateway-chatgpt-auth.zh-CN.md)、[Responses WebSocket](ai-gateway-chatgpt-websocket.zh-CN.md) |
| 搜索、工具与生图 | [Web Search 协议](ai-gateway-web-search-protocol.zh-CN.md)、[Responses Lite 搜索](ai-gateway-responses-lite-web-search.zh-CN.md)、[第三方 tool_search](ai-gateway-third-party-tool-search-plan.zh-CN.md)、[生图工具](ai-gateway-image-generation-5.5-5.6.zh-CN.md) |
| 会话状态与压缩 | [加密内容作用域](ai-gateway-encrypted-content-scope.zh-CN.md)、[Provider 私有状态](ai-gateway-provider-private-state-conversion.zh-CN.md)、[Compact V2 研究](ai-gateway-compact-v2-portable-summary.zh-CN.md) |
| 诊断与目录参考 | [请求日志详情](ai-gateway-request-log-detail-patch.zh-CN.md)、[第三方模型基线示例](third-party-model-baseline.example.json) |

## Codex 客户端兼容资料

- [模型可见性和 Provider 取舍](codex-app-web-run-model-visibility-tradeoff.zh-CN.md)。
- [CDP 模型诊断](codex-app-cdp-model-diagnostics.zh-CN.md)、[增强启动语言问题复盘](codex-app-enhanced-language-debugging-postmortem.zh-CN.md)。
- [Statsig 兼容与历史快速启动方案](codex-app-fast-startup-statsig.zh-CN.md)：旧 `localhost:8000` 方案已经废弃，阅读顶部状态说明。
- [插件兼容收敛记录](codex-app-offline-plugins-plan.md)：包含已撤销方向及分阶段记录，不能据此宣称全部插件兼容完成。

## 设计、历史与待办

| 文档 | 阅读定位 |
| --- | --- |
| [厂商图标来源](provider-logo-assets.zh-CN.md) | 资源来源与维护说明 |
| [Agent Manager 计划](agent-manager-mvp-plan.md) | 未排期设计，TreeCtrl 尚无有效验收记录，不作为已交付功能清单 |
| [Agent Manager 示意图](mockups/agent-manager-wx.svg) / [PNG](mockups/agent-manager-wx.png) | 设计资源 |
| [历史待办](../todolist.md) | 原研究记录保留；新增二开开发统一登记到本目录 |

新专题按 [维护规范](development/documentation.md) 归档并加入本页。同一功能优先更新原专题；删除、替代旧文件必须修复引用并记录去向。本轮清理见 [仓库清理记录](development/repository-cleanup.md)。
