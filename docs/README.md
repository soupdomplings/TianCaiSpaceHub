# TianCaiSpace Hub 文档总入口

维护日期：2026-09-30。当前工作区版本：`0.4.29-4`（本地交付版，未发布）；上游整合基线：CodexHub `v0.4.29`。本目录统一维护二开现状、使用说明、设计依据和开发记录。

## 从哪里开始

| 目的 | 文档 |
| --- | --- |
| 了解全部二开内容、代码入口与保留项 | [二开功能总表](customizations/README.md) |
| 查看每次二开开发变更 | [二开变更记录](customizations/CHANGELOG.md) |
| 开发时知道必须更新哪些文档 | [文档规划与维护规范](development/documentation.md) |
| 接入 WorkBuddy | [WorkBuddy 使用说明](workbuddy.md) |
| 设置自定义模型与能力继承 | [动态 Codex 可见模型](dynamic-codex-models.zh-CN.md) |
| 从 Sub2API 网页导入渠道 | [网页导入](hub-external-import.md)、[两端接口约定](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) |
| 查看品牌、桌面行为和打包约定 | [品牌、桌面与交付](customizations/desktop-and-packaging.md) |
| 了解最新上游合并 | [v0.4.29 整合记录](upstream-v0.4.29-integration.md) |
| 获取最新导入流程调整的本地交付信息 | [v0.4.29-4 Windows 交付版](releases/v0.4.29-4.md) |
| 查看前一版本 HTTP 导入修复及验收记录 | [v0.4.29-3 Windows 修复版](releases/v0.4.29-3.md) |
| 获取当前 GitHub 版本交付信息 | [v0.4.29-2 Windows 预发布](releases/v0.4.29-2.md) |
| 查看过时文档及分支清理结果 | [仓库清理记录](development/repository-cleanup.md) |

二开总表描述当前实现，变更记录描述版本演进，专题文档保存细节。下方技术资料同时包含上游已有实现、历史方案和调试记录；文档标题包含“计划”不代表已经实现。发生冲突时，先核对当前代码和用户最新决定，再修正文档，不能用旧方案覆盖现行行为。

当前平台重点为 Windows，macOS 次之，Linux 不纳入新增需求。当前开发与测试分工见 [维护规范](development/documentation.md)；构建成功和历史测试通过不能替代本次用户验收。

## 当前二开专题

| 文档 | 用途与状态 |
| --- | --- |
| [WorkBuddy](workbuddy.md) | 独立接入、备份还原、协议、重试、思考强度和缓存；已实现 |
| [OpenAI Chat Completions](openai-chat-completions.md) | 通用兼容渠道与按渠道关闭推理；已实现 |
| [动态模型](dynamic-codex-models.zh-CN.md) | 手填、远端获取、路由补齐和能力继承；已实现 |
| [网页导入](hub-external-import.md) | Windows 导入实现与交付；待用户端到端验收 |
| [导入协议 v1](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) | Hub 与 Sub2API 联调契约；不表示主站已上线 |
| [品牌、桌面与交付](customizations/desktop-and-packaging.md) | 天才空间品牌、窗口最大化、取消检查更新、平台和产物规则 |
| [v0.4.28 整合](upstream-v0.4.28-integration.md) | 历史上游整合记录 |
| [v0.4.29 整合](upstream-v0.4.29-integration.md) | 当前上游基线及二开衔接 |

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
