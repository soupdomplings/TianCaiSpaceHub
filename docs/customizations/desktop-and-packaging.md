# 天才空间品牌、桌面行为与交付

维护日期：2026-10-10。当前开发 `0.4.30-5` 的独立 [NVWA MCP 页签与后台桥](nvwa-mcp.md) 修正默认租户误拒、操作提示/预览、共享开关资料保留和密码/ClientSecret 可见性，关联 TC-014；最终 Windows locked GUI build 25.52 秒、38 条警告通过，readable EXE 静态核对通过、未运行，原阶段构建/测试包保留身份，macOS 原生构建和两平台实机验收待完成，不创建标签、Release 或安装包。交付状态见 [v5](../releases/v0.4.30-5.md)。保留 TC-001、TC-007、TC-011、TC-012 的命名、启动、概览与天工功能；最近已发布产物仍为 [v0.4.30-4](../releases/v0.4.30-4.md)，前版 [v0.4.30-3](../releases/v0.4.30-3.md)、[v0.4.30-2](../releases/v0.4.30-2.md) 保留原事实。网页导入见 [说明](../hub-external-import.md)，上游见 [v0.4.30 整合](../upstream-v0.4.30-integration.md)。

`v0.4.30-4` 产品源码 `7b5dd6e…`、注释标签与非草稿 Pre-release 已公开。Windows Actions 与 Mac 修复后同标签重建均成功，八项附件下载静态核验通过；Windows 未签名，Mac universal ad-hoc 且未公证。首次 DMG 失败、工作流修复 `1bd8ffd6…` 与产品源码分开记录，运行程序和升级/同步效果仍待用户验收。旧版与调试 EXE 保持原身份，发布后核验文档使用单独提交进入 `main`，不移动产品标签或改变安装包。

## 名称与兼容身份

当前源码的产品名称统一为 **TianCaiSpaceHub**，不含空格，覆盖窗口、托盘、CLI 帮助、程序资源、Windows 安装与快捷方式、macOS App 及包内程序。本轮按用户要求同时更改编译输出名；原先只更改显示名、保留 `codexhub` 可执行文件的约定已被替代。首次品牌二开见提交 `76e58f3`；已发布包的旧文件名和历史哈希保留原样，不能据当前源码更名推断已下载包也已变化。

| 内容 | 当前值或位置 | 维护规则 |
| --- | --- | --- |
| Cargo package / binary | `tiancaispacehub` / `TianCaiSpaceHub` | `Cargo.toml`、锁文件、构建命令和工作流同步；Windows 输出 `TianCaiSpaceHub.exe` |
| Windows MSI 程序 | `TianCaiSpaceHub\TianCaiSpaceHub.exe` | 安装目录及文件改名，保留产品升级身份；路径改变的文件组件使用新的稳定 GUID |
| Windows 便携程序 | `TianCaiSpaceHub.exe` | 所在父目录仍可含空格，协议注册命令继续正确引用 |
| Windows UpgradeCode | `42B4BF3C-E660-4C83-96AA-56A4426B96A2` | 保持升级连续性，不随版本生成新值 |
| macOS App / 内部程序 | `TianCaiSpaceHub.app` / `Contents/MacOS/TianCaiSpaceHub` | `Info.plist`、DMG/App ZIP 内容与实际程序一致 |
| macOS Bundle Identifier | `com.codexhub.app` | 保留既有安装身份 |
| 图标资源 | [Windows 图标](../../packaging/icons/AppIcon.ico)、[macOS 图标](../../packaging/macos/AppIcon.icns)、[品牌原图](../../assets/tci-hub-icon.png) | 变更时同步各平台所需尺寸及资源引用 |
| 厂商图标 | [来源说明](../provider-logo-assets.zh-CN.md) | 保留来源和许可证 |

配置目录、`CODEXHUB_*` 环境变量、provider/actor/account 等协议标识、日志分类及单实例/网页导入通信身份继续沿用兼容值，不做数据迁移。它们与产品名称、二进制文件名的用途不同。旧版本 `codexhub`/`CodexHub.exe` 进程仍需被已有后台管理正确识别，以便升级过渡；保留识别不表示新构建继续输出旧文件。

打包资源入口：[Windows WiX](../../packaging/windows/TianCaiSpaceHub.wxs)、[Windows RC](../../packaging/windows/TianCaiSpaceHub.rc)、[Windows manifest](../../packaging/windows/TianCaiSpaceHub.exe.manifest)、[macOS plist](../../packaging/macos/Info.plist)、[构建脚本](../../build.rs)。历史 Linux 手动工作流仅同步二进制与资源路径，不增加本轮 Linux 支持或验收。

早期上游曾由 `codex-remote` 改名为 CodexHub。该迁移已经结束，不再执行旧分支合并、远端仓库改名或目录迁移步骤；原始记录可从 Git 历史追溯。

## 当前桌面行为

- `0.4.30-5` 的“NVWA MCP”认证下拉为“账号密码”和“认证服务连接”，新环境仍默认密码。新切到认证服务连接默认勾选共享应用，主区直接显示共享选项、代表账号、`ClientID`、`ClientSecret` 和“在认证服务管理添加应用服务，获取ClientID和ClientSecret”；取消勾选才使用原浏览器页面授权。加载旧密码/应用/浏览器环境保持各自原值，下拉继续可切换，密码模式隐藏并忽略共享选项。签名算法仅应用模式在高级设置显示，浏览器限定账号仍为高级可选项。
- 页面 `?` 打开原生帮助对话框，提供完整配置步骤与共享应用身份说明；认证切换、旧检测“重新检查”等常驻文案及应用身份横幅移除，后台取消旧事务、清空材料和撤销授权照常执行。只填一个服务地址，认证地址默认同服务；高级 MCP 默认直接显示 `/mcp`，可改路径或完整 HTTP(S) URL，空值与 `/mcp` 同义，路径跟随部署前缀、完整 URL 独立覆盖。独立认证地址、租户和登录单位仍在高级设置。多环境、直接可见的密码/ClientSecret、真实身份/两 token expiry、只读工具检测和三端预览保留；后台默认 `127.0.0.1:3849`，仅改 NVWA 受管项，原生刷新/信任保留，见 [TC-014](nvwa-mcp.md)。
- NVWA 密码和 ClientSecret 按用户要求直接可见；主认证下拉切换清理旧材料，共享复选框切换保留 ClientID/ClientSecret/保存密钥选项，两者都取消旧授权并拒绝迟到结果。默认租户 `__default_tenant__` 正常接受、显示服务返回租户与当前账号；不从表单补上下文。GUI 常见登录/检测提示、身份/有效期、确认主内容及帮助有中英，适配器提示仍中文，未知后端错误保留固定安全文本。预览说明环境/目标/文件/后果；未登录接入先提示登录，移除/恢复仍可执行。工具清单与配置保存不承诺真实调用通过。
- 双因子只在服务端挑战时显示，无手填 captcha ID，Hub 尚不显示验证码图片。保存后的刷新只在完整 profile 一致时保留当前 GUI 内存秘密，不因此持久保存；环境切换或不一致时清空。登录加密、日志规则及显式记住的 DPAPI/Keychain 保护不变。

