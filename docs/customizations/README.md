# 二开功能总表

维护日期：2026-09-30。范围：TianCaiSpace Hub 相对于 CodexHub 上游的定制，以及上游升级时必须保留的兼容衔接。

当前源码版本 `0.4.29-4`（本地包已交付，尚无对应 GitHub Release），在 `e25ecd9`（发布标签 `v0.4.29-2`）之后加入 HTTP 兼容修复及导入流程精简，随本次提交归档到 `soupdomplings/TianCaiSpaceHub` 的 `main`。以下“已实现”仅指代码存在；构建与验收记录见 [变更记录](CHANGELOG.md) 和 [本地交付说明](../releases/v0.4.29-4.md)，已发布版本见 [v0.4.29-2](../releases/v0.4.29-2.md)。

## 已实现的二开能力

编号用于后续需求、修复和合并记录引用，已有编号不复用。

| 编号 | 内容 | 当前行为与必须保留的约束 | 详细说明 |
| --- | --- | --- | --- |
| TC-001 | 天才空间品牌与打包 | 产品显示名和图标使用 TianCaiSpace Hub；保留 `codexhub` 内部标识及安装升级身份；支持平台产物及未签名测试包 | [品牌、桌面与交付](desktop-and-packaging.md) |
| TC-002 | WorkBuddy 独立接入 | 单独页签、Chat Completions 本地入口、专用 `workbuddy` 渠道、上游选择、配置备份和还原；普通 Codex 路由排除该专用渠道 | [WorkBuddy](../workbuddy.md) |
| TC-003 | WorkBuddy 错误重试 | 上游 HTTP 502/503 最多额外重试两次，等待 1 秒、2 秒；与传输错误共用预算，同一上游和请求，流已建立后不重发 | [WorkBuddy](../workbuddy.md) |
| TC-004 | WorkBuddy 思考强度与缓存 | 模型、别名和协议联动；Claude 五档；保留有效默认值；OpenAI 缓存键按优先级回退，Anthropic 用原生 `cache_control` | [WorkBuddy](../workbuddy.md) |
| TC-005 | 通用 Chat Completions | 新建通用渠道使用 `compatibility=openai_chat`；保留旧 DeepSeek Chat 行为；按渠道关闭推理，不改全局 | [Chat Completions](../openai-chat-completions.md) |
| TC-006 | 动态 Codex 模型 | 手动新增、同步渠道、远端获取、补齐选定渠道路由；未知模型按家族继承能力，手工覆盖优先；可见模型与路由分开管理 | [动态模型](../dynamic-codex-models.zh-CN.md) |
| TC-007 | 桌面启动与升级方式 | 启动自动最大化；帮助菜单、托盘及启动自动检查更新均取消；保留主动安装新版的打包能力 | [品牌、桌面与交付](desktop-and-packaging.md) |
| TC-008 | 上游升级的二开衔接 | 保留品牌、WorkBuddy、动态模型和桌面行为；衔接 Kimi、ChatGPT 账号凭证、账号模型发现及 OAuth 浏览器启动 | [v0.4.28](../upstream-v0.4.28-integration.md)、[v0.4.29](../upstream-v0.4.29-integration.md) |
| TC-009 | Sub2API 网页渠道导入 | Windows 协议唤起、单实例内存转交、一次性码兑换、预览前模型查询；任意兼容 HTTP/HTTPS 站点直接进入预览准备，无环境开关或来源/跨站查询确认；预览中确认保存，默认禁用，一次一个渠道；同名处理及明确更新选择 | [网页导入](../hub-external-import.md)、[契约 v1](../HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) |
| TC-010 | 导入带来的配置并发保护 | 保存重读最新配置，只合并目标；目标指纹防止覆盖预览期间修改；全量 API 保存带版本，文件锁及原子替换；空模型导入渠道不能事后直接启用 | [网页导入](../hub-external-import.md)、[配置](../configuration.md) |

## 代码与配置定位

