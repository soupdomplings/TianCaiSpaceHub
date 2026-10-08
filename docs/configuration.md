# Configuration

There are two separate config surfaces:

- `TianCaiSpaceHub` config, usually this repository's `config.toml`
- Codex App config, usually `~/.codex/config.toml`
- NVWA MCP 独立环境 `<Hub config stem>.nvwa.json` 与系统保护目录 `<Hub config stem>.nvwa-secrets`（v0.4.30-5 新增）

Do not mix them. `TianCaiSpaceHub` stores IM channel and bridge settings. Codex App stores model provider, auth, and `chatgpt_base_url`.

维护日期：2026-10-08。当前开发基线为 v0.4.30-5，独立 [NVWA MCP](customizations/nvwa-mcp.md) 页签管理认证环境和三端受管 MCP 配置；普通 JSON 只存地址、身份选择和系统保护引用，秘密/令牌/目标备份不写 Hub TOML、日志或连接诊断。集合 version 1、`_revision` 并发保存，默认本机端口 3849；未知非敏感字段保留，未知字段中的秘密拒绝保存。环境/账号修改先保存才能登录或写客户端，变更会撤销旧授权。运行状态、编译和用户验收见 [本轮交付](releases/v0.4.30-5.md)。下文 v4 构建/产物描述保持历史归属。

## TianCaiSpace customization fields and concurrent saves

维护日期：2026-10-07。`v0.4.30-3` 的共用会话、目录/模型、自动授权重连、原生历史/消息、审批和退出端口/项目/临时回复行为继续保留，其固定 `1.1.1` 同步门槛已由本版替代；前版产物与验收保持 [历史记录](releases/v0.4.30-3.md)。当前预发布及完整包核验见 [v0.4.30-4](releases/v0.4.30-4.md)，用户前次反馈不扩大为本轮实机验收；开发方不主动执行程序、交互测试或真实业务。