- 主窗口每次启动最大化到系统工作区；不要求用户手动拖大。
- 关闭窗口隐藏到托盘/菜单栏；需要结束程序时使用“退出”。退出流程停止 GUI 定时器并处理本次启动的后台进程。
- GUI 保持单实例。Windows 网页导入通过命名管道交给现有实例；macOS 使用系统 URL 事件，CLI/重复 App 通过同用户 Unix socket 转交，失败给出脱敏提示。macOS 重复 App 会短时维持隐藏事件循环接收初始 URL，随后退出。
- 自 `0.4.28-2` 起，帮助菜单和托盘中的“检查更新”已移除，启动也不自动检查。`0.4.30-1` 保留上游更新诊断源码及其导出覆盖；更新模块仍仅在 `cfg(test)` 下编译，生产入口继续关闭，不能据此宣称本版会主动生成更新诊断日志。既有导出过滤规则已经支持相应日志，本轮只合入新增导出夹具。
- 主题和语言沿用现有设置；主题实现见 [GUI 主题](../gui-theme.zh-CN.md)。
- v0.4.30-3：状态概览随 Codex、WorkBuddy、天工接入页签切换，公共页保留最近接入端视角，不改变 IM 实际选择。模型配置与外部执行状态分开显示，WorkBuddy 外部消息和天工 Telegram 明确不支持；连线以执行端与通道均就绪为依据。
- v0.4.30-3：天工接入页不再提供 IM 桥接配置框。已连接的飞书、微信或企业微信中首次发送 `/tg` 即由 Hub 自动准备授权、检查连接并按需启动天工；也可先手动打开天工，再直接发送 `/tg`，由 Hub 核验同用户的运行实例并在内存使用临时授权。启用后后台每 5 秒检查，天工重启后自动核验新运行授权并恢复连接，无需重复 `/tg`；普通消息和概览可推动恢复，但只有显式接入可按需启动桌面。天工复用 Codex 在各平台原有的会话交互：飞书直接在卡片选择或填写目录、选择模型后提交，微信与企业微信沿用各自原有菜单或卡片。仅最终创建动作才创建目录与真实桌面任务，各会话固定目录和模型；恢复列表读取全部桌面场景任务，沿用原会话身份和记忆，外部正文保存到官方桌面消息接口。旧授权、地址与安装路径配置兼容，运行中、审批中及执行状态未知的会话继续保护。普通消息不额外发送固定等待句；已核对 1.1.1 资源提供受控局部刷新，首次从 Hub 启动开启本机同步通道，保留草稿、模型和当前会话，原生未保存/执行/审批时延后。常规启动且无通道的旧实例仍能接入，但不能即时更新已打开窗口。详见 [天工自动连接与共用会话流程](gmclaw-im.md)。
- v0.4.30-3：天工模型概览在桌面运行、保存条目指向当前 Hub、当前 Hub 已收到对应本地模型请求时显示绿色“已连接”，不以厂商调用成功作为门槛。请求日志同时记录客户端和上游流式模式，天工非流式请求经上游流式聚合时显示 `Streaming (Upstream)`；历史未知模式不回填。详见 [模型接入](gmclaw.md) 与 [请求日志](../ai-gateway-request-log-detail-patch.zh-CN.md)。

