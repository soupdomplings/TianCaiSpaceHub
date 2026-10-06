# 天工 Claw 模型接入

维护日期：2026-10-06。关联 TC-011。当前能力纳入 `v0.4.30-3` 发布候选，承接已发布 `v0.4.30-2` 与其后全部本地二开；上游仍为 CodexHub `v0.4.30`。多模型独立路由、厂商参数、JSON/SSE 聚合与接入点隔离继续保留，新增模型概览的本地连接证据、客户端/上游 stream 分开记录，以及 [天工外部消息](gmclaw-im.md) 的共用会话、自动连接、原生同步和平台审批。实际提交、Windows/macOS Actions 和附件见 [v0.4.30-3 交付记录](../releases/v0.4.30-3.md)，不以旧版 Actions 或本地 EXE 冒充本版产物。

用户此前对自动重连、历史对话、`fe48829e…` 桌面更新和 `71c3043d…` 审批给出有限反馈，最新对 `c0952f61…` Windows 调试程序测试反馈“目前看着没什么了”并授权发布。仅登记各次本机实际操作，没有全平台/异常验收结论；该调试 EXE 身份仍为 `0.4.30-2`，新候选编译和发布包另行记录。历史诊断、失败和原“未发布”记录保留当时归属，当前实现/限制以正文为准。

## 范围与兼容依据

依据本机安装的 `tiangong-desktop 1.1.1` 应用资源及只读数据库结构核对：模型保存在 `electron-data.db` 的 `model_configs` 表，Python Harness 使用 `OpenAICompatibleAdapter` 向 `<base_url>/chat/completions` 发起请求。Hub 提供独立的 Chat Completions 入口，复用转换器接入 Responses、Chat Completions、Anthropic Messages 等普通 API 渠道。

每个 Hub 管理条目拥有独立模型行、专用渠道和连接地址。同一个上游模型可以通过不同渠道保存为多个条目，也可以为同一来源保留不同参数的条目。模型名和别名用于模型选择，条目身份由独立 ID 决定。来源渠道的完整协议、模型映射及兼容选项在保存时复制；以后修改来源，需要对相应条目重新保存才能同步。

旧 `gmclaw` 渠道和新 `gmclaw:<entryId>` 渠道均排除普通 Codex 路由、可见模型同步和网页导入。WorkBuddy 继续使用自己的 `workbuddy` 渠道。旧入口只使用旧 `gmclaw`，新条目按地址中的 ID 精确选择对应渠道；同名模型不会跨条目随机路由。ChatGPT 账号登录渠道仍在来源列表中排除，保存 API 也拒绝该类型；本轮流式聚合不表示账号渠道已开放或完成验收。

第二阶段的飞书、微信、企业微信外部消息执行由 [天工外部消息执行端](gmclaw-im.md) 单独说明。第二阶段当前实现不包含 MCP 连接增删改查。

