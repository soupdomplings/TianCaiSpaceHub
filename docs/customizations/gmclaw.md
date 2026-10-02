# 天工 Claw 模型接入

维护日期：2026-10-03。关联 TC-011。第二阶段保存点为 `9b9702e`；当前源码 `0.4.30-1` 在其上整合上游 `v0.4.30`，开发中、未发布。
实现状态：多模型管理、厂商参数适配与上游流式聚合已实现，本轮上游整合保留这些源码与行为。当前 Windows GUI/测试代码编译及调试 EXE 构建通过；未执行测试，端到端行为待用户验收。当前产物身份见 [整合专题](../upstream-v0.4.30-integration.md#本轮验证与产物)。

## 范围与兼容依据

依据本机安装的 `tiangong-desktop 1.1.1` 应用资源及只读数据库结构核对：模型保存在 `electron-data.db` 的 `model_configs` 表，Python Harness 使用 `OpenAICompatibleAdapter` 向 `<base_url>/chat/completions` 发起请求。Hub 提供独立的 Chat Completions 入口，复用转换器接入 Responses、Chat Completions、Anthropic Messages 等普通 API 渠道。

每个 Hub 管理条目拥有独立模型行、专用渠道和连接地址。同一个上游模型可以通过不同渠道保存为多个条目，也可以为同一来源保留不同参数的条目。模型名和别名用于模型选择，条目身份由独立 ID 决定。来源渠道的完整协议、模型映射及兼容选项在保存时复制；以后修改来源，需要对相应条目重新保存才能同步。

旧 `gmclaw` 渠道和新 `gmclaw:<entryId>` 渠道均排除普通 Codex 路由、可见模型同步和网页导入。WorkBuddy 继续使用自己的 `workbuddy` 渠道。旧入口只使用旧 `gmclaw`，新条目按地址中的 ID 精确选择对应渠道；同名模型不会跨条目随机路由。ChatGPT 账号登录渠道仍在来源列表中排除，保存 API 也拒绝该类型；本轮流式聚合不表示账号渠道已开放或完成验收。

第二阶段的飞书、微信、企业微信外部消息执行由 [天工外部消息执行端](gmclaw-im.md) 单独说明。第二阶段当前实现不包含 MCP 连接增删改查。

## 使用与多模型管理

1. 安装并至少打开一次天工 Claw，使其创建配置数据库。
2. 在 Hub「大模型接入」配置并启用普通 API 渠道，填写支持的模型或模型映射。
3. 打开「天工 Claw 接入」，刷新并核对配置路径。在已有条目中选择要修改的一项，或使用新增入口创建新条目。
4. 选择来源渠道与模型，核对协议、兼容配置和别名对应的实际模型。首次最大输出为 `8192`、温度为 `0.7`，思考强度默认自动，温度策略默认自动适配。具体参数范围由上游模型决定。
5. 保存时可选择是否设为天工默认模型；默认勾选。取消勾选表示保留当前默认选择，更新已经是默认的条目时不会把它取消为默认。
6. 保存后可直接在天工 Claw 中选择模型使用，并保持 Hub 运行。用户已确认上一轮模型保存后无需完全退出并重开天工；本轮移除强制重启提示，状态 `restartRequired` 固定为 `false`。
7. 可单独将已有条目设为默认或删除。删除只作用于选中的管理条目及对应 Hub 渠道；删除当前默认条目后不擅自选择其他默认模型。可使用还原入口撤销最近一次保存、删除或设为默认操作。

新建条目 ID 为小写 UUID，数据库模型 ID 为 `tiancaispacehub-<entryId>`，对应渠道为 `gmclaw:<entryId>`。新条目地址形如 `http://127.0.0.1:3847/ai-gateway/gmclaw/<entryId>/v1`；监听端口随 Hub 配置生成。旧条目的 `entryId=legacy`、模型 ID `tiancaispacehub`、渠道 `gmclaw` 和地址 `/ai-gateway/gmclaw/v1` 保留。

客户端 Key 固定为 `gmclaw-local`，满足客户端非空检查，本地网关不校验此值；真实上游凭据由 Hub 管理。更改 Hub 监听端口后，需要重新保存要继续使用的条目。模型配置操作不会退出或重启天工，也不读取其运行期鉴权 token。

## 上游流式响应聚合

天工 1.1.1 的模型客户端发送 `stream=false` 并需要完整 Chat JSON。Hub 对天工的 OpenAI Responses 转发启用上游 `stream=true`，收集完整响应、工具调用、用量及推理状态后，再返回天工需要的非流式结果。其他 Responses 类型保持原请求模式，但能够接收有效的 JSON 或 SSE；Chat Completions 保持原 `stream` 值，也能聚合实际返回的 SSE。该行为仅用于天工专用渠道的对应路径。

聚合按实际数据识别 JSON/SSE，兼容内容类型与正文不一致的情况；要求协议结束状态完整，断流、截断、上游失败事件、超时或无效内容都返回错误。收集上限为 `32 MiB`，条目数量有限制，使用渠道 `timeoutSecs` 的共同截止时间。成功响应开始后不会因读取失败重放请求。它不会把天工模型客户端改为逐字显示流式回答，也不修改天工安装资源。

## Hub 的厂商适配

2026-10-03 第二轮按用户要求优先处理 OpenAI、Claude 和 DeepSeek。WorkBuddy 作为已有转换与重试的参考；天工仍只发送简单的 Chat 请求，厂商差异由 Hub 按来源协议、兼容配置及解析后的模型处理。

| 来源 | 当前适配 |
| --- | --- |
| OpenAI Chat Completions | 输出上限转为 `max_completion_tokens`；保留原生工具消息；显式选择的思考强度写入 `reasoning_effort` |
| OpenAI Responses | 输出上限转为 `max_output_tokens`，Chat 历史和工具调用转为 Responses；需要的推理状态由 Hub 衔接 |
| Claude 原生 Messages | 沿用工具与缓存转换；新版模型的显式强度映射为 adaptive thinking 与 `output_config.effort`；已识别的旧思考模型改用固定思考预算 |
| DeepSeek Chat | 使用 `max_tokens`；显式思考设置转为 `thinking.type` 与 `reasoning_effort`；不注入 OpenAI 缓存参数 |
| DeepSeek Responses | 使用原生 `max_output_tokens` 和 `reasoning.effort`；不注入 OpenAI 缓存参数 |

思考强度「自动」不额外指定强度，沿用上游默认；不把所有模型强制设为 `high`。DeepSeek 提供 `none/low/high/max`，协议输入中的 `medium/xhigh` 映射为 `high`；原有 GLM Anthropic 兼容继续使用自身映射。来源渠道的 `chatDisableReasoning` 优先于页签强度；DeepSeek Chat 用原生 `thinking.type=disabled`，其他 Chat 用 `reasoning_effort=none`。原生 Claude 暂不提供强制关闭思考选项。

温度策略为 `auto/omit/preserve`：自动模式对已识别的 GPT-5/GPT-6、o1/o3/o4 推理模型及原生 Claude 省略温度，包括未显式指定强度或选择 `none` 的 GPT 请求；DeepSeek 默认按可能开启思考处理，显式关闭后保留温度。OpenAI/Claude 对应分支同时清理 `top_p/logprobs/top_logprobs`，DeepSeek 保留这些有效字段。GLM 兼容和未知模型保留原温度。不发送模式只省略温度；发送模式保留填写值，由上游校验，Hub 不在报错后擅自改参数重试。界面列出可配置档位，不代表每个第三方站点或模型均支持全部档位；`openai_chat` 兼容配置沿用现代输出上限字段，未知第三方服务是否支持仍待验收。

旧版 Claude 手动思考预算仅用于已识别的 3.7 Sonnet、4.0/4.1/4.5 系列：`low/medium/high/xhigh/max` 分别使用 `1024/2048/4096/8192/16384`，且不超过输出上限的一半；这是 Hub 的预算预设。启用时输出上限至少 `2048`。输出上限本身保持用户设置，新版 Claude 不发送旧预算字段。

官方规则参考：[OpenAI 参数兼容](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.2)、[Claude 手动思考](https://platform.claude.com/docs/en/build-with-claude/extended-thinking)、[DeepSeek 思考模式](https://api-docs.deepseek.com/guides/thinking_mode/)。这些规则不等同于第三方转发服务的实测保证。

### 工具轮次与错误恢复

天工 1.1.1 的消息清理会丢弃 `reasoning_content` 和 Responses 推理签名。Hub 在非流式响应返回前，把相应 Chat 推理字段或 Responses 原始输出暂存在内存；后续请求按渠道身份、真实模型及完整历史前缀匹配后恢复。不会仅凭工具 ID 复用另一段历史，也不伪造缺失的签名。

状态最多保留 128 轮、16 MiB，单轮最多 1 MiB，创建后 1 小时过期；不另外写入持久化文件。本机天工在进入一次任务的工具循环前构建系统提示，循环内复用该历史，因此恢复主要面向同一次任务的工具续轮。下一条用户消息重建系统时间/记忆、历史裁剪或中间件改写、Hub 重启与缓存过期都可能导致严格匹配失败；此时不注入旧状态，不能宣称跨用户轮次、压缩或重启后的推理状态均可恢复。若因此遇到续轮错误，重新开始相关会话。请求详情日志仍遵循既有日志开关。

HTTP `502/503` 及已有可恢复连接错误在每轮原样转发中共用最多 2 次额外尝试；HTTP 重试分别等待 1 秒、2 秒。保持同一渠道、模型和请求体，不自动切换模型。`400/401/403/422/429` 等不进入这项通用 HTTP 重试，成功响应开始后不因读取失败或流中断重放。Responses 既有精确 `400 Failed to read request body` 恢复及旧密文清理仍保留，属于另行计数的兼容修复，不能把整个业务请求描述为严格最多 3 次。

上游 HTTP 200 内嵌失败或无效 Chat JSON 会返回明确网关错误，不再记为空的成功回答；持续失败保留上游错误，便于检查配置。

## 配置、备份与并发

- Windows 默认路径为 `%APPDATA%/tiangong-desktop/electron-data.db`；macOS 路径约定为 `$HOME/Library/Application Support/tiangong-desktop/electron-data.db`，尚未在 Mac 实机核对。`GMCLAW_CONFIG_PATH` 可指定既有数据库的完整路径。
- 不创建数据库、不迁移天工表结构。只管理旧专用行及 `tiancaispacehub-<entryId>` 管理行、必要的默认选择；保留其他模型、目标行未知字段及额外参数，不改聊天、MCP、账号表。思考强度与温度策略只保存在 Hub 的 `gmclawParameters`，不依赖天工未透传的 `extra_params`。
- 页面 `revision` 覆盖全部管理模型行、默认模型选择及全部天工专用 Hub 渠道。任一管理条目发生变化都要求刷新；普通 Hub 渠道修改、普通天工模型的非默认字段修改不会使该版本失效。GET 会重读 Hub 配置；实际写入还受全量配置 `_revision`、文件锁与 SQLite 事务保护。
- 新来源渠道关联指纹忽略 `is_active` 和 `updated_at`，因此只在天工内切换默认模型或更新时间不会丢失来源显示，其他模型字段仍需匹配。旧 v1/早期 v2 完整行指纹兼容仅改变默认标志的情况；若旧记录的时间戳也已改变，需在 Hub 明确重选来源并保存一次。备份和页面并发版本仍完整核对行内容与默认标志，不放宽写入冲突或还原检查。
- 备份与数据库同目录，名称为 `electron-data.db.tiancaispacehub-backup.json`，自定义路径则在数据库文件名后追加同一后缀。备份只保存目标行、此前默认模型和对应 Hub 渠道，不复制聊天数据库；渠道快照可能含凭据，应作为私密本地配置保管。
- 每次成功保存、删除或设为默认替换上次备份，成功还原后消耗该次备份，不提供多级撤销。还原核对目标行、默认选择和对应渠道是否仍为操作后的状态，拒绝覆盖后续目标修改；其他条目和普通渠道保留。
- 新元数据版本为 `2`，兼容读取原版本 `1` 的单模型备份；旧备份仍按 `legacy` 的原数据库哈希校验。新页面版本为 `gmclaw-v3`；旧页面必须刷新，不能把旧 UI token 当作新集合版本。缺失的管理条目不能通过指定旧 ID 意外重建，应明确新增。
- Hub TOML 保存失败时回滚 SQLite 并恢复备份文件；数据库提交失败时尽力补偿 Hub 配置。文件与数据库不具备进程崩溃时的统一原子提交，失败后应刷新并核对两端状态。

降级前应先在当前版本撤销或删除不再使用的新条目，并备份本地配置。旧版本不理解 `gmclaw:<entryId>` 的独立路由及版本 `2` 元数据，不能保证读写新备份；用户自己的普通天工模型不因降级自动改变。

## 本地配置 API

| 入口 | 请求与行为 |
| --- | --- |
| `GET /api/gmclaw/config` | 返回 `entries` 和 `selectedEntryId`，默认优先选择当前管理默认条目，其次旧条目、首项；`?entryId=<id>` 指定编辑项，空值表示新建视图 |
| `POST /api/gmclaw/config` | `entryId` 省略、`null` 或空字符串表示新增；指定已有 ID 表示更新。其余为 `sourceProvider`、`model`、`maxTokens`、`temperature`、`parameters`、`makeActive`、`revision`；`makeActive` 默认 `true` |
| `POST /api/gmclaw/config/activate` | 接收 `entryId` 和 `revision`，设为默认并产生备份 |
| `POST /api/gmclaw/config/delete` | 接收 `entryId` 和 `revision`，删除目标并产生备份 |
| `POST /api/gmclaw/config/restore` | 接收 `revision`，撤销最近一次操作 |

每项 `entries[]` 返回 `entryId`、`modelId`、`model`、`sourceProvider`、`parameters`、`active`、`configured` 和 `localUrl`。原顶层 `model/sourceProvider/parameters/active/localUrl` 保留，表示当前选中条目；未选中时为空或使用默认参数。参数结构为可省略的 `reasoningEffort` 与 `temperatureMode`（`auto/omit/preserve`）。不返回上游凭据或原数据库行快照。

## 维护定位与阶段范围

- [配置、SQLite 事务及备份](../../src/gmclaw_config.rs)、[渠道身份](../../src/ai_gateway/config.rs)
- [页签及异步操作](../../src/gui/gmclaw.rs)、[本地 API](../../src/web.rs)
- [专用入口](../../src/ai_gateway.rs)、[请求分发](../../src/ai_gateway/handler.rs)、[路由隔离](../../src/ai_gateway/router.rs)
- [参数策略](../../src/ai_gateway/gmclaw.rs)、[推理状态](../../src/ai_gateway/gmclaw_replay.rs)、[流式聚合](../../src/ai_gateway/gmclaw_stream.rs)、[重试](../../src/ai_gateway/providers/mod.rs)

天工自有 MCP 连接和 OAuth 设置继续由天工管理。用户已明确本轮第二阶段专注 Hub 外部消息执行，不把 MCP 配置管理列为默认后续目标。外部消息可以触发天工已有工具，只有天工自身策略返回待确认事件时才进入 Hub 的 IM 审批流程；详见 [TC-012](gmclaw-im.md)。

## 验证状态

当前 `0.4.30-1` 上游整合后的编译、构建、静态产物核对及唯一 EXE 身份见 [v0.4.30 整合记录](../upstream-v0.4.30-integration.md#本轮验证与产物)。下列两个 EXE 仅保留历史阶段归属，不能用于识别当前调试输出。

第二阶段保存点 `9b9702e` 曾通过 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin codexhub` 和 `cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin codexhub`。该阶段格式、差异空白和文档本地链接核对通过；GUI 编译保留 23 条警告，测试代码编译保留 51 条警告，主要为未使用代码。已新增隔离 SQLite、多条目路由、JSON/SSE 聚合、Harness 事件与审批等夹具，按用户分工未执行测试。Windows 交互和端到端行为待用户验收；macOS 尚待构建及实机验证，不新增 Linux 验收范围。

`9b9702e` 历史调试程序大小为 `55,158,784` 字节，SHA-256 `81f887e1140daf5f2c0146ffd05cab31cc5994da182c66c47f9e4d1959884872`。当时静态确认 Windows x64 PE、GUI 子系统与直接导入 DLL 文件；Windows API-set 属于系统虚拟契约，未运行程序验证加载器。清单 `.build-tools/gmclaw-feedback-build-manifest.json`，编译/构建日志 `.build-tools/gmclaw-feedback-check.log` 和 `.build-tools/gmclaw-feedback-build.log` 均不入仓库。共享输出路径 `target/x86_64-pc-windows-msvc/debug/codexhub.exe` 已由后续构建覆盖。用户测试当前版本前先退出旧 Hub（含托盘），并核对整合专题中的产物；模型保存后无需为此重启天工。

用户已确认上一轮模型配置保存成功，保存后无需完全退出并重开天工。这是旧单模型版本的有限验收，不推定本轮新增、修改、删除、默认切换、备份恢复、流式上游、工具续轮或 IM 执行通过。

更早厂商适配代码曾通过 Windows GUI/测试代码编译与调试 EXE 构建。该历史 EXE 为 `54,261,760` 字节，SHA-256 `141eaa28fe535fa95113b5f9609c8e82c2a0353c0add3d69cedd6b4f9edf8196`，对应 `3b3f213` 加当时未提交变更；不是多模型/外部消息保存点或当前上游整合产物。构建日志与清单位于 Git 忽略的 `.build-tools/`，应按对应阶段清单识别产物。

开发过程中未启动 Hub/Codex/天工进行交互验证，未发起真实模型或 Harness 执行请求，未修改用户实际模型配置。安装包仍统一由 GitHub Actions 生成，本轮尚未发布，也未在本地生成发布安装包。