| 范围 | 入口 | 配置或兼容注意点 |
| --- | --- | --- |
| TC-001 / TC-007 | [GUI](../../src/gui.rs)、[托盘](../../src/gui/tray.rs)、[Windows MSI](../../packaging/windows/CodexHub.wxs)、[macOS 信息](../../packaging/macos/Info.plist)、[打包脚本](../../scripts/package-hub-import.ps1) | 不把产品显示名替换扩散到协议、状态目录或升级身份；历史更新模块不代表存在用户入口 |
| TC-002 / TC-004 | [WorkBuddy UI](../../src/gui/workbuddy.rs)、[配置与备份](../../src/workbuddy_config.rs)、[协议转换](../../src/ai_gateway/workbuddy.rs)、[请求分发](../../src/ai_gateway/handler.rs) | `WORKBUDDY_CONFIG_PATH`；`models.json` 数组首项、旧单对象兼容、两个备份文件；切换协议时清理不适用缓存字段 |
| TC-003 | [上游请求及重试](../../src/ai_gateway/providers/mod.rs) | 重试不能叠加预算、切换渠道或重放成功建立的流 |
| TC-005 | [渠道配置](../../src/ai_gateway/config.rs)、[Chat 转换](../../src/ai_gateway/providers/deepseek_chat.rs)、[GUI](../../src/gui.rs) | `providerType=chat_completions`、`compatibility=openai_chat`、`chatDisableReasoning` |
| TC-006 | [模型目录](../../src/ai_gateway/model.rs)、[网关配置](../../src/ai_gateway/config.rs)、[Codex 页签](../../src/gui/codex_tab.rs)、[GUI](../../src/gui.rs) | `codexVisibleModels`、`codexModelProfiles`、`modelAliases`；继承不证明上游真实能力 |
| TC-008 | [账号登录](../../src/ai_gateway/chatgpt_auth.rs)、[WorkBuddy 配置](../../src/workbuddy_config.rs)、[浏览器启动](../../src/gui/browser.rs) | 保留账号引用和刷新，不把账号令牌变成普通 API Key；不能因移除更新入口破坏 OAuth |
| TC-009 | [导入模块](../../src/external_import.rs)、[网络](../../src/external_import/client.rs)、[IPC](../../src/external_import/ipc.rs)、[关联注册](../../src/external_import/registration.rs)、[导入 UI](../../src/gui/external_import.rs)、[CLI](../../src/cli.rs) | `tiancaispacehub://import/v1`；`importSource` 持久保存来源；ticket 和待确认 Key 仅在内存 |
| TC-010 | [配置读写](../../src/config.rs)、[本地 API](../../src/web.rs) | `_revision` 仅用于 API，不写 TOML；导入更新保留用户权重、超时、缓存及默认原有映射 |

## 沿用的上游能力

本地二开继续依赖 AI Gateway、IM remote-control、多执行端路由、Codex 初始化与恢复、增强启动、插件兼容、各厂商适配器及请求日志。Kimi K3、ChatGPT 账号登录、Responses WebSocket 和新版内置 GPT 目录来自上游合并；二开记录描述其与定制功能的衔接，不能把上游整项功能记为本地新开发。

相应专题均列在 [文档总入口](../README.md)。对这些模块做新的本地修复时，更新原专题并在本表新增或关联条目，而不是另建重复说明。

## 当前边界与后续衔接

- Windows 优先，macOS 次之；Linux 不纳入新增需求。保留上游 Linux 工作流不代表本次开发和验收覆盖 Linux。
- 网页导入 Hub 侧已实现，`0.4.29-3` 用户反馈本地导入正常；当前 `0.4.29-4` 流程精简、其他站点及安装/升级/卸载待用户验收；macOS 网页唤起尚未开发。
- 模型发现与推理调用分开；远端返回模型 ID 不等于证明所有协议和能力可用。导入不发起计费模型测试。
- Chrome 插件完整兼容、按模型选择 Remote Compact V2、Agent Manager 等研究资料不属于本表已交付功能。用户授权新任务后再更新状态。
- 当前测试由用户负责；已有历史测试数量仅属于对应版本，不能用于宣称后续变更已通过测试。

每次功能变更同时更新对应专题和 [变更记录](CHANGELOG.md)。新增能力扩充本表及 [索引](../README.md)，执行步骤见 [维护规范](../development/documentation.md)。
