# TianCaiSpaceHub 文档总入口

2026-10-20 源码已推送（TC-006/TC-007）：按用户要求，10 月 19 日增强启动超时、有限重连、可选增强项降级及旧版 Fast/Ultra 兼容修复已提交并推送到唯一 `main`（修复提交 `8745292`），版本仍为 `0.4.30-5`；完整身份见 [本轮交付](releases/v0.4.30-5.md#2026-10-20-兼容修复源码提交与-fast-参数核对)。本次没有启动新 Actions 或发布新包，下方“未推送”等记录保留当时状态。只读核对用户自行发起的请求已看到 MAX 的 Fast 参数完整传递并获上游回报；其他样本入站即无 Fast 参数，不能推导仅 MAX 支持。复核入口见 [模型专题](dynamic-codex-models.zh-CN.md#fast-实际请求核对)，完整平台与组合仍待用户验收。

2026-10-19 旧机资源补齐（TC-006/TC-007）：确认 `26.917.51856` 资源使用布尔型 Fast，已兼容其界面与请求结构；旧机及前两份较新资源的 Fast UI/请求、Ultra 均各唯一匹配，静态改写语法通过。原 requirements、返回类型、缓存、模型能力及受管本机范围保留；Fast/Ultra 仍独立可选，缺项继续启动，旧机未知设置结构保持原行为。本轮构建、本地 Windows MSI 与待用户验收见 [交付](releases/v0.4.30-5.md#2026-10-19-旧版布尔型-fast-结构兼容)，现行规则见 [模型控制选项](dynamic-codex-models.zh-CN.md#初始化开启-maxultra-与-fast-选项)。版本仍 `0.4.30-5`、唯一 `main`，未推送；下方资料待补状态保留对应历史阶段。

2026-10-19 当前启动策略（TC-006/TC-007）：Fast、Ultra 与设置入口按能力独立适配，缺项保留原行为和已成功项，不阻塞基础启动；可选 browser 补装最多占 30 秒，包含在原 45 秒同步时限内，失败或会话中断后继续独立 page 基础检查。模型、语言等必要配置与真实状态校验保留。版本仍 `0.4.30-5`、唯一 `main`，本地 Windows 测试 MSI 放 `target/`，待用户验收后再推送。现行规则见 [模型控制选项](dynamic-codex-models.zh-CN.md#初始化开启-maxultra-与-fast-选项)，本轮构建与包身份见 [交付](releases/v0.4.30-5.md#2026-10-19-模型增强按能力启用且不阻塞启动)。下方旧轮必需标记、待资料及包结果保留原阶段归属。

2026-10-19 较旧客户端补充诊断（新策略前的历史记录）：另一台 Windows 机器窗口挂载和 Ultra 适配已成功，Fast 界面/请求两项未加载；当时尚需确认原始资源以扩展 Fast 匹配。该诊断阶段仅更新文档、未修改源码或包；见 [本轮诊断](releases/v0.4.30-5.md#2026-10-19-较旧客户端-fast-适配缺项诊断)。

2026-10-19 后续反馈：用户确认最新 Windows 本地测试包“这版是可以的”。适配没有按 Codex 版本号指定分支，按实际关键结构唯一识别；当前反馈不等于所有版本、Fast 实际请求或 macOS 全面验收。兼容范围见 [模型控制选项](dynamic-codex-models.zh-CN.md#初始化开启-maxultra-与-fast-选项)，本次包身份及用户反馈见 [交付记录](releases/v0.4.30-5.md#2026-10-19-窗口挂载超时后的有限重连)。仅补充文档，未重打包或推送。

2026-10-19 第四次诊断（TC-006/TC-007）：最新截图为桌面适配补装失败；日志确认本次在 `Target.attachToTarget` 5 秒未响应后结束，尚未加载模型适配脚本，不能据此判定上一轮 ULTRA 结构修复效果或商店更新失败。当前将窗口握手单独限 10 秒、连接尝试限 15 秒，最多顺序尝试两次；重试前释放失败连接并重新发现实际 app 窗口，仅重试尚未安装脚本的握手阶段。补装与同步继续共用 45 秒，整体 80 秒、GUI 90 秒保护及真实完成校验保留；普通命令仍限 5 秒，补装不再套早挂载 8 秒窗口。新增固定阶段日志，底层未响应的原因仍待实机核对。 源码、编译与本地测试 MSI 见 [本轮交付](releases/v0.4.30-5.md#2026-10-19-窗口挂载超时后的有限重连)；版本仍 `0.4.30-5`、唯一 `main`，实机待用户验收，测试完毕再推送；下方前轮记录保持当时归属。

2026-10-19 第三次诊断（TC-006/TC-007）：已收到测试机原始 `app.asar`。其模型查询缺少本机较新资源中的 `teamCatalog`，原 ULTRA 规则要求该字段，造成匹配为零；新日志确认只有该标记缺失而等待到阶段截止点，截图因此提示“桌面配置适配未完成”。当前兼容两种已核对结构，仍校验唯一匹配、同局部变量及本地受管范围。两份原始资源的四类静态匹配、等效改写后 JavaScript 语法均通过；不更改安装资源或真实能力。编译与本地 MSI 身份见 [本次修复](releases/v0.4.30-5.md#2026-10-19-测试机原始资源核对与-ultra-兼容修复)，实机待用户验收。仍在唯一 `main` 工作区，版本 `0.4.30-5`，测试包放 `target/`，用户测试完毕再推送及发布；下方前轮记录保持当时边界。

2026-10-19 第二次诊断（TC-006/TC-007）：用户确认 Fast 已出现，ULTRA 仍缺；新日志确认 browser 补装成功、Fast 与设置区块的源码适配命中，但 ULTRA 尚未命中，不能当作完整验收。截图为 GUI 等待本机服务 60 秒后超时，后端日志没有本次结束结果；源码确认 CDP 命令没有单独超时、可能越过循环外层时限。当前补齐单命令 5 秒、同步步骤共用 45 秒截止点、增强启动整体 80 秒保护，GUI 等待 90 秒并用普通中文说明超时。ULTRA 的具体脚本差异仍需测试机 `app.asar` 核对，本次不猜改匹配规则；编译与待补资料状态见 [本次诊断](releases/v0.4.30-5.md#2026-10-19-第二次诊断与启动超时保护)。版本仍 `0.4.30-5`、唯一 `main`；用户追加调整测试安排：本次先在 `target/` 内交付本地 Windows 测试 MSI，由用户安装测试，完成后再进行推送与后续发布；当前未推送或安排新 Actions，下方前轮运行保持原身份。

2026-10-19 TC-006/TC-007 诊断后修复：测试机日志显示早期 renderer 挂载超时，原备用路径只注入增强脚本、漏装模型脚本拦截，最终仅“桌面配置适配”未完成。当前在找到实际 app 窗口后显式挂载 browser 会话，补齐缓存处理、Fetch 与新文档脚本并核对安装响应；新启动首窗才允许 load 后按需单次刷新，已运行窗口不刷新，三个实际加载标记仍须通过。普通日志补充固定适配结果与缺项标记，配置/备份格式及 Hub 简洁提示保留；当前行为见 [模型控制选项](dynamic-codex-models.zh-CN.md#初始化开启-maxultra-与-fast-选项)，本轮编译、源码与两平台构建身份见 [v5 交付](releases/v0.4.30-5.md#2026-10-19-早期挂载超时后的桌面适配补装)。版本仍 `0.4.30-5`、唯一 `main`，实机待用户验收；下方前轮结果保持各自源码身份。 修复源码 `2d24711` 已推送，Windows 必要编译与静态核对通过，两平台 Actions 已确认 job 开始并停止等待；包结果尚未确认。

2026-10-18 TC-006/TC-007 继续修复：用户确认前轮 Fast 标志已显示，ULTRA 仍缺；测试机配置页缺少“模型功能 / 工作空间依赖项”，增强启动反复提示未同步。当前补齐本 Hub 受管本机增强会话的两个设置入口，保留 Work、local host 与真实 primaryRuntime 条件；大脚本连接采用有限 64 MiB 帧/消息上限，并避免旧脚本缓存、及时反馈连接中断与实际缺项。配置/备份格式不变，Hub 仍只显示普通接入提示。现行使用、兼容和回退见 [模型控制选项](dynamic-codex-models.zh-CN.md#初始化开启-maxultra-与-fast-选项)，本轮源码、必要编译与 Windows/macOS 待验收见 [v5 交付](releases/v0.4.30-5.md#2026-10-18-配置区块与增强启动超时修复)。版本仍 `0.4.30-5`、唯一 `main`；前轮 Fast 有限反馈不代表 ULTRA、设置入口或本轮启动已验收。 修复源码 `ff935c7` 已推送，本轮 Windows/macOS Actions 已确认 job 开始并停止等待；包结果尚未确认。

2026-10-16 TC-014 已保存凭据回显：Dumpling-MCP 的密码与 ClientSecret 保存、登录后继续明文显示，加载/刷新/重启后从当前环境的系统保护存储回填已保存值；未保存输入只保留在本次表单。回填绑定环境、认证方式与完整 profile 指纹，当前非空输入优先，主认证方式切换仍清旧输入。普通配置/状态不含秘密，只有受现有实例管理授权保护的专用读取响应返回当前环境凭据，读取失败明确提示重新输入；不改变认证、自动续期或客户端接入。现行使用与接口边界见 [已保存凭据回显](customizations/nvwa-mcp.md#已保存密码与应用密钥回显)，本轮编译、源码、两平台 Actions 与待用户验收见 [v5 交付](releases/v0.4.30-5.md#2026-10-16-已保存密码与应用密钥回显)。版本仍 `0.4.30-5`、唯一 `main`，下方前轮构建保持原身份。

2026-10-16 TC-014 客户端接入恢复修复：Dumpling-MCP 按实际认证范围与真实三项身份恢复接入，不因环境更名、等价默认地址或保存凭据引用改变要求反复预览；更换/取消保存凭据及“取消登录”先挂起，真实同身份重新登录后恢复，显式退出/范围或身份改变仍撤销。保护快照 v2 支持范围校验与旧记录受限迁移，token 不可用和身份/本机凭据损坏分开反馈，客户端操作同步最新状态。现行规则见 [Dumpling-MCP](customizations/nvwa-mcp.md#有效期恢复与重新认证)，本轮源码、编译、两平台 Actions 及待用户验收见 [v5 交付](releases/v0.4.30-5.md#2026-10-16-认证到期后的客户端接入恢复)。版本仍 `0.4.30-5`、唯一 `main`，旧源码与包不作为本轮验证证据。

2026-10-12 TC-006/TC-007/TC-013 保存流程优化：普通/ChatGPT 厂商空模型列表直接保存时自动后台获取，非空结果成功后继续保存，失败或空结果弹窗且不保存；已有列表和单独获取仍保持原规则。客户端模型没有适用路由时返回中文配置指引，HTTP 422 及 `invalid_model_error/invalid_model` 保持兼容。现行使用与边界见 [动态模型](dynamic-codex-models.zh-CN.md#厂商保存时自动获取模型)、[入口与错误](customizations/client-channel-scope.md#模型未配置提示)、[编辑窗口](customizations/desktop-and-packaging.md#大模型接入编辑窗口)，本轮源码、编译、Actions 及待用户验收见 [v5 交付](releases/v0.4.30-5.md#2026-10-12-空模型自动获取与模型未配置提示)。版本仍 `0.4.30-5`、唯一 `main`；下方前轮构建及包不作为本轮验证证据。

2026-10-12 前轮 TC-014/TC-007（当时规则，保存/取消与快照规则已由上方 10 月 16 日修复更新）：Dumpling-MCP 手动重新登录经同环境、同真实身份核验后恢复已有接入，登录期间挂起且不使用旧 token；客户端区改为单一下拉，默认 ChatGPT(Codex)，另选 WorkBuddy/天工Claw，记录与权限仍隔离。大模型新增/编辑窗口采用滚动表单与固定保存/取消按钮，按可见窗口适配。现行行为见 [Dumpling-MCP](customizations/nvwa-mcp.md#有效期恢复与重新认证)、[编辑窗口](customizations/desktop-and-packaging.md#大模型接入编辑窗口)，本轮编译、源码、两平台 Actions 与待用户验收状态见 [v5 交付](releases/v0.4.30-5.md#2026-10-12-重新登录保留接入客户端选择与编辑窗口滚动)；版本仍 `0.4.30-5`、唯一 `main`，旧包不包含本次调整。

2026-10-11 TC-007 页签排列与图标：前三项依次为“ChatGPT(Codex) 接入”“WorkBuddy 接入”“天工Claw 接入”，其后为“大模型接入”“聊天工具接入”“请求日志”，可选 Dumpling-MCP 仍在最后、默认隐藏。三个客户端页签采用各自应用标识改制的透明线条图标，随主题文字色显示并内嵌程序；配置与现有显示开关兼容。当前行为见 [品牌、桌面与交付](customizations/desktop-and-packaging.md#客户端页签顺序与图标)，本轮源码、编译、Actions 与待用户验收状态见 [v5 交付](releases/v0.4.30-5.md#2026-10-11-客户端页签排序与应用线条图标)。下方记录保留各前轮范围，不作为本轮验证结果。

2026-10-11 TC-014 三客户端名称统一：Codex、WorkBuddy、天工 MCP 均使用保存环境名称，旧受管名称经各端预览确认迁移；文件两端定向双槽替换，天工沿用官方本机接口逐步核对，保留冲突与未确认操作保护。现行行为与原生限制见 [Dumpling-MCP 专题](customizations/nvwa-mcp.md)，本轮源码、验证和两平台构建见 [v5 交付](releases/v0.4.30-5.md)，历史包不含本次三端名称统一。 同轮兼容天工刷新工具时重复但相同的会话头；静态代码与工具列表 HTTP 404 现象吻合，具体效果待用户用新包验证。 本轮最终 Windows GUI/测试代码 check 18.86 秒与 GUI build 49.82 秒、EXE 静态身份核对通过；未运行测试或产品，macOS 构建与各端实机待验收。 本轮修复 `b3d704e` 已推送，两平台 Actions 已确认 job 开始；按要求停止等待完成，包结果和实机待确认，链接与源码身份见 v5 交付。

2026-10-11 TC-014 历史接入修复：已确认 Hub 受管记录仍在、客户端目标条目已缺失时，原逻辑把缺失当作外部修改，阻止重新接入。当前允许预览确认后按环境名重新添加 Codex 连接，保护已有修改、未确认操作和同名冲突；恢复以实际缺失前像为准。界面明确检查配置/检测本机桥的区别、重新登录后的接入步骤和按钮旁的失败原因。实现、兼容与回滚见 [Dumpling-MCP 专题](customizations/nvwa-mcp.md)，本轮验证及构建状态见 [v5 交付](releases/v0.4.30-5.md)；不将旧包作为本次修复验收。 本轮修复 `eae9b9b` 已推送，Windows/macOS Actions 已开始运行；按要求停止等待，包生成/核验及用户验收待确认，完整运行身份见 v5 交付。

2026-10-11 TC-007 安装升级修复：首次 main 两平台 Actions 已成功，但用户安装 v5 MSI 后仍是 v4。只读核对确认旧 MSI 语言标识 `0` 与新包升级限定 `1033` 不匹配；两版数字文件版本也相同。当前修复升级跨语言识别、EXE 数字修订号及同修订重打包覆盖，新增 Actions 产物静态检查；新装与升级均显示确认、进度及成功后的“打开 / 完成”按钮。修复 `4966b1f` 已推送，新 Windows Actions 已开始编译，按用户要求停止等待；修复包生成、核验和升级实机尚待完成，完整证据与运行链接见 [v5 交付](releases/v0.4.30-5.md)。首次 main 包保留原身份，不含本次修复。

2026-10-11 前一轮打包启动记录：`main` 的 `b05b489`（含 NVWA 自动重新认证修复）已启动 Windows/macOS 手动 Actions，两者核对时均为 `in_progress`，具体运行链接见 [v5 交付](releases/v0.4.30-5.md)。已复核此前 Windows 的 WiX 扩展参数错误并修正，macOS 原编译/打包成功但等待 Windows 更新清单失败，本次跳过该等待；工作流语法静态核对通过。本次仅上传 Actions 安装包附件，既有 v5 标签和 Release 不变，按用户要求确认开始后停止等待；新包生成、核验及实机验收尚未完成。下方 10 月 10 日记录保留当时交付状态。

当前追加修复（2026-10-10）：唯一 `main` 已完善 [NVWA 按需自动续认证](customizations/nvwa-mcp.md#有效期恢复与重新认证)，password/application 仅复用此前显式保存的凭据，统一真实 HTTP 401 失效处理、已发送请求不重放，并对同代次自动认证失败设置 60 秒重试间隔。此次已实现、未发布，完整版本保持 `0.4.30-5`，既有 v5 标签、Release 和历史包不变，不触发新发布构建；最终 Windows locked GUI build 32.62 秒、38 条既有警告通过，独立新 debug EXE 静态身份/复制哈希核对通过、未运行，见 [专题验证](customizations/nvwa-mcp.md#维护定位与验证)。macOS 平台中立逻辑仅静态审阅通过，未原生编译或实机测试，真实续认证及失败边界待用户验收。下方 v5 发布状态与有限用户反馈属于此前发布阶段，不覆盖此次追加修复。

维护日期：2026-10-10。当前开发基线统一为 `v0.4.30-5`、唯一 `main`，上游仍 CodexHub `v0.4.30`。[Dumpling-MCP / NVWA MCP（TC-014）](customizations/nvwa-mcp.md) 页签改名并使用透明线条品牌图标，帮助菜单勾选“显示 Dumpling-MCP”后显示，默认隐藏并保存偏好。Codex 连接使用环境名称（如 `MCP196`），更新接入兼容旧哈希名称；按钮有悬浮说明，工具清单展示服务返回的中文简介、说明和原代码。应用密钥立即保护保存、两类期限、认证/默认租户与每端隔离继续保留。用户已反馈通过 Hub 接入 Codex MCP，仅覆盖该次接入；本轮名称迁移、页签与工具展示待验收。构建、独立 EXE 身份及此前产物归属见 [v0.4.30-5 交付](releases/v0.4.30-5.md)，用户已反馈 Windows 本机测试版可交付；完整标签/非草稿 Pre-release 已创建，两平台 Actions 已启动、尚未确认完成，按用户要求停止等待，后续包自动上传。

同轮修正 [Windows 关机放行与安装成功入口](customizations/desktop-and-packaging.md)：普通 × 仍隐藏，系统结束会话不再 veto；MSI 完整交互安装成功页提供“打开 TianCaiSpaceHub”和“完成”。安装包生成/安装与关机实机行为待用户验收，发布包只由 GitHub Actions 生成。

此前最终 `v0.4.30-4` 产品、CI 修复和发布核验已整体并入当前基线；桌面能力识别、专用模型入口、IM 会话与审批、天工任务/历史/消息同步及全部既有二开继续保留。历史标签、包和用户反馈保持原身份，见 [v4 交付](releases/v0.4.30-4.md)，不作为新增 NVWA 功能的验收证据。Windows 优先、macOS 次之，不扩展 Linux；本地不生成发布安装包。

工作区已保存对齐前快照并恢复最终 `main`。本地与 live `origin` 当前仅有 `main`，没有其他待删除/合并分支；完整版本通过标签和 GitHub Releases 管理，不建立版本分支。详情见 [仓库清理记录](development/repository-cleanup.md)。

## 从哪里开始

| 目的 | 文档 |
| --- | --- |
| 了解全部二开内容、代码入口与保留项 | [二开功能总表](customizations/README.md) |
| 查看每次二开开发变更 | [二开变更记录](customizations/CHANGELOG.md) |
| 开发时知道必须更新哪些文档 | [文档规划与维护规范](development/documentation.md) |
| 登录 NVWA、检测 MCP、分别接入三客户端 | [NVWA MCP](customizations/nvwa-mcp.md)、[资料索引](nvwa/README.md) |
| 查看当前 v5 基线、编译与用户待验收内容 | [v0.4.30-5 发布交付](releases/v0.4.30-5.md) |
| 配置 WorkBuddy 模型、启动桌面及了解 `/wb` 边界 | [WorkBuddy 使用说明](workbuddy.md) |
| 区分普通渠道与 WorkBuddy/天工专用渠道 | [接入点用途与隔离](customizations/client-channel-scope.md) |
| 获取当前 GitHub 交付、两平台安装包与验收边界 | [v0.4.30-4 发布交付](releases/v0.4.30-4.md) |
| 查看此前已发布包及原版本身份 | [v0.4.30-3 发布交付](releases/v0.4.30-3.md) |
| 管理天工 Claw 多模型、参数、备份及上游流式兼容 | [天工 Claw 模型接入](customizations/gmclaw.md) |
| 启停 Hub/天工、选择完整项目目录、回复状态、会话同步与审批 | [天工外部消息执行端](customizations/gmclaw-im.md) |
| 区分天工模型配置、本地模型连接、IM 授权与上游流式 | [天工 Claw 模型接入](customizations/gmclaw.md)、[请求日志](ai-gateway-request-log-detail-patch.zh-CN.md) |
| 设置自定义模型与能力继承 | [动态 Codex 可见模型](dynamic-codex-models.zh-CN.md) |
| 从 Sub2API 网页导入渠道 | [网页导入](hub-external-import.md)、[两端接口约定](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) |
| 查看品牌、桌面行为和打包约定 | [品牌、桌面与交付](customizations/desktop-and-packaging.md) |
| 了解最新上游合并及待验收项 | [v0.4.30 整合记录](upstream-v0.4.30-integration.md) |
| 查看 macOS 网页导入的历史开发状态 | [0.4.29-5 开发记录](releases/v0.4.29-5.md)，当前交付见 [v0.4.30-3](releases/v0.4.30-3.md) |
| 获取上一版 Windows 导入流程调整的本地包信息 | [v0.4.29-4 Windows 交付版](releases/v0.4.29-4.md) |
| 查看前一版本 HTTP 导入修复及验收记录 | [v0.4.29-3 Windows 修复版](releases/v0.4.29-3.md) |
| 追溯早期 GitHub 版本交付信息 | [v0.4.29-2 Windows 历史预发布](releases/v0.4.29-2.md) |
| 查看过时文档及分支清理结果 | [仓库清理记录](development/repository-cleanup.md) |

二开总表描述当前实现，变更记录描述版本演进，专题文档保存细节。下方技术资料同时包含上游已有实现、历史方案和调试记录；文档标题包含“计划”不代表已经实现。发生冲突时，先核对当前代码和用户最新决定，再修正文档，不能用旧方案覆盖现行行为。

当前平台重点为 Windows，macOS 次之，Linux 不纳入新增需求。当前开发与测试分工见 [维护规范](development/documentation.md)；构建成功和历史测试通过不能替代本次用户验收。

## 当前二开专题

| 文档 | 用途与状态 |
| --- | --- |
| [Dumpling-MCP / NVWA MCP](customizations/nvwa-mcp.md) | TC-014 / v0.4.30-5：保存/登录后明文保留密码与 ClientSecret，加载/刷新/重启后专用受保护接口回填当前环境已保存值，绑定完整 profile 且保留当前非空输入；普通配置/状态不返秘密。同范围/真实身份恢复接入、保护快照 v2、三端环境名、定向配置保护、工具说明和按需续认证保留。本轮验证/构建见 v5 交付，三端及 macOS 实机待用户验收；此前包保持原身份 |
| [WorkBuddy](workbuddy.md) | 模型多条目接入已预发布；v0.4.30-3：接入页提供显式启动桌面按钮，用户本次反馈该按钮可启动，仅覆盖该次本机操作；其他机器及 macOS 待验收。`/wb` 仍提示不可用，外部任务执行尚未接通 |
| [接入点用途与隔离](customizations/client-channel-scope.md) | 普通/专用渠道标识、各接入点及同模型条目的精确路由、模型未配置中文指引、保留名称与回滚注意项 |
| [天工 Claw 模型](customizations/gmclaw.md) | 多条目与独立路由、保存/删除/默认切换恢复、厂商参数及上游流式聚合已预发布；v0.4.30-3：概览按当前专用模型请求、配置 URL 和桌面进程区分等待/已连接，与上游成功和 IM 授权独立。旧单模型保存无需重启已有有限反馈；新增行为待用户验收 |
| [天工外部消息执行端](customizations/gmclaw-im.md) | v0.4.30-3：Windows 监听/启动禁止句柄继承，退出只结束自有后台，旧继承实例需一次正常迁移；目录候选含无任务的原生项目并完整分页，飞书/有效企微回调显示临时准备状态并终态收尾、微信直接文字；启用后每 5 秒后台恢复同用户官方天工运行授权，不自行启动或重放；复用原平台会话交互，创建真实任务、保存消息及恢复全部场景任务。原记忆、精确模型、认领及运行/审批/未知保护保留，原生步数 `1..1000` 透传。`/gpt` 释放认领但保留会话，`/tg` 返回重新认领；`/s` 不伪造取消。本轮修复图标注册导致的安装误判，移除每轮固定等待回复，并加入局部消息同步；v0.4.30-4 标签源码取消固定版本/资源门槛，改核对已安装天工身份、renderer HTML 脚本入口及运行时字段能力；首次从 Hub 启动启用本机通道，普通手动实例仍可 IM 但无通道不能即时刷新。用户已有限反馈自动重连、飞书历史对话及 `fe48829e…` 桌面消息更新；用户已确认 `71c3043d…` 审批测试通过，仅属于该次操作，其他平台、失败回退和身份隔离待验收；本轮修复端口释放、补齐含无任务项目的目录候选，并新增按平台能力收尾的临时回复状态，这些新增行为待验收，不新增永久或单工具授权。原 `/gmclaw` 首版已预发布，本轮状态见品牌专题，开发方未执行测试 |
| [请求日志详情](ai-gateway-request-log-detail-patch.zh-CN.md) | v0.4.30-3：保留 `stream` 的客户端语义，新增可空 `upstreamStream` 记录实际最终上游请求；天工非流式客户端与上游流式聚合分别展示，旧日志上游模式未知 |
| [OpenAI Chat Completions](openai-chat-completions.md) | 通用兼容渠道与按渠道关闭推理；已实现 |
| [动态模型](dynamic-codex-models.zh-CN.md) | 手填、远端获取、路由补齐和能力继承；v5 普通/ChatGPT 厂商空列表保存先自动获取，成功继续保存、失败留窗，客户端未配置模型中文指引；本轮状态见 v5 交付，实机待用户验收 |
| [网页导入](hub-external-import.md) | Windows/macOS 系统接入已随 v0.4.30-2 成功构建打包并预发布；本版实机导入待用户验收 |
| [导入协议 v1](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) | Hub 与 Sub2API 联调契约；不表示主站已上线 |
| [品牌、桌面与交付](customizations/desktop-and-packaging.md) | 产品与程序统一 TianCaiSpaceHub，配置/升级身份兼容；v0.4.30-5：系统关机放行、普通 × 隐藏托盘，Windows MSI 完整交互成功页提供“打开”和“完成”。Windows EXE 编译与静态核对通过，关机/安装待验收、包由 Actions 生成 |
| [v0.4.28 整合](upstream-v0.4.28-integration.md) | 历史上游整合记录 |
| [v0.4.29 整合](upstream-v0.4.29-integration.md) | 历史上游整合记录 |
| [v0.4.30 整合](upstream-v0.4.30-integration.md) | 当前上游基线、12 项二开保留矩阵、冲突取舍和验收边界 |

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
