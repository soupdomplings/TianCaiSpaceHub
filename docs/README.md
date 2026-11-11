# TianCaiSpaceHub 文档总入口

2026-11-11 本批追加：[统一操作反馈](development/operation-feedback.md) 将操作结果集中为右上小浮窗并取消关闭到托盘提醒；[搭档用量查询](customizations/assistant-usage.md) 复用当前账户扣费、Token 和本机请求能力。[三套性格草案](customizations/genius-buddy-personalities-draft.md) 保留干练搭档、软萌猫娘、温柔女友，允许自然使用少量 emoji，供用户编辑审核，暂未接入产品。Windows GUI/release、31 项 Rust、10 项 Node 和唯一 MSI `210620` 核验通过，源提交 `8c3b3ed` 已推送；包身份及实机待验范围见 [V25 交付](releases/v1.0.25.md)。

2026-11-11 **V1.0.25 用户反馈修复**：ZCode 启动探针处理资源计时条目缺失时的 HTTP 409；同批处理搭档回复裁切、Ctrl+Enter 发送残留、可见滚动条，以及助手保存后的模型/渠道刷新和专用客户端模型名称显示。实现见 [客户端](customizations/desktop-client-adapters.md)、[搭档](customizations/genius-buddy.md) 和 [视觉](development/desktop-visual-system.md)；本批检查、唯一新包和实机待验范围见 [V25 交付](releases/v1.0.25.md)。下列历史结果保留原版本归属。

2026-11-11 **V1.0.24 统一集成**：搭档滚轮、Markdown、真实模型解析和保存回读；Codex 模型页同步、五端全展示及轻量动画；小分队统一弹窗和多模型/多客户端导入；ZCode 外部模型免登录启动与天工可见错误反馈。实现见 [搭档](customizations/genius-buddy.md)、[视觉](development/desktop-visual-system.md)、[导入](hub-external-import.md)、[账户](customizations/tiancai-squad-account.md) 和 [客户端](customizations/desktop-client-adapters.md)。最终检查、唯一新包及验收边界以 [V24 交付](releases/v1.0.24.md) 为准，下列历史状态保留原归属。 Windows GUI/release、45 项不同 Rust、7 项 Node 和真实 Server 5 轮验收通过；唯一 MSI `193313` 已生成并完成静态核验，用户实机和 macOS 仍待验收。

2026-11-11 **V1.0.23 搭档配置流水线与界面精简**：修正 Windows 原生业务页遮挡浮层，补充助手操作指引及参数回读；隐藏 WebStorm/VS Code，增加 MiniMax 厂商预设。GUI编译及57项定向回归通过，原真实模型7轮配置流程和屏幕夹具沿原输入复用；新增密钥创建/列表选择→渠道→临时ZCode指定模型的真实3轮流程通过，测试密钥已撤销。Windows release与唯一MSI `181709`已生成，版本/升级及六项载荷核验通过，源码已推送GitHub `main`（`b05775a`）。身份及验收边界见 [V23交付](releases/v1.0.23.md)，行为见 [搭档](customizations/genius-buddy.md)、[账户](customizations/tiancai-squad-account.md) 与 [客户端](customizations/desktop-client-adapters.md)。

2026-11-11 **V1.0.22 天才搭档对话修复与 Hub 内浮层**：修复新版 Server 拒绝旧 `readOnly.access` 参数的问题，配置、能力目录和会话请求复用 Hub Codex 流程，模型仍来自天才小分队。右下角纯对话图标，框内去产品标题，消息可复制、收起可续聊。Windows GUI 测试目标及 release 编译、46 个不同定向离线用例通过，唯一 MSI `152228` 的版本/升级/六项载荷核验通过。实现见 [搭档专题](customizations/genius-buddy.md)、[桌面视觉](development/desktop-visual-system.md)，包身份和验证边界见 [V22 交付](releases/v1.0.22.md)；真实界面、调用与 macOS 待用户验收。

