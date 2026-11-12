# ChatGPT(Codex) 检查与修复

维护日期：2026-11-11。当前已纳入 [V1.0.18 本地测试包](../releases/v1.0.18.md)，关联 TC-006 / TC-007；真实增强启动和修复待用户安装验收，未正式发布。

实现状态：原分类窗口、定向修复、备份恢复保持；新版数字异步资源增加 Fast UI / 请求、Ultra 查询与主机能力适配，深度修复复用同一模型适配器。代理继承与可信路径补丁补齐新版引号写法；完整逐项范围及验证见下方审计。旧 v9 的 Fast / [历史恢复](codex-history-recovery.md)、[v9 包](../releases/v0.4.30-9.md) 和 [v7 交付](../releases/v0.4.30-7.md) 保留各自身份，不作为本轮验收。

## 入口与使用

2026-11-12 搭档接入讨论：用户希望天才搭档理解 ChatGPT 报错，并可结合 `codex-windows-fast-patch` 自动检查／修复。当前 Hub 已内嵌经审阅的部分技能工具，搭档尚无直接调用修复中心的工具。本批 V1.0.29 不新增此能力，也不运行本机修复。

建议以现有分类诊断、执行、备份和恢复作为统一执行层，搭档内置技能的专项知识，根据证据选择处理；缺失专项逐项适配，保持通用 Skill/MCP 页面隐藏。只读诊断可以直接执行，普通模式一次展示本次修复范围并确认，完全访问沿用户交办范围执行已接入专项；需要退出客户端时统一说明，系统 UAC 沿 Windows 正常流程。不因一次报错默认运行全量 MSIX 修补、安装全部可选插件或重复询问每个文件。技能源本身是 Windows 专项，macOS 继续使用已有独立修复实现。此段为设计建议，未表示这些助手工具或授权衔接已实现。

在 **ChatGPT / Codex → 接入设置 → 检查与修复** 打开一个窗口，通过下拉列表选择遇到的问题。窗口可缩放，内容支持上下滚动，底部操作按钮固定显示；每个问题有简短说明，共用进度、结果和修复记录。

1. 打开后只检查“无法加载组织设置 / 启动失败”的程序路径与状态库，分别给出结论；没有迁移冲突也可以重新绑定程序路径，不会立即修复；切换问题后点击“检查所选问题”。检查允许客户端保持运行，不改用户配置、插件、安装文件或状态库；临时解析、资源校验和数据库副本在私有目录处理。
2. 结果识别到可处理项目后启用“修复所选问题”。正常退出 ChatGPT/Codex 后再点击，Hub 全程保持运行，网关服务继续工作。代理继承专项先在客户端运行时检查，确认后退出客户端再修复。
3. 窗口显示具体处理和跳过原因。客户端安装文件修复会额外确认；Windows 可能请求系统授权并获取缺少的 SDK 打包组件；macOS 在应用目录不可写时请求正常的管理员授权。未识别的结构、来源或二进制不强行修改。
4. 修复后关闭窗口，从 Hub 增强启动，再验证相应功能。缓存、helper 或服务文件变化后应新建工具会话。失败时保留修复记录，可“打开备份”或“恢复所选记录”；Hub 重启后记录仍可读取。

用户不需要安装技能、Python 或 Node 开发环境，也不需要打开解压目录。程序路径修复直接调用系统环境接口；其他定向资源工具使用当前客户端附带的 `cua_node`，复制到私有目录执行；客户端没有该运行时时停止并说明，不运行安装目录中的 Electron 来替代。检查不会启动实际 app-server、安装器或模型调用。

## 问题分类和实际范围