`v0.4.30-4` 产品源码 `7b5dd6e…`、注释标签和非草稿 Pre-release 已公开，桌面同步取消固定版本/renderer 文件名/SHA-256 门槛，以天工身份、HTML 脚本入口与能力识别；写入 Ref/缓存须可写、异步读取后再核对，无新增配置字段。Windows Actions 与 Mac 修复后同标签重建均成功，八项附件下载静态核验通过；Windows 未签名，Mac universal ad-hoc 且未公证。首次 DMG 失败与 CI 修复身份独立记录，不改产品标签；发布后核验文档使用单独提交进入 `main`。实机行为仍待用户验收，前调试身份见 [品牌与交付](customizations/desktop-and-packaging.md#当前交付状态)。

| Configuration area | Current contract | Detail |
| --- | --- | --- |
| NVWA MCP 独立环境/凭据 | `<stem>.nvwa.json`、`<stem>.nvwa.lock`、`<stem>.nvwa-secrets/`；默认 `127.0.0.1:3849`，64 环境/1 MiB；DPAPI/Keychain 存 secret/token/连接凭据/目标前像；普通配置不接受秘密，回退前移除/恢复客户端 | [NVWA MCP](customizations/nvwa-mcp.md) |
| `aiGateway.codexVisibleModels` | Saved model IDs displayed to Codex; visibility and provider routing are separate | [Dynamic models](dynamic-codex-models.zh-CN.md) |
| `aiGateway.codexModelProfiles` | Explicit model capability overrides take priority over inferred family defaults | [Dynamic models](dynamic-codex-models.zh-CN.md) |
| Provider `compatibility` / `chatDisableReasoning` | `openai_chat` identifies general Chat Completions; disabling reasoning is per provider | [Chat Completions](openai-chat-completions.md) |
| Reserved providers `workbuddy` / `workbuddy:<entryId>` | 旧 WorkBuddy 专用渠道与多模型条目，独立地址精确选路；排除 Codex、天工、普通模型同步与导入；源码版本 `0.4.30-2` 起支持多条目 | [WorkBuddy](workbuddy.md) |
| 接入点用途 | 大模型列表标明 Codex/可选为来源渠道、WorkBuddy 专用、天工 Claw 专用；用途按内部名称派生，无新增必填 scope 字段；专用请求不跨接入点或条目回退 | [用途与隔离](customizations/client-channel-scope.md) |
| Reserved providers `gmclaw` / `gmclaw:<entryId>` | 旧天工专用渠道与多模型条目渠道，排除普通 Codex 路由、可见模型同步和网页导入；每项按独立地址路由，同模型可使用不同来源；来源配置改变后需重新保存相应条目 | [天工 Claw](customizations/gmclaw.md) |
| `aiGateway.providers[].gmclawParameters` | 仅天工专用渠道使用；`reasoningEffort` 省略时跟随上游，`temperatureMode` 为 `auto`（默认）/`omit`/`preserve`；经天工页签保存并随专用渠道备份恢复，不写天工 `extra_params` | [厂商适配](customizations/gmclaw.md#hub-的厂商适配) |
| GMClaw API `entryId` / `makeActive` / `revision` | 条目身份与模型名分开；空 ID 新增，已有 ID 更新；`makeActive` 默认 `true`；集合 revision 防止覆盖任一管理条目及默认选择的后续改动，不写 Hub TOML | [模型 API 与备份](customizations/gmclaw.md#本地配置-api) |
| `gmclawBridge` | 天工外部消息连接：启动按钮或 `/tg` 显式准备，`/tg` 选择执行端；启用后后台恢复同用户官方实例授权，不因天工重启要求重复命令；运行授权仅内存缓存，后台不启动桌面。同平台复用 Codex 会话交互，原生历史 `1000` 步不套用新建配置限制 | [天工外部消息](customizations/gmclaw-im.md) |
| 天工桌面消息显示 | 无新增 TOML 字段；Hub 显式启动时增加本机 `127.0.0.1:18769` 通道；v0.4.30-4 标签源码核对已安装天工身份、renderer HTML 声明的本地脚本入口与运行时字段能力，不比较产品版本号、JS 文件名或 SHA-256；展示队列最多 128 项，独立来源缓存最多 128 个任务/会话键、每键 128 个回复行 ID，后台每 5 秒处理；按钮下方持续显示 30 秒内的能力/刷新/失败诊断，展示失败不重放任务 | [桌面显示实现](../src/gmclaw_runtime/display.rs)、[天工外部消息](customizations/gmclaw-im.md) |
| `<config stem>.gmclaw-sessions.json` | Hub 配置旁的版本 `1` 关联记录，原子保存任务、会话、精确模型和记忆身份及未知/待审批标志；`context.owned_reply_ids` 缺失默认空，最多 128 个本 Hub 成功创建的回复行 ID，不保存口令或消息正文；如 `config.toml` 对应 `config.gmclaw-sessions.json` | [桌面关联实现](../src/gmclaw_im/desktop.rs)、[天工外部消息](customizations/gmclaw-im.md) |
| 日志 `stream` / `upstreamStream` | `stream` 保留客户端请求语义；新增可空 `upstreamStream` 记录最终实际上游请求体的流式值，旧日志未知；不新增 TOML 设置 | [请求日志](ai-gateway-request-log-detail-patch.zh-CN.md) |
| Provider `importSource` | Imported identity survives renaming; nested fields are `origin`, `key_id`, `site_name`, `key_name` | [Web import](hub-external-import.md) |
| API `_revision` | Read from `GET /api/config`, send back unchanged with `POST /api/config`; never persisted in TOML | [Web import and save behavior](hub-external-import.md) |

The full-config save endpoint requires `_revision`; missing or stale versions return HTTP 409. Reload the latest configuration, review the intended changes, and submit again. Do not blindly retry the old full document, as that could overwrite newer settings.

`POST /api/external-import/commit` is the local GUI save operation after confirmation, not a public deep-link receiver. It rereads the latest file, merges only the selected provider, and checks the preview target fingerprint before updating an existing provider. Deep links arrive through Windows IPC. Both save paths use a file lock and an atomic replacement; `_revision` is the content revision used for conflict detection. Older Hub versions do not participate in this protection, so do not edit the same file from old and new versions at once.

An enabled imported provider must have models, and its alias targets must exist in that list. This rule also applies when enabling it later through the normal configuration editor. The GUI should preserve `importSource` during ordinary edits.

The source of truth for these fields is [provider configuration](../src/ai_gateway/config.rs), [config storage](../src/config.rs), and [local API](../src/web.rs). Back up configuration before downgrade or external edits; remove a newly imported channel or restore its prior backup to undo an import.

### 天工配置边界

模型页通过 `GET/POST /api/gmclaw/config` 读取和保存管理条目，`activate/delete/restore` 操作使用同一集合版本；原顶层状态字段表示当前选中条目，新列表位于 `entries`。新增条目的数据库 ID 为 `tiancaispacehub-<entryId>`，网关地址为 `/ai-gateway/gmclaw/<entryId>/v1`；旧 `entryId=legacy` 保留原行、渠道和地址。保存、删除、设为默认均保留一次定点撤销；新元数据 v2 兼容读取旧 v1，旧程序不能保证理解新备份和多模型路由。

用户已确认上一轮模型保存后可以直接选择使用，无需完全退出重开天工；本轮 API 不再要求重启。新增多模型操作和上游流式聚合仍待用户验收。模型客户端继续接收完整 Chat JSON，OpenAI Responses 上游流式响应由 Hub 聚合，不意味着天工客户端改为逐字显示。请求日志保留 `stream=false` 的客户端含义，新增 `upstreamStream` 从各 Provider 最终提交请求体采样；上游为流式时列表显示 `Streaming (Upstream)`，旧日志显示上游模式未知，不能反推旧请求未使用流式。SQLite 仅兼容增加可空列，不改变既有日志语义；回滚时新增展示不可用，模型 JSON 聚合行为仍按原实现。

天工模型概览中的绿色“已连接”使用本次 Hub 运行期间的本地证据：收到带生成本地 Key 的有效专用模型入口请求、天工数据库中对应 URL 匹配当前 Hub 监听地址和条目、当前天工桌面主进程集合与观察时一致。无请求时等待，进程退出降级，重启、条目或端口变化后需要新请求；历史持久日志不作为当前证据。上游返回错误仍能证明本地链路，不能据此宣称模型已验收。该状态不依赖 IM Harness 授权，不新增配置字段，详情见 [模型接入](customizations/gmclaw.md)。

天工页提供模型配置、「启动天工 Claw」按钮与聊天指引，不提供桥接表单。按钮或显式 `/tg`、`/tg new` 自动准备 `gmclawBridge` 连接配置；IM 仍核对原有权限、运行状态和待审批保护，桌面按钮不切换发送者执行端或创建会话。准备沿用配置版本与保存保护，避免覆盖并发模型配置；本地配置仍可通过 `GET/POST /api/config` 和 `_revision` 维护。按钮调用 `POST /api/gmclaw/runtime/start` 并提交最新 `expectedRevision`，仅启用接入和补充空口令；保存不代表连接成功，启动前后重新核对配置。兼容字段如下：

| 字段 | 默认值与含义 |
| --- | --- |
| `enabled` | `false`；显式启动按钮或 `/tg` 首次启用，普通消息不自行启用；已启用时后台每 5 秒检查并恢复运行授权，不自动启动桌面 |
| `endpoint` | `http://127.0.0.1:7861`；只接受本机回环根地址或 `/v2/chat` |
| `authToken` | 空；显式连接准备时仅空值自动生成，旧值复用，用于 Hub 自行启动的天工。已有实例的运行授权另存临时内存缓存，不覆盖此字段，界面只显示授权状态 |
| `desktopPath` | 省略；自动查找完整天工安装，兼容旧主程序绝对路径，macOS 也接受 `.app` |
| `projectPath` | 空；合法旧绝对路径用作新会话默认目录，否则使用天工数据库同级 `workspace`；保留旧值、不再必填、不参与连接指纹，不覆盖已建会话目录 |
| `modelId` | 省略；兼容天工 `model_configs.model_id`；新建表单明确选择优先，其次此值，再取天工当前默认模型；实际 ID 创建时固定到会话 |
| `maxSteps` | 新建会话默认 `30`，范围 `1–200`；原生历史恢复单独兼容 `1–1000` |

`/tg` 用于首次启用或切换天工，已启用后后台每 5 秒检查运行状态并恢复新授权；普通消息与概览也可推动授权恢复，天工重启后无需重复 `/tg`。先核验保存/缓存的授权，失败时才识别同用户的完整官方安装与 Harness 子进程。识别要求 Windows 同用户 SID、macOS 同用户 UID，以及配套内置 Python/脚本、父 PID、`--node-path` 和固定本地地址相符；发现最多等待 8 秒，未知身份或多个匹配拒绝，不向聊天或日志输出运行口令。核验通过的运行授权只留在内存，不修改系统环境或保存口令。Hub 重启后可基于保存的 `enabled` 自动识别，但发送者当前执行端选择不因此恢复。实现见 [授权识别](../src/gmclaw_runtime/credentials.rs) 与 [外部消息专题](customizations/gmclaw-im.md)。

后台检查、普通消息、概览和切页不自行启动桌面，不自动重放任务；需要启动时通过显式启动按钮、`/tg`、`/tg new` 或对应旧命令，Hub 将保存的 `GMCLAW_AUTH_TOKEN` 传给新子进程。完整状态检查最多 18 秒，同配置复用正在运行的检查 worker，各 HTTP 请求最多 2 秒、连接最多 1 秒；授权发现失败按 5 秒限流。显式接入最多等待 30 秒处理退出过渡或初始化，只重复只读健康/授权检查。端口仍占用时不启动重复实例、不强制结束进程，配置核验锁在实际启动后立即释放。启动按钮通过后台线程处理，GUI 请求最多 45 秒；已有实例直接复用。自动启动只用于默认 `http://127.0.0.1:7861`，其它合规回环地址只连接已有服务；超时或未知状态不当作连接成功。

2026-10-06 安装发现继续识别完整官方安装；Windows 核对用户/系统注册表的 `App Paths`、`InstallLocation`、`DisplayIcon` 和卸载程序路径，注册值只有 `.ico` 时从同目录查找真实 `tiangong-desktop.exe`，因此不再只依赖 C 盘常见安装位置。候选仍须有配套 `resources/app.asar`、Harness 和内置 Python；不会把图标或卸载程序当作启动目标。macOS 继续识别完整 App，已运行实例仍复用。默认扫描失败可通过兼容 `desktopPath` 指定完整安装，不影响模型数据库的 `GMCLAW_CONFIG_PATH`。

桌面消息同步和 Harness 连接分别判断。Hub 新启动天工时加入仅回环的 `--remote-debugging-address=127.0.0.1`、`--remote-debugging-port=18769`，不更改安装资源、系统环境或持久启动项。此前由用户正常打开、未带同步通道的实例仍可在 IM 对话；需要在当前窗口显示外部消息时，先按天工正常方式退出，再从 Hub 显式启动一次。Hub 不强制退出正在执行的实例，不为展示问题自动重启。启动按钮下方持续显示桌面对话同步状态，诊断只采用最近 30 秒的结果，分别说明「能力已准备」「上次已更新」「等待刷新」「上次未完成」等状态，并标明距核对的时间；过期时说明尚无最近更新证据。「能力已准备」要求实际连入 renderer Runtime、定位 Vue 引用并核对所需字段，只读到页面列表或授权连通不满足该条件；准备能力也不等于某次消息已经刷新。普通每轮仍不重复发送固定等待句。

本轮 Windows Hub 主/兼容监听改为不可继承句柄，天工正式程序通过 `CreateProcessW(bInheritHandles=FALSE)` 启动；正常退出先关闭后台、最多等约 1.5 秒，必要时只结束 Hub 自有子进程，最后启动恢复也不结束整棵进程树。天工保持独立运行，Hub 退出后新启动链不应再因继承占用 `3847`；其他程序占用仍明确提示，不强杀。旧天工已继承的句柄无法由新 Hub 撤销，升级时如仍占用，先确认任务/审批后正常退出该旧实例一次，再从新 Hub 启动。没有配置/安装资源迁移，非 Windows 启动/监听保留原行为；回退旧程序可能重现继承问题，仍须核对现有任务。代码见 [监听](../src/main.rs)、[Windows 天工启动](../src/gmclaw_runtime/windows_start.rs)、[后台退出](../src/gui/daemon.rs)，完整边界见 [专题](customizations/gmclaw-im.md#hub-退出与端口释放)。

WorkBuddy 页的「启动 WorkBuddy」按钮调用 `POST /api/workbuddy/runtime/start`，提交 JSON `{ "launch": true }`，只发现/启动已安装桌面并避免重复进程，不修改模型文件或切换 IM。Windows 支持注册表记录的自定义安装目录，macOS 支持完整 App；可选 `WORKBUDDY_DESKTOP_PATH` 仅覆盖安装发现，与模型文件覆盖 `WORKBUDDY_CONFIG_PATH` 无关。该按钮不表示 `/wb` 任务接口已支持；详见 [WorkBuddy 启动及回滚](workbuddy.md#启动-workbuddy-桌面)。

`/tg` 打开当前平台原有的新建/恢复入口，`/tg new` 直接进入同一会话设置。飞书使用原会话卡片，微信使用原文字菜单，企业微信继续通过同一 `TextChatAdapter` 流程使用原卡片；后端经 `session_backend` 分派，已删除天工独有的目录菜单。目录候选依次包含默认目录、官方全部场景有效 work_dir（含手动创建且无任务项目）、同一发送者/当前聊天且连接指纹相同的当前及历史目录，按本机路径身份去重；Windows 统一大小写/分隔符及系统路径前缀，macOS 保留大小写差异。默认目录优先合法旧 `projectPath`，否则为 `gmclaw_config::config_path()` 同级 `workspace`，设置 `GMCLAW_CONFIG_PATH` 会同时影响该默认来源。不存在的目录在最终创建会话动作时才创建/规范化（天工最多等待 10 秒），不新增独立 `/0` 或额外 `/y` 确认规则；按当前平台显示的按钮、序号或共同文字菜单操作。macOS 绝对目录输入不会被普通斜杠命令判断误拦截。

项目目录只读官方 `GET /data/scenarios` 的 `scenario_id/work_dir`，不通过任务数量筛选，也不解析会话正文；响应仍可能含任务摘要。默认场景空 work_dir 取真实天工用户数据目录的 `workspace`，非默认空目录不补造路径。场景和唯一目录各最多 20000 项，响应 8 MiB；非法/重复/缺失字段或超限明确报未完整读取并保留本地默认/发送者已有目录，权限、配置或菜单失效则不返回旧候选。项目连接/读取及前后核验最多 20 秒，模型读取并行最多 5 秒，汇总后再以最多 5 秒核对实例。只浏览不会新建目录/任务；最终创建用同一路径身份匹配已有场景。飞书每页 20 个真实目录、默认/自定义常驻并补当前选项，翻页保留表单草稿，来源页须匹配当前页、缺失/非 object 表单拒绝，`initial_index`/`default_value` 表示实际选中值/输入，读取候选后再核菜单权限/配置；企微共最多 10 个下拉选项（含默认/自定义/已选），超限用原 `1` 或 `/1` 入口、每页 8 项文字菜单，微信沿用 8 项文字分页。无需新增 TOML 或持久字段，回退旧程序会再遗漏无任务目录，但不删除项目。详见 [目录与边界](customizations/gmclaw-im.md#在-im-中选择项目目录)。

模型选项只读天工数据库的 `model_id/model_name/is_active`，同时包含天工原生与 Hub 管理条目，不查询密钥；最多等待 5 秒。表单显示名称与模型 ID，创建时再次验证所选 ID 并固定到会话。后续任务、审批与恢复沿用会话目录和模型 ID；恢复及每次提交前复核 ID，缺失或读取失败不提交、不切换默认模型。恢复 Hub 已关联任务使用持久记录的精确模型 ID；原生非空会话的 `model_name` 必须唯一映射到精确 ID，缺失或歧义拒绝，只有确认确实为空的原生任务才可使用当前默认模型。切换默认目录或默认模型不改变已有会话；模型行参数仍由天工与网关处理。天工不提供会话级推理/权限覆盖，共同能力 DTO 隐藏相关设置并显示继承说明，不支持的非默认值仍拒绝。完整行为见 [天工外部消息](customizations/gmclaw-im.md)。

企业微信共用设置卡片的模型下拉包含默认项、最多共 10 项；更多模型由卡片提示进入原每页 8 项的完整文字列表（当前发送 `2` 或 `/2`，序号由共同动作表计算）。后续页的已选模型会补入卡片末项，不改变所选 ID 或自动回退默认；这与目录选择一样沿用平台共有流程，实机行为待用户验收。

天工工具审批沿用平台原交互：飞书/企业微信点击批准全部或拒绝全部，微信回复 `/1` 批准或 `/2` 拒绝（兼容 `1`/`2`、`y`/`n`，只在当前 pending 解析），微信没有审批按钮。企微工具参数先完整文字投递，再发按钮；不得截断参数后开放审批。旧 `/tg approve <确认码>`、`/tg reject <确认码>` 作兼容与卡片投递失败备用。`GmClawApproval` 展示、天工专用 outbound 载荷及 `GmClawApprovalDecision` 动作独立于 Codex，随机请求标识同时绑定平台/Hub账号/聊天/发送者、当前会话、配置指纹与运行授权/桌面身份，15 分钟有效；非本人、非法索引、重复/过期、切端和重启旧请求不能执行或落 Codex。卡片回执只说明已选择/提交，最终结果以任务答复为准；预检失败保留待审批并可再操作当前有效卡，未知执行不重放。不新增永久授权、单工具审批或持久化可点击记录，不改变 TOML/关联格式。回退程序仅按旧版入口处理新审批，旧控件不恢复；程序重启不停止工具。详见 [审批交互](customizations/gmclaw-im.md#按当前平台处理审批)。

会话设置使用 Hub 内部 `session_scope` 隔离执行端、发送者、连接指纹及界面代次，`session_scope/session_entry` 不接受传输 JSON 注入；不新增 TOML 必填字段。卡片恢复目标必须来自当前请求已展示的列表，普通文字保留消息去重，卡片动作不因共用消息 ID 而被当作重复文字丢弃。请求 ID 含随机 UUID 和序号，防止重启复用旧编号。Hub 重启不恢复发送者当前绑定、旧卡片或可继续操作的审批控件；桌面任务、消息和会话关联记录持久保留，可重新选择真实历史。项目目录不等同于文件系统沙箱。

新建和恢复使用官方本机 `http://127.0.0.1:18768` 数据接口，并携带与 Harness 相同的已核验运行授权。新建时创建真实场景/任务及会话元数据，关联 `task_id/session_id`、精确模型、原 `user_id/project/session` 与目录；默认原生场景记忆项目 ID 为 `0`，其他场景用实际场景 ID。恢复列表通过 `GET /data/scenarios` 读取全部真实桌面任务，最多 20000 项、响应 8 MiB；现有允许名单中的获准发送者均可浏览，不再只限自己在本次 Hub 创建的历史。恢复保留原记忆身份与 cwd，原生默认场景无目录时使用天工真实用户数据目录下 `workspace`，不取旧 `projectPath` 或 `GMCLAW_CONFIG_PATH` 的覆盖目录。实现见 [桌面数据接口](../src/gmclaw_desktop.rs) 与 [会话关联](../src/gmclaw_im/desktop.rs)。

同一桌面会话由一个 IM 发送者认领，避免多个发送者同时向其提交；`/q` 释放认领，`/gpt` 成功切离时保留当前会话但释放认领，`/tg` 返回原会话时重新认领。若已被他人认领则拒绝继续使用，并提示可用 `/tg new` 新建。普通恢复与执行前按真实事件行 ID 核对最新一轮：有 `harness_sidecar` 用户事件，其后为终态 `over`，末尾仍为该终态且本轮无 `confirm`；新用户轮没有结束事件时，即使元数据是 `completed` 也拒绝。最后一轮含确认时不以同轮 `over` 清除审批，较新用户事件本身也不证明旧审批解除。`running`、`awaiting_confirmation`、未知状态及 Hub 保存的待审批/未知标志仍拦截，已有当前有效审批沿用独立天工按钮、文字选项或兼容命令的同一批准/拒绝预检。共享列表不取消原账号/群提及/发送者允许名单，卡片作用域仍绑定当前操作人。

天工桌面在收到 `over` 后中止流，可能使 Harness 来不及更新持久 `processing`，所以该字段不再单独证明仍在执行。仅当最后轮次上述结束证据成立、明确的 `message_count` 与实际事件数量一致、当前轮次 `chat_id` 存在且非空、`metadata.updated_at` 不晚于结束事件 `created_at`，并在至少 1 秒后二次读取时元数据（包括 `chat_id`）与全部事件保持一致，才允许把 `processing` 视为残留。异常、事件数量字段缺失或不符、轮次身份缺失、时间不可核对或快照变化均阻止本次操作，保留原会话；确实空的原生任务继续按空会话规则处理。不新增 TOML 字段或改写官方状态，不重放任务。官方没有原子的每会话实时状态接口，双读只能缩小请求准备与首条事件落盘的并发窗口，不能锁住桌面；`/ctrl/status` 恒空的 `current_session` 不作为空闲证据。完整边界见 [恢复状态判断](customizations/gmclaw-im.md#恢复时的状态判断)。

消息提交前先向桌面保存用户消息和回复占位，答复约每秒增量 PUT 并保存终态，完成后标记任务完成；工具审批提示也持久保存。Hub 旁的 `<config stem>.gmclaw-sessions.json` 原子记录任务/会话/精确模型/记忆身份和未知/待审批标志，不存口令或消息正文；备份时与配置一起保留。每轮普通对话前的固定“正在等待天工 Claw 处理…”文字已移除；飞书显示可更新准备/处理卡并在终态替换为答复，企微仅本次 Message 回调可使用 finish=false→true 的原生 stream，微信继续文字答复。审批、错误、队列拒绝和超时均收尾，异常返回/取消 Drop 只报未知、不重放。旧 Hub 纯内存会话及缺失的桌面消息不能补造，不扫描私有日志拼接历史；恢复桌面内容也不代表恢复 Hub 的审批控件。

临时回复状态使用独立 `GmClawTurnStage/GmClawTurnFinished` 载荷与 worker，随机回合固定发送者/路由/会话/配置，最多 128 个活动回合、256 个完成卡片标识，20 分钟展示 TTL、约每 15 秒清理，展示 API 单次最多 10 秒；不写 Codex runtime、TOML 或关联文件。飞书卡超过 24 KiB 或更新失败，先无损发送当前可投递全文，再更新原卡完成态；企微 stream 的 20 KiB 边界同样先文字后收尾。答复仍沿用原 16000 字上限与截取提示，完整记录在天工桌面；微信正常/无 token 备用/context 恢复保留回合身份，每段正文前重核允许名单、账号和配置，变化后停止未发段，已发内容不能撤回；context 恢复可能重复前缀，普通网络失败不保证投递，不新增业务重试。已知原消息/stream 回执终态更新失败会保留固定 closing 状态，每约 15 秒只更新原控件最多 3 次、最长至回合 20 分钟 TTL；不重发正文或任务；每 tick 先纯移除过期 closing、最多发起 2 次更新，跳过错过的 tick。账号禁用不发/不消耗次数，在 TTL 内重新启用可继续；权限/配置变化只固定说明，无回执不补造控件。平台断开/停用、回执丢失或 Hub 直接终止时仍无法保证聊天旧卡实际更新；回退旧 Hub 不恢复临时控件。详见 [准备回复与终态](customizations/gmclaw-im.md#准备回复与终态) 与 [关键代码](../src/im/core/executor_turn.rs)。

关联文件保持版本 `1`，新增可选 `context.owned_reply_ids`：缺失默认空，空数组省略，每关联保留最近 128 个互不重复的正整数，最大 `9007199254740991`，使 renderer 能准确识别 ID。整文件读取/写入仍限制 8 MiB 和 20000 个关联，新增字段不能突破该边界。ID 只来自本 Hub 官方 `append_message(role="system")` 成功返回的行，创建占位后尝试额外保存；这次展示元数据保存失败不阻止已授权执行，原提交前必需的未知状态保存、终态保存和审批保护继续生效。恢复带 ID 的关联时必须核对原 `task_id`、官方 `session_id/user_id/project_id` 与持久关联完全一致，核对通过才逐行重新加入展示队列；身份变化或元数据缺失时拒绝恢复，不把任意原生消息认作 Hub 回复。

天工 1.1.1 的官方事件回调没有转给已打开的 renderer，原生任务重复打开又会复用旧的页面缓存，因此仅保存数据库不能使该窗口更新。本轮在成功保存消息或恢复任务后，按真实 `task_id/session_id` 排入最多 128 个任务的内存同步队列；同任务更新合并，队列满时淘汰最早展示项，官方已保存消息不删除。另以任务/会话组合键保存独立的来源缓存，最多 128 个键、每键 128 个 Hub 回复行 ID；取出、完成或覆盖展示队列项不丢掉仍在该缓存范围内的来源标识，后续终态更新可沿用同一回复行。独立后台每 5 秒处理展示项，不阻塞授权恢复或 IM 最终答复；单次同步总限时 12 秒，读取分页、响应大小及消息身份均设边界。Windows 以原生 TCP 表、进程映像和用户 SID 核验监听归属；macOS 以本机监听、进程和 UID 核验。通道必须属于当前同用户天工进程，页面必须是对应安装的 `app.asar/out/renderer/index.html`。

v0.4.30-4 标签源码不比较 `package.json.version`、固定 JS 文件名或资源 SHA-256。安装核对读取天工应用身份和实际 `out/renderer/index.html` 声明的本地脚本入口，确认脚本位于同一 renderer 目录、ASAR 资源范围与大小合法，再以运行时字段/IPC 能力决定是否可更新窗口；不执行或读取入口脚本正文。页面仍须属于准确安装位置，不泛化任意布局。未知身份、入口或字段能力停止展示修改；Windows/macOS 使用相同行为，实际兼容和桌面效果待用户验收。已发布 `v0.4.30-3` 当时的固定 `1.1.1` 与资源指纹门槛保留在交付记录，该版 Actions 结果不作为本轮编译或验收证据。通过核对后只更新精确绑定任务的原生消息 ref、该任务缓存及场景列表；未打开的任务无需自动切换窗口，后续原生打开读取最新保存数据。原生正在流式执行、等待审批、加载历史、存在未保存消息，或读取期间任务/消息快照变化时延后；草稿文字、选中附件、当前模型和活动会话保持原值。不整页刷新、不重新发送模型请求，也不因 UI 同步失败改动任务结果或审批状态。

前阶段页面地址核对的 Windows 混合路径分隔符修复保留：预期页面按 `out`、`renderer`、`index.html` 分段构造，URL 转回本机路径后逐一比较所有组件，在 Windows 仅允许 ASCII 大小写差异；完整安装位置及路径层级仍须一致。非本机 file 地址、不同完整路径、query/hash 和不能唯一匹配的页面仍拒绝，保留安装身份/HTML 脚本入口、当前进程/监听/同用户与 renderer 字段核验。固定脱敏诊断区分「页面尚未出现」「页面地址与安装路径不匹配」「不能唯一确认页面」，安装核对失败改说明「安装身份或界面入口」；不显示地址或用户内容。已经由 Hub 启用通道的天工可保持运行，正常退出旧 Hub 并换用本轮程序后直接重连；作用域修复同样不要求再重启天工，也不改变首次手动实例需要启用通道、旧无来源占位需正常重新加载的边界。

当前展示核验按根视图与完整任务/会话/活动属性定位组件，不依赖编译后的 `__name`；App/ChatPanel 均有界检查 `Closure`/`Block`，每组件最多 8 个候选，要求所需引用完整来自同一作用域并通过只读 setup schema，完整有效候选必须唯一；忽略 `Module`/`Global`/`Script`，不拼接作用域。布尔运行/审批引用、合法空确认值和任务/会话身份类型继续严格核对。前阶段在官方 1.1.1 中发现 App 引用位于 `Closure`、Panel 引用位于 `Block`，当时结构/schema 诊断仅属于该次证据，不再把某一种编译作用域写成通行条件。诊断分别说明 App/Panel 引用以及 IPC、任务和消息/摘要结构失败，能力准备与实际更新仍分开。实现见 [setup 能力核对](../src/gmclaw_runtime/display-setup-schema.js)、[字段能力核对](../src/gmclaw_runtime/display-schema.js) 和 [同步适配](../src/gmclaw_runtime/display-sync.js)。

能力检查对实际写入的 `scenarios`、`messages`、`showWelcome`、`hasOlderMessages` Ref 排除 readonly，同时核对组件实例上 `seedMessages`、`messageSummaries`、`hasOlderMessages` 缓存字段的可写能力；纯读取的 Ref 不因 readonly 被额外拒绝。异步消息读取后再次核对字段、可写能力、任务/会话与原生运行/审批/未保存状态，发生变化时不修改窗口；无需新增可写开关或手工修改客户端配置。

前阶段结构诊断已使用本机 `HTTP GET 127.0.0.1:18769/json/list` 定位页面，再通过 WebSocket CDP 核对 Runtime 结构/schema；未启动应用、执行交互测试/测试夹具、调用同步函数、更新窗口、读取 DataServer 消息，或发起 IM/模型/Harness/DataServer 业务请求，未读取私有日志、数据库或记忆。作用域枚举中的基础类型授权可能随检查临时进入 Hub 内存，但不选择、输出或持久保存口令，不写日志/配置/文件，正文和草稿引用不展开预览。运行期实际消息读取仍在天工页面内通过官方 IPC 获取授权后调用只读数据接口；口令和正文不作为同步结果返回配置 API、聊天、日志或文件，身份队列与来源缓存也不保存它们。固定回环同步通道属于本机调试接口，不是新增外部消息服务或通用页面控制 API。回退程序后局部展示更新按旧实现处理，已保存的桌面消息和模型配置仍保留；关闭 Hub 或停用接入不会结束天工中的任务。用户随后确认 `fe48829e…` 修复后桌面消息能更新，仅覆盖该次操作；其他平台、完整分页、重启恢复及原生保护仍待用户验收；已发布 v0.4.30-3 的 macOS Actions 已通过；本版 v0.4.30-4 的 macOS 首次 DMG 失败保留历史，同标签修复重建及包核验已通过，实机同步仍待确认。

旧版关联读取可忽略 `owned_reply_ids`，但旧软件重写关联时会丢弃该新增字段；降级前应备份配置与关联记录。升级时字段缺失仍可读取，Hub 无法凭占位文本猜测旧缓存行的来源。对于前阶段已经卡住、又没有持久来源记录的窗口，用户需确认任务和审批后正常退出天工，再从 Hub 启动一次以加载已保存记录；此后新回复才有明确来源可跨 Hub 重启恢复。若来源缓存被淘汰、关联保存失败或原生存在未落盘内容，同步继续保留安全延期，已保存消息可由天工正常重新打开读取；不自动结束、重放或批准任务。

`GMCLAW_CONFIG_PATH` 是 Hub 定位既有模型数据库的路径覆盖，与授权环境变量用途不同。Hub 启动口令保存在自身本地配置，运行授权只临时缓存；两者都不应写入仓库、截图或日志，状态摘要不返回口令。连接检查只读健康与授权状态接口，不调用模型或执行工具；`GET /api/gui/dashboard` 的 `clientOverview` 分别展示模型配置、本地模型连接与 IM Harness 授权，不改变发送者的 IM 绑定。概览复用不超过 15 秒的状态缓存，未完成显示未验证，可推动已启用连接的授权识别，但不启动桌面或重放任务。

外部消息执行使用当前 IM 账号允许名单及 `/tg`（兼容 `/gmclaw`）选择执行端，不扩展附件、主动取消或 MCP 配置管理。连接指纹包含 `enabled/endpoint/authToken/modelId/maxSteps`，不包含安装路径或旧全局项目目录；待审批另固定产生该审批时的运行授权身份，自动重连也不把旧工具调用提交给新连接。旧运行授权审批需按提示到桌面核对后通过 `/tg new` 归档为未知，不批准、不重放，也不声称已停止。旧配置缺失 `desktopPath` 时兼容自动查找。此前历史状态修复本身不需配置或关联格式迁移；本轮新增可选回复行字段的兼容及回退见上文。回退到历史状态修复前，残留 `processing` 可能再次阻止恢复。回退前一授权识别版本将失去后台重连、真实桌面同步及全场景恢复；当时旧程序不读取新关联文件，不会因此删除桌面任务/消息。回退更早已发布程序还可能要求全局 `projectPath` 并在保存时丢弃新字段。应先备份配置与关联记录、在对应桌面确认任务/审批状态；重启不会取消任务。详见 [TC-012](customizations/gmclaw-im.md)。

恢复天工原生历史的 `max_steps` 来自原会话元数据；已核对天工 1.1.1 桌面提交 `1000`，Hub 恢复、元数据登记及执行请求允许 `1–1000` 并原样传递，字段缺失沿用官方默认 `20`。`0` 不是无限，超出支持范围仍拒绝；新建配置的默认值和 `1–200` 限制不变。Codex “归到 AI Gateway 会话”仅调整其 `model_provider`，与天工恢复无关。步数兼容不需格式迁移或自动数据改写；回退到该修复前的程序可能重新拒绝 `1000` 步原生历史。详见 [原生步数兼容](customizations/gmclaw-im.md#原生会话执行步数兼容)。

## `codexhub` Config

Use an explicit config path for predictable behavior:

```powershell
TianCaiSpaceHub --config D:\path\to\config.toml daemon
```

Example:

```toml
bind = "127.0.0.1:3847"
statePath = "codexhub-state.json"

[outboundProxy]
mode = "system"
url = ""

[feishu]
appId = ""
appSecret = ""
mentionOnly = true
allowedOpenIds = []
allowedChatIds = []

[telegram]
botToken = ""
allowedChatIds = []

[wechat]
accountId = "wechat"
botToken = ""
baseUrl = ""
userId = ""
botType = "3"
allowedUserIds = []

[bridge]
enabled = true
accountId = "default"
sendStreaming = true
```

Paths relative to the config file are normalized at startup.

### `bind`

HTTP bind address for the local backend API and remote-control websocket.

Default:

```toml
bind = "127.0.0.1:3847"
```

Keep this on localhost. Do not expose it directly to a network.

### `outboundProxy`

Controls only requests that CodexHub sends to external services such as model providers,
WeChat, Telegram, Feishu HTTP APIs, and update endpoints. It does not change the operating
system proxy or the environment of other applications.

```toml
[outboundProxy]
mode = "system" # system | direct | custom
url = ""
```

- `system` follows the operating system proxy and proxy environment variables.
- `direct` disables proxy discovery for CodexHub HTTP requests.
- `custom` uses `url` as an explicit HTTP, HTTPS, SOCKS5, or SOCKS5H proxy.

Example for a local Clash mixed port:

```toml
[outboundProxy]
mode = "custom"
url = "http://127.0.0.1:7890"
```

The desktop GUI exposes the same setting under `Network` and applies it immediately while the
daemon is running. Local GUI-to-daemon requests always bypass proxies. A VPN implemented as a TUN or Network Extension may still route traffic below
the HTTP proxy layer; configure loopback exclusions in that VPN when necessary.

### `statePath`

Path to the persisted state JSON file.

This stores local bridge state such as IM conversation bindings. It should not be committed.

## Feishu

```toml
[feishu]
appId = ""
appSecret = ""
mentionOnly = true
allowedOpenIds = []
allowedChatIds = []
```

### `appId` / `appSecret`

Feishu app credentials. The desktop GUI onboarding flow can populate these automatically.

Do not commit real credentials.

### `mentionOnly`

When `true`, group messages are ignored unless the bot is mentioned. Direct messages are still accepted.

### `allowedOpenIds`

Optional allowlist of Feishu user `open_id` values.

Empty means no user-level allowlist.

### `allowedChatIds`

Optional allowlist of Feishu chat ids.

Empty means no chat-level allowlist.

## Telegram

```toml
[telegram]
botToken = ""
allowedChatIds = []
```

### `botToken`

Telegram Bot token from BotFather. `bot_token` is also accepted for hand-written config.

This is the private-chat bot flow: create your own bot with BotFather, then send messages to that bot from your Telegram account. It does not require Telegram `api_id`, `api_hash`, phone login, or an MTProto user session.

Group chats are intentionally ignored for now. This prevents other group members from controlling the host machine through the bot.

Existing configs may still contain `mentionOnly`; it is kept for compatibility but is not used while Telegram group chats are disabled.

### `allowedChatIds`

Allowlist of Telegram private chat ids as strings.

Empty means "bind the first private chat". After the first private Telegram message is accepted, `codexhub` writes that chat id into `allowedChatIds` and rejects other private chats.

For stricter setup, prefill this list before starting the bridge:

```toml
allowedChatIds = ["123456789"]
```

## WeChat

```toml
[wechat]
accountId = "wechat"
botToken = ""
baseUrl = ""
userId = ""
botType = "3"
allowedUserIds = []
```

WeChat config is normally written by the GUI QR onboarding flow. The implementation follows the OpenClaw WeChat bot path: QR login through `https://ilinkai.weixin.qq.com`, bot type `3`, long polling through `ilink/bot/getupdates`, and text replies through `ilink/bot/sendmessage`.

### `accountId`

Local label for the WeChat bot account. It is used in route keys and persisted state.

### `botToken`

WeChat bot token returned by QR onboarding. Do not commit real tokens.

### `baseUrl`

WeChat iLink API base URL. Leave empty unless the QR flow returns a redirected host.

### `userId`

The WeChat user id returned by onboarding. It is stored for display and allowlist defaults.

### `botType`

Current bot type. The default is `3`.

### `allowedUserIds`

Optional allowlist of WeChat user ids.

Empty means no user-level allowlist.

## WeCom (Enterprise WeChat)

```toml
[wecom]
enabled = true
accountId = "wecom"
botId = ""
secret = ""
displayName = "企业微信机器人"
websocketUrl = "wss://openws.work.weixin.qq.com"
allowedUserIds = []
allowedChatIds = []
```

The GUI QR flow normally writes `botId` and `secret`. CodexHub then subscribes to the official WeCom AI Bot WebSocket and supports direct/group text, streaming and final replies, initial/history thread routing cards, image/file input and output, and interactive approval template cards. Empty allowlists accept all users and chats. Keep `secret` private.

## Bridge

```toml
[bridge]
enabled = true
accountId = "default"
sendStreaming = true
```

### `enabled`

Controls whether the IM bridge should run.

When disabled, Feishu and WeCom websocket listening, Telegram polling, and WeChat polling stop, and IM messages are not forwarded to Codex.

### `accountId`

Local label used to build route keys:

```text
feishu:<accountId>:<chatId>
telegram:<accountId>:<chatId>
wechat:<accountId>:<userId>
wecom:<accountId>:<userId-or-groupChatId>
```

### `sendStreaming`

Controls whether assistant deltas are streamed into Feishu cards.

## Codex App Config

Codex App must point ChatGPT backend traffic at the local daemon:

```toml
chatgpt_base_url = "http://127.0.0.1:3847/backend-api"
```

This belongs in the Codex App config home, usually:

```text
~/.codex/config.toml
```

Third-party model provider keys stay in the Codex model provider section. Example:

```toml
model_provider = "llmx"
model = "gpt-5.5"

chatgpt_base_url = "http://127.0.0.1:3847/backend-api"

[model_providers.llmx]
name = "llmx"
base_url = "https://ai.llmx.cloud"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "your-third-party-key"
```

`chatgpt_base_url` is not the model API base URL. It is the ChatGPT backend-shaped URL used by Codex App features such as remote-control enrollment.
`codexhub` does not manage Codex App runtime settings such as `[features]`, `[windows]`, `[desktop]`, `[mcp_servers]`, or per-plugin `enabled` flags.

When CodexHub injects its default local AI Gateway provider, it keeps ChatGPT-shaped authentication enabled so Codex App retains its account-backed model catalog and Remote Control state:

```toml
web_search = "live"

[model_providers.ai-gateway]
name = "ai-gateway"
base_url = "http://127.0.0.1:3847/ai-gateway/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "dummy-token"
```

The provider identity remains `ai-gateway`, so `provider.is_openai()` is false and OpenAI-only remote compaction, request compression, and private metadata behavior stay disabled. The managed provider intentionally does not use Actor Authorization by default.

Actor Authorization requires `requires_openai_auth = false`, which makes the provider account API return no account. In Codex App 26.707.8479 this also causes the frontend to apply the official Statsig `available_models` allowlist; custom CodexHub models then disappear even though `/ai-gateway/v1/models` returns them. For that reason the native `web.run` provider gate remains disabled in the default configuration, and GPT-5.6 uses CodexHub's hosted `web_search` compatibility path instead. The `/alpha/search` proxy remains available for future Codex versions or explicit experimental configurations.

## Codex App Auth

Remote-control requires ChatGPT-compatible auth. API-key-only auth is rejected before the websocket connects.

For this local backend, use `chatgptAuthTokens` in Codex App's `auth.json`:

```json
{
  "auth_mode": "chatgptAuthTokens",
  "OPENAI_API_KEY": null,
  "tokens": {
    "id_token": "<local ChatGPT-shaped JWT>",
    "access_token": "<local ChatGPT-shaped JWT>",
    "refresh_token": "",
    "account_id": "acct_codexhub_local"
  },
  "last_refresh": "2026-05-26T00:00:00Z"
}
```

The local JWT needs to parse as a JWT and include the ChatGPT-shaped auth metadata Codex reads:

```json
{
  "email": "codexhub-local@example.local",
  "https://api.openai.com/auth": {
    "chatgpt_account_id": "acct_codexhub_local",
    "chatgpt_user_id": "user_codexhub_local",
    "user_id": "user_codexhub_local",
    "chatgpt_plan_type": "pro",
    "chatgpt_account_is_fedramp": false
  }
}
```

This identity is local bridge identity only. The model provider key controls the actual model provider.

The desktop GUI provides Codex App configuration controls that write the local Codex App config for you.

The CLI equivalent is:

```powershell
TianCaiSpaceHub --config config.toml configure-codex-app
```

Optional provider fields:

```powershell
TianCaiSpaceHub --config config.toml configure-codex-app --provider-name llmx --provider-base-url https://ai.llmx.cloud --provider-key sk-... --model gpt-5.5
```

When provider fields are supplied without `--provider-name`, `llmx` is used as the provider name.

The daemon does not modify Codex App config on startup. It writes these files only when the desktop GUI or CLI command is used.

## Feishu App Requirements

For a manually created Feishu app, enable bot messaging and websocket event delivery. Subscribe to:

```text
im.message.receive_v1
card.action.trigger
```

Typical permissions:

```text
im:message
im:message:send_as_bot
im:resource
```

Depending on Feishu app type and tenant policy, additional scopes may be required for card updates or attachment downloads.

## Local Files To Keep Private

These should stay ignored:

```text
config.toml
codexhub-state.json
*.log
.im/
target/
target-verify/
reference/
```

Do not commit Codex App `auth.json`, third-party provider keys, Feishu credentials, open ids, or chat ids.