- v0.4.30-3：天工与 WorkBuddy 接入页分别提供「启动天工 Claw」「启动 WorkBuddy」。按钮在线程内调用本地服务、暂时禁用同页操作，识别已有实例并防重复启动，不改变 IM 执行端或会话、不发任务；无需先保存模型。天工自动准备启用接入与空口令，保存及启动前后核对配置版本，复用 `/tg` 的启动/授权检查；Windows 通过 App Paths/卸载元数据定位正式程序，图标或卸载程序路径仅用于确定父目录，兼容非系统盘安装，不执行注册表命令。WorkBuddy 仅打开桌面，`/wb` 外部任务仍暂不支持。路径发现、接口及回滚见 [天工](gmclaw-im.md#直接从聊天接入) 和 [WorkBuddy](../workbuddy.md#启动-workbuddy-桌面)。

维护入口：[GUI](../../src/gui.rs)、[托盘](../../src/gui/tray.rs)、[浏览器打开](../../src/gui/browser.rs)、[天工启动 API](../../src/web/gmclaw_bridge.rs)、[WorkBuddy 启动 API](../../src/web/workbuddy_launch.rs)。OAuth 仍需打开浏览器，不能在清理更新模块时一起删除。

`0.4.30-1` 合入上游 Windows 增强启动预检修复：主检测和备用检测共同核对官方安装包身份与桌面主程序路径，兼容 `Codex.exe`、`ChatGPT.exe`，避免把 npm Codex CLI、后台 app-server 或第三方客户端误认作官方桌面进程。该变化不恢复更新入口；当前新增界面文字使用 TianCaiSpaceHub 品牌。上游曾报告增强启动及插件列表恢复，该反馈属于上游历史，本地新版行为仍待用户验收。Chrome 认证兼容仍是独立未解决事项，详见 [认证说明](../auth-notes.zh-CN.md)。

## 平台与产物

新开发以 Windows 为先，macOS 为后续优先级，Linux 不纳入。`0.4.30-2` 发布起，版本标签只自动构建 Windows 和 macOS；Linux 历史工作流保留手动入口，macOS 发布不再等待 Linux 清单。不将历史工作流存在解释为新增功能已完成多平台测试。

- [Windows 工作流](../../.github/workflows/release-windows.yml)：使用 Cargo 锁文件构建 MSI、便携 ZIP，ZIP 包含导入说明；有签名凭据时签名。
- [macOS 工作流](../../.github/workflows/release-macos.yml)：DMG、App ZIP；包含 `tiancaispacehub` 协议声明和导入说明，签名前检查 plist 和双架构程序。保留签名/公证流程和无 Developer ID 凭据时的测试包路径。ad-hoc 签名不等于 Developer ID 签名或公证。普通手动分支构建只上传 Actions artifact；标签触发或显式 `workflow_dispatch.release_tag` 对既有完整标签重建可上传对应 Release，重建使用更新后工作流、原标签产品源码，并校验两者身份。
- 标签带 `-` 的二开版本沿用 Pre-release 标记，不自动设为 Latest；macOS 发布前最多等待 30 分钟确认同版 Windows 清单。
- [Windows 本地打包](../../scripts/package-hub-import.ps1)：基于已构建的程序生成导入测试包，拒绝覆盖同名产物，并记录版本、构建 profile、基线提交、本地变更、签名状态和 SHA-256。

用户已要求所有发布安装包通过 GitHub Actions 生成；Windows 和 macOS 分别运行对应工作流，手动对分支构建时从 artifact 获取安装包。当前源码本地只做必要编译核对，例如：

```powershell
cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub
cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub
```

以下旧命令保留当时打包记录，不适用于当前已更名的 binary，也不用于本次及后续发布：

```powershell
cargo build --locked --release --features gui --bin codexhub
./scripts/package-hub-import.ps1
```

`0.4.29-2` MSI 的三段 ProductVersion 为 `0.4.29`，二开后缀由程序版本和分发文件名标识；现有 WiX 配置允许同 ProductVersion 升级。安装升级结果必须单独验收，不能只依据构建成功判断。

## 更新与回滚

升级通过取得指定版本安装包或替换便携程序完成。先正常退出旧 Hub 并备份配置，避免两个版本同时写入同一配置。恢复旧程序时同时检查配置兼容，按需恢复备份。

本轮 MSI 仍使用同一 UpgradeCode，安排在安装新文件前卸载旧布局；文件和快捷方式路径发生变化的组件使用新 GUID，网页导入注册表组件保持原身份。安装、升级、回滚和卸载必须在 Actions 生成安装包后分别验收，源码和编译核对不能替代 MSI 实机结果。便携旧文件不被源码更名自动删除；拿到新包后应退出旧进程，并从新程序位置重新注册用户级网页导入。回退同样需要使用旧包及对应路径重新注册。

网页导入 MSI 使用系统级关联，便携版使用当前用户关联；用户级关联可能优先，移动便携程序后需重新注册。只解除当前程序拥有的关联，详见 [网页导入](../hub-external-import.md)。

macOS 保持 `com.codexhub.app` 的安装身份，完整 App 声明导入协议；菜单重新注册只更新 Launch Services，不改写签名内容。关联随 App 由系统管理，移除旧副本或从保留副本重新注册，不提供 Windows 式解除注册。

由于 `.app` 文件名由 `TianCaiSpace Hub.app` 变为 `TianCaiSpaceHub.app`，Finder 不保证自动覆盖旧名称副本。后续更新时先退出旧版，保留一个所需版本的 App，再从该 App 重新注册；回退时采用相同方式，避免系统关联仍选到另一副本。当前未改动用户实际安装、配置或系统关联。

根目录 `RELEASE_NOTES.md` 和 `UPDATE_NOTES.md` 仍被发布工作流读取，不能当作旧残留删除；后者保持最多四条。新包必须记录实际构建来源和是否签名，不自动发布或覆盖已交付文件。

## 当前交付状态

### 本轮：0.4.30-5 源码实现，未发布

基线为唯一 `main` 的 `8d1ccea7…`，包含上一版产品、CI 修复与交付核验文档。原混合工作区先完整快照，逐项比对并确认主线包含后安全对齐，见 [整理记录](../development/repository-cleanup.md)。本版新增 TC-014、普通引用配置、系统保护恢复、各端定向备份及 GUI 操作，不本地生成发布安装包，不覆盖已交付程序或包。

2026-10-10 本轮修正默认租户误拒、可理解的登录/检测/预览说明、共享开关保留应用资料及密码/ClientSecret 可见性。**最终 Windows locked GUI build 通过（25.52 秒、38 条警告），readable EXE 构建及静态核对通过、未运行**，独立目录 `outputs/nvwa-v0.4.30-5-windows-readable-20261010/`。真实验收待用户；未运行测试、Hub/Codex/WorkBuddy/天工/NVWA 或真实认证/MCP/模型/业务，实际产物状态见 [v5 交付](../releases/v0.4.30-5.md)。

此前认证服务连接名称、新切换默认共享、主区字段及 `?` 帮助阶段，Windows locked GUI build 通过（13.85 秒、38 条既有警告），`outputs/nvwa-v0.4.30-5-windows-auth-service-20261010/` 测试程序静态身份核对通过、未运行；保持原阶段归属，不覆盖本轮默认租户/提示/输入修改。

上一阶段已实现单服务地址、个人认证简化、折叠高级、切认证清理、挑战式双因子、一致 profile 刷新保留 GUI 内存秘密及可编辑 MCP `/mcp`；当时 Windows locked GUI 编译及 debug EXE 构建通过（17.55 秒、38 条既有警告），产物静态身份已核对。该构建与 `outputs/nvwa-v0.4.30-5-windows-ui-20261010/` 测试 EXE 保留原阶段归属，不代表本轮名称、共享默认和帮助已编译或验收。

此前 NVWA 首版第二轮 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub` 已通过，27.61 秒、38 条警告；它和后续历史复核/测试 EXE 只属于各自当时源码，不是 2026-10-10 最终修正的编译或验收证据。macOS Keychain 源码/API 已静态核对，本版原生 Mac 编译、Windows/macOS Actions 安装包、签名及实机行为尚未核验。

当前 `0.4.30-5` 未发布，本轮不创建标签、Release 或安装包，也不创建版本/release 分支；以后发布安装包仍仅由 GitHub Actions 生成。Windows 用户自行验收，macOS 原生构建与验收待完成。普通/高级界面、模式清理、保存后继续认证、图形验证码限制/双因子、回调注册、两 token 真实 TTL、原生刷新/信任、目录加载、并发写入、移除/恢复及工具未知结果均待用户测试。下节保留历史 `v0.4.30-4` 已发布身份和结果。

### 已发布：v0.4.30-4 预发布，两平台包已核验

基线为唯一 `main` 的 `8cfebe6`，产品源码 `7b5dd6e02d135bf1109cab9c5632853728f6a487`（20 个变更文件）已推送；注释标签对象 `e020eda…` 指向该源码，非草稿 Pre-release `Latest=false`，不建版本分支。Windows [37485845155](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/37485845155) 成功；Mac 首次 [37485845411](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/37485845411) 失败于 DMG，工作流修复 `1bd8ffd6…` 后按原标签重建 [37492460126](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/37492460126) 成功，运行号 `17`、31 分 4 秒，产品源码仍 `7b5dd6e…`。八项附件已上传/下载并静态核验，Windows EXE/MSI 未签名，Mac universal ad-hoc 且未公证；安装运行与功能验收另列，完整哈希/身份见 [交付记录](../releases/v0.4.30-4.md)。

本版承接此前去除同步 `version == 1.1.1`、固定 renderer JS 文件名与 SHA-256 门槛的修改，改核对已安装天工应用身份、renderer HTML 声明的本地脚本入口和 App/ChatPanel 运行时字段能力；版本变化或重新打包但结构兼容时不再被版本白名单阻止。保留同用户/监听归属、唯一准确页面、精确任务/会话、原生执行/审批/未保存状态和自有回复行来源保护。没有稳定接口且运行时结构不兼容时仍停止页面修改，不能推定所有未来版本均已验收。

组件名称与 Closure/Block 编译布局不再单独决定兼容，完整字段/只读 setup schema 的有效候选仍须唯一；实际写入的 `scenarios`、`messages`、`showWelcome`、`hasOlderMessages` Ref 与实例缓存须可写，纯读 Ref 不额外限制，异步读取后再次核对。这些能力保护阻止未知或只读结构被部分写入，不改变消息保存与执行结果。常规路径尝试显式释放 CDP 对象组；`display` 函数整体超时取消 future 时通过断开 CDP 会话结束本次检查，不宣称显式释放已获确认。

不新增 TOML 或关联文件字段，不迁移天工数据库，不修改安装资源、运行配置、系统环境或持久启动项。回退已发布 `v0.4.30-3` 会恢复固定版本和资源门槛，已经保存的模型、任务和消息保留；退出或回退 Hub 不会取消天工任务。初次使用同步通道仍需从 Hub 启动天工，既有运行中实例不强制重启。

#### 本版 macOS 打包修复与成功重建

首次 DMG 失败时宿主仍有约 43 GiB 空闲，报错来自新镜像内卷容量不足。CI 改按 staging 中常规文件逻辑字节数生成明确 HFS+ 大小：`max(256, ceil(逻辑字节数 × 2 / MiB) + 128)` MiB，忽略 `/Applications` 符号链接，Developer ID 和 ad-hoc 路径复用 builder；创建 UDZO 后执行 `hdiutil verify`。新增可选 `workflow_dispatch.release_tag`，从更新后 `main` 工作流 checkout 既有完整标签源码，核对 Cargo/标签与实际提交；清单和上传绑定该标签，不移标签或替换 Windows 资产。修复提交 `1bd8ffd6a5bae243a3513325fcf23bb4d2c48bce` 已推送，重建 `37492460126` 使用产品源码 `7b5dd6e…`；实际逻辑文件 `88,148,046` 字节、明确容量 `297 MiB`，Actions `hdiutil verify` 的 checksum 为 `VALID`，打包/清单/上传均成功。输入留空的普通分支构建仍只上传 artifact。

回退该 CI 修改可撤销工作流修复，不能重写产品标签或已交付包；再次使用原容量估算可能重现 DMG 失败。发布包仍全部由 Actions 生成，本机不打包、挂载或运行 Mac App。该修复不修改运行配置、桌面安装资源或天工同步协议。

修复前本地 YAML 解析、10 个 run 块 `bash -n` 和 3 个内嵌 Python 语法编译通过，未本地执行工作流命令；实际 Mac 打包/镜像验证来自上述成功 Actions。下载的 App ZIP CRC/结构、plist 品牌与短版本 `0.4.30`/build `17`、完整 `0.4.30-4`、arm64/x86_64 两切片和 ad-hoc 签名结构均通过；实际 codesign 校验来自 Actions，Developer ID/公证跳过。Mac 程序 `84,799,456` 字节、SHA-256 `e90b3ade…`，DMG 本机只核 UDIF 不挂载/解包。八项附件与旧版包核验见交付记录；这些结果不替代实机同步或安装验收。

#### 发布前实现阶段核对（0.4.30-3 调试身份）

发布前实现阶段的 Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-version-independent-sync-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 36 条警告、测试代码 59 条，其中 20 条重复）；同目录/目标 GUI `cargo build --locked` 通过（36 条警告）。四份 renderer 脚本已作语法编译，不执行函数；Rust 格式和 Git 差异空白核对通过。该阶段 272 个指定源码/资源文件的内容指纹为 `ba460beb31e4b8c187e5d0d637e432fe4e2fecc67504e419ff6cdb25bd25a207`，最终编译前后保持一致。独立构建缓存准备时曾因未复制原生品牌库出现 `LNK1181`，补生成库后重建及该阶段最终构建通过，原失败日志保留，不把该缓存问题算作功能验收。

该阶段独立 Windows 调试程序为 `target/tg-version-independent-sync-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `58,543,616` 字节，SHA-256 `e7b3fba8bfae44da0d5d5ad4bce0bf81fca36f408d2d7931558e97c7d969268d`。PE 身份为 AMD64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，品牌为 `TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-3`。这是发布前功能核对的本地调试 EXE，既不是已发布 `v0.4.30-3` 的 Actions 安装包，也不是本版 `0.4.30-4` 程序；同版号不能代替来源与哈希核对。忽略目录记录为 `.build-tools/tg-version-independent-check-final.log`、`tg-version-independent-build-final.log` 与 `tg-version-independent-build-manifest.json`；此前 `c0952f61…` 调试 EXE 和已下载的 8 个前版 Release 附件均保持不变。

功能已实现，发布前实现阶段完成上述 Windows 必要编译与静态产物核对；本版候选 Windows locked GUI/测试代码编译已通过（31.28 秒，程序 36 条警告、测试代码 59 条，其中 20 条重复），日志 `.build-tools/release-v0.4.30-4-check.log`，只执行 check、不生成或覆盖 debug EXE。本版 272 个指定源码/资源文件指纹为 `46d4b650efd4f6dd158def2f1722243ab4f09315fefb62794a276e584065cec2`，源码提交前 12 份文档 607 个本地文件链接有效；Windows Actions 与 Mac 修复重建的八项包/附件核验通过，不复用旧源码或产物身份。测试夹具和 renderer 函数不执行，未启动应用或真实业务，没有读取用户私有数据库/日志/运行口令或修改运行配置、安装资源、系统环境/关联。两平台实机同步、天工升级/重新打包兼容及未知结构、执行/审批/草稿保护仍待用户验收；不本地生成安装包，不覆盖此前交付 EXE 或旧版 Actions 产物。

### 已发布：v0.4.30-3 预发布

已按用户授权将全部后续本地二开保存于 `90868fa`，推送唯一 `main` 与注释标签 `v0.4.30-3`，并发布非草稿 Pre-release（`Latest=false`），未新建版本分支。Windows/macOS Actions 均成功，4 个安装包及 4 份更新附件已上传、下载并静态核验；完整提交、运行记录、文件大小与哈希见 [发布交付记录](../releases/v0.4.30-3.md)。

Windows EXE/MSI 均为 `NotSigned`，程序、资源和安装显示名称为 `TianCaiSpaceHub`。macOS App 的名称/显示名称/可执行文件均为 `TianCaiSpaceHub`，短版本 `0.4.30`、build `15`，Mach-O 包含 arm64 与 x86_64；ad-hoc 签名及 Actions `codesign` 校验通过，Developer ID 和公证步骤跳过，包未公证。DMG 仅核对下载哈希和 UDIF 尾部，不在本机挂载或解包；包静态核验不等同于安装或运行验证。

本版 Windows locked GUI/测试代码编译通过（普通程序 36 条警告、测试代码 59 条，其中 20 条重复），测试夹具未执行。提交前 271 个指定源码/资源文件指纹及 14 份文档的 598 个有效本地链接见交付记录。用户最后一轮本机测试反馈“目前看着没什么了”并授权发布，仅覆盖实际操作，不推定两平台安装升级、全部交互、异常或隔离均通过。此前 `c0952f61…` EXE 仍为 `0.4.30-2`，下节保留原构建身份，不覆盖或冒充本版包。

升级前核对任务/审批并正常退出旧 Hub。旧天工若已继承 `3847`，需正常退出旧实例一次，再由新版 Hub 启动；后续退出 Hub 只关闭自有后台，不终止天工任务。桌面即时同步仍限定官方 1.1.1 已核验 renderer 与本机通道，微信没有可撤回等待卡，`/wb` 暂不支持外部执行。配置/安装升级身份保持兼容，降级前备份客户端配置、Hub 配置及关联；详见交付记录。

以下为各开发阶段交付时的构建、诊断和用户反馈，原“未发布”/版号保留历史事实，代码已纳入本次预发布。发布安装包全部来自 Actions，开发方未主动启动应用、执行交互测试或真实业务，不扩展 Linux。

### 前阶段：天工退出端口、完整项目目录与临时回复状态

用户已确认前阶段 `tg-approval-ui-fix-20261006`（SHA-256 `71c3043d…`）的审批测试通过，只登记该次实际操作，其他平台及异常、重启、旧卡隔离仍待验收。本轮继续修复三个后续需求：退出 Hub 后保留天工时的监听继承、创建会话目录遗漏桌面手动项目，以及天工准备回复时的临时窗口。

Windows 静态核对发现默认监听及子进程启动存在句柄继承路径，尚未实机确认用户报告时 `3847` 的实际持有者。当前主/兼容回环监听通过不可继承的标准库 socket 创建并清除继承位；天工用 `CreateProcessW(bInheritHandles=FALSE)` 启动，Unicode 环境中替换子进程口令、移除 `ELECTRON_RUN_AS_NODE`，不把口令放进命令行或更改系统环境。GUI 退出先请求自有后台正常关闭、最多等待约 1.5 秒，再只终止该 Hub；恢复流程不再用 `/T` 结束天工进程树。天工任务继续独立运行，不自动抢占或杀死其他端口持有者。入口：[监听](../../src/main.rs)、[专用启动](../../src/gmclaw_runtime/windows_start.rs)、[GUI 生命周期](../../src/gui/daemon.rs)。

新建目录按默认、官方场景 `work_dir`、当前发送者同配置的当前/历史目录合并，包含没有任务的桌面手动项目；菜单读取不创建目录或任务。Windows 路径去重覆盖大小写/分隔符/verbatim/UNC，macOS 保留大小写，实际显示保留原值。完整读取失败明确提示列表不完整并提供本地候选及自定义输入，权限或配置失效不显示旧列表。飞书每页 20 个目录，默认/自定义及跨页已选目录保留；表单使用实际选项 `initial_index` 与输入 `default_value` 恢复草稿，校验表单对象与来源页。企微最多 10 项，完整列表通过共用 8 项文字分页；已有自定义路径不反复提示重输。限制、身份复核和配置兼容见 [项目目录](gmclaw-im.md#在-im-中选择项目目录)，代码：[目录后端](../../src/gmclaw_im/sessions.rs)、[原生项目](../../src/gmclaw_desktop.rs)、[飞书表单](../../src/im/feishu/renderer/threads/create.rs)、[企微卡片](../../src/im/wecom/adapter.rs)。

天工回合独立于 Codex runtime，排队前发送准备状态，执行时更新处理中，完成后同卡替换正式答复并结束生成态。飞书沿用原回复卡片；企微仅有效普通消息回调使用同一 stream 的 `finish=true` 收尾。微信接口没有同卡更新/撤回能力，继续直接发送最终文字，不增加永久等待句。审批、拒绝排队、预检失败、错误、超时或取消均有终态；未知执行不重放。长回复先投递当前可展示全文，再结束原控件；既有 16000 字截断和桌面全文规则保留。展示状态/API 时限、有限原控件收尾更新、微信身份复核及平台不可用边界详见 [临时回复状态](gmclaw-im.md)，代码：[回合展示](../../src/im/core/executor_turn.rs)、[执行](../../src/gmclaw_im.rs)、[出站](../../src/im/core/outbound.rs)。

本轮仍基于唯一 `main` 的 `2e6de88` 后托管 detached 工作区，保留全部前阶段未提交二开，产品 `0.4.30-2` 不提升。Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-lifecycle-project-turn-fix-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 已通过（普通程序 36 条警告、测试代码 59 条，其中 20 条重复）；同目录/目标的 GUI `cargo build --locked` 已通过（36 条警告）。新增监听、启动环境、项目目录、分页/草稿和回合收尾夹具只编译，不执行；实机行为待用户验收，macOS 原生构建待 Actions。Rust 格式、Git 差异空白及 EXE 静态身份核对通过，11 份相关文档的 580 个本地文件链接有效。本轮没有启动 Hub/天工/Codex、执行交互测试、真实 IM/模型/Harness/DataServer/CDP 调用，未读取用户会话数据库、记忆、私有日志或真实口令，未改运行配置/安装资源/系统关联，未提交、推送或发布。发布安装包继续由 Windows/macOS Actions 生成。

本轮独立 Windows 调试程序为 `target/tg-lifecycle-project-turn-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `58,491,904` 字节，SHA-256 `c0952f6132df8a9e51cf0d0cef83291f3d98687de8635a08984bab8cd95468f8`。PE 身份 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，资源 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。忽略目录记录为 `.build-tools/tg-lifecycle-project-turn-check.log`、`tg-lifecycle-project-turn-build.log`、`tg-lifecycle-project-turn-build-manifest.json`、`tg-lifecycle-project-turn-exe-identity.json` 与 `tg-lifecycle-project-turn-source-inventory.txt`；271 个指定源码/资源文件内容指纹为 `42bc4e43094c1ee3d96c528086a9b8e8371324aa50ded179b563f1cadc29b10c`，编译前快照与最终核对一致。前阶段审批 EXE 不停止或覆盖，哈希仍为 `71c3043d5289b73534dc38bf9e248545eda22202112fdd864530fd9037741d3d`。

升级时先处理或核对当前任务/审批，再正常退出旧 Hub（含托盘）。**旧天工若已继承监听，正常退出该旧天工一次，再打开本轮 Hub 并用按钮启动天工；新代码无法撤销旧进程已有的句柄。** 此后是否能保留天工反复退出/重开 Hub 是本轮待用户验收的行为。回退程序会恢复旧目录来源/等待交互并可能再次出现句柄继承；不删除原生项目、消息或模型，不通过重启声称任务已结束。本轮无新增持久配置或关联字段，临时卡片记录不跨 Hub 重启恢复。

### 前阶段：天工平台审批交互

用户确认前阶段 `fe48829e…` 程序中的天工桌面消息可以更新；这仅登记该次桌面显示操作的有限验收，不扩大为所有平台、历史分页或工具审批验收。本轮按用户要求让天工审批复用各平台现有交互：飞书与企业微信使用批准/拒绝按钮，微信沿用 Codex 的 `/1`、`/2` 选项回复。工具参数完整展示，大卡片先分段全文再发按钮；卡片失败保留旧 `/tg approve/reject <确认码>` 备用操作。天工独立请求与提交路径不进入 Codex 待审批记录；保留发起者、会话、配置、运行身份、15 分钟和未知状态保护。消费前在桌面准备之后再次核对身份与有效期；回执只说明选择并提交处理，不宣称工具已成功。详见 [审批专题](gmclaw-im.md#按当前平台处理审批)。

源码仍是唯一 `main` 基线 `2e6de88` 后的托管 detached 工作区，保留全部前阶段未提交二开，版本仍为 `0.4.30-2`。本轮未提交/推送/发布，不生成本地发布安装包，未修改用户配置、安装资源或系统关联。Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-approval-ui-fix-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；同目录/目标 GUI `cargo build --locked` 通过（33 条警告）。Rust 格式、Git 差异空白及 EXE 静态身份核对通过，11 份相关文档的 533 个本地文件链接有效。macOS 原生编译待 GitHub Actions，审批行为待用户验收。新增回调隔离、非法选项、旧卡和全文参数夹具只编译，不执行；本轮没有启动 Hub/天工/Codex、运行交互测试或真实 IM/模型/Harness/DataServer/CDP 调用，也没有读取私有日志、用户会话数据库、记忆或真实运行口令。前阶段只读结构诊断归属下节，不计为本轮执行。

本轮独立调试程序位于 C 盘工作区的 `target/tg-approval-ui-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `58,186,752` 字节，SHA-256 `71c3043d5289b73534dc38bf9e248545eda22202112fdd864530fd9037741d3d`。PE 身份为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，资源 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。忽略目录中的记录为 `.build-tools/tg-approval-ui-check.log`、`.build-tools/tg-approval-ui-build.log`、`.build-tools/tg-approval-ui-build-manifest.json`、`.build-tools/tg-approval-ui-exe-identity.json`；269 个指定源码/资源文件的内容指纹为 `f9c5a5dd8ff13d0d7b851720cebf7501d123816a672a22eb5fc14f4c8e785c19`。前阶段 EXE 未停止或覆盖，哈希仍为 `fe48829e66eaca5864153c0dd05e99ac43f4aaafda28624414e431fe629f978f`。

已有待审批任务应先在当前程序处理，再正常退出旧 Hub（含托盘）切到本轮独立程序；天工可保持运行。新触发的审批才产生本轮卡片，Hub 内存中的旧卡/确认码不会随程序升级迁移，不自动续执行。回退 Hub 不修改天工已有消息或权限策略，但恢复旧手动审批交互；切换程序前同样先处理当前待审批任务。所有发布安装包仍通过 Windows/macOS Actions 生成。

### 前阶段：天工会话消息引用作用域修复

用户测试 `9cdc7398…` 前阶段程序，飞书可回答，但天工窗口仍没有消息，状态为“原生视图或闭包字段未通过核对”；该阶段实时同步未通过用户验收。本轮核对官方安装源码并对当前运行窗口进行只读结构诊断，确认 App 的 4 个引用在 `Closure`，ChatPanel 因解构参数而把 11 个消息/状态值放在 `Block`；旧白名单只查 Closure 会漏掉全部会话值。修复分别限定两种视图的真实类别，各最多 8 个候选且完整所需字段必须来自同一作用域，Module/Global/Script 不枚举，null 和原生状态保护不放宽。固定诊断另区分 App/Panel 引用、IPC、缓存/任务身份及数据结构，不输出原始错误或私有内容。默认值和兼容/回退见 [局部同步](gmclaw-im.md#已打开窗口的局部消息同步)。

诊断仅在已核对唯一 `127.0.0.1:18769` 监听、正式天工 EXE、同用户 SID、官方 `1.1.1` 和 renderer 指纹后，读取本机页面列表并使用 CDP Runtime 获取引用与字段类型；未读取消息正文或调用官方 DataServer。实际结果为 App Closure 4 个引用齐全、Panel Closure 缺少 11 个值，Panel Block 包含全部句柄/合法 null，schema 返回 `ready`。该结果证明本机引用能力检查通过，不能替代消息实际更新。没有主动启动 Hub/天工/Codex、执行交互测试/测试夹具、`display-sync.js` 或窗口更新，也没有发起 IM/模型/Harness/DataServer 业务调用；scope 枚举可能让未选择的基础类型授权暂入本机内存，不输出、记录或持久保存，正文/草稿引用不展开。未读取私有日志、数据库或记忆，未改用户运行配置、安装资源或系统关联。

本轮继续使用 C 盘临时 detached checkout，基于唯一 `main` 的 `2e6de88` 并保留全部前阶段二开，产品仍 `0.4.30-2`，没有本轮源码提交/推送、发版或本地发布安装包。Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-panel-scope-fix-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；同目录/目标 GUI `cargo build --locked` 通过（33 条警告）。Rust 格式、Git 差异空白、三份脚本语法编译及 EXE 静态身份核对通过，11 份相关文档的 517 个本地文件链接有效。新增两份作用域/固定错误夹具仅编译、未执行；实际运行的只读 schema 核对另如上记录，`display-sync.js` 未执行。macOS 待 GitHub Actions；连续多轮消息即时更新仍待用户验收。

本轮独立调试程序为 C 盘工作区的 `target/tg-panel-scope-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `58,051,072` 字节，SHA-256 `fe48829e66eaca5864153c0dd05e99ac43f4aaafda28624414e431fe629f978f`。PE 静态身份为 x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`；资源 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。忽略的本地记录为 `.build-tools/tg-panel-scope-check.log`、`.build-tools/tg-panel-scope-build.log`、`.build-tools/tg-panel-scope-build-manifest.json`、`.build-tools/tg-panel-scope-exe-identity.json`；268 个指定源码/资源文件的内容指纹为 `186233ec75e5d0b46f30a6cb0587b239493bb35948e9c67d4fe4e2481c1c39cb`。只读结构证据 `.build-tools/tg-panel-scope-readonly-diagnosis.json` 仅保存固定检查项、计数和 schema 结果，不保存实际页面 URL、任务/会话 ID、口令、正文或草稿。

原 `tg-window-path-fix-20261006` 程序不停止或覆盖，SHA-256 核对仍为 `9cdc739879d40a2d8611a3b46e8641f9d7dfaeb6bd4fc9e1595698c2ca164edd`；已经从 Hub 启用通道的天工可保持运行。用户正常退出旧 Hub（含托盘），打开本轮新目录程序并恢复原天工会话后继续测试，不需为本次作用域修复再退出天工。回退旧 Hub 不删除已保存消息，但会再次遗漏 Block 中的会话引用；无通道或更早无来源占位的限制保留原阶段归属。

### 前阶段：Windows 天工页面路径误判

用户测试前阶段 `e407305f…` 程序，从 Hub 启动天工后，飞书回复正常，但持续状态为“上次未完成，天工会话窗口尚未就绪”，桌面消息仍不更新。该阶段同步未通过用户验收。已静态核对确定原因：预期 ASAR 页面路径混合使用正/反斜杠，而 Windows URL 转路径使用反斜杠，按字符串比较会拒绝同一页面。当前逐段构造路径，并比较完整 Windows 路径组件，保留 ASCII 大小写兼容；页面未出现、地址不匹配、多匹配及标识异常分别显示固定脱敏原因。限定安装资源/页面、监听归属、Vue schema、原生未保存/运行/审批和 Hub 来源保护继续生效。实现、兼容及回退见 [局部同步](gmclaw-im.md#已打开窗口的局部消息同步)。

本轮继续在同一 C 盘临时 detached checkout 开发，基线为 `main` 的 `2e6de88`，承接全部未提交二开。产品仍 `0.4.30-2`，不新增版本分支，没有本轮源码提交/推送、发版或本地发布安装包。Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-window-path-fix-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；同目录/目标 GUI `cargo build --locked` 通过（33 条警告）。Rust 格式、Git 差异空白、三份展示脚本语法编译及 EXE 静态身份核对通过，相关 11 份文档的 511 个本地文件链接有效。两个 Windows 路径回归夹具仅编译、不执行；macOS 待 GitHub Actions，实际 CDP 页面连接及连续多轮即时更新仍待用户验收。没有运行应用、测试夹具或真实 HTTP/IM/模型/Harness/DataServer/CDP 调用，没有读取真实运行口令、私有日志、会话数据库或记忆，没有修改运行配置、安装资源或系统关联。

本轮独立调试程序为 C 盘工作区的 `target/tg-window-path-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `58,042,368` 字节，SHA-256 `9cdc739879d40a2d8611a3b46e8641f9d7dfaeb6bd4fc9e1595698c2ca164edd`。PE 静态身份为 x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`；资源 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。忽略目录的记录为 `.build-tools/tg-window-path-check.log`、`.build-tools/tg-window-path-build.log`、`.build-tools/tg-window-path-build-manifest.json`、`.build-tools/tg-window-path-exe-identity.json`；268 个指定源码/资源文件的内容指纹为 `a581ddf6d8a9a285b3da209159a168c2bd939091c148c511cf836d5ad960d967`，不包含用户数据。

前阶段 `tg-live-sync-fix-20261006` 程序不停止或覆盖，核对 SHA-256 仍为 `e407305fd362cb52a52b54a002dbd7f62dc83e6842beb67fbefd15021da28437`。已经从 Hub 启用同步通道的天工可保持运行；用户正常退出旧 Hub（含托盘），打开本轮新目录程序后，恢复原天工会话或新保存消息即可重新排队展示，本次路径修复不要求再次退出天工。未带通道的手动实例首次仍需正常退出后从 Hub 启动；更早无来源占位需正常重新读取的规则保留原阶段边界。回退到前阶段 Hub 不修改官方消息，但 Windows 页面误判可能重新出现，不自动关闭桌面或重放任务。

### 前阶段：占位消息持续卡住与同步状态

用户在 `8b3c8a30…` 前阶段程序中从 Hub 启动天工，飞书连续收到完整回复，但天工窗口仅保留第一轮占位，只有重启后显示新内容。该反馈确认本次 Hub 启动操作可完成，不表示前阶段窗口同步通过。此次修复在途刷新与新消息覆盖时丢失 Hub 回复行来源的确定路径，独立 128 键/每键 128 个 ID 缓存保留来源，会话关联按精确任务、会话与记忆身份持久记录这些 ID；旧缓存不猜来源，实际原生未保存/执行/审批仍保护，执行与展示重试分开。

启动检查加入只读 ref/schema 能力核对，限定同一已核对版本资源和本机身份；不再仅依据可列出 CDP 页面宣称能够刷新，闭包枚举按所需字段而非唯一名称查找。启动按钮下方新增持续“桌面对话同步”状态，概览同时带最近结果，30 秒内区分准备与实际更新、暂缓原因和失败，过期不把历史成功当作当前状态。没有 GUI 主线程新增网络操作或模型重放。默认限制、关联兼容和回退见 [局部同步](gmclaw-im.md#已打开窗口的局部消息同步)。

本轮在同一 C 盘临时 detached checkout 开发，基于唯一 `main` 的 `2e6de88` 并保留全部前阶段未提交二开。版本仍 `0.4.30-2`，没有本轮源码提交/推送、版本发布或本地发布安装包。Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-live-sync-fix-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；同目录/目标 GUI `cargo build --locked` 通过（33 条警告）。Rust 格式、Git 差异空白、三份展示脚本的语法编译及 EXE 静态身份核对通过；脚本函数和测试夹具未执行。macOS 原生构建待 GitHub Actions，实际连续多轮窗口同步待用户验收。

本轮独立调试程序位于 C 盘工作区的 `target/tg-live-sync-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `58,031,616` 字节，SHA-256 `e407305fd362cb52a52b54a002dbd7f62dc83e6842beb67fbefd15021da28437`。PE 静态身份为 x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`；资源 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。忽略目录中的构建与核对记录为 `.build-tools/tg-live-sync-check.log`、`.build-tools/tg-live-sync-build.log`、`.build-tools/tg-live-sync-build-manifest.json`、`.build-tools/tg-live-sync-exe-identity.json`；268 个指定源码/资源文件的内容指纹为 `58b6657be2ec6021b0c3dee4f6df335d23cd4b66512aa5576046f76121e0d660`，不包含用户配置或私有数据。相关 11 份文档的本地文件链接均有效，最终数量见该清单。

未运行应用、测试夹具或真实 HTTP/IM/模型/Harness/DataServer/CDP 请求，不读用户真实运行口令、私有日志、会话数据库或记忆，不改运行配置、安装资源或系统关联。前阶段 C 盘 `tg-launch-display-fix-20261006` 程序不停止或覆盖，SHA-256 仍为 `8b3c8a309b24c7356ac510afc4dc12b4199f8b36314a004d9df026ef6e7f2ad4`；其窗口同步用户反馈失败，不能作为本轮通过证据。本次更换后需正常退出旧 Hub（含托盘）和天工一次，清除旧版未登记来源的占位，再从新版 Hub 打开天工测试连续对话，后续每轮对话不要求重启。

### 前阶段：安装发现、IM 提示与局部桌面同步

2026-10-06 用户确认 WorkBuddy 启动按钮可用，飞书可选择天工历史并继续对话；这些反馈只覆盖当次操作。同时报告天工启动误报未安装、每轮额外发送固定等待句以及桌面已打开窗口不显示后续 IM 消息。本轮修复卸载注册表只有图标路径时的安装发现，继续保留正式程序及资源检查；删除普通消息无条件等待回复，队列满、超时、审批及错误仍明确反馈。此前原生 `1000` 步、残留 `processing`、后台重连、真实任务/消息及全部本地二开保留。

天工 1.1.1 原生窗口缓存不会因持久保存自动重新读取。新增限定版本/完整 renderer 资源指纹的局部展示适配，显式新启动附加固定 `127.0.0.1:18769` 通道，并核对监听的正式程序、同用户与精确页面。首次手动启动实例需要用户正常退出后从 Hub 打开一次，以后不需逐轮重启；已有实例不会被强行关闭。成功恢复或消息保存后只排队真实任务/会话及少量 Hub 回复行身份，展示失败独立于执行；原生执行、审批、历史加载、未保存及读取期间变动延后，不重放任务。局部更新保留输入草稿、附件、模型、当前选择及已加载历史；不改天工安装资源或持久启动配置。实现、临时内存凭据边界、默认限制与回滚见 [局部同步](gmclaw-im.md#已打开窗口的局部消息同步)。

本轮源码位于 Codex 托管的临时 detached checkout `C:\Users\Administrator\.codex\worktrees\c71f\tiancaispace-hub`，来自唯一主分支 `main` 的 `2e6de88`，承接原 `D:\traeworkspace\tiancaispace-hub` 全部此前未提交二开；该临时工作树不是产品版本分支。产品仍 `0.4.30-2`，本轮没有提交/推送/发布或本地发布安装包。Windows GUI/测试代码 `cargo check --locked --target-dir target/tg-launch-display-fix-20261006 --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；同目标/目录 GUI `cargo build --locked` 通过（33 条警告）。Rust 格式、Git 差异空白、两个展示脚本的语法编译及程序静态身份通过，11 份相关文档的 491 个本地文件链接有效。没有执行测试夹具或脚本函数，实机显示和启动修复待用户验收，macOS 原生构建及相同 renderer 指纹适配待 GitHub Actions/实机。开发方未启动应用、执行测试或真实 HTTP/IM/模型/Harness/DataServer/CDP 调用，未读取用户真实授权、私有日志、会话数据库或记忆，不改运行配置、安装资源或系统关联。

本轮独立程序位于此 C 盘工作区的 `target/tg-launch-display-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `57,947,136` 字节，SHA-256 `8b3c8a309b24c7356ac510afc4dc12b4199f8b36314a004d9df026ef6e7f2ad4`。PE 静态身份为 x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，资源 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。忽略的本地核对记录为 `.build-tools/tg-launch-display-check.log`、`.build-tools/tg-launch-display-build.log`、`.build-tools/tg-launch-display-build-manifest.json`；清单记录 267 个指定源码/资源文件的内容指纹 `8bc2dfbfa81e7c50077358206f8a0c2960ed795d87a19eb015e61c45350c9c10`。原 D 盘前阶段 `tg-steps-launch-fix-20261006` 程序未停止或覆盖，哈希仍为下段 `045b4b3e…`。测试时先正常退出旧 Hub（含托盘），打开本轮程序；首次要即时同步窗口，正常退出已有天工，再从 Hub 点击启动，后续每轮对话不要求重启。

### 前阶段：原生步数兼容与桌面启动入口

2026-10-06 用户反馈飞书恢复仍提示“原会话执行步数无效，未恢复”。只读核对天工 1.1.1 安装源码确认桌面正常请求提交 `max_steps=1000`，旧 Hub 把新建配置上限 `200` 套用于原生历史；恢复、元数据登记及实际请求现共用 `1–1000` 兼容边界并保留原值。Hub 新建默认 `30`、范围 `1–200` 不变。Codex 归到 AI Gateway 的会话管理只改其 `model_provider`，与天工恢复无关。所有原状态/审批、模型、目录、记忆和发送者保护保留，既有自动重连、桌面正文、上下游流式日志和其他本地二开保持；实现与回退见 [步数兼容](gmclaw-im.md#原生会话执行步数兼容)。

本轮两个显式启动按钮已实现；天工仅准备连接配置并启动/复用，WorkBuddy 仅启动桌面，两者不改 IM 选择或执行任务。源码为唯一 `main` 提交 `2e6de88` 后未提交工作区，版本仍 `0.4.30-2`；无本轮推送/发布或本地发布安装包。Windows GUI/测试代码编译检查通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；现有夹具补充 `1000` 步元数据/请求原值与无效步数拒绝，仅编译未执行。Windows 独立调试构建通过（33 条警告），格式/差异和 EXE 静态身份核对通过，11 份相关文档的 469 个本地文件链接有效；macOS 原生构建待 GitHub Actions，恢复、继续对话及两个启动按钮待用户验收。

本轮调试程序为 `target/tg-steps-launch-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `57,590,272` 字节，SHA-256 `045b4b3e875e333b495d885eb72c4d4b7c385fbeb597aedb85595f85838069f5`。PE 为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，实际资源为 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。必要编译使用 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 和 `cargo build --locked --target-dir target/tg-steps-launch-fix-20261006 --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub`。忽略目录中的记录为 `.build-tools/tg-steps-launch-check.log`、`.build-tools/tg-steps-launch-build.log`、`.build-tools/tg-steps-launch-build-manifest.json`，清单包含 264 个源码/资源文件的内容指纹。前阶段程序仍为下文 `d0ab334b…` 哈希，未停止或覆盖；用户正常退出旧 Hub（含托盘）后可从新目录打开，天工可以保持运行。

本轮仅静态阅读源码、安装元数据与文件属性，执行必要编译和文档核对；未启动应用、执行测试、读取真实运行口令/私有日志/用户会话数据库或记忆，也未发起实际 HTTP/IM/模型/Harness/DataServer 请求。新编译目录为 `target/tg-steps-launch-fix-20261006`，不停止或覆盖用户正在使用的前阶段程序，不改运行配置/安装资源或系统关联。上一阶段用户只确认自动连接成功，随后两次历史恢复报错，不登记本次恢复为已验收。

### 前阶段：历史状态修复

2026-10-06 本轮修复点击历史会话误报“天工会话正在执行或等待审批”。静态核对官方 1.1.1 安装源码确认：桌面收到 `over` 主动中止流，可能让 Harness 的持久状态残留 `processing`。恢复和普通执行前共用最新轮次结束判断：核对真实行 ID、完整 `harness_sidecar user→over`、无本轮未处理确认、明确事件数量，残留 `processing` 另核对时间、轮次身份和至少间隔 1 秒的稳定元数据/事件双读。实际运行、待审批、没有完整终态或 Hub 已标记未知的会话仍拒绝；不回写天工状态或重放任务。官方没有原子的每会话执行查询，双读核对不能锁住用户后续在桌面发起的任务。实现、兼容和回滚见 [恢复状态判断](gmclaw-im.md#恢复时的状态判断)。

用户本次确认前阶段程序确实自动连接；仅登记这一场景的有限验收，历史恢复修复、工具审批、模型调用及桌面显示仍待用户验收。本轮保留自动重连、真实任务/对话、完整历史、精确模型、原生记忆、占用保护和全部此前本地二开。源码为唯一 `main` 提交 `2e6de88` 后未提交工作区，版本仍为 `0.4.30-2`。Windows x64 GUI/测试代码编译检查通过（普通程序 33 条警告、测试代码 57 条，其中 18 条重复）；五个新的恢复边界夹具仅编译，未运行。同目标独立 GUI 调试构建通过（33 条警告），EXE 静态身份核对通过；macOS 原生构建待 Actions，本轮未提交/推送/发布或生成本地发布安装包。

本轮开发只读取官方安装源码和项目文件，未启动应用、执行测试或发起实际 HTTP/IM/模型/Harness/DataServer 请求；未读取真实运行口令、私有日志、用户会话数据库或记忆文件，不改用户运行配置和天工安装资源。新编译目录为 `target/tg-history-recovery-fix-20261006`，不停止或覆盖用户当前运行的前阶段 EXE。

本轮独立调试程序为 `target/tg-history-recovery-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `57,408,000` 字节，SHA-256 `d0ab334b3c60034fb03a55157f94c47834333e60d839365f0484b74c68ad030e`。PE 静态核对为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，实际版本资源为 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。使用 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 与 `cargo build --locked --target-dir target/tg-history-recovery-fix-20261006 --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub` 完成必要编译。格式与差异空白核对通过，11 份相关文档的 446 个本地文件链接有效。忽略目录中的核对记录为 `.build-tools/tg-history-recovery-check.log`、`.build-tools/tg-history-recovery-build.log`、`.build-tools/tg-history-recovery-build-manifest.json`；清单记录 263 个源码/资源文件的内容指纹。旧版 EXE 的 SHA-256 仍为下段 `c89da7c0…`，没有被本轮编译覆盖。用户正常退出旧 Hub 后可从本轮目录打开新版，天工可以保持运行；程序未由开发方启动或安装。

以下 2026-10-06“自动重连、真实桌面任务与全场景恢复”记录属于前阶段，不替代本轮历史状态修复验证。该阶段修复天工重启后自动连接、IM 真实桌面任务与消息保存，以及恢复全部桌面场景任务。启用后每 5 秒后台检查并核验同用户官方天工的新运行授权，不需再次 `/tg` 授权；后台不启动桌面或重放任务。新建/恢复沿用原平台会话界面，使用天工原生记忆身份，并将用户文字、答复和审批提示保存到真实任务。恢复保留目录与模型，运行/审批/未知状态继续保护；`/gpt` 释放占用、保留当前会话，再 `/tg` 返回时重新核对占用。此前手动实例直连、模型概览绿色状态、上下游流式日志和全部本地二开保留。实现细节、默认值、持久关联及回滚见 [天工外部消息](gmclaw-im.md)。

该阶段源码为唯一 `main` 提交 `2e6de88` 后包含全部前轮二开的未提交工作区，版本仍为 `0.4.30-2`。Windows x64 GUI/测试代码 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 34 条警告、测试代码 58 条，其中 19 条重复），同目标 GUI 调试 `cargo build --locked --target-dir target/tg-desktop-sessions-20261006 --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub` 通过（34 条警告）。测试夹具仅编译未执行；未启动应用、发起实际 HTTP/IM/模型/Harness/DataServer 调用或读取用户真实口令/会话正文；不改用户运行配置和天工安装资源。macOS 原生构建待 GitHub Actions，该阶段尚未提交、推送、发布或生成本地发布安装包；后续有限用户验收如本节前文。

本轮独立调试程序为 `target/tg-desktop-sessions-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `57,395,200` 字节，SHA-256 `c89da7c0d46ab198d723a4f46dd08ec19e5f94e9463cd5776d44227125b454b5`。PE 静态核对为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，实际资源为 `ProductName=TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。格式与差异空白检查通过，11 份相关文档的 436 个本地文件链接有效。日志与源码/产物身份清单为 `.build-tools/tg-desktop-sessions-check.log`、`.build-tools/tg-desktop-sessions-build.log`、`.build-tools/tg-desktop-sessions-build-manifest.json`；清单记录 263 个源码/资源文件的内容指纹，均为忽略的本地记录。前阶段正在使用的 EXE 未停止或覆盖，其 SHA-256 仍与下段一致。新版未安装或运行；用户测试时正常退出旧 Hub 后从本轮目录打开，天工可保持运行。

天工 1.1.1 官方接口支持任务/正文持久保存，但没有外部 renderer 热刷新接口；已打开的天工窗口可能要自身刷新或重新打开后才显示新增内容，不能将数据保存宣称为即时显示。旧版只在 Hub 内存中的会话及没有 `task_messages` 的旧正文无法本轮补造。本轮未改写天工安装包来强制刷新，也不扫描用户私有日志拼接旧历史。

以下 2026-10-06“已有实例接入、连接等待与模型状态修复”记录属于前阶段，不替代本轮自动重连与桌面会话验证。该阶段修复手动打开天工后 `/tg` 接入、启动/退出等待、上下游流式日志及天工模型绿色连接状态。显式接入仅在核验同用户、安装、进程与参数身份后使用临时运行授权，授权不写配置；保留共用会话流程、逐会话目录/模型及此前二开。源码基于 `main` 提交 `2e6de88` 后未提交工作区，产品版本仍为 `0.4.30-2`。Windows GUI/测试代码编译、调试构建与 EXE 静态核对通过。测试夹具仅编译未执行；未启动客户端或发起真实模型、IM、Harness 调用，不读取真实运行口令、不改用户运行配置。macOS 待 Actions 原生构建；不生成本地发布安装包，不发布，功能待用户验收。

该阶段 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub` 通过（普通程序 29 条警告、测试代码 54 条，其中 15 条重复），同目标 GUI 调试 `cargo build --target-dir target/tg-connection-fix-20261006` 通过（29 条警告）；未执行测试。该阶段调试程序为 `target/tg-connection-fix-20261006/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `56,657,408` 字节，SHA-256 `69ba2043fed9f3faa723f745c802d4ac8b635ad13ed8c54e3538e8155c13cc32`。PE 静态核对为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，产品名 `TianCaiSpaceHub`、原始文件名 `TianCaiSpaceHub.exe`、版本 `0.4.30-2`。该阶段使用独立构建目录，未停止或覆盖当时运行的旧调试 EXE；新程序未安装或运行。日志与源码/产物身份清单为 `.build-tools/tg-connection-fix-check.log`、`.build-tools/tg-connection-fix-build.log`、`.build-tools/tg-connection-fix-build-manifest.json`，保留原阶段归属。格式、差异空白与文档本地链接的核对结果见该阶段清单；已发布包未被替换。

以下 2026-10-04 共用会话流程记录属于上一阶段，不能替代本轮连接修复验证。该阶段将天工改为复用 Codex 的各平台会话流程、卡片与菜单，由共用后端提供各执行端的目录、模型、新建和恢复操作；移除天工专有文字目录菜单及额外确认规则。基于同一 `main` 提交后的未提交工作区，产品版本 `0.4.30-2`；Windows 编译与静态核对通过，功能待用户验收。测试夹具未执行，也未主动启动客户端、真实模型或 IM 调用。

该阶段最终编译包含恢复/提交前模型存在性复核及企业微信共用多模型分页选择回显修复。GUI/测试代码 `cargo check` 通过（53 条警告），GUI 调试 `cargo build` 通过（28 条警告）；未执行测试。当时程序为 `target/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `56,412,160` 字节，SHA-256 `fc7e6843b4e9bf5192c7ae40cdf99c717fe700d467b0e9c776c36e7e538c037a`。PE 静态核对为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，产品名 `TianCaiSpaceHub`、原始文件名 `TianCaiSpaceHub.exe`、版本 `0.4.30-2`。格式和差异空白检查通过，9 份相关文档的 363 个本地文件链接无失效。日志与源码/产物身份清单为 `.build-tools/shared-session-flow-check.log`、`.build-tools/shared-session-flow-build.log`、`.build-tools/shared-session-flow-build-manifest.json`，保留历史归属。

以下“IM 直接接入、移除桥接表单、天工独立文字目录菜单”记录属于上一阶段。独立菜单已由本轮共有流程替换，旧 `/y` 操作与产物哈希不作为本轮操作或验证。

上一阶段移除天工 IM 桥接表单，新增 `/tg` 自动接入与 IM 逐会话项目目录选择/确认创建，承接 `main` 提交 `2e6de88` 后未提交二开。该阶段 Windows GUI/测试代码编译、调试构建及 EXE 静态核对通过，功能未验收；未执行测试、真实客户端、模型或 IM 调用，不改用户运行配置。

该阶段必要编译使用 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub`（53 条警告）以及同目标 GUI 调试 `cargo build`（28 条警告），均通过；测试夹具只编译未执行。当时调试程序为 `target/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `56,274,432` 字节，SHA-256 `cd3b20d42fddbd9ad47d469657771bcf6029826ac34425d0d5ad2f7823178796`。静态核对为 Windows x64 `0x8664` / PE32+ `0x020b` / GUI subsystem `2`，产品名 `TianCaiSpaceHub`，原始文件名 `TianCaiSpaceHub.exe`，版本 `0.4.30-2`。日志与清单为 `.build-tools/im-project-flow-check.log`、`.build-tools/im-project-flow-build.log`、`.build-tools/im-project-flow-build-manifest.json`。当时核对 9 份相关文档的 342 个本地文件链接，无失效链接；未运行客户端或生成发布安装包。

以下自动授权按钮与概览记录属于上一阶段；当前 GUI 已移除按钮，不能按该阶段操作流程使用本轮程序。

上一阶段新增天工自动授权、显式启动与页签概览，基于 `main` 提交 `2e6de88` 后的本地未提交工作区，Windows GUI/测试代码编译和调试构建通过，EXE 版本资源、PE 与哈希已静态核对。Windows/macOS 均有平台实现；macOS 新增原生构建待 GitHub Actions，功能和实机行为待用户验收，不新增 Linux。未启动客户端或发起真实模型/工具调用，不改用户安装、运行配置或系统关联，不生成本地发布安装包。

该阶段通过 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub`（53 条警告）及同目标 GUI 调试 `cargo build`（28 条警告）；测试代码仅编译，未执行。该阶段调试 EXE 为 `target/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `56,128,512` 字节，SHA-256 `e7e3b5201980d02318fcf70a4aea72a8666ca4b34102023a4db146fd763535bb`。PE 静态核对为 x64 `0x8664`、PE32+ `0x020b`、GUI subsystem `2`；产品名与原始文件名为 `TianCaiSpaceHub` / `TianCaiSpaceHub.exe`，版本仍为 `0.4.30-2`。本地日志和清单位于 `.build-tools/auto-connect-overview-check.log`、`.build-tools/auto-connect-overview-build.log`、`.build-tools/auto-connect-overview-build-manifest.json`，不入仓库；程序未安装或运行，本轮尚未发布。

以下为上一阶段统一命名的构建记录，尚未发布，不能作为本轮自动连接和概览的验证：Windows x64 GUI 与测试代码编译检查通过（54 条既有警告），GUI 调试构建通过；未执行测试或启动程序。该阶段包含同轮 IM 短指令修改。

改名前调试输出经未运行检查与哈希核对后归档到 `.build-tools/legacy-debug-before-brand-rename/`，当前调试输出目录仅保留新名 EXE。历史已发布包、下载文件及用户安装的旧程序未被移动或替换，旧产物身份见 [IM 验证记录](gmclaw-im.md#验证状态)。

统一命名阶段的本地调试文件：`target/x86_64-pc-windows-msvc/debug/TianCaiSpaceHub.exe`，大小 `55,794,176` 字节，SHA-256 为 `df2b7333a533e688db7631ff7880e416a88b73ba85b2c1c56aae32bbf91cd78a`。PE 静态核对为 x64（`0x8664`）、PE32+（`0x020b`）、Windows GUI subsystem（`2`）；实际版本资源 `ProductName` 和 `FileDescription` 均为 `TianCaiSpaceHub`、`OriginalFilename=TianCaiSpaceHub.exe`、`FileVersion=0.4.30-2`。本地日志与清单为 `.build-tools/brand-name-check.log`、`.build-tools/brand-name-build.log`、`.build-tools/brand-name-build-manifest.json`，属于忽略的本地构建记录。共享 EXE 路径会被后续构建覆盖，须按哈希区分阶段；此前 IM 专题中的旧名构建同样保留原归属。

`0.4.29-2` 首批本地 MSI/ZIP 保留原样；GitHub 交付采用按提交源码重新打包的 Windows 预发布产物，见 [版本交付说明](../releases/v0.4.29-2.md)。后续 Windows 本地包及用户反馈见 [导入专题](../hub-external-import.md)。`0.4.29-5` 当时补齐 macOS 网页接入代码，但未完成 Mac 包；该实现已随本次 `0.4.30-2` 的 macOS universal 包构建发布，原生交互仍待用户验收。

`0.4.30-1` 的 Windows 编译记录属于前轮整合版本；`0.4.30-2` 已由 GitHub Actions 成功构建并预发布 Windows x64 MSI/ZIP 和 macOS universal DMG/App ZIP，具体签名与哈希见 [交付记录](../releases/v0.4.30-2.md)。Windows 包未签名，macOS 为 ad-hoc 签名且未公证。功能测试、桌面交互和安装升级仍待用户验收，构建成功不代表这些项目通过。本轮不修改用户运行中的 Hub 配置、协议关联或已交付包。