| 窗口中的问题 | 实际处理 | 平台与限制 |
| --- | --- | --- |
| 无法加载组织设置 / 启动失败 | 独立检查当前内置程序及 CODEX_CLI_PATH，可按用户脚本重新绑定路径；数据库另检查物理副本，只有已知冲突才备份、原生迁移并验证后替换 | Windows/macOS；不以迁移数量少为删库标准。数据库未知原因不改库，路径分支仍独立处理。详见 [启动修复](codex-startup-repair.md) |
| 模型与速度选项缺失 | 合并已有思考列表中的 `max/ultra`，设置 `features.fast_mode=true`，下一次增强启动按实际页面组件适配 | Windows/macOS；保留用户已有列表与原真实模型能力；没有组件时保留原菜单，不强制全部请求 Fast |
| 插件市场或缓存不完整 | 从当前安装包恢复 stable 市场、reserved 镜像和已有插件缓存；使用正式插件安装命令在隔离 home 注册、列出确认，再把结果纳入文件计划 | Windows/macOS；不安装无关可选插件，不将缓存存在等同正式安装；不导入隔离 home 的登录、数据库或其他配置 |
| 内置浏览器 / Chrome 本机连接失效 | 同源插件、当前 Node/Node REPL/CLI 稳定副本、native messaging manifest、host-config、两份 v2 连接记录和 Windows HKCU 注册路径 | Windows/macOS；保留可信 `browser-client.mjs` 原字节；浏览器扩展本身仍需正常安装，真实系统权限由系统管理 |
| 浏览器提示缺少 Codex 登录 token | 对照当前包来源和受支持完整哈希，兼容第三方厂商下精确的 `Codex auth token is unavailable` 错误 | 仅匹配已知 service；保留请求头、真实认证和其他策略错误，未知哈希跳过 |
| 浏览器辅助功能资源解压失败 | 对 AX 资源做 Brotli 解压和 WASM 校验，从同版本、同 service 的健康来源恢复 | Windows/macOS；没有可信健康来源不混用其他版本 |
| Windows 电脑操控 / Any App 不可用 | 刷新相应插件与运行时，补齐本机开关；旧版匹配脚本与说明的完整哈希，新版 descriptor-only 匹配主进程 surface 结构 | Windows；macOS 显示不适用并提供系统权限入口。保留工具审批，不改临时 `.mcp.json` 冒充持久修复 |
| 电脑操控跨次调用失败 | 已知 helper-transport 请求上下文重新绑定 | Windows；完整输入/输出 SHA-256 匹配的 `@oai/sky` profile，其他版本跳过，不移除审批 |
| Windows 10 截图失败或超时 | 已知 helper 字节区间修复，逐项核对运行时、原/输出完整哈希 | 仅 Win10 build `10240..21999`，且用户错误包含 `SetIsBorderRequired`、`0x80004002` 或 `FrameArrived timed out`；Win11/Mac/未知 helper 不套用 |
| 更新后已配置的 MCP 无法启动 | 只改已删除的 Codex CUA Node 绝对命令路径，替换源必须与当前包 Node 完整哈希一致 | Windows/macOS；保留参数、独立 env 和其他 MCP；没有健康运行时先提示修复浏览器连接。代理 `env_vars` 勾选默认关闭 |
| 浏览器 / 电脑操控子进程未继承代理 | 检查真实根进程与工具子进程的代理变量名；仅有根进程已配置、子进程丢失的证据时修复生成器 | Windows/macOS；结果绑定 home、ASAR 哈希并缓存 10 分钟，退出客户端后仍可使用；不显示/保存代理值，不替用户填写代理地址 |
| Image-guided Assets 插件不可见 | 仅匹配原包已包含的 bundled descriptor，恢复可发现性 | Windows/macOS；保留 `installWhenMissing=false`，不自动安装或启用该可选插件 |
| 界面与插件入口深度修复 | 独立匹配新版 Fast UI / 请求、Ultra 查询 / 主机能力，以及旧 Power、Ultra 偏好、语言、Goal、插件入口、浏览器主进程与可信路径；生成候选 ASAR、语法及归档校验后交原生安装流程 | Windows/macOS；新版模型例外仍限本地主机，自定义 Provider 才增加 Ultra。通常先尝试增强启动。未知或已开放项保留原样；包修复不会作为增强启动的自动前置步骤 |

