# Codex 桌面修复能力接入 Hub 评估

评估日期：2026-10-20。Hub 源码基线：唯一 `main` / `0dd9d5b2ca96cc589ef2703d8f7c53a52c484527`，当前产品版本 `0.4.30-5`，关联 TC-006 / TC-007。

文档定位：这是 `0dd9d5b2…` / v5 的开发前评估，表中的“当前覆盖”“可加入”“尚未实现”均指评估当时，保留来源和取舍。用户随后已授权开发，v0.4.30-6 的现行范围、分类窗口、修复与恢复、验证边界见 [检查与修复中心](codex-repair-center.md) 和 [本轮交付](../releases/v0.4.30-6.md)。本次实现没有运行真实客户端或修改用户运行配置。

## 结论与建议入口

建议保留“增强启动”处理会话内的显示和兼容问题，另提供“修复浏览器与电脑操控”处理缓存、路径、运行时及 Chrome 本机连接。确实需要更新 ChatGPT/Codex 安装包的少数情况，再转入独立修复流程。既有“修复无法加载组织设置”继续处理已知状态库冲突，不把全部问题塞进一个数据库修复按钮。

Hub 已覆盖 Fast 界面与请求条件、配置中 MAX/ULTRA、模型列表过滤、语言初始化及部分插件目录展示。主要新增价值是 Power 滑块、特定布局的 Ultra 偏好保存、Goal/安装入口，以及浏览器和电脑操控的底层修复。单纯新增开关不能修复缺失的运行时、损坏的资源或过期的本机连接。

增强启动可增加有界只读检查，但不直接调用技能的全量修补入口。文件修复先由按钮检查并展示具体问题；用户只需正常退出 ChatGPT/Codex，Hub 保持打开并执行、显示结果及提供恢复。可选项不兼容只记诊断并保留其他成功项，不追加启动错误或反复刷新窗口；真实基础配置失败仍如实提示。

## 评估时的源码证据

