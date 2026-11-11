# 多客户端与外部模型接入：适配规格

## VS Code 与 WebStorm 界面入口隐藏（本轮开发）

日期：2026-11-11。用户要求在 Hub 界面隐藏 VS Code 和 WebStorm。当前只展示 ChatGPT / Codex、WorkBuddy、天工 Work、DeepSeek Harness、ZCode；侧栏、旧最近记录、概览活动图和状态卡、配置位置选择、添加模型来源与天工模型分享均按同一可见列表处理。旧导航及助手要求跳转隐藏端时返回概览；历史请求日志和统计来源名称保留。

实现入口为 `src/desktop/clients.rs::ClientId::{VISIBLE,is_visible}`、`RecentClients::normalize_visible`，以及 `src/gui/{shell,preferences,overview,model_fields,client_overview,tiangong_models}.rs` 和 `src/gui.rs` 的客户端构建/绑定。`ALL`、枚举序列化身份、配置发现、模型 writer、启动器及 API 保持，不删除已有用户资料；天才搭档的后端工具仍支持原有端，真实验收使用的临时 VS Code 目标也不因界面隐藏而失效。旧最近记录采用原 JSON 格式，下一次正常导航按原保存流程写入已替换的槽位。

回滚恢复可见列表和 GUI 投影即可重新展示两端，原配置和模型无需重建。管理会话已完成GUI测试目标编译；新增三个定向用例及受影响旧用例通过，纳入本批57项定向回归，唯一包见 [V23交付](../releases/v1.0.23.md)。未启动用户原有客户端或安装器，Windows实际界面及macOS待用户验收；既有两端后台测试结果保持各自版本归属。前文入口沿革见 [桌面视觉](../development/desktop-visual-system.md#2026-11-11-客户端入口精简本轮开发)，下文七端适配资料继续作为实现和兼容参考。

## Harness 模型不可见与“待核对”修正（未发布）

日期：2026-11-11；关联 TC-018。开发基线为 `main` / `16f1118` 加 V1.0.19 后累计工作区，功能已提交于 `6b86df5`。状态：已实现，Windows GUI 测试目标离线编译及本专题 7 项合成回归通过，纳入 [V1.0.20 统一测试交付](../releases/v1.0.20.md)；本次 Harness 模型修复的用户反馈见下，其他范围待实机验收，未正式发布。

用户于 2026-11-11 反馈 V1.0.20 的 DeepSeek 问题已解决，仅登记该次 Harness 模型实测通过。本次修复在共享 `models.rs` 中的新分支均限定 `ClientId::DeepSeekHarness`，VS Code 的 `chatLanguageModels.json`、ZCode 新旧配置、WebStorm Bridge/ACP 及 ChatGPT/WorkBuddy/天工专用 writer 保持各自格式；共享保存/恢复事务未改写。其他端的配置与事务离线结果沿原交付归属复用，不能据此宣称七端全部实机通过。跨端的页内操作反馈影响见 [视觉规范](../development/desktop-visual-system.md)。

用户反馈保存 `gpt-6.1-sol` 后长期显示“待核对”，Harness 内也没有该模型。只读核对本机 Harness `0.2.2-rc.1` 的安装资源，确认以下互相独立的原因，未读取用户凭据、历史或真实配置：

- 桌面版固定启动 `profiles/desktop`。旧 writer 在只有 web Profile 时只写 web，未准备 desktop；现在保存受管模型时总是同时准备 desktop 的 `cordis.patch.yml`，并继续合并其他已有 Profile。首次启动的原生 Profile 初始化会保留此文件；原官方模型和默认选择保持。
- 新版 `llm-pi-ai` 的 `models[].reasoningEfforts` 需要字典或 `false`，Hub 共用能力字段是数组。现在将例如 `low,high,xhigh` 写为 `{low: low, high: high, xhigh: xhigh}`；`none` 对应 `off: none`，`off` 对应不发送字段的空值。空列表不写入，只有关闭档位时写 `false`，不支持的档位在生成计划时明确报错。图片能力对应客户端的 `input` 列表，仍保留原模型 ID、接口协议、地址与凭据引用。
- 新版 `dsh-settings` 启动时将旧 `settings.yaml` 导入活动 Profile 后重命名为 `.imported`。旧 writer 反复创建该文件，并把它作为必须存在的受管配置，正常启动后反而被误判为后改。现在不再新建它；已有旧文档仍仅同步 Hub 自有 provider，保留其他命名空间和外部模型。旧保存记录中的文档消失时，只在**活动 desktop Profile 的受管 provider 与保存记录相同**时认可正常导入；`.imported` 不参与读取或判定。活动配置确实缺失或被修改仍需核对，不以历史归档冒充当前配置。
- Cordis 支持 `insert` 内的插件条目，以及后续按 ID 覆盖整份 `config`。模型 writer 与自有内容读取现在识别插入条目，并同步每个有关覆盖层，避免后面的 `llm-pi-ai` 配置遮住 Hub 模型；其他 provider 和插件配置保留。自有快照按最后生效的配置比较，兼容原快照漏记空白层，以及原生导入器为插入插件追加同值覆盖节点；活动配置被清空仍会报告变化。

维护入口为 `src/desktop/harness_models.rs::{plan,owned_providers,settings_imported}` 与 `src/desktop/models.rs::{verify_later_changes,owned_section}`。没有配置结构迁移，也不在后台自动改写用户文件；旧版已只写 web、或已保存不合法思考字段的模型，需要通过正常保存/明确替换更新活动配置。旧记录本身可读取，正常导入不会再单因 `settings.yaml` 消失而报“待核对”。本次仅针对 Harness 桌面接入，不将独立编辑器的旧 ACP 组合视为新增支持。

增加五项合成回归：只有 web 的目录也准备可见 desktop 模型及空思考列表；原生思考字典与不支持档位；插入插件及后续覆盖层；按实际 `SettingsForms.update` / `ConfigEditor.edit` 形状模拟旧文档导入、追加覆盖节点后的状态与归档边界；旧快照空白层与最后生效层兼容。连同两项原有 Harness 合并/旧代际检查，本次 7 项均通过；通用模型 3 项、保存/恢复事务 2 项与付款规则 2 项同时通过，合计 14 个不同用例。Windows GUI 测试目标离线编译通过，记录在 `target/harness-ui-fix-20261111/`，没有运行全部历史测试。未启动实际 Hub、Harness、安装器或真实模型调用，macOS 也未实机验证。回退恢复上述两处代码即可；已保存 Profile 与原有其他配置不自动删除或从 `.imported` 回写。

## DeepSeek Harness 状态与四端启动修正

日期：2026-11-11。适用 V1.0.19；Windows release 与唯一新 MSI 已生成，版本、升级和六项载荷核验通过，交付身份见 [V19 记录](../releases/v1.0.19.md)，真实客户端待用户验收。

用户实测桌面版已打开而 Hub 显示“未打开”。只读核对进程发现桌面版主程序名为 `DeepSeek Harness.exe`，现有检测只列出连字符形式和 `dsh`，导致漏报。`src/desktop/runtime.rs::client_opened` 补齐带空格的 `deepseek harness.exe` / `deepseek harness`，按完整进程名忽略 ASCII 大小写匹配，保留连字符及 `dsh` 名称；辅助进程或名称子串不算主程序。进程枚举失败仍显示未知，枚举成功但未命中才显示未打开，沿用两秒状态缓存。

同时确认 Windows 启动器只找 `dsh.exe` 等旧入口，没有读取桌面安装器登记的自定义目录。已按 Magpie 的桌面安装发现规则补齐，并复核 VS Code、ZCode、WebStorm 的安装、启动及模型配置入口。**模型成功导入后，点击“启动客户端”直接打开本机程序，不增加账号登录、CLI 安装或模型测试步骤。** Harness 的自定义 API 模型由原 writer 写入 `llm-pi-ai` Profile 和 `.credentials.yaml` 凭据引用；在客户端选择已导入模型即可，不强改用户默认模型。Hub 本机网关在使用期间保持运行。

另只读核对本机 Harness `0.2.2-rc.1` 安装包内代码：`lib/main.js::needsWelcome` 仅在既未登录、又没有已配置 API 凭据时返回真；其状态读取会遍历可配置模型 provider 的 `apiKeyEnv` 引用，不只看官方 DeepSeek Key。`dsh-credentials-local` 明确读取 `.credentials.yaml` 的 `version: 1` / `refs` 字符串，和现有 writer 一致。因此这里无需新增登录绕行或覆盖官方 Key；此结论是安装资源静态核对，未启动客户端或读取个人凭据。

### 四端核对与共用实现

| 客户端 | 本轮结果 | 配置与使用边界 |
| --- | --- | --- |
| DeepSeek Harness | 补齐带空格的主进程名；Windows 读取固定安装 GUID、App Paths 和 Uninstall 登记，支持自定义盘符、带引号路径和 `DisplayIcon` 的 `,0` 后缀；保留旧 `dsh` 入口 | 桌面版不依赖额外 CLI；现代 Profile 与凭据引用沿原模型服务，已有默认模型保留 |
| VS Code Chat | 补齐 PATH 的 `bin/code.cmd` / `code`；转换为原生 `Code.exe` 启动，复用已有产品身份、版本目录和 Scoop shim 解析；保存的位置优先，PATH 与配置发现保持同序，再回退安装登记/常见目录 | 保留便携目录和已登记 Profile/继承关系；模型仍写 `chatLanguageModels.json` 的受管 `customendpoint` 组，识别到其他发行版时跳过 |
| ZCode | 补齐已登记的自定义安装目录，复核主进程完整名称 | 保留 Windows `HOME`、`ZCODE_HOME` 和 `dataBaseDir` 的原有优先级；模型仍写新旧两种配置，端内手动规则优先 |
| WebStorm | 补齐已登记安装目录、用户 Programs 与 JetBrains 版本目录中的 `bin/webstorm64.exe`；复核两种 Windows 主进程名 | Toolbox/自定义目录有安装登记时可据此定位，未登记布局仍可显式选择程序；AI Assistant 继续使用 Bridge/PasswordSafe，ACP 继续使用固定 OpenCode 组件，两种前提分别保留 |

`src/desktop/launcher/windows.rs` 共用安装记录解析与主程序筛选：依次核对 `InstallLocation`、`DisplayIcon`、`UninstallString` 对应目录，在 HKCU/HKLM 的 64/32 位视图读取；卸载器只用于取得目录，不能成为启动候选。该发现仅在用户启动时按需执行，不放进两秒运行状态刷新。保存的程序位置保持最高优先级，已有位置失效仍明确提示重新选择。VS Code 共用 `targets/paths.rs::app_roots` / 产品判断，不另写一套便携目录逻辑。运行中与配置可用是独立证据，不能由发现安装目录推断已打开或已连通模型。

固定参考为 Magpie [`eb56de0b590bd93b56e2fa2c12d9eab713a9c073`](https://github.com/yetone/magpie/tree/eb56de0b590bd93b56e2fa2c12d9eab713a9c073)：[`desktopapp.go`](https://github.com/yetone/magpie/blob/eb56de0b590bd93b56e2fa2c12d9eab713a9c073/internal/agent/desktopapp.go)、[`desktopapp_windows.go`](https://github.com/yetone/magpie/blob/eb56de0b590bd93b56e2fa2c12d9eab713a9c073/internal/agent/desktopapp_windows.go) 的 Harness 注册规则移植为 Rust；`dsh.go`、`dsh_desktop_test.go`、`vscode.go`、`vscode_detect_test.go`、`zcode.go`、`air.go` 用于对应配置和边界审阅。既有 V1.0.12 路径移植仍保留其 `e6cdeac9` 原来源。没有运行 Magpie 或其 Go 测试，运行状态检测为 Hub 自身实现，MIT 归属见 [第三方许可](../../THIRD_PARTY_NOTICES.md)。不照搬整组联动、后台自动改写默认模型或其他客户端范围。

### 验证、兼容与交付状态

基线为唯一 `main` / `16f1118` 加 V1.0.18 后累计工作区，本次源码未单独提交。Windows GUI 测试目标离线编译通过，32 个不同定向用例通过：运行状态 4、安装/启动发现 4、路径/Profile 19、Harness 模型 2、通用模型 3。运行状态前三项沿用先前已通过结果，最终 PATH 顺序与测试临时目录规范化调整后仅补编译和 4 项安装发现检查，不重复累计；记录在 `target/client-detection-magpie-20261111/`，编译分别见 `launcher-compile.jsonl`、`validated-compile.jsonl`，先前状态编译在 `target/harness-runtime-20261111/compile.jsonl`。文件测试只使用合成目录和不可执行的假程序；没有运行实际 Hub、客户端、安装器、登录或真实模型调用。四端实机与 macOS 仍待用户验收。

没有配置格式迁移，本次开发没有修改用户客户端配置。回滚恢复 `runtime.rs`、`launcher.rs` 及新 Windows 模块、`targets.rs` / `targets/paths.rs` 的共用入口即可；原保存程序位置和模型文件保持。**已交付 V1.0.18 MSI 不包含本次修正**，保留原名原身份；用户明确要求提供可安装测试包，本次递增至 V1.0.19，构建与包核验状态独立见 [交付记录](../releases/v1.0.19.md)。

## V1.0.16 天工内置模型来源

天工 Work 模型页读取两个内置模型及源参数，原生个人登录可选；需要个人账号时读取已有会话或 Hub 登录。目标端仍沿各自 writer / 专用 API 使用，认证不导出。本次例外是天工内置生成和上下文参数由源决定，普通来源跨端仍独立调整。已有通用端能力/WorkBuddy 行通过原 writer 更新，后改则停止该目标；Codex 目录映射源窗口及 70% 阈值。删除 Codex 可见条目后再分享恢复原别名。随机身份只在用户主动测试中使用。

ChatGPT 添加和删除统一使用左侧待保存清单与右侧渠道候选，获取不会全选或被单模型搜索隐藏；新增草稿不切另一套通用配置。其他端保留各自有效字段和原生保存接口。参数、配置兼容、unary 压缩限制及验证见 [天工内置模型](tiangong-builtin-models.md)、[动态目录](../dynamic-codex-models.zh-CN.md)、[V16 交付](../releases/v1.0.16.md)。

## 按端模型字段和固定操作区（V1.0.11）

添加模型仍在当前列表加入唯一新建草稿，保存才生效，切换保留输入，放弃不写。统一保存位于模型页右下角，移除/放弃在其左侧；连接及冲突恢复在操作区左侧。仅字段区真实放不下时滚动。WorkBuddy/天工仍用原完整编辑器和保存服务；Codex 渠道可见模型保留原勾选/保存。各端不再展示无法生效的共用字段。

| 客户端 / 路线 | 可选字段 | 实际写入合同 |
| --- | --- | --- |
| VS Code Chat | 显示名称、上下文、输出上限、图片、可选思考强度 | `chatLanguageModels.json` |
| DeepSeek Harness | 上述字段及客户端协议，按来源预选 | `llm-pi-ai` 模型/协议和凭据引用 |
| ZCode | 显示名称、上下文、输出上限、图片、可选思考强度 | provider / model rules，手动规则优先 |
| WebStorm Air / ACP | 显示名称、上下文、输出上限、图片 | `tiancai-opencode.json` 的 limit/modalities |
| WebStorm AI Assistant | 显示名称，无能力输入 | 原插件来源接续及 Hub 模型目录 |
| WorkBuddy | 原来源、模型、接口、默认强度 | 原专用保存服务 |
| 天工 Work | 原来源、模型、协议、输出、强度、温度策略/温度 | 原专用保存服务 |
| ChatGPT / Codex | 渠道勾选可见模型；新增选来源/模型 | 原可见模型与独立路由 |

新四端能力都是可选，“可选参数”默认收起，未填沿用客户端默认，不承诺上游全部支持。WebStorm ACP 上下文/输出同时填或同时留空，输出不能大于上下文；切配置位置即时调整字段。支持合同为 `desktop/clients.rs::parameter_support`，界面为 `gui/model_fields.rs`，writer 为 `desktop/{models,harness_models,webstorm_native}.rs`。Harness 协议按来源 `type` 初始化，可显式修改。来源切换由同一原生事件处理模型列表及协议更新，避免多个处理器互相遮挡；合成键盘切换验证来源、模型和协议同时匹配。

只编辑本端支持字段；隐藏旧能力、未知 JSON、未改 input 模态保留，WebStorm 原生不发送虚构能力更新。无格式迁移，来源/目标、独立复制、revision/锁/备份/原子写入/后改保护保持；回退恢复旧表单，不清条目。各端字段/载荷、WebStorm 双路线、隐藏字段保留和固定操作均有合成检查，实际结果及包身份见 [V1.0.11](../releases/v1.0.11.md)，真实 IDE/客户端和 macOS 待用户验收。

## 评审阶段研究留档

以下为此前的参考软件研究和原始规格，涉及“当前缺口/待实施”的表述只对应评审时点；现行行为以上文、当前源码与 V1 文档为准。

维护日期：2026-11-08。状态：**研究规格已获批准，V1.0.0 实施中，尚未完整交付或验收**。产品基线为 main `25d024b2baa6af25f66bd69d5b237e0caa35df80` 加既有 v13 修改。以下研究保留当时事实；当前七端范围及代码进度以 [V1 实施记录](../development/v1-implementation.md) 为准。Cursor 暂缓，新增 VS Code Chat、DeepSeek Harness、ZCode、WebStorm 双模式。

用户确认：模型、Skill、MCP 在所选客户端中直接使用，普通页面用“使用中 / 未使用”；跨端复用后各端独立调整，更新时选择同步。用户要求学习 Magpie 的客户端和外部接口适配，待原型确认后用于正式程序。本文把可核对的参考事实、Hub 当前事实与建议契约分开，不把参考软件的支持范围登记成 Hub 已支持。

关联：[整体方案](desktop-ui-redesign.md)、[管理交互](desktop-capability-management.md)、[参考研究](desktop-ui-research.md)、[原型与验证](../mockups/desktop-ui-review/README.md)。

## 1. Magpie 可以接哪些客户端〔公开资料事实〕

固定研究提交为 [`e6cdeac9c681ad0377b0f863af556d4c1d7be2a8`](https://github.com/yetone/magpie/tree/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8)。[README 的 Supported agents](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/README.md#supported-agents) 列出 74 个名称。以下按原顺序分段，便于核对；不是能力或平台分类：

| 顺序 | 声明名单 |
| --- | --- |
| 1–19 | Claude Code、Claude Desktop、Claude Science、Codex、Gemini CLI、Antigravity CLI、OpenCode、OpenChamber、MiMo Code、DevEco Code、Pi、oh-my-pi、StepCode、MothX、PenguinHarness、Aside、OmO、Prime Agent、Goose |
| 20–38 | Cursor CLI、Cursor Private Inference、Zed、VS Code Chat、VS Code Insiders、VSCodium Chat、Zoo Code、JetBrains Air、Air (JetBrains IDEs)、Copilot (JetBrains)、Copilot CLI、Crush、DeepSeek Harness、Reasonix Studio、Command Code、fx、Devin、Hermes Agent、Mister Morph |
| 39–57 | Kimi Code、jcode、Qwen Code、Muse Code、Empryo、MiniMax Code、Droid、Cline、Qoder、Qoder CN、Grok Build、Grok App、ZCode、WorkBuddy、WorkBuddy AI、CodeBuddy、Pencil、OpenPencil、OpenBot |
| 58–74 | NewMax、LiveAgent、Raycast、Kiro Crew、OpenMausBot、T3 Code、OpenHanako、AtomCode、Snow CLI、Alma、Cindy、Lody、AionUi、Ante、AstrBot、QwenPaw、OpenClaw |

固定 README 同时说明只显示本机已安装客户端。Hub 默认三个固定位置，初次为 ChatGPT(Codex)、WorkBuddy、天工 Work；访问已列端不重排，列表外端替换最久未使用的位置。按需展开其他端，资源选择网格成功使用端排前；不默认铺开 74 名。

这表示项目声明具备相应适配；不是本次已逐一测试，也不表示每端均具备外部模型、Skill、MCP、启动和运行检测的全部能力。Magpie 的模型 Agent 列表、Library 目标和请求归属也不是同一张表。其 `All()` 还可纳入插件或其他安装环境，不应把 74 解释成固定运行实例上限。[Agent 机制](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/docs/subsystems/agent-wiring.md)、[Library 目标实现](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/library/targets.go)。

### 已进一步读源码的适配差异

下表仅总结固定提交的实现，不是对这些客户端所有新旧版本的独立保证。路径中的 `~` 表示对应客户端的配置根目录；环境变量、安装实例和平台路径仍需适配器解析。

| 客户端 | 外部模型如何接入 | Skill / MCP 如何处理 | Hub 可借鉴的要点 |
| --- | --- | --- | --- |
| Codex | `config.toml` 的模型/provider 与 Responses 入口，另有模型目录和桌面适配 | 原生技能目录；MCP 转成 TOML 配置 | 延用 Hub 已有增强启动、初始化、模型目录与历史保护，不用参考实现替换现有规则 |
| Claude Code | `settings.json` 中 endpoint/auth 环境配置和模型/tier 映射，请求使用 Messages | 技能目录；用户 MCP 在其原生 JSON 中 | 主模型与 tier、环境变量优先级需一起处理；Hub 需要 Messages **入站**适配 |
| WorkBuddy / WorkBuddy AI | 各自 `models.json`，支持数组及对象变体；让模型进入客户端选择器，模型由任务中选择 | 各自 `skills/`、`mcp.json`；WorkBuddy 中仍有 MCP 信任动作 | 两个发行版独立身份；不能只写磁盘便宣称切换了正在执行的任务；文件监听存在版本差异 |
| CodeBuddy Code | 独立配置根与本机身份，模型处理复用部分 Buddy 格式逻辑 | Skill 原生目录；MCP 按已有 `.mcp.json` / `mcp.json` / `.codebuddy.json` 优先级定位 | 名称相近也不能与 WorkBuddy 互写；选择实际读取的文件，不新建一个优先级更低的无效文件 |
| OpenCode | 识别已有 jsonc/json/config.json，增加专用 provider 与模型引用，保留其他配置 | 自身技能目录与 MCP 格式 | 适合优先验证“原生 provider + Hub 模型目录”；版本不同可能还有 `providers` 原生结构 |
| Gemini CLI | `settings.json` 与 `.env`，设置模型、鉴权方式和 `GOOGLE_GEMINI_BASE_URL` | 技能目录；MCP 写 settings | 客户端发原生 Gemini 请求，不能拿 Chat URL 直接替代；需新增 Gemini 入站 |
| Qoder / Qoder CN | 各自配置根、provider/base URL 与模型偏好；源码注明自定义 provider 还受客户端登录和 BYOK 套餐资格限制 | 各自技能目录与 MCP 配置 | 国际/国内版分别发现、分别保存；不能把写入成功当成获得客户端资格 |
| Cursor CLI | `cli-config.json` 中本端 model 选择；该函数的选项只有 `auto`，没有通用外部 provider 接入 | Cursor 编辑器与 CLI 可共用部分 MCP/Skill 目录，读取范围另有区别 | 不能因为名单包含 Cursor CLI 就承诺任意第三方模型；Cursor Private Inference 是另一适配 |

对应源码：[Codex](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/codex.go)、[Claude](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/claude.go)、[WorkBuddy](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/workbuddy.go)、[CodeBuddy](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/codebuddy.go)、[OpenCode / Cursor CLI](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/agents.go)、[Gemini](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/gemini.go)、[Qoder](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/qoder.go)。Skill/MCP 目标以 `internal/library/targets.go` 为准。

## 2. 外部模型接口怎样工作〔Magpie 事实〕

它把来源与客户端分开：用户添加一个厂商预设或自定义接口，保存地址、协议、Key/账号和模型；客户端选择相应模型时，Magpie 写该端自己的配置，让其请求本机网关。模型引用通常带来源身份，例如 `provider/model`，客户端本身不必会说上游协议。[来源与账号](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/docs/subsystems/providers-accounts.md)。

```text
客户端原生模型选择
    ↓ 本端配置中的本机地址与身份
Magpie 的协议入口 → 来源/模型映射 → 对应上游地址与认证
    ↑ 按客户端协议返回流式文本、推理、工具调用和使用量
```

网关提供 Chat Completions、Responses、Anthropic Messages、Gemini 等入口；客户端协议与上游协议不同，需要请求和响应双向转换，流式及工具续轮也属于转换范围。[网关路由](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/docs/subsystems/gateway-routing.md)。例如 Claude Code 使用 Kimi，不是把 Kimi 的 Chat URL 直接填入 Claude 的 Messages 字段，而是 Claude → 本机 Messages 入口 → 转换 → 所选 Kimi 上游。

Magpie 还实现账号/Key 轮换及路由组回退，并限制部分回退发生在尚未向客户端发送回复时。本次只借鉴入口适配、身份和配置效率；不据此把自动跨厂商回退、订阅账号发现或智能调度加入 Hub 的本轮需求。

### Skill / MCP 为什么可以直接勾选

[官方 Library](https://usemagpie.ai/docs/library) 和 [Library 页面实现](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/gui/assets/plugins/library/client.js) 表明：勾选会触发真实的写配置流程；异常后重新读状态，不能仅把开关涂成成功。Library 读取各端原生条目，技能以 `SKILL.md` 及资源目录识别，MCP 按目标格式处理；已有外部内容与自己管理的内容保留归属，相同名称但内容不同不会自动当成同一资源。

参考实现记录 applied/stash、备份和外部后改。Skill 默认可用链接，Windows 有 junction 处理，也支持副本；副本编辑还可能回收进公共库再分发。**Hub 采用已确认的独立副本、选择同步，不照搬这套全端联动。** MCP 对本地命令/远程地址、环境变量、传输、认证和客户端信任有实际差异；写对配置不等于可以省略客户端自己的信任机制。

这些复杂性放在适配器内部和异常处理。日常界面只需资源、客户端和使用开关；需要重新打开或信任时才给一条对应操作，不让用户逐个执行“保存、写入、检查、验证”。

## 3. Hub 已有什么、缺什么〔当前源码事实〕

| 层次 | 当前事实 | 影响 |
| --- | --- | --- |
| Axum 挂载 | [web.rs](../../src/web.rs) 将网关挂在 `/ai-gateway` | 参考软件的根路径不能原样复制到 Hub |
| Responses 入站 | [ai_gateway.rs](../../src/ai_gateway.rs) 提供 `/v1/responses`、WebSocket 及 compact 等 | 保持 Codex 原有普通路由、初始化和工具历史身份 |
| Chat 入站 | 同文件 `/v1/chat/completions` 调用旧 WorkBuddy handler；另有 `/workbuddy/{entry_id}/…`、`/gmclaw/…` | **现有 Chat 地址不是供所有新客户端任意共享的普通入口** |
| Messages / Gemini 入站 | 当前 router 未列出这两类原生入口 | 已有 Anthropic **上游**转换，不证明已能接 Claude Code **客户端入站**；Gemini 同理 |
| 已有配置保存 | [workbuddy_config.rs](../../src/workbuddy_config.rs)、[gmclaw_config.rs](../../src/gmclaw_config.rs) 已实现 load/save/delete/restore 等保护 | 新 UI 包装原 API；不重新实现一份较弱文件写入器 |
| Skill / 通用 MCP | 统一管理领域与 API 尚未实现；当前有专用 Dumpling | 原型开关是内存演示，不能作为真实安装记录 |

普通模型渠道、WorkBuddy 专用条目、天工专用条目必须继续隔离。新端不能拿 WorkBuddy 的 URL、凭据引用或 entryId 充当自己的配置；同名模型不构成授权或绑定依据。来源与副本同步仍按 [现行隔离契约](client-channel-scope.md)，不因管理界面合并而改变。

## 4. 推荐给 Hub 的正式实现〔待原型确认〕

### 用户只表达使用意图

| 用户动作 | 程序自动完成 | 普通页面反馈 |
| --- | --- | --- |
| 在某客户端添加/复用模型 | 读取目标、校验来源/协议/参数、生成本端路由与配置，调用原受保护保存 | 该端模型“使用中”；仅在该端需要时提示重新加载/增强启动 |
| 给已有 Skill / MCP 打开某端开关 | 核对实际原生配置，处理兼容、依赖和认证，按本端格式保存 | 使用中；失败保持旧值并提供重试或解决入口 |
| 添加新 Skill / MCP | 一次填写来源与必要字段、选择客户端；内部登记定义并配置所选端 | “添加并使用”；不再先收藏、再找第二个加入按钮 |
| 关闭某端开关 | 只禁用或移除该目标的一项，保留来源、其他客户端和可恢复备份 | 未使用；外部原有内容或后改时先明确范围与冲突 |
| 来源有变化 | 比较来源与独立副本，用户选择同步目标/内容 | 未选端不变，端内参数默认保留 |

“使用中”定义为该客户端当前配置确实选择/包含该资源；不表示正在发送模型请求或 Skill 每次都会触发。读取失败显示错误并保留上次选择，不能显示成未使用。运行状态的流动仍只依据新鲜的进行中请求，不从使用开关推导。底层日志、状态证据和保护机制照常保留，但不在每次成功后摆出四个阶段让用户理解。

### 可共用的 Rust 领域契约

以下是**建议类型和职责，当前不存在这些统一接口**。名称可用于后续实现评审；不在本轮新增正式 API 或更改配置 schema。HTML 视图留在原型，正式 wxDragon 页面和本机 API 共用 Rust 领域服务。

| 契约 | 主要输入 / 输出 | 必须遵守 |
| --- | --- | --- |
| `ClientTarget` | clientId、installationId、platform、version、scope、可选 projectRoot | 用户级/项目级、多实例与国内/国际版各有稳定身份；路径规范化后定位，不按显示名查找 |
| `ClientCapabilities` | model 的入站协议/参数；Skill 格式；MCP 传输/认证/变量语法；支持的作用域；reload/launch 能力 | 每类能力单独报告支持/不支持/未知和理由，客户端被发现不等于所有能力可选 |
| `discover()` / `read()` | 本机客户端与资源引用；`inUse: true/false/null`、revision、observedAt、ownership、指纹 | 只读；失败/null 不变成 false。只扫描相关配置和资源元数据，不把内容上传、运行或自动接管 |
| `plan(SetUse)` | resourceRef、target、use、最新 revision、用户已给出的决策；返回精确修改集与阻断项 | 内部自动规划；无争议无需用户看预览；有冲突、授权或依赖才返回需要的决策 |
| `apply(plan)` | 计划身份、expectedRevision、operationId / 幂等键；返回真实配置结果与必要下一步 | 写前再读、锁、定向备份、原子替换、后改保护；不复用过期计划 |
| `set_use()` | 上述读/计划/执行编排的单一用户意图入口 | GUI 开关与添加/复用共用；繁忙时锁当前操作，成功确认后才点亮并将使用端排前，失败不改旧状态/顺序；取消/迟到结果按代次处理 |
| `restore()` | 精确目标、受管记录、备份与当前 revision | 只撤销匹配的受管修改，保留未知字段和用户后改；多端部分成功不声称全部回滚 |

`resourceRef` 不能只含名称：模型含普通来源身份、模型映射和快照/版本；Skill 含内容指纹与依赖；MCP 含定义身份与凭据引用。配置结果与请求证据分开储存，正常 ViewModel 只呈使用选择、忙状态、错误及可执行的下一步。错误细节中可查看路径、版本、受管身份和证据时间。

部分前端实际共用一个配置根，例如 CodeBuddy IDE 与其 CLI，或 Cursor 编辑器与 CLI 的部分扩展目录。适配器应按解析后的物理目标去重/加锁，显示会同时影响的端；这类原生共享配置不能承诺两端独立调整，不通过复制到客户端不读取的位置伪造独立性。Hub 当前三端独立路由与已确认副本规则保持。

已有模型端先调用 `/api/workbuddy/config`、`/api/gmclaw/config` 及原 restore/delete/runtime/start；Codex 延用 configure/models/preflight 等接口。Dumpling 始终复用 prepare/preview/apply/detect/restore 与原环境授权，不改成任意 URL 写入器。统一编排不意味着建立一个绕过原保护的万能配置表。

### 外部 API 与新客户端的入口规划

| 客户端发出的协议 | Hub 后续需要的适配 | 不可省略的回归 |
| --- | --- | --- |
| Responses | 使用现有转换能力；新端增加明确身份，不改变 Codex 普通路由 | SSE / WebSocket、工具 id/续轮、取消、目录与 max/推理字段 |
| Chat Completions | 增加新端的明确命名空间与路由策略；保留旧 WB 路径兼容 | 同名不同端/条目不借配置；stream 与非 stream、工具、错误与输入输出用量 |
| Anthropic Messages | 新增 Messages 入站解码、流式编码、tools/thinking 与上游转换 | 文本块/工具结果/停止原因、取消和错误边界；不能仅添加 URL 路由便宣称支持 |
| Gemini | 新增 generateContent / streamGenerateContent 入站、模型目录形状及返回转换 | 多部分内容、函数调用、签名/推理差异、流式和用量；不把 Chat 响应冒充 Gemini |

建议未来新端使用显式客户端命名空间，例如 `/ai-gateway/clients/{client_id}/{entry_id}/v1/…`；**这是讨论草案，尚未注册**。实际入口按协议与保存契约确定，clientId/entryId 必须验证所属关系。可根据需要绑定本机接入凭据；客户端身份不只凭 User-Agent 猜测，UA 也不充当安全边界。旧配置不批量迁移，旧路径继续按原语义服务。

上游凭据归来源/专用副本，本机接入凭据与外部 MCP 认证各有范围；不能为了复用把别端专用 Key 直接塞入新端。模型能力只按来源、端版本与转换器实际支持开放，不把“目录出现/能选 max”解释成完整能力保证。地址处理沿用 v13 原路径优先、明确 HTTP 失败后同源补 `/v1` 一次，不照搬其他软件的任意 URL 归一或重试规则。

## 5. 首批五类客户端的研究与实施建议

**用户指定的首批学习范围：Cursor、VS Code、DeepSeek Harness、ZCode、WorkBuddy。** 以下是固定 Magpie 提交的公开源码事实和 Hub 建议，未在产品新增支持；先完善已有三端，未来适配按能力分别开放，不把五个名称当作一套通用文件格式。

| 首批对象 | 参考实现确认的差异 | Hub 正式实施建议与回归重点 |
| --- | --- | --- |
| Cursor | 编辑器、CLI、Private Inference 分别判断。CLI 的 `cli-config.json` 仅有 auto 选项；Library 使用 `~/.cursor/mcp.json`、`~/.cursor/skills`，部分范围与编辑器共享。Private Inference 以 product.json 识别特定构建，使用 `CURSOR_LOCAL_AGENT_BASE_URL/API_KEY` 与 `/models`；用户模型/打开配置的优先级可能盖过环境值，需完整退出重开 | 先核具体构建能否接外部模型；不把 Private Inference 写环境变量的方法当作普通 Cursor 通用接入，不直接改系统用户环境。Skill/MCP 按实际共享路径提示影响范围，保留端内原有内容 |
| VS Code | 适配自身 Chat / Custom Endpoint，在用户及 profile 的 JSONC `chatLanguageModels.json` 增加专属组、模型 Chat URL 和 Authorization；`chat.defaultModel` 面向新会话。模型适配不等于 Codex 插件执行通道，也不等于 Library 全能力；固定 `library/targets.go` 没有 vscode 目标 | 识别 stable/profile/portable、实际 provider 能力与原认证优先级，保留非受管组。Skill/MCP 另按官方版本及 harness 适配；模型入口需补新端命名空间，不能借 WorkBuddy 地址 |
| DeepSeek Harness | 尊重 `DSH_HOME`；现代自定义 provider 使用 `llm-pi-ai`，旧配置、profiles 的 patch YAML、0.2 settings.yaml 分代处理。桌面和网页 profile 不同，桌面凭据读 `.credentials.yaml`，只写 `.env` 不充分。Skill 是 home 下 `skills`，MCP 是 `@deepseek-ai/dsh-mcp-client` patch 条目 | 不接管官方 DeepSeek provider；按版本/profile 合并而非整体覆盖。配置凭据、选模型作用域及新会话生效分别核对；保留共享 profile 与未知字段，MCP 不当成普通 mcpServers JSON |
| ZCode | 旧 `v2/config.json` 与 3.14+ `v2/provider_config.json` 模型规则兼容，保留手动协议、参数和规则；模型数据根可能迁移。Library 的 MCP 在 `~/.zcode/cli/config.json` 的 `mcp.servers`，Skill 在 `~/.zcode/skills` | 模型数据根与 Library 根分别定位，不粗略合并。参考默认可用 Messages，但 Hub 尚无 Messages 入站；按实际协议补适配后才开放模型，不能仅写地址。保留用户规则与启动时读取差异 |
| WorkBuddy | 原生 `models.json` 数组/对象变体、配置根覆盖及发行版分别处理；Skill 在 `skills/`、MCP 在 `mcp.json` 的 `mcpServers`；`mcp-approvals.json` 的客户端信任不能由文件写入冒充 | 沿用 Hub 已有专用路由和 v13 max 保存/传递，不重写已有保护。补 Skill/MCP 原生读取、受保护使用/停用与信任入口；文件监听/原子替换差异按版本报告，必要才提示重开。WorkBuddy AI 是独立发行版，不默认并入本轮 |

固定来源：[Cursor Private Inference](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/cursorlocal.go)、[VS Code](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/vscode.go)、[DeepSeek Harness](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/dsh.go)、[ZCode](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/zcode.go)、[WorkBuddy](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/agent/workbuddy.go)、[Skill/MCP 目标](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/internal/library/targets.go)。四类新增阅读及对应 `_test.go` 摘录在 `target/ui-client-grid-20261107/magpie/`，WorkBuddy/Library 延用前轮快照。这些测试是参考证据，未在本轮执行 Go 测试。

VS Code 的原生能力补充：用户/profile 的 `mcp.json` 与工作区 `.vscode/mcp.json` 采用顶层 `servers`；CLI 可携带的 Copilot JSON 有不同格式。Skill 目录随 harness 变化，例如 `.github/skills`、`~/.copilot/skills`；多个端可能共享，正式适配应发现实际作用范围，不能承诺只影响 VS Code。依据：[官方 MCP 格式](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)、[官方 Skill](https://code.visualstudio.com/docs/agent-customization/agent-skills)。以上是对现有目录/格式的研究，不授权本轮扫描用户磁盘。

实施建议：先保持三端模型与 Dumpling 的既有行为，增加统一发现/读取/使用服务，再按上述五类逐项验证模型、Skill、MCP；允许已确认的某类能力先开放，不强等所有协议齐备。Claude/Gemini 等其余端保留后续研究，不取代用户指定的首批。

导航不随每次访问重排，默认三端位置保持；更多按钮放第三端下方，展开后标题右侧收起。资源选择采用小图标网格，成功才点亮且使用端在前，失败保留状态/顺序；新增表单先选目标，最终成功才记录实际使用。扩展到七八端时按需展开搜索，运行图仅展示活动端，不增加逐端状态切换页。

## 6. 可复用的实施与回归清单

原型确认后，优先复用的是**任务语义、目标身份、能力表、错误/决策合同与合成场景**；HTML/JS 不作为 wxDragon 的正式配置写入层，也不运行外部 Go 软件。按下面的稳定场景组构造 Rust 临时目录/合成服务回归，完成后再登记“已实现”：

| 场景组 | 要验证的结果 |
| --- | --- |
| A · 发现与读取 | 未安装、残留目录、多实例、配置根覆盖、读取失败/旧快照、未识别版本、原生已启用/禁用；失败不改成未使用 |
| B · 模型复用 | 普通/专用隔离、同名模型、缺来源、账号限制、端参数独立、重复加入幂等；来源更新只改选定副本 |
| C · 文件保存 | JSON/JSONC/TOML/SQLite 各保留未知字段；revision、锁占用、原子替换后观察、权限失败、写到一半与结果未知；后改不覆盖 |
| D · Skill | SKILL.md 元数据、完整相对资源、重名异内容、依赖衔接、链接/循环/越界、独立副本、外部内容的明确停用/恢复 |
| E · MCP | 本地命令/HTTP/SSE 兼容、参数/env/变量语法、独立认证、信任入口；添加不执行命令，失败保留原选择 |
| F · 使用操作 | 一次使用/停用、必要决策后回原任务、连续点击/取消/迟到结果、多端部分失败只重试失败端、结果未知先读回 |
| G · 网关 | 按新增入站协议验证文本/图片/工具/推理/流式/断开；上游与客户端协议、路由和用量分别记录，不改变原日志 |
| H · 原生与平台 | Windows 优先：路径/DPI/窗口/主题/焦点/托盘；macOS 目录、权限、Keychain 和打包分别验证；本轮不新增 Linux 支持 |

回滚优先逐目标恢复受管字段与文件，不回写整份用户配置；管理记录与备份必须可定位写前/写后指纹。新管理记录采用独立可演进格式，旧客户端配置按需接入，不要求用户手工重建。新 GUI 与旧 GUI 不并行写同一目标。实现阶段每次同步对应专题、总表与 CHANGELOG，重新核对版本；按目前基线下一次功能构建应为 v0.4.30-14。

## 7. 研究、复用与验证边界

本轮只读取公开文档/源码和仓库源码，保存的参考摘录在忽略目录 `target/ui-direct-use-20261107/magpie/` 及 `target/ui-client-grid-20261107/magpie/`；未安装/运行 Magpie，未读取用户客户端配置、技能目录、诊断资料或本地登录数据。74 名单可核对，其余未深读项不补造能力结论。官网随时间变化，以本文固定提交为可复核基线。

Magpie 此提交采用 [MIT License](https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/LICENSE)。本轮未将其源代码并入 Hub；以后若引用其代码或实质片段，在相应文件与第三方声明中保留该许可证及版权信息，另行核对引入依赖。当前建议用 Hub 的 Rust 领域层实现已确认行为，复用现有保护与网关，而不是再嵌入一个 Magpie 进程。

本轮可确认的是研究规格与合成原型行为；产品实现、构建、真实配置、实际客户端/模型/MCP、Windows 原生与 macOS 用户验收、推送与发布均未执行。原 v13 开发和请求日志不变。撤回仅处理评审新增文件和精确新增引用，不能整文件恢复已有脏文档或清理工作区。