缓存/插件修复使用当前安装包的市场，而不是从技能默认模型或远端未知来源下载功能。保留显式禁用的插件；检查与结果不能证明某个可选插件已通过真实工具调用。需要账号或后台服务的插件仍受其真实依赖限制。

## 2026-11-11 新版检查修复审计

本次只读核对 Windows x64 `OpenAI.Codex 26.1103.7773.0` 的安装资源和 Hub 当前处理规则，未读取用户登录、配置或私人日志，也未执行真实客户端或修复。新版已原生开放、命中旧规则和未知结构分别判断；下表的“适用”表示静态契约满足，不能替代真实修复/重启验收。

| 检查项目 | 本版资源证据与处理结论 |
| --- | --- |
| 启动 / CODEX_CLI_PATH | 当前 `app/resources/codex.exe` 存在，原内置程序定位仍适用。状态库冲突必须检查用户明确选择后的物理副本；本次未读库，不扩大迁移修复范围。 |
| 模型与速度 | 前端迁至 `assets/async/<数字>.<hash>.js`，7 份认证型 Fast UI、7 份请求 helper、11 份 `includeUltraReasoningEffort` 查询及 1 份 Host bootstrap 均仍受旧拦截范围遗漏。**本轮修正**，保持原 requirements、能力及非本地主机路径；普通模型分类仍只改基础配置。 |
| 插件市场 / 缓存 | `openai-bundled/.agents/plugins/marketplace.json` 保持同名和 `plugins` 数组，当前浏览器 / Chrome / CUA descriptor 版本为 `26.1103.60933`。原同源镜像与已有缓存复制规则适用；正式注册的真实 CLI 输出仍待用户点击修复后核对，不以 descriptor 存在宣称插件已安装。 |
| Chrome 本机连接 / 运行时 | 当前包仍包含 `cua_node/bin/node.exe`、`node_repl.exe`、CLI、两个扩展 ID、`com.openai.codexextension` 和 x64 native host，注册表契约仍是 HKCU Chrome NativeMessagingHosts。原稳定副本、manifest 及两份 v2 记录规则适用；新版可直接使用安装包运行时，不把“尚无用户提取副本”误判为原生功能损坏。Hub 此专项需要副本时仍显式准备自己的稳定运行时，扩展及真实连接待验收。 |
| Chrome 缺 token | 两份插件 service 同源，当前插件 service 指纹 `5750cf73900e…`、运行时 service `449b7426cd56…` 均不在已审阅完整文件 profile 中。**本版跳过旧 token 补丁**；上游审阅 ref 也未提供这两份的新 profile，不放宽来源和其他认证错误。 |
| AX 资源 | browser、chrome 和运行时附带的三个 `browser-accessibility.wasm.br` 均通过 Brotli / WASM 检查，现行同版本同 service 恢复规则仍适用，本次无需修补这些健康来源。 |
| Windows CUA descriptor / surface | `unified-computer-use` 已是 `mcpServers: "./.mcp.json"` 的 descriptor-only 布局，无旧 `scripts/launch.mjs`；主程序保留真实插件 installed / enabled / AVAILABLE、Node 路径、`mcpToolExposure` 及 Windows / macOS 条件。按原新版分支检查，旧 Darwin-only surface 补丁未匹配就保留，不在原生规则旁新增放行。 |
| CUA 跨调用 helper / Win10 | 当前 `helper_transport.js` 指纹 `502b93f0d8be…` 不属于现有 context 输入/输出 profile；**本版跳过旧 helper 补丁**。Win10 截图专项仍要求原系统版本、具体报错和完整 helper profile，本次未提供对应实机证据，不更改其二进制规则。 |
| 代理继承 | 当前 `early-bootstrap.js` 有唯一完整 `CODEX_WINDOWS_REGISTERED_CORE` 环境合并形状，但字面量改为双引号。**本轮采用上游等价引号识别修正**，仍只追加父进程实际存在的标准代理变量名，保留外层平台和 WSL 条件。正式专项仍要求原根/子进程代理证据；本次未采集实际进程环境，静态适配不代替该前提。 |
| 更新后 MCP 路径 | 本次确认包内 Node 可作为健康来源，原规则只更新已删除、位于 Codex 用户 CUA 目录中的绝对 Node 命令。用户已有可运行的包内路径不修改；没有匹配的 Hub 用户稳定副本时，仍先用浏览器连接修复准备副本。保留 MCP 参数、env 和可选代理设置，不靠启动客户端反复等待提取。 |
| 深度修复：Power / Ultra / 语言 / Goal / 插件 / 可信路径 | 当前 Power 已有 `composer.modelPicker.power`、`PowerSlider`、`selectedPowerSelection` 原生实现，结果明确记录并保留；新版不含旧 Ultra 账号迁移段，保留其原生流程。新版语言经 Statsig layer 提供，旧语言/Goal/插件放行正则未命中时不改；原生插件可用性和模型组合检查保持。可信路径仍是 `[u]:p([t,m],g)`，key 改为双引号，**本轮补引号识别**后按原逻辑合并显式进程可信根；新版 Fast/Ultra 同用前述模型适配器。 |
| Image-guided Assets | 当前 bundled marketplace 不包含该可选插件及其旧 descriptor，按不适用处理，不安装替代插件或拼造入口。 |

