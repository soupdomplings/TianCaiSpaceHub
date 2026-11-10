# 桌面重构交互评审原型 · 第三稿

更新：2026-11-08（北京时间；首稿 2026-11-07）。产品基线：唯一 main `25d024b2baa6af25f66bd69d5b237e0caa35df80`、`v0.4.30-13` 加既有未提交开发。**本目录为合成原型，未实施到 Rust 产品，不是新版本或产品截图。**

当前版新增一级 [天才小分队](index.html#squad)，已核对用户提供的本机站点源码；账号菜单按站点权限与开关显示，API 密钥复用原 Hub 导入。仍是合成原型，真实登录及财务功能未接入。

运行状态第一、使用统计第二；客户端概览改成六个可操作节点；模型、Skill、MCP 直接在选定客户端使用。跨端保持独立调整，来源变更选择同步。普通页面不展示“保存到库 → 写入 → 加载 → 调用”的多阶段状态，异常保留原选择并提供继续入口。Magpie 的客户端/外部 API 研究已形成 [正式实施参考契约](../../customizations/desktop-client-adapters.md)，参考支持不等于 Hub 已适配。

## 打开与推荐路径

浏览器直接打开 [index.html](index.html)，旧页面刷新即可。无需 Hub、Node、服务器、安装包或网络；HTML、六份 CSS 和八份浏览器 JS 保持同目录。全部动作只改内存或展示说明，“重置演示”恢复固定初始数据。

1. **[WorkBuddy](index.html#workbuddy) / Codex / 天工 → 状态概览**：看标题旁短状态及六节点图。上排客户端 → Hub → 模型，下排 Skill、MCP、聊天与执行；节点可操作，不再逐行列状态卡。进程、配置、模型/执行证据仍分别判断，WorkBuddy /wb 尚未接通；普通请求不自动归为 Codex。只有新鲜进行中请求的模型链路流动，扩展配置线不流动。
2. **[Skill 管理](index.html#skills) / [MCP 管理](index.html#mcp)**：全局资源行只列使用客户端摘要，点击打开每行 2–4 个的紧凑图标网格：成功才点亮，使用端排前，再点停用后变暗；失败保留状态和顺序。端较多再展开搜索；客户端页面直接管理当前端。模拟读到的 WorkBuddy 原有内容已在使用。添加 MCP 默认完整“表单配置”，旁边是可编辑“JSON 配置”；远程地址/认证/Headers、本地命令/逐行参数/环境变量/传递变量/目录直接可见。双向同步草稿，JSON 多服务先选一项，客户端同窗选择后一次“添加并使用”；已有配置沿原身份复用。依赖/授权处理后回原任务。天工远程 HTTP MCP 可演示，Skill/本地命令 MCP 待核实；Dumpling 已有三端适配，不再笼统显示整端不支持。不是只收藏到库的演示。
3. **WorkBuddy → 模型 → 用于其他客户端 → 天工 Work**：目标明确后无争议直接使用。再在天工调整参数，不影响 WorkBuddy 和公共模型。来源更新场景可选择同步目标，默认保留各端参数；账号来源用于天工仍按原边界阻止并能返回选择。
4. **[概览](index.html#home) → 运行状态（默认）**：当前三个打开客户端、一条进行中模型路由，模型与渠道逐行配对；图内整行列标题已移除。点模型先列完整路由的请求集合，点渠道直达对应配置；同名不同专用身份不混用。右下演示状态可选“全部关闭”仅 Hub，或“8 端打开”容量图；A–E 仅规划占位。空闲模型/历史渠道不占图。
5. **概览 → 使用统计**：时间、接入点、来源和模型筛选联动。仅请求数、输入用量、输出用量、完成率；缓存含在输入中不单列，首字统计已移除。Token/请求趋势可下钻同范围原请求；返回后筛选保留。
6. **[模型与渠道](index.html#sources)**：我的模型只列模型与来源，分配/调整入口在客户端页；账号与渠道可添加现有八类厂商来源。ChatGPT 账号授权、智谱 API/Coding Plan、Chat 推理选项各走自己的分支并保留草稿。此表单不是完整原厂商编辑器。
7. **Codex → 接入设置 / 历史与维护**：初始化/更新、WebSocket、增强启动说明、App/VSCode/CLI 证据、账号来源、可见模型、历史及修复均保留。MCP → Dumpling 先配置环境，密码/应用/浏览器字段随认证切换，高级字段可展开；保存后授权，再选择客户端连接。新建环境不改变全局场景。聊天统一叫“聊天接入”。

左侧默认三端依次为 ChatGPT(Codex)、WorkBuddy、天工 Work；访问或成功配置已在这三个位置中的客户端时不重排，只更新使用记录。列表外端进入时替换最久未使用的那个位置，其余位置不动。收起时“⋯”在第三端下方；展开后该按钮隐藏，“客户端”标题右侧显示“收起”。原型仅用合成内存，正式版需用 Hub 自己的记录，不以资源网格排序改动导航。正常仅三端，概览的显式八端容量演示才显示额外 A–E 规划占位。下拉共用搜索和键盘规则。

客户端选择采用每行 2–4 个的紧凑图标网格：图标在上、名称在下，单块最大宽 124 CSS px、最小高 84 px，图标容器 26 px；不再横向撑满弹窗。点击已有资源的目标后，配置成功才点亮；使用中的排在前面，组内保持原目录顺序；再点并成功停用后变暗。失败不改旧选择和顺序，读取失败保留上次选择并禁用修改。这个排序只用于资源选择网格，不重排左栏。新增表单的勾选只是目标草稿，一次“添加并使用”成功后才成为实际使用。

顶部场景可选首次使用、占用/错误、配置冲突、来源更新、读取失败。首次接入页提供“查看已配置示例”，避免把场景空态误认作真实检测。读取失败保留旧选择并禁用改变；错误保留草稿可重试；冲突重核不自动写入，另行决定。多端部分成功保留成功端，仅继续失败端。模型停用不取消正在处理的请求；并发后改会保留旧配置并要求重新核对。

每个页面/子页右上圆形“？”解释当前按钮及范围，关闭不清草稿；常驻页面只保留数据、状态和动作。动效可全局暂停并遵循系统减少动态效果。

## 天才小分队模块（2026-11-08）

刷新 [原型](index.html#squad)，点左侧“天才小分队”。初始未登录，点“登录天才小分队 → 继续演示”进入合成账号；不输入密码。顶部“账号场景”为评审控件，可切个人、成员、组织/部门管理员、独立日志权限、读取失败、登录过期和站点功能开关，正式版不会让用户自己选择权限。

页内分为账户概览、API 密钥、使用记录、按权限出现的组织与扣费、模型广场、充值/订阅、账户与工具。API 密钥点“导入 Hub”沿现有预览确认，默认不启用、不追加 Codex；保存普通渠道不自动覆盖各端独立配置。可演示取消、过期、兑换失败和并发重核；退出账号不删除已导入渠道。

组织与扣费内保留扣费顺序、组织管理、成员用量、外部账号、架构与日志的条件入口。组织管理员不自动有日志权限；管理页里的结算、成员、预算等动作仍须沿站点对象权限及预算模式判断。站点用量使用单独 21 条合成记录，账单不与 Hub 本机 9,069 条记录相加。个人资料、返利及站内工具按原菜单规则保留。

推荐正式程序局部使用 wxWebView，站点新增复用原视图和权限的 Hub 布局；保持原生 Hub 和后台。当前 Cargo 未启用 WebView，Windows WebView2、macOS WebKit、登录会话与支付返回仍需样件及打包核验。详细源码映射、成本、迁移阶段与回归见 [模块评审](../../customizations/tiancai-squad-account.md)。深层创建密钥、组织编辑、安全和交易目前只是入口说明，不当成完整表单或真实成功。

下列 9 对图由 [独立绘图脚本](render-squad-layouts.cjs) 使用相同合成数据生成，**不是 HTML 截图**：

| 场景 | PNG | SVG |
| --- | --- | --- |
| 未登录入口 | [登录](squad-login-v3-layout.png) | [矢量图](squad-login-v3-layout.svg) |
| 账户概览 | [账户](squad-account-v3-layout.png) | [矢量图](squad-account-v3-layout.svg) |
| 深色账户概览 | [深色账户](squad-account-v3-dark-layout.png) | [矢量图](squad-account-v3-dark-layout.svg) |
| API 密钥与导入入口 | [密钥](squad-keys-v3-layout.png) | [矢量图](squad-keys-v3-layout.svg) |
| 组织与扣费 | [组织](squad-organization-v3-layout.png) | [矢量图](squad-organization-v3-layout.svg) |
| 窄窗扣费顺序 | [紧凑组织](squad-organization-v3-compact-layout.png) | [矢量图](squad-organization-v3-compact-layout.svg) |
| 沿原流程导入 | [导入](squad-import-v3-layout.png) | [矢量图](squad-import-v3-layout.svg) |
| 充值与订阅 | [充值/订阅](squad-billing-v3-layout.png) | [矢量图](squad-billing-v3-layout.svg) |
| 账户与动态工具 | [工具](squad-tools-v3-layout.png) | [矢量图](squad-tools-v3-layout.svg) |

图尺寸一般为 1366×940，窄窗组织 900×1068；图仅用于评审，正式最小窗口、缩放和键盘无障碍另验收。源码目录只读并记录 20 份文件哈希；不把无 Git 元数据的导出目录当已确认的线上版本。

## 第三稿独立布局图

以下是独立 SVG 绘图及 PNG，**不是 HTML 浏览器截图**。供直接查看构图，HTML 实际渲染仍待评审。

| 场景 | PNG | SVG |
| --- | --- | --- |
| MCP：默认完整表单 | [添加入口](mcp-entry-v3-layout.png) | [矢量图](mcp-entry-v3-layout.svg) |
| MCP：可编辑 JSON 与同窗客户端 | [JSON 配置](mcp-json-v3-layout.png) | [矢量图](mcp-json-v3-layout.svg) |
| MCP：深色完整表单 | [深色添加](mcp-entry-v3-dark-layout.png) | [矢量图](mcp-entry-v3-dark-layout.svg) |
| MCP：窄窗单列表单 | [窄窗添加](mcp-entry-v3-compact-layout.png) | [矢量图](mcp-entry-v3-compact-layout.svg) |
| 成功后点亮、使用端排前，紧凑三列 | [客户端选择](client-targets-v3-layout.png) | [矢量图](client-targets-v3-layout.svg) |
| 窄容器两列选择 | [紧凑选择](client-targets-v3-compact-layout.png) | [矢量图](client-targets-v3-compact-layout.svg) |
| Dumpling：密码方式与环境设置 | [密码环境](dumpling-password-v3-layout.png) | [矢量图](dumpling-password-v3-layout.svg) |
| Dumpling：共享应用与签名设置 | [共享应用](dumpling-application-v3-layout.png) | [矢量图](dumpling-application-v3-layout.svg) |
| Dumpling：浏览器授权，深色 | [浏览器方式](dumpling-browser-v3-layout.png) | [矢量图](dumpling-browser-v3-layout.svg) |
| MCP：远程地址、认证与 Headers | [远程添加](mcp-remote-add-v3-layout.png) | [矢量图](mcp-remote-add-v3-layout.svg) |
| MCP：STDIO 命令、参数与环境 | [本地添加](mcp-local-add-v3-layout.png) | [矢量图](mcp-local-add-v3-layout.svg) |
| 侧栏展开全部，额外五端仅规划 | [导航展开](client-nav-expanded-v3-layout.png) | [矢量图](client-nav-expanded-v3-layout.svg) |
| 搜索下拉展开状态 | [下拉样式](selector-open-v3-layout.png) | [矢量图](selector-open-v3-layout.svg) |
| WorkBuddy：六节点状态概览 | [浅色](client-status-v3-layout.png) | [矢量图](client-status-v3-layout.svg) |
| Codex：模型与执行证据分开，深色 | [深色](client-status-v3-dark-layout.png) | [矢量图](client-status-v3-dark-layout.svg) |
| 天工：扩展、执行与桌面同步，900 宽 | [窄窗](client-status-v3-compact-layout.png) | [矢量图](client-status-v3-compact-layout.svg) |
| Skill：各端直接使用 | [Skill 管理](skills-use-v3-layout.png) | [矢量图](skills-use-v3-layout.svg) |
| MCP：各端直接使用 | [MCP 管理](mcp-use-v3-layout.png) | [矢量图](mcp-use-v3-layout.svg) |
| 一次选内容与客户端 | [添加并使用](capability-add-v3-layout.png) | [矢量图](capability-add-v3-layout.svg) |
| 配置用于另一客户端 | [复用选择](reuse-v3-layout.png) | [矢量图](reuse-v3-layout.svg) |
| 使用统计：输入/输出、曲线与分布 | [浅色统计](usage-v3-layout.png) | [矢量图](usage-v3-layout.svg) |
| 深色使用统计 | [深色统计](usage-v3-dark-layout.png) | [矢量图](usage-v3-dark-layout.svg) |
| 900 宽统计：指标 2×2 | [窄窗统计](usage-v3-compact-layout.png) | [矢量图](usage-v3-compact-layout.svg) |
| 当前活动：模型/渠道两列短节点 | [运行流向](runtime-flow-v3-layout.png) | [矢量图](runtime-flow-v3-layout.svg) |
| 深色当前活动 | [深色运行](runtime-flow-v3-dark-layout.png) | [矢量图](runtime-flow-v3-dark-layout.svg) |
| 900 宽当前活动 | [窄窗运行](runtime-flow-v3-compact-layout.png) | [矢量图](runtime-flow-v3-compact-layout.svg) |
| 无活动端，仅 Hub | [Hub 待用](runtime-hub-only-v3-layout.png) | [矢量图](runtime-hub-only-v3-layout.svg) |
| 显式八端容量，A–E 未适配 | [八端容量](runtime-capacity-v3-layout.png) | [矢量图](runtime-capacity-v3-layout.svg) |
| 页面“？” | [帮助弹窗](page-help-v3-layout.png) | [矢量图](page-help-v3-layout.svg) |
| 我的模型，仅模型与来源 | [模型库](models-v3-layout.png) | [矢量图](models-v3-layout.svg) |
| 来源更新，选择同步目标 | [来源差异](source-update-v3-layout.png) | [矢量图](source-update-v3-layout.svg) |
| 精确路由的请求集合 | [相关请求](route-requests-v3-layout.png) | [矢量图](route-requests-v3-layout.svg) |
| 直达专用条目渠道 | [渠道详情](channel-detail-v3-layout.png) | [矢量图](channel-detail-v3-layout.svg) |
| 智谱 API / Coding Plan 厂商分支 | [添加来源](source-add-v3-layout.png) | [矢量图](source-add-v3-layout.svg) |

[绘图脚本](render-workspace-layouts.cjs) 的统计与 HTML 使用同一份 [usage-data.js](usage-data.js)。需 Node 与已有 sharp，可用 `--sharp` 指定，不自动下载依赖；写原有 34 组第三稿图；上节单独脚本写新增 9 组，合计 43 组。可用 `--only` 按名称选择输出。运行/客户端图 1366×1068 或 900×1068；MCP 窄窗表单为 700×1220，模型/来源表单图 1366×940，统计窄窗 900×1460；长页面图不表示窗口须有此高度。

七组第二稿历史图保留供追溯，**不代表最新流程**：[概览](home-layout.png)、[Codex 设置](codex-settings-layout.png)、[Skill](skills-layout.png)、[MCP](mcp-manager-layout.png)、[模型](model-layout.png)、[Dumpling](mcp-layout.png)、[冲突](conflict-layout.png)，SVG 同名；[旧绘图脚本](render-layouts.ps1) 未重新运行。全目录共 50 对 PNG/SVG（43 组第三稿与 7 组历史）。

## 统计口径与演示边界

- 固定种子生成 60 日合成记录，共 9,069 条，截止 `2026-11-07 17:45 UTC+08:00`。时间不会自行推进；7/30 天为滚动范围、按日分桶，首尾可能不完整而出现 8/31 个日期。聚合、筛选、曲线和明细同一数据源，12 条一页。
- 请求按逻辑请求计一次。完成率为完成数/终态数，进行中不计分母；失败、取消、断开、未完整结束分开。输入与输出仅合计已知样本，缓存归一包含在输入，不再次加总；覆盖率、分母和时区解释收进“？”。无用量为 `—`，不当作零。移除统计首字/等待/分位数；**原请求日志列表/详情仍含原耗时与缓存字段，未修改**。
- 普通接入不能确认客户端时保持未知。费用未估算，不造货币总额。全范围统计、分页/聚合和一致快照仍需正式后端实现，演示不证明产品已有这些 API。
- 模型复用及来源同步只改内存。目标端参数独立；来源只更新选中专用副本并保留参数，Codex 保持普通路由与候选来源语义，不按同名锁定唯一来源。旧手动模型/来源编辑器和引导只是场景示意。
- Skill/MCP 使用固定合成的原生发现条目，不扫描用户磁盘；MCP 不再展示用途分类或示例服务目录；表单/JSON 双视图共用草稿，单服务/mcpServers 多项先明确选择，保留附加字段、SSE、Windows 路径和空参数，名称/值编辑请求头及环境变量，变量传递只编辑名称；Skill 不解析真实 SKILL.md 或拉取仓库，添加本地命令不运行。天工远程 HTTP 是规划演示，Skill/本地 MCP 格式尚未核实；既有 Dumpling 接入单列保留。真实版本、传输、作用域与信任仍须逐端适配。
- 所有授权、保存、启动、修复、导入、目录和检测都是演示。无 fetch/XHR/WebSocket、用户文件读取或持久化浏览器存储；CSP 禁止 connect/form/object。占位来源为 example.com/example.net，不输入真实账号、Key 或私有资料。

[client-marks.js](client-marks.js) 沿用仓库已有 [Codex](../../../packaging/brand/chatgpt-codex-line.svg)、[WorkBuddy](../../../packaging/brand/workbuddy-line.svg)、[天工](../../../packaging/brand/tiangong-claw-line.svg)、[Dumpling](../../../packaging/brand/dumpling-mcp.svg) SVG，无外部字体/图标。独立图的字符占位不更换产品品牌资产。

## 实际验证与未执行项

| 项目 | 当前结果与边界 |
| --- | --- |
| 合成行为 | 125 组 Node VM + DOM 替身检查通过（原 107 + 账号 18）；138 组原页面、20 组原新增表单、117 组账号页面/子页标记与原 29 个帮助上下文加 1 个模块共用帮助。新增覆盖独立组织权限、会话失效、数据快照、站点统计分离、导入默认/取消/失败/冲突、账号退出及交易边界。原检查覆盖直接使用/停用、原有内容、同名隔离、读取失败、依赖/授权/冲突、添加草稿/部分成功、六节点图与流动条件、模型停用并发保护；统计/复用/路由/日志保护继续通过。不是产品离线回归 |
| 语法与资源 | 八份浏览器 JS 和两份 JS 绘图脚本 node --check；HTML/CSS/actions/CSP/UTF-8、本地文档链接、SVG/XML、PNG 尺寸和声明断点静态检查；记录见 verification.json |
| 开发保护 | 原 33 份开发文件字节不变；三份原已脏索引仅精确替换已登记新增评审块，去除此块后匹配原基线；原始合成数据、两处原型日志函数和三处 Rust 日志源码不变 |
| 独立布局图 | 43 组第三稿、7 组历史图；本轮逐一查看新增账号模块 9 组。前轮 MCP 等图保留各轮查看记录；不证明 HTML 像素一致 |
| 浏览器 | 前轮本地 file: 自动预览被策略阻止后停止，本轮没有重试或通过服务器/其他浏览器绕过。未做 HTML 交互、缩放、浅深色的浏览器渲染验收 |
| 原生与实际配置 | 未启动 Hub/Codex/WorkBuddy/天工或安装器，未做真实模型/MCP、Windows 原生和 macOS 验收。未构建、不升版、不改 Rust/vendor/Cargo/发布摘要/用户配置/v13 包，不提交、推送或发布 |

最新检查在忽略目录 `target/ui-squad-account-20261108/`：smoke.cjs / smoke.json 为合成行为，verify.py / verification.json 为静态和字节保护；before/ 与 before-hashes.json 为本轮明确文件快照；local-source-manifest.json 为站点只读源码身份，final-report.json 汇总新增图与检查。前轮 MCP 107 组保留在 `target/ui-mcp-form-json-20261108/`。Magpie 首批五类适配研究见 [规格第 5 节](../../customizations/desktop-client-adapters.md#5-首批五类客户端的研究与实施建议)，首批公开源码摘录在 `target/ui-client-grid-20261107/magpie/`。前轮 76 组在 `target/ui-review-refinements-20261107/`；更早固定源码在 `target/ui-direct-use-20261107/magpie/`；60 组合成检查归属上一轮，不累计成产品回归。此前客户端/来源轮 45 组检查在 `target/ui-client-status-20261107/`；更早 35 组等记录保留各自归属，不累计成产品测试数。

此前用量/动效、运行状态、多端图谱、渠道/帮助和注释收敛记录分别在 `target/ui-usage-motion-20261107/`、`target/ui-runtime-status-20261107/`、`target/ui-route-map-20261107/`、`target/ui-route-channels-help-20261107/`、`target/ui-copy-cleanup-20261107/`，早期第三稿在 `target/ui-review03-20261107/`。产品 v13 的 344 项不算本次原型验证。

## 常驻说明收敛与帮助入口

统计标题副文案、指标注释、粒度/示例时区、曲线点击提示及类似维护脚注移至“？”。保留实际数据、名称/身份、单位/图例、必要状态、规划标记、空/错误恢复与本次确认范围，不按浅色或 small 标签一概隐藏。关闭帮助不切页、不保存、不清草稿，恢复焦点；技术路径和协议字段在合适的详情区。

完整文档：[天才小分队账号](../../customizations/tiancai-squad-account.md)、[整体方案](../../customizations/desktop-ui-redesign.md)、[功能盘点](../../customizations/desktop-ui-inventory.md)、[管理规格](../../customizations/desktop-capability-management.md)、[Magpie 与客户端适配合同](../../customizations/desktop-client-adapters.md)。没有配置迁移；撤回只处理本轮评审文件和精确新增引用，保留全部 v13 开发及已脏文档原文。

## MCP 完整表单 / JSON（2026-11-08，前轮）

刷新 [MCP 管理](index.html#mcp)，点“添加 MCP”，默认进入“表单配置”，旁边可切“JSON 配置”。名称、简介、连接类型直接显示；HTTP/SSE 填地址、认证和 Headers，STDIO 填命令、逐行参数、环境变量、传递变量与工作目录。两视图共用草稿；JSON 可直接粘贴单服务或 mcpServers，多项需明确选择，错误保留原文。客户端图标网格和“添加并使用”始终同窗。

前两轮用途三选一、单粘贴框均已被本轮替代，基本字段不藏高级。帮助收“？”，已有配置复用保留，Dumpling 继续环境与 NVWA 原认证。全部仅合成数据，不安装、不运行命令或请求、不写真实客户端文件。原型与独立图均待你评审，不等于正式程序可用。

107 组合成检查通过，更新五组独立图、新增 JSON 图；检查、快照、静态及原开发字节保护记 `target/ui-mcp-form-json-20261108/`。前两轮记录在 `target/ui-mcp-entry-20261107/` 和 `target/ui-mcp-direct-20261108/`，不代表用户认可。自动 file: 预览曾被策略拒绝后停止，本轮未重试或绕过；HTML/原生/真实使用待分别验收。