| 位置 | 当前行为与边界 |
| --- | --- |
| [配置与恢复](../../src/codex_app_config.rs) 的 `enable_model_controls`、`write_config_toml` | 初始化/更新合并推理选项并开启 `fast_mode`，保留原选择；移除阻挡插件的 false 配置，启用既有 browser/chrome/computer-use 配置；这些不证明插件文件、主进程开关和本机连接可用 |
| [模型脚本适配](../../src/codex_app_enhanced/model_controls.rs) 的 `fetch_options`、`adapt_source` | 只拦截已知模型/配置页面脚本；独立适配 Fast UI、Fast request、Ultra 模型查询、设置区块。没有 Power、Ultra 账户偏好 fallback、Goal 或 Electron 主进程修补 |
| [增强启动](../../src/codex_app_enhanced.rs) 的 `enhanced_statsig_script` | 配置可见模型与 `enable_i18n`，适配本地 curated 市场展示并刷新查询缓存。这里的 browser CDP 是调试连接，不代表已经修好产品的内置浏览器、Chrome 或 Computer Use |
| [本机配置兼容](../../src/remote_control_backend/compatibility.rs) | 提供增强启动的模型和语言基础配置；不是浏览器扩展或原生电脑操控运行时的修复服务 |
| [插件目录服务](../../src/web/plugins.rs) 与 [插件专题](../codex-app-offline-plugins-plan.md#上游-2026-10-01-chatgpt-改版后的插件页核查) | 有本地 curated 展示及远端后台依赖筛选，不能将目录可见当作每个插件可安装、可调用；不能把需要远端账号服务的插件一律改为本地可用 |
| [现有组织设置修复](codex-startup-repair.md) 与 [操作协调](../../src/codex_access.rs) | 已有原生检查、备份、副本验证、恢复和同用户文件锁，可复用其交互及写入协调思路；其状态库修复算法不能用来处理插件文件 |

## 分项可行性

下表“可加入”表示建议开发，不能解释成当前 Hub 已修复。页面脚本适配以实际结构和能力识别；原生二进制必须按完整文件哈希识别，不以版本字符串猜测兼容性。

| 问题 | Hub 当前覆盖 | 建议放在哪里 | 必须保留的条件 |
| --- | --- | --- | --- |
| Fast 标志缺失、开关打开但请求未用 Fast | 已有布尔型和访问状态型 UI/请求独立适配及日志核对 | 继续增强启动，按新结构补兼容；可补日志中的分项结论 | 保留真实 requirements、加载状态和原返回类型；`service_tier=priority` 入站/出站分别核对，不能强制所有模型或全部请求为 Fast |
| 已有模型被隐藏 | 已按 Hub 保存的可见模型提供列表和显示配置 | 继续增强启动；补目录、Provider 返回值与可见列表的一致性检查 | 技能默认模型 ID 不能照搬。只显示已配置且可路由的模型；不凭空制造模型或推理/速度能力 |
| Power 拖动条未出现 | 未针对 `harborEnabled` 适配 | 可加入增强启动的可选页面适配 | 客户端本身须有该组件，模型/强度组合满足组件要求；无组件时保留现有模型菜单 |
| Ultra 开关灰色、重启后偏好丢失 | 已处理推理列表及模型查询过滤，未处理账户偏好 fallback | 可加入增强启动的可选适配，并纳入受管备份/恢复 | 仅在匹配到对应旧偏好机制且账户设置不可用时使用本地 `show-ultra-in-model-picker-slider`；正常账户 API 保留。当前无需该机制的客户端不额外写入 |
| 语言重置 | 已有早期语言初始化、i18n 配置和冷启动处理 | 维护现有增强启动 | 保留用户实际语言选择；不能把每次启动强制中文当作修复 |
| Goal 入口消失 | 无专门适配 | 可加入增强启动的可选页面适配 | 先确认后端及本地任务支持；已开放的布局原样保留，不对 cloud、侧边会话等原限制一律放行 |
| 插件市场缺条目、安装按钮不可用 | 已有 curated 展示别名、目录桥和部分配置处理 | 页面筛选/按钮兼容可加入增强启动；缓存/市场复制问题交给修复按钮 | 分别核对“可发现、已安装、已启用、可调用”；保留现有可选插件状态和真正的远端账号依赖 |
| Image-guided Assets 被客户端隐藏 | 无专门处理 | 独立修复流程；第一轮增强启动不承诺覆盖 | 上游技能新方案修改主进程 bundled 描述符，保留 `installWhenMissing:false`。须原包已包含该插件及已知匹配结构，仅恢复可发现性，不自动安装/启用 |
| 内置浏览器/Chrome 入口缺失 | 有基础配置，未覆盖浏览器全部可用性链路 | 页面门控可作可选适配；主进程发送/接收、信任路径交独立修复 | renderer 页面注入不能改 Electron main；浏览器扩展、本机连接、运行时与真实策略结果单独检查 |
| Store 升级后 Chrome 连接、插件路径失效 | 无完整的稳定缓存/native-host 修复 | 优先新增“修复浏览器与电脑操控”按钮；增强启动只检查 | 同一安装包来源、同一当前运行时；检查 stable cache、latest 链接、native-host manifest/注册信息、两份 v2 状态及完整可信路径。不重写受信任的 `browser-client.mjs` |
| 自定义 Provider 报 `Codex auth token is unavailable` | 无对应浏览器 service overlay | 修复按钮，按已知完整源码哈希启用 | 仅精确错误、非 OpenAI Provider 且显式 `requires_openai_auth=false`、Chrome 扩展后端、受支持 request-header 能力；保留请求头及其他策略。不把通用 fetch/策略加载失败吞掉 |
| 浏览器 AX 报 `Decompression failed` | 无 AX 资源修复 | 修复按钮；缓存变化后可做轻量检查 | Brotli 解压与 WASM 验证；仅同描述符版本、相同 service 的健康同源资源，先备份再恢复；无可信源不能拼装其他版本。修复后需新工具会话 |
| Any App 灰色、只有 browser 没有 computer surface | 基础配置不等于 Windows 原生 surface 已暴露 | 修复按钮分流到插件脚本修复或独立安装包修复 | 旧版有 `scripts/launch.mjs` 才能修该文件；descriptor-only 新版需主进程适配。不能改生成的 `.mcp.json` 冒充持久修复，也不能关审批来证明成功 |
| 跨次调用报 `node_repl exec context not found` | 无专门修复 | 修复按钮；已修后启动只检查兼容状态 | 仅已知 `@oai/sky` helper-transport 完整哈希，修复请求上下文重新绑定并保留审批。新版无需该补丁时跳过，修复后重建工具会话 |
| Win10 可枚举窗口但截图失败/超时 | 无截图 helper 兼容补丁 | Windows 专项修复按钮，不默认随启动执行 | Windows 10、确切截图错误及受支持 helper 完整输入/输出哈希均满足；备份和恢复。Win11、macOS 和未知文件不套用；上游静态验证与 Win10 实机验收分别记录 |
| 浏览器/CUA 子进程未继承代理 | 现有 Hub 网关环境配置不能证明 CUA 子进程继承 | 增强启动可提供只读诊断，修复按钮按根进程或子进程分流 | 读取完整真实进程环境，只记录变量名/状态；根进程缺配置与子进程丢变量是两类问题。仅主进程生成器问题才考虑安装包修补，不能关闭网络/证书策略来兜底 |
| 更新后自定义 MCP 指向已删除 Node 运行时 | 无通用的历史命令修复 | 修复按钮，仅匹配已确认失效的条目 | 保留原 MCP、参数和独立 env；不扫描到任意 Node 就替换，不安装新 MCP。独立 stdio MCP 的代理 allowlist 也需逐条确认，CUA 修复不能代替 |

## 技能脚本的复用范围

技能的 Windows 安装、注册表、签名和 native helper 工作流不能直接用于 macOS，也不能作为 Hub 每次启动的前置步骤。建议移植经过审阅的探测和定向修复逻辑，随 Hub 交付；不要求终端用户另装这个技能、Python、Node 开发环境或自己找解压目录。

| 技能来源 | 可复用内容 | 集成时必须调整 |
| --- | --- | --- |
| `scripts/patch_codex_fast_mode_windows_msix.ps1`，`-OnlyModelExperience` | Fast UI/请求分离、模型过滤、Power、Ultra 偏好的匹配逻辑 | 页面适配移入现有 Fetch 拦截器，限定本 Hub 受管 local host；未知/歧义只跳过该项，不能把整个打包入口塞入增强启动 |
| 同文件的 Goal、locale、plugin UI、browser gates | 页面门控定位与已开放布局识别 | 扩展实际脚本拦截范围；页面、主进程和服务端条件分别确认，避免全局 gate 覆盖 |
| `scripts/install-computer-use-local.ps1` | 当前包/运行时定位、稳定缓存、Chrome native-host、插件与运行时一致性检查 | `-VerifyOnly` 会修复并可能停止占用进程、写配置/环境/注册信息；只有 `-StrictVerifyOnly` 是其只读入口。Hub 应拆出自己的只读检查和显式写入步骤，避免全量脚本的附带变更 |
| `scripts/repair-browser-accessibility-assets.cjs` | AX 压缩资源验证、同源恢复、受保护路径检查 | 独立处理 browser、chrome 与当前 standalone runtime，不能只验证一个插件；物理路径/链接检查、备份和原子写入需保留 |
| `scripts/patch-chrome-custom-provider-headers.cjs` | 精确缺 token 错误的已知 service overlay | 插件和 standalone runtime 各用自己的来源哈希。未知版本仅跳过该 overlay，不报告认证已修复；不扩大到策略加载失败 |
| `scripts/repair-cua-surface-lock.ps1`、`scripts/patch-computer-use-node-repl-context.ps1` | 特定插件脚本 surface/说明修复、跨调用上下文补丁、恢复保护 | 确认原文件完整匹配，保留审批与后续用户修改；缺相应旧布局时不生成替代文件 |
| `scripts/patch-computer-use-helper-win10.ps1` | Windows 10 helper 原/补丁哈希、已知字节区间、备份/恢复 | 只用于匹配错误和完整哈希的 Windows 10；不能把同版本号视为同二进制，不能与普通缓存刷新混用 |
| 上游 `scripts/start-model-experience-repair.ps1`、`scripts/repatch-model-experience-recoverable.ps1`、`scripts/lib/msix-safe-install.ps1` | 外部执行器、同用户会话校验、签名/包身份、原位更新与更高修订号恢复包 | 是独立高级流程的参考。需正常 UAC、安装前检查与可恢复交付；失败不先卸载现有工作程序，不改变系统策略。不用于普通增强启动或 macOS |

Image-guided Assets 的新增可发现性方案在本次 fetch 到的上游 `4c318c4` 中，本机技能原提交 `ffe47a4` 没有该实现。上游还细分了新 service-tier access 结构、可选插件筛选和浏览器策略加载错误，不能把旧补丁无条件套到新文件。

## 建议实施顺序与 Windows/macOS 边界

1. **先补统一只读诊断和浏览器/电脑操控修复按钮。** 复用 Hub 现有任务、结果展示、备份恢复和写入协调模式，按问题选择最小范围修复；检查不得安装、启用可选插件或重启客户端。增强启动仅使用短时检查及按文件身份缓存的结果，重活转入按钮。
2. **再补增强启动可选项。** 优先 Power、需要时的 Ultra 本地偏好、Goal/插件页面按钮适配；已有 Fast、模型、语言规则继续维护。每项分别记录“不适用、已兼容、已适配、未识别或失败”，不将其他项成功等同于全成功。用户正常界面仍使用简短结果，技术细节留在诊断。
3. **最后评估独立安装包修复。** 仅用于确证的 Electron 主进程 surface、可信路径、代理生成器及 Image-guided Assets 描述符问题。准备恢复包并核对源包身份后才执行，Hub 自身保持运行。不是所有浏览器错误都能由安装包补丁修好。

Windows 原生修复可参考现有 PowerShell/注册表/MSIX 方案。macOS 可以共享模型、语言及受支持的 renderer 适配框架，但插件目录、Chrome native messaging、runtime、签名和权限必须各自实现；系统辅助功能/屏幕录制权限由正常系统入口处理。Windows helper、注册表、UAC 和 MSIX 修补在 macOS 上不适用。两平台分别构建、分别由用户验收，不能将技能的 Windows 历史结果写成 Hub 的 macOS 覆盖。

建议验收按层分别记录：目录可见、插件已安装/启用、运行时可加载、本机连接可用、实际调用成功。Fast 以真实请求字段为据；原生枚举不能代替截图/输入成功，AX 验收不能由普通 DOM 请求替代。正常开发不主动发模型调用或操作用户桌面，具体产品行为由用户测试。

## 证据来源与本轮验证

- [技能上游 SKILL.md](https://github.com/chen0416ccc-cpu/codex-windows-fast-patch-skill/blob/4c318c42fb1d9faa09418f96a7eb53bf852f9fb4/SKILL.md)、[问题与修复案例](https://github.com/chen0416ccc-cpu/codex-windows-fast-patch-skill/blob/4c318c42fb1d9faa09418f96a7eb53bf852f9fb4/references/restriction-debug-cases.md)、[主修补脚本](https://github.com/chen0416ccc-cpu/codex-windows-fast-patch-skill/blob/4c318c42fb1d9faa09418f96a7eb53bf852f9fb4/scripts/patch_codex_fast_mode_windows_msix.ps1)。以上通过本地 Git 对象读取，不据此宣称上游脚本已在 Hub 内集成。
- 本机已安装技能为 `ffe47a4f6416b7eb54e6afad6302c83c88809644` 加两份本地脚本修改。本轮按技能检查更新，fetch 成功；快进更新因 `patch-chrome-custom-provider-headers.cjs` 有本地修改被 Git 拒绝，保持文件原样，没有 stash/reset 或部署补丁。只读对照上游 `4c318c4` 的新增方案；其中历史验收属于该技能/贡献者，并非 Hub 验收。
- 本轮仅更新评估、索引、变更记录和后续版本规则；只进行文档与源码定位核对。没有新增功能实现、编译、打包、推送或实机测试，不改变上一轮 Actions 输入和包身份。