前端适配验证了两种路径：CDP 带增强会话标记；深度修复由用户明确选择后写入持久候选。两者共用模型改写函数，后者只启用模型范围，不将整套其他页面放行规则一起带入。每处要求完整函数/数据流的局部变量一致和唯一匹配，26 个实际前端模块共 52 次语法检查通过。真实主程序的代理、可信路径候选各另 1 次通过，共 **54 次**；28 项 Node 隔离用例通过（含深度计划只读、当前数字块、幂等和原生 Power 保留）。Rust 增强 28 项及修复中心 12 项通过，另 4 项增强用例保留忽略状态。当前输出只在 `target/codex-model-experience-20261111/`，安装资源未改；Windows 原生交互/部署、真实 Fast 参数及 macOS 仍待用户验收。

技能审阅从本机 HEAD `ffe47a4f6416b7eb54e6afad6302c83c88809644` 对照已 fetch 的上游 `762c170bf5bf9dce1c15363472cf80cc5e4c3ae5`，定向读取 26.1103 profile、Fast / native Ultra 测试、限制记录及对应代理 / token patcher。技能两个既有本地修改未覆盖，未运行安装或全量修补入口。网络 lazy-fetch 部分资源失败后，通过同一精确 ref 的公开原始资源完成只读审阅；旧内嵌工具仍保留原来源，新增增量在 [provenance](../../assets/codex-repair/provenance.json) 单独记录。

## 增强启动的衔接

保留原 Fast UI / request、Ultra 模型查询、配置页面区块适配。新纯页面适配器增加 Power、Ultra 本地偏好 fallback、Goal、插件安装入口、浏览器/电脑操控 rollout 和语言开关；仅本 Hub 增强会话的标记开启时生效。Ultra 的正常账号 API 成功路径及返回对象保留，只有账号设置查询失败才使用原本地偏好接口；不每次强制开启 Ultra。

脚本按命名资源家族及原生数字异步资源路径拦截，要求每项结构唯一且局部变量一致；没有对应组件或结构歧义时独立跳过。原加载状态、真实 requirements、模型能力、cloud/侧边会话限制及基础启动校验继续存在。可选适配超时最多 10 秒放行，不反复整页刷新；页面源码标记仍为 v2，增强脚本当前为 v26，使已有增强连接知道需要更新。历史恢复使用独立 v1 源码标记，Fast/Ultra 未勾选时也安装内存拦截器；历史配置等待和分页重试不执行本窗口的磁盘修复，详见 [历史恢复](codex-history-recovery.md)。

