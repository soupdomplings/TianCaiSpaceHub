# 动态 Codex 可见模型

维护日期：2026-10-20。当前开发版本 `v0.4.30-5`、唯一 `main`，关联 TC-006、TC-007。已收到实际旧机资源，补齐布尔型 Fast 界面/请求结构，与较新的访问状态结构共用唯一匹配和局部变量校验；Ultra 既有过滤兼容保留。Fast、Ultra 和设置入口继续独立可选，缺项不阻塞模型列表、语言等基础启动检查。必要编译和本地测试 MSI 见 [构建交付](releases/v0.4.30-5.md#2026-10-19-旧版布尔型-fast-结构兼容)，本次按用户要求提交源码及有限 Fast 参数核对见 [最新状态](releases/v0.4.30-5.md#2026-10-20-兼容修复源码提交与-fast-参数核对)；现有本机请求结果不扩展为本轮测试包、全部版本、全部思考强度或 macOS 已验收。版本沿革见 [二开变更记录](customizations/CHANGELOG.md)。 本轮 Commands 模式、状态库原生迁移保护和独立修复按钮源码已随 `500574c` 推送；Windows/macOS Actions 已开始，见 [最新打包](releases/v0.4.30-5.md#2026-10-20-启动修复源码提交与两平台打包已启动)，不把启动构建当作实机验收。

Codex 可见模型现在支持内置目录之外的模型。模型来源包括已配置渠道保存的模型列表，以及 Codex 接入页中手动输入的模型 ID。

## 初始化与增强启动固定 Codex 对话模式

「无法加载组织设置」且原生日志为 SQLite 初始化失败的情况，可点击本页本地配置区的“修复无法加载组织设置”，见 [状态库检查与修复](customizations/codex-startup-repair.md#hub-内置按钮与使用)。Hub 保持打开，仅要求 ChatGPT/Codex 完全退出，自动检查备份，无需单独脚本或 Python。本轮 Hub 源码停止提前创建原生远程控制 schema，增强启动不自动修复旧库；该问题与本节 Work 模式身份检查分开处理，修正对话模式不能替代状态库修复。

2026-10-20：初始化、“更新 Codex 配置”和每次增强启动前，Hub 都确保当前 Codex home 的 `config.toml` 包含以下值。键属于 `[desktop]`，不是 TOML 顶层或 Provider 段：

```toml
[desktop]
conversationDetailMode = "STEPS_COMMANDS"
```

原键缺失、为 `STEPS_PROSE`、`STEPS_EXECUTION` 或其他字符串时，先备份再改成 `STEPS_COMMANDS`。受管配置缺少该值或值不同时，接入页显示可以更新；仅查看状态或升级 Hub 不写配置。增强启动预检查发现值已正确时不重写配置或受管恢复点；需要修改时只处理模式键，不重新初始化认证或模型选项，保留其他字段。

修正在创建 Codex 进程前完成。GUI 沿用先退出已运行 Codex 的流程；直接调用启动接口时，如果 Codex 仍在运行且模式需要修正，提示先完全退出再增强启动，避免只改磁盘却把进程内旧模式当作已修复。配置格式、字段类型、备份或写入失败时返回错误，不继续创建新进程；模式正确的运行实例仍按原 CDP 规则处理。普通直接打开客户端不经过预检查；用户随后在客户端切换 Work 时，原生应用仍可能改回 `STEPS_PROSE`，下一次 Hub 增强启动再修正，不在会话中持续强制模式。

问题依据：本机 Windows Codex `26.1013.4894.0` 的只读资源显示，`STEPS_PROSE` 会使 `requireWorkBrowserIdentity=true`；浏览器运行配置存在、已认证身份为空时，于提交前抛出 `Work browser access requires an authenticated account`。[社区 issue #2539](https://github.com/yetone/magpie/issues/2539) 也报告切回 Commands 后 Codex 可用、切回 Work 会复发。该修改用于恢复 Codex 模式，不提供 Work 浏览器账号身份。实现按 TOML 字段工作，不修改 ASAR、签名或按 Codex 版本号分支；Windows/macOS 共用实现，客户端实机仍待用户验收。

沿用受管 manifest v1/v2，新增可选 `conversation_mode.original_mode`，记录本次修正前的模式字符串或原键缺失，不含秘密。旧 manifest 缺少该项时正常读取，首次确需修改时补记；受管值未变的重复初始化/启动保留恢复点，用户更改或删除后再次修正，以这次修改前的值为新恢复点。“恢复配置”仅在当前模式仍为 Hub 写入的 `STEPS_COMMANDS` 时还原记录；原键缺失则移除，用户后改的模式保留。`config.toml.bak` 和原子写入继续使用，manifest 与 TOML 分步写入，不宣称跨文件事务；回退旧 Hub 前先用新版恢复，旧程序不负责撤回新增字段。

维护入口：[配置、检查与恢复](../src/codex_app_config.rs)（`set_managed_commands_mode`、`prepare_codex_app_conversation_mode`、`managed_gateway_config_is_current`、`restore_conversation_mode`）、[增强启动](../src/codex_app_enhanced.rs)（进程状态检查后、创建进程前）。回归代码覆盖初始化/重复更新、启动修正、正确值不重写、运行中拒绝修正、缺失键恢复及用户后改保护；本轮编译、测试包身份和未执行项见 [本轮交付](releases/v0.4.30-5.md#2026-10-20-初始化与增强启动修正对话模式)。

## 初始化开启 MAX、ULTRA 与 Fast 选项

在“ChatGPT(Codex) 接入”点击“初始化 Codex 配置”或“更新 Codex 配置”时，Hub 同时补齐模型控制选项。已有接入但缺少这些设置时，按钮显示可更新；只升级 Hub、启动或查看接入状态不会自动补写本节的模型控制选项。增强启动会另行检查并修正上节的对话模式。操作成功后完全退出 Codex App，再从 Hub“增强启动”打开。配置页、按钮说明、悬浮说明和成功弹窗使用通用接入提示，不额外列出 MAX/ULTRA/Fast。

写入目标沿用当前 Codex home（默认 `~/.codex/config.toml`，支持原有 home 覆盖），Windows/macOS 使用同一 TOML 读写流程：

```toml
[desktop]
enabled-reasoning-efforts = ["low", "medium", "high", "xhigh", "max", "ultra"]

[features]
fast_mode = true
```

已有 `enabled-reasoning-efforts` 数组时，保留原条目、顺序和数组注释，只补缺少的 `max`、`ultra`，不重复追加；只有原键不存在时使用示例六档，原空数组只补两档。除上节受管对话模式外，其他 desktop/features 设置、当前 `model`、`model_reasoning_effort`、`service_tier` 及 `desktop.default-service-tier` 均保留，不自动切换当前使用的强度或速度。内联表按原有 helper 合并；desktop/features 类型错误、推理列表不是纯文字数组或 Fast 开关不是布尔值时，在写入 config.toml 前提示修正，不清空这些配置来强行初始化。模型控制选项沿用既有字段。

Fast 开关依据 [OpenAI 官方速度配置说明](https://learn.chatgpt.com/docs/agent-configuration/speed)。它不足以独立保证自定义 Provider 的桌面入口显示。只读核对 Codex `26.1013.4894.0` 确认：默认 `requires_openai_auth=false + Actor Authorization` 时，界面没有 ChatGPT 登录态；Fast 的界面和请求辅助函数分别据此返回不可用，ULTRA 的模型列表还受 `1186680773` 控制。前轮 `a68b5a6` 只写配置，用户在同版本、增强启动、多个模型下反馈仅 MAX 显示。

增强启动通过原有本机 CDP 会话，定向适配内存中加载的模型控制脚本：只在当前 Provider 确认为本 Hub 受管 `ai-gateway`、且 `features.fast_mode=true` 时启用，具体例外限定 `hostId=local`。Fast 支持已核对的 `isServiceTierAllowed` 布尔结构和 `serviceTierAccess` 访问状态结构：旧版仅判断 ChatGPT 认证、请求返回 false/布尔 requirements；较新版另支持 PAT、返回 null/访问状态对象。只向各自原认证条件增加受管本机分支，保留原返回类型、认证读取、待加载/空数据保护、真实 requirements、缓存、显式禁止项及失败处理，不强制返回可用，也不改全局账号或 Provider 身份。ULTRA 仅放行本地模型查询的显示过滤，仍须推理列表勾选和模型真实能力，不全局覆盖 Statsig gate。前两份资源的模型配置查询默认 `includeUltraReasoningEffort=true`，Hub 不为此改 ChatGPT 账号设置或额外写入 slider 配置。

适配不按 Codex 版本号、版本白名单或 ASAR 哈希选择分支，而是识别加载模块的关键字段、调用结构并核对局部变量；例如 `teamCatalog` 有无均按实际结构处理。source marker v2 / 增强脚本 v23 是 Hub 自身脚本标记，不是 Codex 版本限制。无法唯一识别的可选结构保持原脚本，不将缺少 Fast 或 Ultra 作为启动错误，也不阻止另一项成功适配。两项都未适配时仍按客户端原有行为启动；已安装的拦截器继续处理后续按需加载的模块。当前不能保证所有历史和未来版本均出现这些选项，成功启动也不代表所有增强选项可用。

新提供的实际旧机 ASAR 资源元数据为 `26.917.51856`，与此前 `26.1007.71326` 不同；这些值仅表示资源样本，不是适配白名单或桌面显示版本。旧机使用 `isServiceTierAllowed` 布尔 UI 及返回 false 的请求 helper，旧规则只识别较新访问状态结构，故原日志 Fast 两项未加载，而 Ultra 已加载。本轮补齐这两种 Fast 结构的共同认证/host 前缀和已核对结果形态，增加逗号边界，校验所有复用变量和结果，再在整个模块唯一匹配时修改。三份实际资源中 Fast UI/请求与 Ultra 均各唯一匹配，静态等效改写后完整模块语法通过；旧机本轮实机仍待用户验收。[前轮诊断](releases/v0.4.30-5.md#2026-10-19-较旧客户端-fast-适配缺项诊断) 中“尚缺该机资源”属于当时状态，现已补齐。

2026-10-19 按用户要求参考本机 `codex-windows-fast-patch` 的 Model Experience 诊断方法，并只读对照测试机与本机原始资源。测试机资源的推理枚举已接受 `ultra`，模型过滤器也已实现 ULTRA；不能将它未出现解释为旧版没有这项功能，升级客户端不是本轮修复的前提。

| 诊断层 | 已核对资源中的实际条件与 Hub 处理 | 验证边界 |
| --- | --- | --- |
| Fast 界面 | 布尔 `isServiceTierAllowed` 与访问状态 `serviceTierAccess` 两类 helper 原认证分支增加受管本机例外；保留真实 requirements、加载/空值及失败状态 | 源码适配实际加载与 Fast 按钮可用分别核对 |
| Fast 请求 | 布尔型和访问状态型 helper 均有独立认证判断；使用同范围例外，原返回类型、requirements 查询、缓存和结果计算保留 | 显示标志或源码标记不证明请求已使用 Fast；需选择 Fast 后在实际模型请求中核对 `service_tier=priority`，仅模型列表请求或模型口头回复不能证明 |
| Ultra 模型列表 | 普通 Codex 查询默认 `includeUltraReasoningEffort=true`；随后依次应用 `1186680773`、`enabled-reasoning-efforts` 和真实 `supportedReasoningEfforts`，配置页开关写同一推理列表 | 仅适配本地查询的 gate；已勾选且模型真实支持时才提供该档，不改其他调用的 gate 或模型能力 |

此前两份资源的调查未发现需要套用 skill 旧 Power 滑块或账户迁移 fallback 的证据；新旧机资源中虽存在 `chatgpt-ultra-effort-migration`，本轮不改该账户迁移分支。其 Codex Ultra `model/list` 查询已由现有规则唯一识别，补齐 Fast 不要求同时改写账户偏好或安装 ASAR。只参考 `codex-windows-fast-patch` 的 Fast UI/请求分层和旧布尔型结构保护，没有运行 skill 安装、重签名、修补或真实请求验证；Windows MSIX 工作流也不用于 macOS。Fast 显示、真实 `service_tier=priority` 及各旧机设置入口仍由用户验收，不由静态匹配推导。

同一增强会话还独立适配“设置 → 配置”的“模型功能”和“工作空间依赖项”入口。安装源码中它们分别受 `3693343337`、`3026692602` 控制；仅在原本机受管范围内增加例外，保留 Work 页面排除、local host kind 与真实 `primaryRuntime` 检查，中间 `2106641128` 不改。没有依赖运行时的客户端不会凭空出现可安装内容；Hub 不安装、重置或伪造工作空间依赖。设置模块按访问加载，`settingsSections` 独立合并，不纳入启动 ready；实际旧机资源未提供现有 `configuration-panel` 结构，当前该标记保持 false，不据此伪造新入口或判定客户端没有设置能力。已有任一 v2 适配标记时，不因其他可选项缺失刷新窗口。

该适配使用 Windows/macOS 共用协议，不改 Codex 安装文件、签名或用户套餐。只拦截已知 `app://.../assets/` 的模型与配置脚本响应，四类结构必须唯一匹配且局部变量一致才修改；未知布局或正文获取失败保留原脚本，最多等待 10 秒后放行。增强会话先启用 Network、禁用该会话资源缓存，再启用 Fetch；改写响应移除旧缓存/长度等元数据并加 `Cache-Control: no-store`。browser/page CDP 的帧和消息上限都设为有限 64 MiB：本机脚本原文 14,403,933 字节，base64 为 19,205,244 字节，可能超过库默认 16 MiB 单帧限制；此前中断可静默转成外层 45 秒超时。该传输边界修正保留。2026-10-19 收到的测试机诊断包明确显示另一处缺口：早期 renderer 自动挂载超时后，原 page 备用路径只安装增强脚本、没有安装 Fetch 拦截器，因此三个模型源码标记无法齐备；不能再将此情况笼统归因于传输大小。自动挂载最初为何错过 renderer，现有日志未能确认。第二次诊断仅有 browser 补装、Fast/设置命中和 ULTRA 未命中的布尔结果；第三次收到测试机原始 ASAR 后确认，其模型查询不含本机较新资源的 `teamCatalog` 字段。原模式要求该字段，测试机 ULTRA 原始模块匹配为零；当前仅将该尾字段设为可选，已在两份原始资源中各唯一匹配，并通过等效改写后完整模块语法核对。其余字段顺序、gate、局部变量及查询/select 校验均保留，未知或歧义结构仍不修改；静态核对不替代用户实机验收。

启动完成条件只检查基础配置：增强脚本已安装且应用、模型列表与预期一致、模型和语言配置可用、必要界面 gate、工具目录及本机配置连接已安装。Fast UI、Fast request、Ultra models 和 settingsSections 分别保留实际加载状态；`modelControlsReady` 仍如实表示前三项是否齐备，允许其为 false 时基础启动成功，不伪造完成标记。普通日志新增 `model_controls_optional_status`（本次状态快照）和 `model_controls_optional_unavailable`（可选连接失败后继续基础配置）；Fast 界面和请求适配不互相冒充，按钮出现不证明实际请求档位。Hub 的正常成功提示与配置弹窗继续使用普通接入说明，不额外提示这些可选功能。

早期挂载未覆盖实际窗口时，仍以真实 targetId 显式挂载 browser 会话并核对安装响应；窗口握手限 10 秒、每次尝试限 15 秒、最多顺序两次，仅重试尚未安装脚本的握手。可选补装总计最多占用 30 秒，包含在原 45 秒同步时限内，为独立 page 基础配置保留至少约 15 秒；失败或超时只写诊断并继续 page 配置。browser 拦截会话中断也不直接中止启动，随后由独立 page 检查基础配置是否确实可用。基础 page 连接或模型、语言等必要配置失败仍返回真实错误，不掩盖成成功。普通 CDP 命令限 5 秒、增强启动整体 80 秒、GUI 等待 90 秒的保护保留，未延长任何截止点。

仅本次新启动首窗、拦截安装确认后，可以在 load 完成后 250ms 单次按需刷新，以补救错过首轮脚本加载的情况；已有 v2 源码标记（包括部分成功）就不刷新，已运行窗口不自动刷新。替换前释放旧 browser 连接，同 session 不重复安装，暂停请求最多 10 秒后放行。普通日志保留固定阶段、唯一匹配/原样放行、连接失败及各项布尔状态，不采集脚本正文、认证信息或私人配置。维护入口：[配置及恢复](../src/codex_app_config.rs)、[增强启动](../src/codex_app_enhanced.rs)、[模型与设置脚本适配](../src/codex_app_enhanced/model_controls.rs)、[启动错误显示](../src/gui/api.rs)。

MAX/ULTRA 的 desktop 键来自安装 schema 和只读配置白名单，官方通用配置页面未列出这些键；不读取或登记完整个人配置、密钥与私有日志。普通启动、CLI 和其他客户端不经过这条桌面适配。可选项仍须模型真实支持：例如当前目录的 `gpt-6-luna` 有 MAX、无 ULTRA；无速度档的模型不制造 Fast 按钮。启用入口不代表已选择 Fast，不强制请求服务档，也不承诺代理或上游实际提速。

### Fast 实际请求核对

Fast 在 OpenAI Responses 请求正文中对应 `service_tier: "priority"`；思考强度位于 `reasoning.effort`，需分别核对。Hub 的原生 Responses HTTP 和 WebSocket 路径保留收到的顶层服务档，不因强度为 MAX 或其他值自动补上 Fast；开启配置或看到 Fast 图标不能代替实际请求证据。

在 Hub“请求日志”页勾选“启用请求日志”和“记录详情”，由用户开启 Fast 发起一条请求，再双击对应记录，对比“Codex 请求”“上游请求”和“响应 / 上游 SSE”中的 `service_tier`。未开启详情的历史正文不能恢复；只检查目标会话与目标模型的对应记录，不以其他并发调用判断当前选择。

| 已记录的值 | 可确认的结论 |
| --- | --- |
| Codex 请求和上游请求均为 `priority` | Hub 已将该次 Fast 参数发给厂商 |
| Codex 请求没有该字段，上游请求也没有 | Codex 此次没有向 Hub 提交 Fast 参数，Hub 没有丢掉它 |
| Codex 请求为 `priority`，上游请求缺失或不同 | 需要定位 Hub 出站处理；不能先归因于厂商 |
| 上游响应回报 `priority` | 该接口回报的实际服务档为 Fast |
| 上游响应没有回报服务档 | 仅凭请求参数不能确认上游实际采用的档位 |

2026-10-20 对用户自行发起的现有本机请求做只读核对：MAX 样本入站、出站及上游回报均为 `priority`；同期另有 `xhigh` 样本入站即无此字段。样本涉及不同会话，不将该差异写成“仅 MAX 支持 Fast”的规则，也没有据此修改网关、强制全部请求使用 Fast。完整归属见 [交付记录](releases/v0.4.30-5.md#2026-10-20-兼容修复源码提交与-fast-参数核对)。

### 备份与恢复

沿用 Hub 受管备份 manifest v2，可选 `model_controls` 记录原推理列表（缺失和空数组区分）、最后写入列表及原 fast_mode，不含账号或秘密；对话模式使用上节独立的可选 `conversation_mode`。旧 v1/v2 在首次显式初始化/更新时补记，不重置原 ChatGPT 登录备份。重复初始化且受管值未变时保留原恢复点；用户手工修改/删除对应项后再显式初始化，分别以本次修改前值为新恢复点。

“恢复配置”仅在当前推理列表仍等于 Hub 最后写入列表、或对应开关仍为 Hub 写入的 true 时，分别还原原值；原键缺失则删除，其他 desktop/features 字段保留。用户后来修改/删除的项不覆盖；没有受管记录时不猜测并删除模型选项。前端适配只在本次增强会话内存中，完全退出 Codex 后结束；不必修复安装包。若需回退旧程序，应先用新程序恢复；旧程序不识别新增备份字段，也不会负责撤回它们。原有逐次 config.toml 备份与原子文件写入继续保留，配置和 manifest 为分步写入，不宣称跨文件事务。布尔型 Fast 兼容没有新增持久配置或备份字段；只回退本轮匹配修改会失去旧布尔型 Fast 适配，但上一轮可选启动策略继续生效，缺项仍可启动。若同时回退上一轮启动策略，才会恢复 Fast/Ultra 必须齐备及可选连接失败阻止启动的旧条件；两者均无需迁移用户数据。

维护入口：[初始化、更新状态与恢复](../src/codex_app_config.rs)（`enable_model_controls`、`desktop_reasoning_efforts`、`managed_gateway_config_is_current`、`restore_model_controls`）、[按钮入口](../src/gui/codex_tab.rs)、[中英文帮助及成功提示](../src/gui/text.rs)。开发方只做必要编译与静态核对，不运行 Hub/Codex、测试夹具或真实模型请求，不改本机正在使用的配置；Windows/macOS 实机分别待用户验收，完整构建身份见本轮交付。

## 厂商保存时自动获取模型

“大模型接入”新增或编辑普通厂商时，模型表中没有任何条目，点击“保存”会先在后台获取远端模型；获取到非空模型列表后继续完成这一次保存。ChatGPT 账号类型也支持该流程，但须已完成账号授权；空列表获取成功后默认选中获取到的全部模型再保存。列表已有条目时仍按原有选择、模型映射和校验规则保存；ChatGPT 有模型但一个也未勾选时仍要求选择，不把未勾选当作空列表重新获取。

开始自动获取前会先校验名称、保留名称限制和正整数优先级；普通厂商还校验服务地址、可选模型列表地址为完整 HTTP(S) 地址。缺少这些信息时直接说明需要修正的字段，不先发送发现请求。成功获取后仍使用原有最终保存校验，不略过模型映射或配置保存保护。

获取失败或返回空列表时弹窗说明失败原因，厂商不会保存，编辑窗口保留，可以修正地址、密钥或账号后重试。普通厂商筛选后没有可用模型、ChatGPT 返回空列表或只有空白模型 ID，也按失败处理，不先清掉已有列表及勾选。自动获取期间暂时禁用保存、获取、厂商选择和相关输入，避免把旧输入的结果保存成新配置；“取消”仍可关闭窗口，迟到的获取结果不继续保存。ChatGPT 模型获取还核对本次授权账号，其他事件不会提前解除获取的忙碌状态。该操作只查询模型目录，不发起推理或计费验证调用。

单独点击“获取远端模型列表”仍只是更新编辑窗口中的列表，不自动保存。下面的“ChatGPT(Codex) 接入 → Codex 可见模型”则是另一处可见模型编辑入口，继续按其原有“保存模型列表”流程工作，不能把厂商编辑器的自动保存规则套用到这里。

维护入口：[普通厂商编辑器与远端获取](../src/gui.rs)、[ChatGPT 账号厂商编辑器](../src/gui/chatgpt.rs)。不增加配置字段、不改变渠道身份、已有模型别名或账号凭据保存方式；旧配置无需迁移。回退程序恢复旧保存流程及手动获取操作，已保存的模型列表仍按旧格式读取。

## 使用方式

1. 在“大模型接入”中配置并启用渠道，填写 API Key 和地址。
2. 在“ChatGPT(Codex) 接入 → Codex 可见模型”选择渠道，点击“获取远端模型列表”；也可点击“同步渠道模型”读取所有已启用渠道中已保存的模型和映射（不包括 WorkBuddy 与天工专用渠道）。
3. 在文本框中保留需要的模型，每行一个，可手动增删。远端获取和同步均保留已有手动输入。勾选需要展示的内置模型。
4. 点击“保存模型列表”。文本框中尚无可用路由的模型将以同名加入所选渠道；已有路由及别名映射保持不变。未选择渠道且存在无路由模型时，会提示选择渠道，不会保存半成品配置。
5. 使用增强模式重新启动 Codex，验证模型下拉框和对话。普通启动的前端模型过滤仍取决于 Codex App 版本。

内置模型继续使用原能力描述。未内置的模型按完整名称前缀匹配同系列内置模型，完整继承其上下文、推理等级、图片、工具、Responses Lite、速度档、压缩参数和基础指令等能力与高级参数；只替换模型 ID、显示名称、说明及展示排序。

- 当前 `gpt-6-luna`、`gpt-6-sol`、`gpt-6-astra`、`gpt-6.1-sol` 都已内置，直接使用各自目录能力。只有未知 ID 才按匹配规则继承 GPT-6 家族模板；不能把 Luna 当成继承 Astra 的例子。
- `0.4.30-1` 新增目录后，未知 `gpt-6-next`、`GPT-6-NEXT`、`vendor/gpt-6-next` 依既有规则默认继承 `gpt-6.1-sol`，此前为 `gpt-6-astra`。默认上下文 `272000`、最大上下文 `872000` 未变，其他能力与高级参数随模板继承；有明确 `capabilityProfile` 或手填字段时仍优先采用用户覆盖。这里是本地目录行为，不是对未知模型真实能力的承诺。
- `gpt-5-new` → GPT-5 系列中数字版本最高的内置模型，当前为 `gpt-5.6-sol`。
- `deepseek-v4-new` → `deepseek-v4-pro`；`deepseek-v4-flash-next` → 更精确匹配的 `deepseek-v4-flash`。
- 优先最长完整前缀，再比较数字版本（6.10 大于 6.9）；版本相同按内置目录顺序选择。大小写和 `vendor/` 命名空间不影响匹配；`gpt-6` 不会误匹配 `gpt-60`。
- 使用任意别名时，也会尝试按该渠道映射的上游模型名继承。
- 手动指定 `capabilityProfile` 优先于自动匹配，可填写具体内置模型 ID 或协议模板名。手动填写的上下文、图片和推理设置最后覆盖，未填写的字段继续继承。

仅当既没有指定模板、也找不到同系列内置模型时，才使用原来的保守配置（32K、文本输入、普通 Responses）。继承规则来自本地模型目录，不代表已向厂商核验新模型的真实规格。

高级能力目前通过 Hub 的 `config.toml` 配置。按上游实际支持情况添加条目后重启 Hub；能力配置变化也会改变模型目录 ETag。不要把示例数值当作某个新模型的官方规格。

```toml
[[aiGateway.codexModelProfiles]]
id = "my-new-model"
displayName = "My New Model"
capabilityProfile = "openai_responses"
contextWindow = 64000
maxContextWindow = 128000
supportsImages = true
supportsReasoning = true
```

`supportsReasoning = true` 保留继承模型的完整推理等级；模板没有推理等级时才补充 low/medium/high。`false` 会清空推理等级。高级配置不是路由绑定，模型别名仍使用渠道的模型映射。旧版 `codexVisibleModels` 配置继续兼容。在 Codex 可见模型编辑页，获取失败不会清空列表，获取操作不自动保存，删除可见模型也不会删除已有渠道模型。

## 客户端所选模型未配置

客户端选中模型不代表 Hub 已有可用路由；仍须在“大模型接入”的对应厂商模型列表中保存这个模型或适用映射。网关找不到符合当前入口的候选渠道时，共用错误说明改为：

> 所选模型「模型ID」不在“大模型接入”的相关模型列表中。请新增大模型厂商，或检查对应厂商的模型列表。

HTTP 状态仍为 `422`，`error.type=invalid_model_error`、`error.code=invalid_model` 不变。网关正文固定中文，不随 GUI 语言切换；Codex 接入帮助也同步中英文配置指引。客户端自己加上的 `unexpected status 422` 或 `Unprocessable Entity` 等外层文字可能继续显示；Hub 改的是返回正文。停用、协议不适用或专用条目模型不匹配也可能导致没有候选，应同时检查对应厂商的启用状态、协议和客户端条目；不跨接入点自动改模型或回退到其他厂商。完整入口隔离与维护位置见 [接入点用途与隔离](customizations/client-channel-scope.md#模型未配置提示)。

## 建议验收

此处为用户验收参考。`0.4.30-1` 已同步目录和未知模型继承的测试期望，Windows 测试代码编译通过，未执行测试或真实模型调用。`v0.4.30-2` 将 WorkBuddy 多条目命名空间一并排除，已通过 Windows/macOS Actions 原生构建及打包并预发布；功能测试、安装升级、实机和真实模型调用仍待用户验收。Windows 未签名、macOS universal/ad-hoc/未公证的安装包信息见 [版本交付](releases/v0.4.30-2.md)。

本轮 `v0.4.30-5` 新增行为另行记录于 [本轮交付](releases/v0.4.30-5.md#2026-10-12-空模型自动获取与模型未配置提示)，不复用上述历史构建作为通过证据。普通/ChatGPT 厂商空列表保存、成功自动保存、失败或空结果留窗、已有列表与未勾选边界、取消和迟到结果、中文错误在各客户端及 Windows/macOS 的显示均待用户验收；开发方不主动运行产品或真实模型调用。

- 新模型 ID 不在内置目录中，手动输入后保存，并验证 Codex 能选择和发送请求。
- 直接获取厂商列表，删掉不需要的条目后保存；检查手动输入仍保留。
- 已有模型别名继续转发为原上游名称；WorkBuddy 与天工专用渠道继续排除普通 Codex 路由。
- 关闭并重启 Hub，确认新增、删除和内置勾选结果保留。
- 使用错误地址或密钥获取列表，确认错误提示及原有列表保留。

## 与网页导入的关系

上述“获取失败保留列表”和“保存时补齐所选渠道路由”属于可见模型编辑页。网页导入采用独立预览：先查询模型，失败时允许空列表禁用保存；默认不追加可见模型，只有勾选后才追加，不替换已有列表。导入也不自动初始化 Codex。见 [网页导入](hub-external-import.md)。

代码定位：[网关配置](../src/ai_gateway/config.rs)、[模型目录](../src/ai_gateway/model.rs)、[Codex 页签](../src/gui/codex_tab.rs)、[GUI 保存与发现](../src/gui.rs)。旧版 `codexVisibleModels` 仍可读取；回退到不支持动态能力的版本前应备份配置，未知模型能力以实际上游为准。