v0.4.30-3 支持在 IM 发送 `/tg` 自动准备连接，并复用 Codex 原有平台会话流程、卡片、目录表单及模型选择。天工页提供模型配置、启动按钮与聊天指引，不提供「IM 桥接」表单；概览随接入页签切换。2026-10-06 模型概览改为识别本地连通：配置指向当前 Hub 的天工专用入口、当前天工桌面进程仍在，且本次 Hub 运行已观察到该入口携带天工本地 Key 的有效模型请求，即显示绿色「已连接」；不用真实模型调用成功作为门槛。不具备请求记录时显示「等待天工连接」，天工退出时显示「天工未启动」；Hub 重启、桌面进程更换或本地入口变化后重新识别。模型连通和 Harness 外部执行就绪分别显示，真实模型效果仍由用户在天工中验证，模型保存无需重启的原有流程不变。2026-10-04 前轮 Windows GUI/测试代码编译通过（53 条警告，未执行测试）；本轮编译及当前调试产物身份以 [品牌与交付](desktop-and-packaging.md#当前交付状态) 为准，功能待用户验收；会话目录、自动授权、已运行实例及回滚边界见上述专题。

IM 新建会话中的模型选择只读天工既有 `model_configs` 的 ID、名称与默认标记，同时包含天工原生配置和 Hub 管理条目；不读取密钥。表单使用名称显示、以真实 `model_id` 选择，同名条目仍各自独立。创建时把选中 ID 固定到该会话，后续任务、审批与恢复沿用，不改变天工桌面的默认选择；模型行自身参数仍按原配置生效。共用表单只开放天工可控的目录和模型，思考参数沿用该模型接入配置，工具权限沿用天工策略。代码入口为 [只读模型列表](../../src/gmclaw_config.rs)、[共用会话后端](../../src/im/core/session_backend.rs) 与 [天工会话](../../src/gmclaw_im/sessions.rs)；这项读取能力不会扩大模型页原有的写入范围。

本轮外部消息修复 Windows Hub/天工句柄继承及退出端口释放、补齐含无任务的手动项目目录，并增加按平台能力收尾的临时准备/处理状态；不改变模型配置格式、上游参数、专用路由或流式聚合。飞书状态卡最终更新为答复，企业微信仅有效消息回调结束同一 stream，微信保持文字能力；三项已纳入 `v0.4.30-3`，最新本机有限用户反馈见验证状态，实际发布进度见交付记录。具体使用、限制与回滚见 [外部消息专题](gmclaw-im.md)。

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

### 启动桌面与外部消息显示

「天工 Claw 接入」页的「启动天工 Claw」按钮在后台发现完整安装，自动准备本地连接并复用已运行实例；不要求先发送 `/tg`，也不切换 IM 执行端或发送模型任务。Windows 除常见目录外，读取用户/系统注册表的 `App Paths`、`InstallLocation`、`DisplayIcon` 和卸载程序记录；只有图标或卸载路径的记录会转换到同目录真实主程序并核对完整资源，支持非 C 盘安装，不把 `.ico` 或卸载程序用于启动。macOS 继续支持完整 App。兼容 `desktopPath` 可指定安装，`GMCLAW_CONFIG_PATH` 只负责模型数据库，两个覆盖入口用途不同。

Windows Hub 新监听与天工子进程均禁止句柄继承，正常退出仅关闭/必要时结束自有 Hub 后台，天工和任务继续独立运行；旧天工如果已继承 `3847`，需确认任务/审批后正常退出该旧实例一次迁移，新程序无法撤销旧句柄。目录候选现从官方全部场景读取有效 work_dir，包含没有任务的手工项目；完整读取失败明确说明并保留本地目录选项，浏览不创建目录/任务。无新 TOML 或模型数据库迁移，回退会失去新目录来源并可能重现旧启动链问题；详见 [端口释放](gmclaw-im.md#hub-退出与端口释放)、[项目目录](gmclaw-im.md#在-im-中选择项目目录)。

IM 当前行为已移除每轮固定“正在等待天工 Claw 处理…”文字；本轮新增的飞书临时卡/有效企微回调 stream 会在答复、审批或错误终态结束生成标记，微信直接文字，状态并非逐字模型输出，已知原控件关闭失败仅作固定终态更新、每15秒最多额外3次且受20分钟TTL限制，每tick至多2个更新、不重发正文/任务，平台不可用不保证卡已更新，详见 [准备回复](gmclaw-im.md#准备回复与终态)；成功恢复或保存对话后，真实任务身份进入最多 128 项的内存展示队列，后台每 5 秒处理，单次同步最多 12 秒。另用独立缓存保留最多 128 个任务/会话组合键、每键 128 个本 Hub 回复行 ID，展示队列取出或完成不丢失该缓存范围内的来源记录；后续终态更新仍可识别同一回复行。天工原生任务消息仍先保存在官方数据服务，展示问题不重放模型、工具或 IM 请求，不影响任务执行结果。

官方 1.1.1 没有直接刷新原生聊天的外部事件接口，已打开任务的消息缓存也不会因数据库保存自动重读。本轮 Hub 显式启动天工时增加仅回环的 `127.0.0.1:18769` 同步通道；先核对同用户进程、监听归属、安装页面 URL、版本及 renderer 资源 SHA-256，再局部更新精确 `task_id/session_id` 的原生消息与缓存。当前只支持 `out/renderer/assets/index-oms9jgdP.js` 指纹 `f47a5a8a5049b8df664053eec2428098d6c691ff28bbbc3bad55b2f4cdc133e3`，跨平台也须字节匹配；不匹配的版本继续保存消息，但停止页面修改。已打开的对话空闲时更新内容；未打开的任务只更新列表，用户后续打开读取最新记录，不自动选择会话。原生执行、审批、历史加载、未保存状态或读取期间页面变化会延后，草稿、附件、当前模型和活动会话保持原值，不整页重载。

前阶段 Windows 页面定位修复继续保留：预期安装路径按目录逐段构造，以全部路径组件进行 ASCII 大小写不敏感比较，避免混合 `/`、`\` 把同一页面误判为未就绪；完整安装位置、组件数量及层级仍必须一致，query/hash、非本机 file 地址、其他完整路径及多页面匹配仍拒绝。固定诊断分别说明页面未出现、地址不匹配和不能唯一确认，监听/版本/字段能力与原生忙状态保护保留。前一作用域修复阶段，已通过 Hub 开启通道的天工无需再退出；本轮旧实例若已继承 Hub 的监听句柄，仍需核对任务后正常退出旧天工一次，再从新 Hub 启动。后续不要求每次退出 Hub 前关闭天工。

用户此前正常启动且没有同步通道的天工仍能在飞书等 IM 对话。若需要该窗口显示后续外部消息，请在确认当前任务和审批后按天工正常方式退出，再用 Hub 的「启动天工 Claw」打开一次；无需手填口令或环境变量。Hub 不强制结束实例，也不因为消息展示问题自动重启。启动按钮下方持续显示同步诊断，只采纳最近 30 秒的核对结果并显示经过时间，区分「能力已准备」「上次已更新」「等待刷新」「上次未完成」等状态；超时说明尚无最近的更新核验。「能力已准备」须实际连接 renderer Runtime、取得所需 Vue 引用并完成字段核对，只能证明同步能力具备，具体消息是否更新由后台刷新结果说明。模型绿色「已连接」和 Harness 授权成功分别保留原含义，普通每轮回复不重复同步提示。

展示核验分别限定 App `Closure` 与 ChatPanel `Block`，每类最多 8 个候选，以同一作用域完整字段形状确认响应式引用，忽略 `Module`/`Global`/`Script`，不拼接不同作用域的字段。官方 ChatPanel 的解构参数使 V8 将正文函数体所需值放入 `Block`，原来只查 `Closure` 会漏掉全部 11 个会话字段；前阶段只读 Runtime 核对已确认 App 4 个引用、Panel 11 个句柄/合法 null 及能力 schema `ready`，未调用同步函数。固定诊断区分 App/Panel 引用失败和 IPC、任务/消息/摘要结构等结果，能力准备仍与实际窗口更新分开。作用域中的基础类型运行口令可能短暂进入 Hub 内存，不选择或输出口令，不写入配置 API、聊天、日志或文件。实际消息读取由天工页面使用官方 IPC 授权后访问只读数据接口，正文不作为展示同步结果返回 Hub；队列与来源缓存只保存任务/会话身份及 Hub 回复行 ID。原生执行、审批、未保存消息或快照变化的保护继续生效。

Hub 会话关联文件仍为版本 `1`，新增可选 `context.owned_reply_ids`，字段缺失默认空、空数组省略，每关联最多保留最近 128 个互不重复的正安全整数行 ID（不超过 `9007199254740991`）；整文件 8 MiB、20000 个关联的限制保留。仅本 Hub 官方 `append_message(role="system")` 成功返回的 ID 能登记，创建占位后额外尝试保存；该次展示元数据保存失败不会阻止已授权模型执行，原提交前未知状态保存、终态保存与审批保护仍保持。Hub 重启后重新恢复会话时，须核对任务、官方 `session_id/user_id/project_id` 与原持久记忆身份相符，才把这些行重新加入同步队列；不将其他原生行当成 Hub 回复，也不续执行旧任务。

同步通道不修改安装文件或持久启动项；回退程序后展示行为按旧实现处理，原生消息与模型配置保留。旧软件可忽略新增字段，但重写关联文件会丢掉回复行 ID，因此降级前应备份配置与关联。再次升级可以读缺失字段的旧记录，但无法凭旧占位文字补认来源；前阶段已经卡住且没有来源记录的窗口需由用户确认任务/审批后正常退出天工，再从 Hub 启动一次，重新加载官方已保存记录。新回复记录来源后，Hub 重启可经恢复会话重新建立来源缓存；原生未落盘信息、读取失败或范围边界仍会安全延期。完整队列、诊断与回滚见 [外部消息专题](gmclaw-im.md) 与 [配置边界](../configuration.md#天工配置边界)。

## 上游流式响应聚合

天工 1.1.1 的模型客户端发送 `stream=false` 并需要完整 Chat JSON。Hub 对天工的 OpenAI Responses 转发启用上游 `stream=true`，收集完整响应、工具调用、用量及推理状态后，再返回天工需要的非流式结果。其他 Responses 类型保持原请求模式，但能够接收有效的 JSON 或 SSE；Chat Completions 保持原 `stream` 值，也能聚合实际返回的 SSE。该行为仅用于天工专用渠道的对应路径。

聚合按实际数据识别 JSON/SSE，兼容内容类型与正文不一致的情况；要求协议结束状态完整，断流、截断、上游失败事件、超时或无效内容都返回错误。收集上限为 `32 MiB`，条目数量有限制，使用渠道 `timeoutSecs` 的共同截止时间。成功响应开始后不会因读取失败重放请求。它不会把天工模型客户端改为逐字显示流式回答，也不修改天工安装资源。

v0.4.30-3 请求日志修复：请求日志分别记录客户端与上游请求模式。旧 `stream` 仍是天工原始请求的 `false`，新增 `upstreamStream` 来自实际发送的上游请求，OpenAI Responses 流式聚合因此在列表显示 `Streaming (Upstream)`，详情显示 `client_stream=false`、`upstream_stream=true`。其余协议按实际发送模式显示，非流式请求不会仅因来自天工就标成流式。此概要记录不依赖「详细日志」开关；旧日志缺少的上游模式保持未知，不根据渠道名称回填。数据库新增可空列、API 保持原 `stream` 语义，降级后旧版继续读取原字段；详见 [请求日志详情](../ai-gateway-request-log-detail-patch.zh-CN.md#2026-10-06客户端与上游流式模式)。当前仅完成源码与迁移夹具，未执行夹具或真实模型请求；本轮编译结果由 [品牌与交付](desktop-and-packaging.md#当前交付状态) 记录。

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
- [模型连通概要与内存活动记录](../../src/client_overview.rs)、[概览显示](../../src/gui/client_overview.rs)：只记录本次 Hub 运行中的本地接入证据，不增加模型验证请求、不写用户模型配置。
- [专用入口](../../src/ai_gateway.rs)、[请求分发](../../src/ai_gateway/handler.rs)、[路由隔离](../../src/ai_gateway/router.rs)
- [参数策略](../../src/ai_gateway/gmclaw.rs)、[推理状态](../../src/ai_gateway/gmclaw_replay.rs)、[流式聚合](../../src/ai_gateway/gmclaw_stream.rs)、[重试](../../src/ai_gateway/providers/mod.rs)
- [上下游请求日志](../../src/ai_gateway/request_log.rs)、[日志列表](../../src/gui/request_logs.rs)、[日志详情](../../src/gui/request_log_detail.rs)
- [Hub 不可继承监听](../../src/main.rs)、[Windows 天工进程启动](../../src/gmclaw_runtime/windows_start.rs)、[Hub 后台退出](../../src/gui/daemon.rs)、[只读全部项目目录](../../src/gmclaw_desktop.rs)、[目录合并与校验](../../src/gmclaw_im/sessions.rs)、[独立临时回合](../../src/im/core/executor_turn.rs)
- [桌面安装发现及展示队列](../../src/gmclaw_runtime.rs)、[本机通道与资源核验](../../src/gmclaw_runtime/display.rs)、[精确视图选择](../../src/gmclaw_runtime/display-target.js)、[局部消息与缓存刷新](../../src/gmclaw_runtime/display-sync.js)

天工自有 MCP 连接和 OAuth 设置继续由天工管理。用户已明确本轮第二阶段专注 Hub 外部消息执行，不把 MCP 配置管理列为默认后续目标。外部消息可以触发天工已有工具，只有天工自身策略返回待确认事件时才进入 Hub 的 IM 审批流程；飞书/企微使用平台原审批按钮，微信使用文字选项，独立天工回调不提交到 Codex；详见 [TC-012](gmclaw-im.md)。

## 验证状态

当前 `v0.4.30-3` 发布候选的实际提交、Actions 和包身份见 [交付记录](../releases/v0.4.30-3.md)。用户最后一轮反馈“目前看着没什么了”仅覆盖本机实际操作，未指定逐项或跨平台验收。以下原版号与未发布说明属于各开发阶段，不作为本版安装包或全功能验收证据。

2026-10-06 用户确认前阶段 `tg-panel-scope-fix-20261006`、SHA-256 `fe48829e…` 独立程序修复后天工桌面消息能更新，仅登记该次实际操作；此前失败与只读结构诊断继续保留原阶段归属。用户随后确认 `tg-approval-ui-fix-20261006`、SHA-256 `71c3043d…` 的审批测试通过，仅属于该次实际操作，其他平台、全文分段/失败回退、重复/过期/非本人及重启隔离仍待用户验收；本轮端口、目录、临时回复状态未因此验收，开发方未运行程序或真实业务。构建与产物由 [品牌与交付](desktop-and-packaging.md#当前交付状态) 登记，macOS 待 Actions，未发布。完整状态见 [外部消息专题](gmclaw-im.md#验证状态)。

2026-10-06 前阶段 `tg-window-path-fix-20261006`、SHA-256 `9cdc…` 程序仍被用户反馈「原生视图或闭包字段未通过核对」，当时即时同步未验收。该作用域修复阶段在确认唯一回环 `18769` 监听、正式 EXE、同用户 SID 和官方 renderer 指纹后，执行了仅本机页面定位与 CDP Runtime 结构/schema 诊断；确认 App `Closure` 4 个引用完整、Panel `Closure` 缺少全部 11 个字段，而 Panel `Block` 包含全部所需句柄/合法 null，能力 schema 返回 `ready`。据此分别限制 App/Panel 作用域并细分固定结果诊断；未执行同步函数或窗口更新。Windows 最终编译及 `tg-panel-scope-fix-20261006` 程序身份以 [品牌与交付](desktop-and-packaging.md#当前交付状态) 为准，交付时多轮即时同步仍待用户验收，后续有限反馈如上记录，macOS 待 Actions；未提升版本或发布，现有启用通道的天工不需再退出，仅换 Hub 后继续验证。

前阶段结构诊断未启动应用、执行交互测试/测试夹具，未发起 IM/模型/Harness/DataServer 业务请求或读取私有日志、数据库、记忆。已使用本机 `HTTP GET 127.0.0.1:18769/json/list` 定位页面，再以 WebSocket CDP 检查 Runtime 结构与只读 schema；结构枚举可能让基础类型授权短暂进入内存，但不选择、输出或持久保存口令，不写日志/配置/文件，正文与草稿引用不展开预览。此次只读结构能力核对不能代替真实消息更新或用户验收。前阶段 `e407305f…` 程序从 Hub 启动后「窗口尚未就绪」的失败、Windows 路径修复及当时未进行 CDP 调用的编译/静态证据保留原归属，不回写为本轮全部功能通过。

更前阶段 `tg-launch-display-fix-20261006`、SHA-256 `8b3c8a30…` 也曾由用户反馈桌面停留第一轮占位、重启才显示消息，该阶段未通过窗口同步验收。之后增加闭包能力核验、独立来源缓存、回复行关联持久化及持续诊断的实现与编译证据保留原阶段归属，不据此宣称用户已确认即时同步。

此前用户确认 WorkBuddy 启动成功、天工历史可从飞书选择并继续对话，只覆盖这些操作；天工安装发现修复、固定等待句移除与其他场景不能据此统称验收。当前同步修复保留模型配置、专用渠道、原生记忆、未知状态与工具审批保护，完整行为见 [外部消息专题](gmclaw-im.md)。

2026-10-06 未发布本地版本在「天工 Claw 接入」页新增「启动天工 Claw」按钮，自动准备本地连接并复用已有实例；不要求先发送 `/tg`，IM 仍通过该命令选择执行端。启动成功与模型入口绿色状态分别核对，按钮不主动调用模型。保留既有模型条目、备份、无需重启的保存流程与专用渠道隔离；未改变模型配置格式。行为、启用配置的回滚与原生历史 `1000` 步兼容见 [外部消息专题](gmclaw-im.md#直接从聊天接入)，本轮 Windows 编译及独立调试程序见 [品牌与交付](desktop-and-packaging.md#当前交付状态)，按钮和功能待用户验收；macOS 待 Actions。

`v0.4.30-2` 已通过 Windows/macOS Actions 原生构建并发布预发布包，功能、安装升级、实机及真实调用仍待用户验收。Windows 未签名；macOS 为 Apple Silicon/Intel universal 包，ad-hoc 签名且未公证。当前安装包身份见 [版本交付](../releases/v0.4.30-2.md)；`0.4.30-1` 当时的编译及 EXE 身份见 [上游整合记录](../upstream-v0.4.30-integration.md#本轮验证与产物)。下列两个 EXE 仅保留历史归属，不能用于识别当前产物。

第二阶段保存点 `9b9702e` 曾通过 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin codexhub` 和 `cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin codexhub`。该阶段格式、差异空白和文档本地链接核对通过；GUI 编译保留 23 条警告，测试代码编译保留 51 条警告，主要为未使用代码。已新增隔离 SQLite、多条目路由、JSON/SSE 聚合、Harness 事件与审批等夹具，按用户分工未执行测试。该保存点当时未完成 macOS 构建及实机验证；当前构建结果见本节顶部，Windows/macOS 端到端行为仍待用户验收，不新增 Linux 验收范围。

`9b9702e` 历史调试程序大小为 `55,158,784` 字节，SHA-256 `81f887e1140daf5f2c0146ffd05cab31cc5994da182c66c47f9e4d1959884872`。当时静态确认 Windows x64 PE、GUI 子系统与直接导入 DLL 文件；Windows API-set 属于系统虚拟契约，未运行程序验证加载器。清单 `.build-tools/gmclaw-feedback-build-manifest.json`，编译/构建日志 `.build-tools/gmclaw-feedback-check.log` 和 `.build-tools/gmclaw-feedback-build.log` 均不入仓库。共享输出路径 `target/x86_64-pc-windows-msvc/debug/codexhub.exe` 已由后续构建覆盖。用户测试当前版本前先退出旧 Hub（含托盘），并核对整合专题中的产物；模型保存后无需为此重启天工。

用户已确认上一轮模型配置保存成功，保存后无需完全退出并重开天工。这是旧单模型版本的有限验收，不推定本轮新增、修改、删除、默认切换、备份恢复、流式上游、工具续轮或 IM 执行通过。

更早厂商适配代码曾通过 Windows GUI/测试代码编译与调试 EXE 构建。该历史 EXE 为 `54,261,760` 字节，SHA-256 `141eaa28fe535fa95113b5f9609c8e82c2a0353c0add3d69cedd6b4f9edf8196`，对应 `3b3f213` 加当时未提交变更；不是多模型/外部消息保存点或当前上游整合产物。构建日志与清单位于 Git 忽略的 `.build-tools/`，应按对应阶段清单识别产物。

开发和发布过程中未启动 Hub/Codex/天工进行交互验证，未发起真实模型或 Harness 执行请求，未修改用户实际模型配置。`v0.4.30-2` 安装包由 GitHub Actions 生成并已预发布，未在本地生成发布安装包；发布成功不代表功能已验收。