增强启动不复制插件、不写注册表、不重签应用或安装 MSIX，也不修数据库。初始化原有受管恢复和 `[desktop].conversationDetailMode="STEPS_COMMANDS"` 保持原行为。隐藏模型仍以 Hub 已保存并可路由的目录为依据；Fast 实际传输继续看 `service_tier=priority` 入站/出站证据，见 [模型专题](../dynamic-codex-models.zh-CN.md)。

## 备份、恢复和配置兼容

不新增 Hub TOML 配置字段。已有 `config.toml` 用 `toml_edit` 合并并复验；原注释、非目标配置和显式禁用值保留。字段类型未知时停止，不覆盖整个表。

普通文件备份在实际 Codex home 的 `hub-desktop-repair-backups/<时间戳-随机码>/`，旧组织设置备份仍在实际数据库同目录的 `hub-startup-repair-backups/`。窗口合并读取最近 60 条记录。文件快照只在本机，可能包含用户原配置，应与用户数据同等保管，不提交到仓库或自动上传。

- 程序路径另记录 `launch-path.json`，Windows 保存当前用户环境原字节，Mac 保存当前登录会话原值；没有数据库冲突也能执行，恢复无需 Node/CLI。完整行为见 [启动修复](codex-startup-repair.md#程序路径修复与恢复)。
- 文件计划记录原内容、完整 SHA-256、权限/链接、创建和移除的目录；提交前及每次写前检查未变化，备份成功才写入，文件原子替换。恢复先验证全部文件及备份，再逆序还原；新建文件恢复时删除，并清理本次创建的空目录，避免留下假的插件安装版本。
- 插件正式注册在临时 home 进行，只采纳当前 CLI 证明已安装的本地 cache 输出；旧 `local`/过期版本一并备份，避免旧目录覆盖当前版本。原生连接注册信息和 Windows 电脑操控环境开关单独保存原值，恢复也检查后续变化。
- 安装文件计划通过 `relatedBackup` 关联配置备份。恢复先核对安装文件、配置和注册/环境的全部前提，再恢复应用及关联项；安装文件已恢复而关联恢复中断，可继续完成。已经恢复的记录显示“已恢复”并禁用重复恢复。
- 写入阶段清单持久保留；意外中断显示原因并保留路径，不承诺多个文件或 MSIX 部署具有掉电原子性，也不会因失败自动清空 Codex home。

所有相关写入使用与初始化、增强启动、偏好和会话迁移相同的跨进程独占锁。普通检查持共享锁。用户需先关闭 ChatGPT/Codex；Hub 不强杀客户端、不停止自身网关。任务期间窗口防止普通关闭，系统关机放行。

## Windows 与 macOS 安装文件处理

Windows 不原地修改 WindowsApps。候选包和更高 revision 的原内容恢复包都在部署前生成、签名、校验 payload/blockmap 和更新身份，再原位 `Add-AppxPackage`，没有先卸载的兜底。执行器核对同用户 SID、桌面会话、包身份及来源哈希，使用正常 UAC；部署前持久化恢复身份，部署后核对实际 ASAR 与 EXE 嵌入完整性。失败保留原注册或用已准备的恢复包还原；后续 Store 更新/其他修改会阻止盲目恢复。打包组件按需获取，只有本次安装的 SDK 才进入清理流程。

macOS 独立处理 `.app`：完整复制原应用，核对文件和链接树，更新 `Info.plist` 中既有 ASAR 完整性记录，用本地 ad-hoc 签名并严格验证，再同父目录交换候选和原应用。目录不可写时使用 `osascript` 正常管理员授权，不静默绕过权限；交换失败尝试恢复原应用，中断位置和完整备份保留；修复或恢复交换中断时，先校验并回位暂存的原包或修补包，再继续恢复。恢复前核对完整应用树，后续用户或系统更新不覆盖。辅助功能、屏幕录制、浏览器扩展和组织策略仍需正常授权，本地签名也可能需要用户重新确认系统权限。

这些安装文件流程已实现，Windows 部署/UAC与 macOS 签名/授权没有由开发方在真实客户端执行；其效果分别待用户实机验收。源码不按版本字符串猜补丁，完整哈希仍是原生二进制/旧 service 的必要保护。

## 维护定位和来源

| 位置 | 职责 |
| --- | --- |
| [修复窗口](../../src/gui/startup_repair.rs)、[按钮文字](../../src/gui/text.rs) | 分类、滚动、后台进度、共用结果、历史恢复、macOS 权限入口 |
| [协调入口](../../src/codex_repair_center.rs)、[安装文件](../../src/codex_repair_center/package.rs)、[代理证据](../../src/codex_repair_center/proxy.rs) | 范围、配置合并、锁、定位、原生注册/包/签名、联合恢复 |
| [程序路径修复](../../src/codex_repair_center/launch_path.rs) | Windows 当前用户/Mac 登录会话的 CODEX_CLI_PATH、阶段备份及后改保护恢复，不依赖数据库或 Node |
| [资源清单](../../src/codex_repair_center/assets.rs)、[修复规划](../../assets/codex-repair/hub-repair.cjs)、[隔离注册](../../assets/codex-repair/plugin-install.cjs) | 内嵌工具、只读规划、原子备份和恢复、插件正式安装输出校验 |
| [页面适配](../../assets/codex-repair/renderer-adapter.js)、[Fetch 拦截](../../src/codex_app_enhanced/model_controls.rs) | 按组件独立适配，不改安装文件 |
| [原生状态库引擎](../../src/codex_startup_repair.rs)、[合成测试](../../scripts/test-codex-repair-center.cjs) | 已知迁移冲突算法及隔离文件/协议/页面验证 |

参考用户指定的本机 `codex-windows-fast-patch`，初始内嵌工具来源固定到技能上游 commit `4c318c42fb1d9faa09418f96a7eb53bf852f9fb4`，本轮 `762c170b` 审阅增量单独登记；逐文件原始来源及提取补丁见 [provenance.json](../../assets/codex-repair/provenance.json)。只调用审阅过的定向函数/补丁，不运行技能全量安装入口；本机技能已有修改未覆盖。Windows 技能不在 Mac 上运行，Mac 使用上文独立实现。

插件市场与安装规则另核对了 [OpenAI 官方插件文档](https://developers.openai.com/plugins/build/plugins) 和官方 Codex 的本地安装实现；正式命令的实际版本/输出仍在用户点击修复时由当前内置 CLI 校验，未知结果停止。

## 验证边界

v6 已通过 20 项 JavaScript 隔离测试，覆盖计划只读、写前变化、恢复整体预检、后改保护、损坏备份、创建目录清理、旧插件版本恢复、正式注册隔离与未知输出、AX、ASAR 元数据/完整性、Power、Ultra、Goal、插件/语言适配及安装文件/配置关联。另通过 29 项 Rust 隔离测试（修复中心 7、模型控制 6、原生启动修复 16）及 Windows locked GUI check/build，含 Mac 应用中断恢复的纯文件预检；v6 MSI 原样保留。v7 路径修复的聚焦测试、最终构建与新包身份见 [本轮交付](../releases/v0.4.30-7.md)，不复用旧包作为新分支已验收证据。

只读解析本机客户端资源及临时 home 的检查计划，确认当前插件/运行时和部分主进程结构可被识别，不等于真实修复成功。开发方未运行 Hub/Codex、实际内置 CLI/app-server、安装器、UAC/MSIX 部署、真实浏览器/电脑操控或模型调用。macOS 原生构建与两平台实机检查、修复、重启和恢复效果待用户验收；Windows 专项不能据此宣称 Mac 已修好。