2026-11-11 **V1.0.21 天工增强启动与搭档 Server 修复**：使用 Hub 已保存的外部模型直接初始化天工本地工作区，保留任务恢复及真实官方登录状态；接入设置保留普通启动。搭档 Server 补桌面更新后的来源发现与新版 npm 布局，修复缺少运行端时的单次安装提醒，取消不下载。当前行为见 [天工专题](customizations/gmclaw.md#外部模型免登录增强启动v1021)、[搭档专题](customizations/genius-buddy.md)，构建及唯一测试包见 [V21 交付](releases/v1.0.21.md)。用户已确认 V1.0.20 DeepSeek 问题解决；其他客户端保持原配置路径，不能据此扩大为全部实机通过。

2026-11-11 **V1.0.20 统一测试交付**：Harness 模型不可见与正常设置迁移误报“待核对”修正，客户端及账户常用操作精简多余进度弹层；沿用本批 14 项不同离线用例及 GUI 测试目标编译，Windows release、唯一 MSI 和静态核验结果见 [V20 交付](releases/v1.0.20.md)。实现见 [模型兼容](customizations/desktop-client-adapters.md#harness-模型不可见与待核对修正未发布)、[操作反馈](development/desktop-visual-system.md)、[变更记录](customizations/CHANGELOG.md)。真实界面、Harness 加载及 macOS 待用户验收；天工免登录增强启动仅评估，未实施，历史包保持原身份。

2026-11-11 当前 **V1.0.19 四端启动修复测试交付**：Harness 已打开状态与自定义目录启动修正，VS Code 复用便携/版本/Scoop 发现，ZCode/WebStorm 补已登记安装目录；模型导入后 Harness 直接启动，无需额外登录。离线编译及 32 个不同定向用例通过，Windows release 与唯一新 MSI `113557` 已生成，版本、升级及六项载荷核验通过，见 [V19 交付](releases/v1.0.19.md)；实现与兼容见 [客户端适配](customizations/desktop-client-adapters.md#deepseek-harness-状态与四端启动修正)。保留 V1.0.18 累计功能和原包，真实客户端与 macOS 待用户验收；下文旧交付保持各自归属。

2026-11-11 当前开发 **天才搭档与共享 Server（TC-023）**：新增独立助手模块和右下角对话入口，天工协助共用“天才搭档 Server”，缺少运行包先确认再准备；两项都停用后可手动移除 Hub 自管安装。通用 Skill/MCP 管理入口隐藏，Dumpling-MCP 独立；临时会话在真正退出时清理。小分队按最新决定只新增 [单 GET 配置接口](contracts/assistant-server.md)，返回普通模型 API 与能力，工具和摘要内置于本机。本批同时修正新版 ChatGPT 的数字资源 Ultra/Fast、代理继承与可信路径识别，见 [修复中心审计](customizations/codex-repair-center.md)。最终 Windows GUI 测试目标编译、77 个不同 Rust 定向用例、28 项 Node 和 54 次当前包候选语法检查通过，具体归属见 [本批集成](customizations/genius-buddy.md#本批集成记录2026-11-11)。本批 V1.0.18 Windows release 与唯一 MSI 已生成，版本、升级和内嵌载荷核验通过，包身份见 [V18 交付](releases/v1.0.18.md)，真实调用及 macOS 待验收。以下旧版交付记录保留各自归属。

2026-11-10 当前 **V1.0.17 统一 Windows 测试交付**：缺少 CLI 时首次启用自动准备官方完整运行包并持久复用，协助后台增加可选完全访问，概览与日志统一显示“协助处理 MCP”。104 项定向离线回归、两个受影响示例编译、Windows release 构建及唯一 MSI 静态核验通过；浅深设置两图已人工查看。唯一新包 `232744`、源码身份及待用户验收范围见 [本批交付](releases/v1.0.17.md)。首次真实下载、协助权限/图谱及 macOS 待用户验收，本批未提交、推送或发布；下方 V1.0.16 及更早状态保留原阶段归属。

2026-11-10 当前 **V1.0.16 统一 Windows 测试交付**：集中汇总顶部菜单/主题日历/密钥实际用量，以及 Codex CLI 启动与 MCP 升级、移除同步修复。唯一新包 `220951` 已生成，1354 个不同 Rust 用例及 14 项 Node 检查通过，受影响 4 项补跑替代同名结果，不重复计数；release 构建及静态包核验通过，准确包身份与验收边界见 [本批交付](releases/v1.0.16.md)。长期采用 [统一会话协作与测试打包](development/documentation.md#统一会话协作与测试打包)，根 [AGENTS.md](../AGENTS.md) 按管理/开发/打包职责分工。累计有效开发及本轮文件整理已按授权提交并推送至 GitHub `main`（`4f3e3a0`），详见 [清理记录](development/repository-cleanup.md)。旧包保留，真实安装待用户验收；本轮未触发 Actions、标签或正式发布。下方各阶段检查和“未提交”等状态保留当时事实。

2026-11-10 当前 **V1.0.16 协助启动及MCP管理修正**：[本机协助](customizations/tiangong-local-assistance.md)明确CLI执行端、自动查找/商店包运行缓存及系统失败恢复；工具目录独立定时更新、删除/停用同步；[资源管理](customizations/desktop-capability-management.md)补端内移除和缺失使用记录清理。本任务开发就绪，GUI测试目标编译及68项定向离线检查通过；汇总测试包由统一打包会话核对共同源码后生成，本会话未独立打包，见[V16记录](releases/v1.0.16.md)。保留下方界面/账户增补和全部未提交开发，真实启动/同步及macOS待用户验收。

2026-11-10 当前 **V1.0.16 界面与密钥统计增补**：移除旧顶部菜单，诊断在帮助；日期改为主题日历浮层；API密钥读取站点今日/近30天已用及独立限额，人民币数字原样。见 [视觉](development/desktop-visual-system.md)、[账户](customizations/tiancai-squad-account.md)、[开发交接](releases/v1.0.16.md#本任务离线验证与统一打包交接)。本任务开发就绪、固定快照离线检查通过，由统一打包会话汇总；本会话未生成独立MSI。保留本机协助与全部未提交开发；真实账户/浮层/缩放及macOS待用户验收。

2026-11-10 当前 **V1.0.16 本机协助参数修正**：[天工本机协助](customizations/tiangong-local-assistance.md)（TC-022）补齐真实任务上下文、完整MCP目录及统一调用示例，针对用户缺少 `context_ref` 的实测反馈修正；新增按模型选择思考强度，支持时默认 `xhigh`。默认关闭、只读、自动清理及进度/人类确认回天工保持。构建/离线/新包身份见 [V16交付](releases/v1.0.16.md)，真实端到端和macOS待用户验收；保留前轮包，未提交、推送或发布。下方评估记录为历史。

2026-11-10（北京时间）：新增 [天工调用本机 Codex 协助评估](customizations/tiangong-local-assistance-assessment.md)。按用户反馈，首期即以处理过程中自动协助为正常路径，不要求点名 Codex：Hub MCP + 自动生效协作规则 + 本机执行通道；核对每次 MCP 新建连接、上下文及长任务/审批边界。**纯方案，未实现、不升版本、不改客户端或现有测试包**。

2026-11-10（北京时间）当前 **V1.0.16 参数及模型清单修正**：天工来源参数、可选个人登录、用户主动身份测试和有限 Codex unary 文本压缩；ChatGPT 添加/获取统一候选区、删除后保存及再次分享恢复原别名。实现见 [天工专题](customizations/tiangong-builtin-models.md)、[动态目录](dynamic-codex-models.zh-CN.md)，最终构建/离线/新包身份见 [V16 交付](releases/v1.0.16.md)。前两轮包保留原身份，真实调用/压缩及客户端/macOS 待用户验收，未推送或发布。

2026-11-10（UTC）：前次 **V1.0.16 天工登录包** 增加已有登录读取、Hub 登录及两个内置模型跨端分享，普通页面不再要求个人 PAT；当时仍有独立参数及额外登录核验行为，本轮已按上段修正。保留本版 Windows 一体化外框、移除 Hub 模型广场和其余开发。入口：[原交付身份](releases/v1.0.16.md#前次增补天工登录与内置模型分享)、[天工现行行为](customizations/tiangong-builtin-models.md)、[窗口视觉规范](development/desktop-visual-system.md)、[账户范围](customizations/tiancai-squad-account.md)。旧包验证保留原归属，不代表本轮已验收。

2026-11-10（UTC）：新增 **V1.0.15** 天工内置模型发现与跨端授权适配。使用步骤、个人 PAT 前提与边界见 [天工内置模型](customizations/tiangong-builtin-models.md)，本轮验证和包身份见 [V1.0.15 交付](releases/v1.0.15.md)。真实授权/调用及客户端、macOS 待用户验收；未提交、推送或发布。下方 V1.0.14 记录保留原阶段归属。


2026-11-10（UTC）：上一轮修复版本 **V1.0.14**。修正统计选中、模型新建默认强度/流式偏好、小分队原生日历和列表留白，新增广场价格对比并重排充值/订阅。保留全部既有开发。规范见 [桌面视觉规范](development/desktop-visual-system.md)，实现、验证及本地 MSI 身份见 [V1.0.14 交付](releases/v1.0.14.md)。**真实客户端、账户/支付、Windows 实际缩放和 macOS 待用户验收；尚未提交、推送或发布。**

- [V1.0.14 本地测试交付](releases/v1.0.14.md) · [桌面视觉规范](development/desktop-visual-system.md)。
- [V1.0.13 本地测试交付](releases/v1.0.13.md)：上一轮视觉系统及原版本包身份，验证结果保留原归属。
- [V1.0.12 本地测试交付](releases/v1.0.12.md) · [客户端配置核对与剩余边界](customizations/client-configuration-audit.md)。
- [V1.0.11 本地测试交付](releases/v1.0.11.md)：筛选、广场、按端模型表单、登录恢复及原版本验证/包身份。
- [V1 桌面使用与验收指南](v1-desktop-guide.zh-CN.md)：新导航、运行/用量、七端模型复用、真实 Skill/MCP、账户和测试顺序。
- [V1 实施记录](development/v1-implementation.md)：代码、接口、配置兼容、回滚、验证边界及已知限制。
- [二开功能总表](customizations/README.md) · [变更记录](customizations/CHANGELOG.md) · [文档规范](development/documentation.md)。
- [客户端适配](customizations/desktop-client-adapters.md) · [模型/Skill/MCP](customizations/desktop-capability-management.md) · [天才小分队账户](customizations/tiancai-squad-account.md)。
- [Skill 广场与共用仓库](customizations/skill-marketplace.md)：真实公共仓库、固定提交完整包、使用客户端、容量和回退规则。
- [界面功能盘点](customizations/desktop-ui-inventory.md) · [原型与研究](customizations/desktop-ui-redesign.md) · [主题](gui-theme.zh-CN.md) · [运行维护](gui-runtime-maintenance.zh-CN.md)。
- [第三方参考及许可](../THIRD_PARTY_NOTICES.md)。站点保持已部署状态，Hub 原生界面不需部署网站；新增默认浏览器接续登录需现有站点应用 [配套补丁](../integrations/tiancai-squad-browser/README.md)，本轮尚未部署。

## 前期原型及版本记录

以下记录保留其原阶段身份；其中旧路线、未实施表述和合成验证不代表 V1 当前状态。现行入口和真实实现以上述文档为准。

2026-11-08 用户确认按最终方案开始 **V1.0.0 整体实施**，版本基线改为 `1.0.0`，覆盖此前下一功能版 `-14` 的安排。当前实施进度、完整范围和验证边界统一见 [V1.0.0 实施记录](development/v1-implementation.md)。保留 v13 未提交开发；本轮不推送或发布，整体完成后由用户安装验收。天才小分队只开发 Hub 原生账户模块并使用现有 API，**不修改或重新部署站点**；先前评估中的站点 Hub 布局路线不再执行。下方原型和历史验证保留其原阶段归属。

2026-11-08 桌面整体重构评估（**第三稿合成原型与适配研究，未实施到产品、不升版本**）：基于 v13 未提交 main 工作区，推荐沿用 Rust + vendor wxDragon 分阶段实施。入口：[整体方案](customizations/desktop-ui-redesign.md)、[全界面盘点](customizations/desktop-ui-inventory.md)、[交互原型及 43 组第三稿布局图](mockups/desktop-ui-review/README.md)。

新增一级 [天才小分队](customizations/tiancai-squad-account.md)：已核对用户提供的本机站点源码，原型含登录、账户/密钥/使用记录、条件组织与扣费、模型广场、充值/订阅及账户工具；六项组织权限、独立开关与精简规则沿站点。API 密钥复用原 Hub 单次导入，默认不启用、不追加模型；站点账单与本机统计分开。推荐局部 wxWebView 复用站点原页并新增共享布局，不迁移整个 Hub。真实登录/组织/财务及两平台承载尚未实施。证据与检查在 `target/ui-squad-account-20261108/`。

运行状态默认第一，统计只列输入/输出并保留实际日志；活动图谱为客户端 → Hub → 进行中模型 → 对应渠道，无活动端仅 Hub。按完整路由先列请求集合，渠道直达对应配置。客户端标题短状态与六节点概览图整合模型、Skill、MCP、聊天与执行，证据仍各自判断。

模型添加集中在客户端页；模型、Skill、MCP 使用操作直接配置到选定端，普通界面只呈使用选择，异常才给决策与恢复；原型仅改内存。各端独立调整和来源选择同步保持，添加来源按当前八类厂商，聊天入口统一叫聊天接入。详见 [管理规格](customizations/desktop-capability-management.md)。新 [Magpie 客户端及外部接口研究](customizations/desktop-client-adapters.md) 核对 74 名声明范围、重点端的原生配置、协议缺口、Rust 适配合同与回归清单，不冒充 Hub 新增支持。

本轮按用户继续细化：默认三端位置固定，访问已列端不重排，新增端只替换最久未用位置；“⋯”在第三端下方，展开后标题右侧收起。客户端选择为每行 2–4 个小图标，最大宽 124/最小高 84 CSS px；成功才点亮且使用端排前，失败保留旧状态/顺序，网格排序不改变左栏。运行图整行列标题移除。首批按用户指定研究 Cursor、VS Code、DeepSeek Harness、ZCode、WorkBuddy，详见 [适配规格](customizations/desktop-client-adapters.md#5-首批五类客户端的研究与实施建议)。

本轮 125 组合成检查（原 107 + 账号 18）、138 组原页面、20 组原新增表单、117 组账号页面/子页及原 29 个帮助上下文加 1 个模块帮助通过；静态资源/链接、原开发字节及日志保护已核对。第三稿 43 组、第二稿历史 7 组独立图，本轮逐一查看账号模块新增 9 组图；前轮 MCP 结果保留原归属。自动浏览器前轮被策略阻止后停止，HTML/原生/真实客户端与用户验收未执行；未改 v13、配置或包，未构建、提交、推送或发布。

本轮补充：2026-11-08 用户提供天工 Work 与 ChatGPT 截图后，MCP 添加改为默认完整“表单配置”与可编辑“JSON 配置”；远程地址/认证/Headers、本地命令/逐行参数/环境变量/传递变量/工作目录直接显示，两视图共用草稿，JSON 多项明确选择、错误与附加字段保留。目标客户端同窗选择后一次添加并使用，已有配置复用和 Dumpling 环境/NVWA 流程保持。前两轮用途分类及单粘贴框已替代。详见 [管理规格](customizations/desktop-capability-management.md)。验证及原开发保护记 `target/ui-mcp-form-json-20261108/`，仍未实施到产品。

2026-11-07 `v0.4.30-13` 本地测试开发：普通模型地址先请求原路径，失败后同源补 `/v1` 一次，确认有效成功才保存，双失败保留原错；已确认修正保留路由与工具续轮身份。WorkBuddy 为 GPT-6、GPT-5.6 家族补齐 `max` 选择、保存与转发。当前规则见 [地址修正](customizations/provider-url-recovery.md)、[WorkBuddy](workbuddy.md)，实际隔离验证、构建和包身份见 [v13 交付](releases/v0.4.30-13.md)；未推送、未发布，待用户实机验收，以下记录保留原版本归属。

2026-11-02 `v0.4.30-12` 用户反馈测试成功且刚才报错解决；源码已按授权推送至 GitHub 唯一 `main`（`5466ec2`），Windows/macOS Actions job 均已实际开始，核对时 `in_progress`。347 个产品输入与用户测试包一致，版本和功能不变；运行链接及反馈边界集中见 [v12 交付](releases/v0.4.30-12.md#2026-11-02-源码已推送与两平台-actions-打包)。本轮产物上传 Actions artifacts，未创建标签/Release；不扩大为全部工具或 macOS 验收，下方保留原阶段。

2026-11-02 `v0.4.30-12` 本地测试交付：Dumpling-MCP 移除手工检查、刷新、单独上游检测及退出登录入口，配置由“连接并检测”自动查找核对；选择框缩短、主按钮紧邻，原“更多操作”改为仅三项维护的小型“⋯”菜单。断开单端连接与删除整套环境资料的范围分别说明。42 项离线回归、Windows 构建与 MSI 静态核验通过，准确包身份见 [v12 交付](releases/v0.4.30-12.md)，当前操作与回滚见 [Dumpling-MCP](customizations/nvwa-mcp.md)。本轮未推送、未发布，实机待用户验收；下方记录保留所属版本。

2026-11-02 `v0.4.30-11` 本地测试交付：Dumpling-MCP 日常入口收敛为“保存环境 → 登录 / 授权 → 连接并检测”，配置、诊断和管理收进默认折叠的“更多操作”。连接时自动核对所选客户端的真实设置，缺少接入或需要修复时显示一次确认，先备份再保存并继续检测；兼容设置保留，明确冲突仍停止。42 项离线回归、Windows 构建及 MSI 静态核验通过。现行行为见 [Dumpling-MCP](customizations/nvwa-mcp.md)，包身份与验收状态见 [v11 交付](releases/v0.4.30-11.md)。本轮未推送、未发布，用户实机待验收；下面记录保留原版本和阶段。

2026-11-02 `v0.4.30-10` 源码已按用户授权推送至 GitHub 唯一 `main`（`7c78350`），Windows x64、macOS universal Actions job 均已实际开始，核对时 `in_progress`。用户本地试用反馈“可以了”，344 个构建输入与测试包清单一致，代码和版本不变；运行链接与实际状态见 [v10 交付](releases/v0.4.30-10.md#2026-11-02-源码已推送与两平台-actions-打包)。本轮上传 Actions artifacts，反馈不扩展为全部功能或 macOS 实机验收；下面的未推送/未发布记录保留当时阶段。

2026-11-01 `v0.4.30-10` 本地测试交付：Dumpling-MCP 检测结果改为模块/工具数量与少量用途摘要，完整 schema 不回传 GUI；目录读取达到容量上限时区分“已连通、目录未完整读取”和真实认证/协议失败。继承 v9 历史恢复，未修改本机授权生命周期。30 项离线回归、Windows 构建和 MSI 静态核验通过，准确包身份见 [v10 交付](releases/v0.4.30-10.md)，当前未推送/未发布，实机待用户验收；现行行为见 [Dumpling-MCP](customizations/nvwa-mcp.md)。

2026-10-29 `v0.4.30-9` 本地测试开发：已实现 Codex 新版 Fast 认证结构适配、打开具体历史会话时的执行配置等待，以及部分历史加载失败后的分页重试。增强脚本 v25，不在启动时遍历历史；持续 `EBADF` 写入异常的根因仍待核对。Windows 构建与本地 MSI 状态见 [v9 交付](releases/v0.4.30-9.md)，当前未推送/未发布、待用户安装验收。现行行为与回滚见 [历史恢复](customizations/codex-history-recovery.md)。

2026-10-27 `v0.4.30-8` 已发布源码与 Pre-release：用户初步反馈本地包“暂时没问题”后授权发布，产品源码 `f322634` 已推送到唯一 `main`，完整标签和非草稿 Release 已创建；Windows x64、macOS universal Actions 均已确认 job 开始，按要求停止等待。正式包由 Actions 完成后自动上传，本地 debug MSI 不作为正式分发包；构建结果、安装包及 macOS/完整实机验收仍需分别确认。准确源码和运行链接见 [v8 交付](releases/v0.4.30-8.md#2026-10-27-源码与-release-已发布两平台-actions-已开始)。

2026-10-22 本地测试阶段 `v0.4.30-8`：已实现天工 Work 主运行收尾/审批、任务模型绑定、默认流式上下文与后台运行/重连保护；WorkBuddy、天工 Work 的页面字段、说明及按钮统一，思考强度并入连接卡片，三页只读/编辑底色统一。Hub 不再提供天工默认模型设置，原配置、渠道及 `/tg` 保留。Windows 编译、162 个不同 Rust 聚焦用例及本地 MSI 静态核验已通过；当时源码未推送、未发布，详见 [v8 交付](releases/v0.4.30-8.md)、[Work 适配](customizations/tiangong-work-compatibility-assessment.md)。

2026-10-20 当前交付：`v0.4.30-7` 的分类修复中心、增强启动 v24 和程序路径修复已提交并推送到 GitHub 唯一 `main`（`3b96fa6`）；Windows x64、macOS universal Actions 均已确认 job 开始，按用户要求停止等待完成。运行链接、源码身份和打包边界见 [v7 两平台打包](releases/v0.4.30-7.md#2026-10-20-源码已推送与两平台-actions-已开始)。启动修复分别报告 `CODEX_CLI_PATH` 与状态库，无迁移冲突仍可修路径，Hub 保持运行；使用与恢复见 [启动修复](customizations/codex-startup-repair.md)、[修复中心](customizations/codex-repair-center.md)。本轮仅生成 Actions artifacts，包结果和两平台实机待确认；[v6 测试包](releases/v0.4.30-6.md) 与 v7 本地包保留原身份，不当作正式发布结果。

以下 v7 及更早阶段的状态按当时事实保留，不代替 v8 本地测试交付。

2026-10-20 最新交付：按用户要求，组织设置修复按钮、原生迁移保护及 Commands 对话模式修复已提交到 GitHub 唯一 `main`（`500574c`），版本仍 `0.4.30-5`。Windows x64 与 macOS universal Actions 均已确认 job 开始，按既有要求停止等待；运行链接和准确源码身份见 [本轮打包](releases/v0.4.30-5.md#2026-10-20-启动修复源码提交与两平台打包已启动)。Hub 内置按钮保持 Hub 打开，仅要求关闭 ChatGPT/Codex，自动定位、备份和验证已知冲突，无需解压、终端或 Python；用法与恢复见 [启动修复](customizations/codex-startup-repair.md)。本轮只生成 Actions artifacts，构建结果/正式新包及实机尚待确认；此前本地包、独立脚本和未推送记录保留当时身份，不代表已发布或验收。

2026-10-20 当前追加（TC-006/TC-007，未推送）：初始化、更新配置和每次增强启动前，确保 `[desktop].conversationDetailMode="STEPS_COMMANDS"`；缺失或其他字符串先备份再修正，正确值的启动预检查不重写，运行中的旧模式要求先退出。受管恢复记录原值并保留用户后改，不提供 Work 浏览器身份。使用、兼容及回滚见 [对话模式](dynamic-codex-models.zh-CN.md#初始化与增强启动固定-codex-对话模式)，本轮编译与本地测试包见 [交付](releases/v0.4.30-5.md#2026-10-20-初始化与增强启动修正对话模式)；Windows/macOS 实机待用户验收，下方已启动 Actions 不包含本轮修改。

2026-10-20 最新打包状态：按用户后续要求，`main` 的 `645f6fd` 已触发 Windows 和 macOS universal Actions，两平台 job 均已开始；按既有要求停止等待。运行链接、源码和边界见 [本轮打包](releases/v0.4.30-5.md#2026-10-20-最新-main-两平台-actions-打包已启动)。版本仍 `0.4.30-5`，仅生成 Actions artifacts；包结果及实机待确认，下方未启动 Actions 等表述保留源码提交阶段的事实。

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

2026-10-10 历史交付记录：当时开发基线统一为 `v0.4.30-5`、唯一 `main`，上游仍 CodexHub `v0.4.30`。[Dumpling-MCP / NVWA MCP（TC-014）](customizations/nvwa-mcp.md) 页签改名并使用透明线条品牌图标，帮助菜单勾选“显示 Dumpling-MCP”后显示，默认隐藏并保存偏好。Codex 连接使用环境名称（如 `MCP196`），更新接入兼容旧哈希名称；按钮有悬浮说明，工具清单展示服务返回的中文简介、说明和原代码。应用密钥立即保护保存、两类期限、认证/默认租户与每端隔离继续保留。用户已反馈通过 Hub 接入 Codex MCP，仅覆盖该次接入；本轮名称迁移、页签与工具展示待验收。构建、独立 EXE 身份及此前产物归属见 [v0.4.30-5 交付](releases/v0.4.30-5.md)，用户已反馈 Windows 本机测试版可交付；完整标签/非草稿 Pre-release 已创建，两平台 Actions 已启动、尚未确认完成，按用户要求停止等待，后续包自动上传。

同轮修正 [Windows 关机放行与安装成功入口](customizations/desktop-and-packaging.md)：普通 × 仍隐藏，系统结束会话不再 veto；MSI 完整交互安装成功页提供“打开 TianCaiSpaceHub”和“完成”。安装包生成/安装与关机实机行为待用户验收，发布包只由 GitHub Actions 生成。

此前最终 `v0.4.30-4` 产品、CI 修复和发布核验已整体并入当前基线；桌面能力识别、专用模型入口、IM 会话与审批、天工任务/历史/消息同步及全部既有二开继续保留。历史标签、包和用户反馈保持原身份，见 [v4 交付](releases/v0.4.30-4.md)，不作为新增 NVWA 功能的验收证据。Windows 优先、macOS 次之，不扩展 Linux；本地不生成发布安装包。

工作区已保存对齐前快照并恢复最终 `main`。本地与 live `origin` 当前仅有 `main`，没有其他待删除/合并分支；完整版本通过标签和 GitHub Releases 管理，不建立版本分支。详情见 [仓库清理记录](development/repository-cleanup.md)。

## 从哪里开始

| 目的 | 文档 |
| --- | --- |
| 了解全部二开内容、代码入口与保留项 | [二开功能总表](customizations/README.md) |
| 查看每次二开开发变更 | [二开变更记录](customizations/CHANGELOG.md) |
| 开发时知道必须更新哪些文档 | [文档规划与维护规范](development/documentation.md) |
| 按管理、开发、统一打包职责协作 | [共同规则](../AGENTS.md)、[管理](development/agents/management.md)、[开发](development/agents/development.md)、[打包](development/agents/packaging.md) |
| 区分本机站点参考资料与 Hub 配套补丁 | [站点资料说明](tiancaispace/README.md)、[配套补丁](../integrations/tiancai-squad-browser/README.md) |
| 登录 NVWA、检测 MCP、分别接入三客户端 | [NVWA MCP](customizations/nvwa-mcp.md)、[资料索引](nvwa/README.md) |
| 查看 v12 精简接入界面、本地测试包及验收边界 | [v0.4.30-12](releases/v0.4.30-12.md) |
| 查看 v13 地址缺版本恢复、WorkBuddy max 和测试包 | [v0.4.30-13](releases/v0.4.30-13.md) |
| 查看 v9 历史恢复与 Fast 本地测试包 | [v0.4.30-9](releases/v0.4.30-9.md) |
| 查看 v8 本地测试包、适配范围和验收边界 | [v0.4.30-8](releases/v0.4.30-8.md) |
| 追溯 v5 基线、编译与当轮验收边界 | [v0.4.30-5 发布交付](releases/v0.4.30-5.md) |
| 配置 WorkBuddy 模型、启动桌面及了解 `/wb` 边界 | [WorkBuddy 使用说明](workbuddy.md) |
| 区分普通渠道与 WorkBuddy/天工专用渠道 | [接入点用途与隔离](customizations/client-channel-scope.md) |
| 追溯 v4 GitHub 交付、两平台包与验收边界 | [v0.4.30-4 发布交付](releases/v0.4.30-4.md) |
| 查看此前已发布包及原版本身份 | [v0.4.30-3 发布交付](releases/v0.4.30-3.md) |
| 管理天工 Work 多模型、参数、备份及上游流式兼容 | [天工 Work 模型接入](customizations/gmclaw.md) |
| 启停 Hub/天工、选择完整项目目录、回复状态、会话同步与审批 | [天工外部消息执行端](customizations/gmclaw-im.md) |
| 区分天工模型配置、本地模型连接、IM 授权与上游流式 | [天工 Work 模型接入](customizations/gmclaw.md)、[请求日志](ai-gateway-request-log-detail-patch.zh-CN.md) |
| 设置自定义模型与能力继承 | [动态 Codex 可见模型](dynamic-codex-models.zh-CN.md) |
| 检查 Windows/macOS「无法加载组织设置」与状态库迁移冲突 | [Hub 内置修复按钮与独立工具](customizations/codex-startup-repair.md) |
| 按问题检查与修复模型选项、插件、浏览器、电脑操控及启动 | [检查与修复中心](customizations/codex-repair-center.md)；[开发前评估](customizations/codex-repair-integration-assessment.md) |
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
| [Dumpling-MCP / NVWA MCP](customizations/nvwa-mcp.md) | TC-014 / v0.4.30-12：紧凑客户端选择与“连接并检测”，自动核对、确认接入/修复并检测；手工检查/刷新/单独上游检测/退出登录入口移除，“⋯”只保留断开、撤销和删除环境。已保存凭据回显、同身份恢复、定向备份与工具摘要保留；实际验证见 v12 交付，本次 Windows 试用用户反馈成功，其他范围及 macOS 另行验收 |
| [WorkBuddy](workbuddy.md) | 模型多条目接入已预发布；v0.4.30-3：接入页提供显式启动桌面按钮，用户本次反馈该按钮可启动，仅覆盖该次本机操作；其他机器及 macOS 待验收。`/wb` 仍提示不可用，外部任务执行尚未接通 |
| [接入点用途与隔离](customizations/client-channel-scope.md) | 普通/专用渠道标识、各接入点及同模型条目的精确路由、模型未配置中文指引、保留名称与回滚注意项 |
| [天工 Work 模型](customizations/gmclaw.md) | v8 移除默认控件，统一管理模型/渠道和按钮；新旧模型与独立路由保留，流式补全推理/工具状态；适配和本地包见 v8，实机待验收 |
| [天工 Work 外部消息](customizations/gmclaw-im.md) | v8 主 run 收尾/审批、原生任务模型绑定、后台任务与重连/队列保护；旧授权、会话、目录、平台交互和持久关联保留；隔离验证通过，真实 IM/桌面链路待用户验收 |
| [请求日志详情](ai-gateway-request-log-detail-patch.zh-CN.md) | v0.4.30-3：保留 `stream` 的客户端语义，新增可空 `upstreamStream` 记录实际最终上游请求；天工非流式客户端与上游流式聚合分别展示，旧日志上游模式未知 |
| [OpenAI Chat Completions](openai-chat-completions.md) | 通用兼容渠道与按渠道关闭推理；已实现 |
| [动态模型](dynamic-codex-models.zh-CN.md) | 手填、远端获取、路由补齐和能力继承；v5 普通/ChatGPT 厂商空列表保存先自动获取，成功继续保存、失败留窗，客户端未配置模型中文指引；本轮状态见 v5 交付，实机待用户验收 |
| [Codex 启动修复](customizations/codex-startup-repair.md) | TC-007：v7 程序路径与状态库独立检查/修复；可重新绑定当前内置程序，原路径/已知冲突分别备份恢复，Hub 保持运行；实机待用户验收 |
| [Codex 历史恢复](customizations/codex-history-recovery.md) | TC-006/TC-007；v9 按需等待执行配置、取消与请求合并、部分历史失败后实际分页重试；持续 EBADF 根因未修复，Windows 包及用户验收见 v9 交付 |
| [检查与修复中心](customizations/codex-repair-center.md) | TC-006/TC-007：v6 分类窗口、定向修复、插件隔离正式注册、备份联合恢复和可选页面适配；Windows/Mac 原生流程独立，实机待用户验收 |
| [Codex 修复接入评估](customizations/codex-repair-integration-assessment.md) | 开发前的来源、取舍和限制；当前实现以修复中心专题为准 |
| [网页导入](hub-external-import.md) | Windows/macOS 系统接入已随 v0.4.30-2 成功构建打包并预发布；本版实机导入待用户验收 |
| [导入协议 v1](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md) | Hub 与 Sub2API 联调契约；不表示主站已上线 |
| [大模型地址缺少 /v1 时自动修正](customizations/provider-url-recovery.md) | TC-015 / v13：原地址优先，确认补版本成功后保存，双失败保留原错，自动修正前后保持渠道身份 |
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
