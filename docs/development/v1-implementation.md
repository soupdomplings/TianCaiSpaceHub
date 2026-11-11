# V1 桌面重构实施记录

2026-11-11：本轮实施 [天才搭档与共享 Server（TC-023）](../customizations/genius-buddy.md)，沿用 V1.0.17 累计未提交开发，按用户要求版本进入 V1.0.18，Windows release 和唯一 MSI 已生成、静态核验通过，包身份见 [V18 交付](../releases/v1.0.18.md)。共享运行端、确认准备/仅卸载自管包、临时会话及退出回收、配置工具与隐藏通用资源入口的现行行为和验证统一记录在专题；小分队已精简为 [单配置 GET + 复用普通模型 API](../contracts/assistant-server.md)，工具及摘要由本机处理。本批同时完成新版 ChatGPT 数字资源 Ultra/Fast、Node REPL 代理与可信路径适配，逐项检查修复审计见 TC-006/007 专题；最终 Windows GUI 测试目标编译、77 个不同 Rust 定向用例、28 项 Node 和 54 次候选语法检查通过，真实客户端与 macOS 待验收。下方 V1.0.17 等验证归属原输入，不能覆盖本轮。

## V1.0.17 协助模型请求来源（2026-11-10）

用户实测天工已可通过本机协助 MCP 执行任务，但该入口直接调用固定来源的 Responses handler，没有经过普通网关的活动中间件，造成真实模型请求缺少活动及日志来源。现在受信任务入口核对活动任务令牌、模型与固定来源后，调用 `ai_gateway/handler.rs::handle_local_assistance_responses`，复用 `desktop/activity.rs` 的请求及响应体生命周期。活动保留实际发起端 `tiangong`，另记录 `source: local-assistance-mcp`；完成、错误或取消均沿响应体释放结束活动，并只保留原有五秒短请求显示证据。

日志沿现有 `client_id` 列保存 `local-assistance-mcp`，统一显示“协助处理 MCP”；列表、详情、筛选和按来源用量分布共用 `desktop/request_source.rs`。不新增普通客户端、不改变模型路由与输入/输出口径。来源只由 Hub 受信入口设置，公开 URL、User-Agent 和外来内部活动头无法声明此来源；内部活动 ID 不转发上游，任务令牌仍按现有凭据脱敏边界处理。旧日志不回填，不需数据库或用户配置迁移；停用本机协助即停止新增该来源，回退旧包保留现有日志列数据。

`gui/runtime_scene.rs` 从带该来源的真实模型请求保持协助 MCP 节点，`gui/visual.rs` 将该节点连接到实际 `route + model`，不能串到其他来源的同名模型；真实天工发起端保持。仅启用配置、目录检查或进度轮询不产生模型边；开始工具返回也不会提前结束仍在响应中的模型边。完成先停流再按原规则暗淡，离开重进不复活旧结束请求。协助 MCP 节点点击下钻其来源日志。相关视觉规则见 [视觉规范](desktop-visual-system.md#v1017-协助-mcp-真实模型连接2026-11-10)。

已新增合成回归：`local_assistance_response_lifetime_preserves_source_for_logs_and_usage` 覆盖固定模型来源、流式/非流式、取消、日志详情与用量；`local_assistance_keeps_the_real_mcp_model_route_after_the_start_tool_returns`、`local_assistance_model_edges_stop_for_waiting_completion_and_page_reentry` 覆盖同名不同路由、结束退流及重进。公开入口现有链路用例补充伪来源 User-Agent 不生效。唯一打包会话已完成本批 104 项定向用例、两个示例编译和 Windows release 构建，包含以上回归；唯一 MSI 及实际结果见 [V17 交付](../releases/v1.0.17.md)。真实图谱、调用和 macOS 待用户验收，不把用户此前调通结果写成本修复已验收。

以下为各历史阶段记录，保留当时版本及验证归属。

2026-11-10 当前 V1.0.16：Windows 主窗口标题区与应用壳层融合并保留系统/托盘交互；使用原海豚图标。按用户要求移除 Hub 模型广场的入口、视图与读取路径，可用渠道归账户；不删除模型配置、天工内置来源或 Skill 广场。实现及当前验证见 [V1.0.16 交付](../releases/v1.0.16.md)、[视觉规范](desktop-visual-system.md)、[账户专题](../customizations/tiancai-squad-account.md)。下方旧版本范围保留其原阶段归属，不代表被移除功能仍在当前包中。


更新：2026-11-10（UTC）。当前修复版本 **1.0.12**：配置路径与写入规则核对修正。全部 v13/V1 未提交开发保留；验证及新包身份见 [V1.0.12](../releases/v1.0.12.md)。真实客户端及 macOS 待用户验收，未提交、推送、Actions 或发布。

## V1.0.12 本轮实施

`desktop/targets/paths.rs` 按固定 Magpie 提交移植 VS Code 便携、Scoop、版本目录、Profile 登记/继承与 ZCode HOME/迁移规则；目标可选 `modelRoot`、缺省 `modelsInherited` 保留旧记录。Library 将保存的程序路径送入只读定位；模型选择不列继承默认的 Profile，后端也拒绝写无效配置。ZCode writer 使用独立模型根并识别旧受管文件目录变化，资源根和目标 ID 不跟着搬。

`workbuddy_config` 补原生目录覆盖和容器保留，`AvailableChange` 可选记录本操作可见性差额；恢复保留其他后改并拒绝本目标冲突。Harness MCP 发现实际 Profile，旧模型目录缺现代 Profile 时拒绝假写入；`mcp.rs` 修正 WorkBuddy/ZCode 远程类型。原锁/revision/备份/原子写入和三端/WebStorm 专用服务保持，未改账户或站点。详细逐端核对及未移植项见 [配置核对](../customizations/client-configuration-audit.md)。

## V1.0.11 本轮实施

统一账户/组织筛选组件，广场使用真实 groups/models 派生选项并保留同名分组/费率；客户端按真实 writer 展示可选字段、操作固定底部，保留未知参数和草稿/revision/隔离。启动与进入账户先恢复已保存会话，失败可重试，退出不恢复。vendor 补空输入 hint，无配置迁移、站点修改或部署。必要构建、隔离回归和本地包核验见 [交付](../releases/v1.0.11.md)，用户实机验收另计。

## V1.0.10 本轮实施

`desktop/squad_money.rs` 忽略显示汇率与充值倍率，只将原数值格式化为人民币；编辑原数字并保留未修改字段精度。`Squad::display_money` 不请求 checkout，删除显示元数据缓存；财务 cents 按分转元、实际支付报价及模型广场费率保持原合同。替代 V1.0.9 的展示换算；无账本或会话格式迁移。

`gui/workbuddy.rs`、`gmclaw.rs` 在原 ModelList 末尾增加唯一内存草稿，直接使用完整原参数表单；纯编辑事件在构造时绑定，网络保存/启动仍原绑定与 API。草稿保留打开 revision 及切换前原始输入，保存后才清理，显式读取才更新基线。天工四参数两列、只读协议与固定操作区；选择器用 DIP 高度。`clients.rs/model_fields.rs` 保留新四端已有条目和草稿，捕获/恢复原始表单；修复放弃后旧快照回填及显式载入更新被旧草稿覆盖。Codex 同列表列已有可见 ID，`codexVisibleModels` 只读字段承接原目录，`ChannelModels::focus_model` 仅定位搜索，不自行改变勾选；保存仍原独立别名和受保护复制。

组织工作台当前 12 分区由共用 `squad_view/organization_data.rs` 定义，移除本轮七项预算/分组/报告入口；站点源码、数据和兼容后端接口不删。该模块按站点真实字段区分配置、本期和财务账期，提供合法比例写入载荷。`member.rs` 同窗嵌入可编辑分页，`FieldEditor` 沿原解析和未知字段/并发时间保护；`dialog_job::bind_open` 成功后留窗、失败保留输入、写入期间禁止切页，不重放请求。`allocation.rs` 可视化编辑比例和本期金额；`selection.rs` 搜索保留身份、显式继续，批量余额/限额仍原确认及接口。无新增持久格式；回退不撤销已完成站点操作，回退 V1.0.9 会恢复其旧金额显示。

旧两端完整表单保留明确跨端来源选择。`desktop_models::CopyLegacy` 新增可选 `workbuddy` / `tiangong` 完整保存载荷，先验证与外层客户端/来源/模型一致，再沿原 `save_copy` 校验原生 revision、锁、条目归属与备份原子写入。普通保存的专用渠道过滤及缺完整载荷的旧复制行为保持；参数不因复用退回默认值，也不改源条目。原生刷新回调区分自动读取与显式载入更新：前者保留草稿来源快照和 revision，后者才重建基线；读取或保存失败才出现恢复按钮，参数保留。

原生隐藏回归直接使用七端实际客户端/旧参数组件与合成状态，不启动真实 Hub、客户端或网络调用；账户布局例程复用上述生产组件，不能代替真实权限与财务验收。结果集中见本轮交付。

## V1.0.9 实施留档（以下换算和旧新增布局已由 V1.0.10 替代）

`model_fields::ModelPage`持有实际嵌套模型页，切换新增/已有模型直接布局该页、共用编辑区和原编辑器；此前只Layout外层ClientPage，内页尺寸不变时新增区可能保持零尺寸。原三端有返回入口并保留草稿，revision未读取时不隐藏原编辑器；Codex新增也使用共用区，保存仍调用原复制接口并失效重读可见模型，WB/天工已有参数继续原受保护服务。单一实际位置自动选择，多Profile才显示选择；草稿隐藏完整空标题容器，默认新增表单在最小窗口保存区完整显示。`client_header::ClientSettings`统一七端“接入设置”、本端/Profile配置文件及支持的程序定位；读取及进入设置时重排实际父容器，WorkBuddy备份恢复移入设置，Codex设置追加原业务控件而不覆盖统一内容，天工和WebStorm原维护保留。生产ModelPage/ClientSettings可在独立隐藏例程中反复切换，不运行实际Hub。

`desktop/squad_money.rs` 只做展示与编辑单位转换：USD账本按实时付款配置的显式汇率显示人民币，财务cents按元，模型价目保持原单位；无有效汇率时非零金额显示¥—、金额编辑保留原值并禁用。未修改字段保持原始精度。`Squad::display_money`仅缓存按账号/代次隔离的显示元数据300秒，实际checkout不缓存。

`squad_view/purchase.rs`为生产页内金额/方案与支付组件，提交前实时重读并检查价格、规则和可用性；`gui/squad.rs`以异步结果槽处理下单与兜底开关，成功重读、失败保留旧值或草稿。新请求可选expected_account，写锁内校验userId/generation；旧请求兼容，未知写入仍不重放。

`squad_view/organization.rs`及原controller按四组任务组织18个分区，列表提供显式进入按钮；`detail.rs`只展示白名单业务字段，隐藏内部标识和凭据，扣费身份提供实际操作。`squad_form.rs`统一表单/详情主题、标题、返回与固定保存，`finance.rs`及`dialog_job.rs`沿统一风格。少量原生选择器保留。个人资料无编辑、安全入口，logout仅在该页正文下方。无配置/数据库迁移，不改站点导出或线上；回退不撤销已完成站点交易。源码、合成回归、隐藏布局与包身份见本轮交付。

## V1.0.8 本轮实施

`AppState.desktop_squad` 按配置位置、代理及监听端口复用 `Arc<Squad>` 和HTTP连接池；`desktop/squad/cache.rs` 只缓存白名单成功GET结果，按账号/登录代次/路径/完整query隔离、TTL15–300秒、32项/4 MiB有界。刷新API、保存和会话变化递增失效代次，清理前的在途结果不能回填；保留会话及写锁，付款/查单不缓存。明确下载401强制重验会话，避免旧资料缓存抑制刷新。

服务端菜单与生产导航只保留六组，八项移除由菜单白名单保证，充值购买/付款恢复、原组织管理及财务兼容接口保留。原生密钥导入创建票据补固定官网Origin，兑换/目录复用连接并校验账号/来源/所选密钥；账户错误解开本机HTTP/JSON包装。回环服务验证真实读取计数、查询隔离、保存/刷新/错误/查单、导入合同和账号变化；无持久配置迁移，站点导出源码及线上不改。完整状态见交付专题。

## V1.0.7 本轮实施

`gui/runtime_scene.rs` 删除从客户端已用配置创建 MCP 节点的路径，只使用 `mcpCalls`；调用结束、短调用证据、淡出、稳定位置和重进清理保持。`desktop/squad_browser.rs` 先探测可选接口，HTML404/SPA回退/不兼容版本按未启用处理，5xx/网络单独作为暂时不可用，不产生授权请求；`web/squad.rs` 提供受管能力入口。站点补丁仍未部署。

账户实际生产布局抽到 `gui/squad_view.rs`、`squad_view/` 及 `squad/dashboard.rs`，`gui/squad.rs` 负责真实会话、原业务操作和结果呈现；同一生产组件在隐藏例程中用合成数据渲染。分组菜单仍由服务端开关/六项组织权限派生，刷新不触发选择事件，回包带实际页面身份。资料不枚举内部字段，业务列与日期/分页明确按页定义，读取失败暂停写入可重试。API密钥、组织/财务/安全编辑器及导入接口保留，配置/交易不重放保护无改动。

用户确认以 1.0.0 为本次版本基线，替代此前下一版 0.4.30-14。上游来源仍为 CodexHub v0.4.30，不把原生桌面重构冒充上游升级。

## V1.0.6 本轮实施

`codex_models.rs`按普通来源生成可见模型组，保留全局已选ID集合；`codex_tab.rs`的可见模型和复制草稿以SimpleBook切换。`clients.rs/model_fields.rs`的草稿保存原revision，不保存不产生配置，普通来源及显式跨端复制沿原`models/use`与`models/copy`，未改变路由隔离。ModelList复用控件；旧WB/天工Panel+内部配置滚动替代整页滚动，新四端参数按需展开。滚动底层每次按内容重算virtual size，不隐藏真实溢出。统计范围常量及焦点外观、短Choice宽度同步。

`desktop/skill_market.rs`和`gui/skill_market.rs`提供共用公共仓库及真实包，缓存固定commit；选择目标后应用，沿原Library/事务/适配。公开源码核对与隔离fixture分开，见[专题](../customizations/skill-marketplace.md)。普通单文件32 MiB、完整Skill64 MiB/2048文件和256 KiB说明文件，原文件不截断。

Windows登录脚本改AtDocumentStart并检查返回值，修复wxWebViewEdge不支持文档末注入的确定缺陷。状态确认后业务列表错误以原生页重试处理，不把仪表盘失败当未登录。原账户API/权限/会话不变。构建、隔离回归、隐藏布局和回环WebView验证结果统一见[交付](../releases/v1.0.6.md)，实机闪退与线上账户由用户验收。

追加账户：`desktop/squad_browser.rs`复用受保护会话、WRITE/SESSION门及原auth/me确认；短期证明只存OS保护区，轮询离开写锁等待网络，接受前复核请求和generation。`gui/squad_account.rs`顶部资料/余额按分钟读，账户变化丢弃旧回包；`squad_browser.rs`为无网络纯展示组件，默认浏览器入口+核对码+内置回退。原生工作区提供回到主站。站点配套补丁位于`integrations/tiancai-squad-browser`，双向交换新TokenPair，各端独立刷新；没有修改本机站点导出目录或部署，具体上线依赖及Go编译边界见[账户专题](../customizations/tiancai-squad-account.md)。

## V1.0.5 二级导航与资源确认

Shell在Dumpling的PageId上保留实际来源并显示返回；资源Tab用SimpleBook承接列表、TargetPicker和IssueList，非弹窗子页都有←。卡片的端内“管理使用”和全局“管理客户端”都进入草稿，不直接写入。应用期间禁用选择/返回；过期revision、未知/不兼容目标、重复目标与共享目录矛盾由新selection API校验，之后复用原逐目标安全适配。部分失败重读再选，不自动重放不确定结果；原use API及添加/编辑/恢复入口兼容，成功停用天工的精确目标才停止相应MCP桥。

DiscoveryIssue保留类型、客户端、具体项目/位置、原因和处理建议，旧warnings兼容；GUI按类型和本端过滤。超限继续跳过并保留源文件，不放宽8 MiB/32 MiB/文件数保护。只读配置文件视图可由用户打开文件夹，开发检查不执行此入口。

InlineLogin仅激活未登录模块时创建私有WebView，官网同源+nonce+长度+非空令牌核对后接收一次；模块离开/认证成功清理视图。旧登录弹窗删除；账户/支付接口和安全验证窗不变。无本地会话的状态查询立即返回未登录，不依赖公开API；会话存储格式不变。详见[资源管理](../customizations/desktop-capability-management.md)与[账户](../customizations/tiancai-squad-account.md)。

## V1.0.4 窗口、导航与模型工作区

取消`frame.maximize(true)`，保留初始DIP及屏幕约束。`UsageView`改Panel，统计内容不足560 DIP高时只显示趋势或分布，删除请求按钮，仍可点图/表下钻；不靠隐藏溢出来伪装自适应。侧栏滚动指示按实例隐藏；显示槽位与recent记录分开，展开冻结显示及点击目标，收起应用记录。

`ClientHeader`禁末项拉长，顶部只显示启动，原隐藏读取句柄仅保留旧模块回调兼容；进入及添加模型后用明确回调自动读取。WorkBuddy无设置页，备份恢复条件式放模型页；其他端保留实际初始化、执行、路径或插件设置。`ModelList`以透明原生导航按钮代替边框ListBox；新四端复用`ModelWorkspace/ModelFields`直接编辑名称/上下文/输出/图片/强度、Harness协议以及显式来源同步。新增统一叫添加模型并用同一来源窗口；WB/天工删伪“新增模型”项，原参数业务保留。

编辑时存row和view的revision快照；后台轮询不改草稿，也不拿最新revision替旧草稿保存。配置变更后可载入更新；`/models/use`继续核对revision、文件及后改。`models.public`只优化来源展示名与明确客户端归属，原routeId和鉴权作用域不变。Skill/MCP进入自动discover（十秒合并），单一刷新图标，失败才重试，恢复移至设置；discover收录Hub库而不向其他客户端使用资源。原持久格式、保护写入、日志和统计口径保持，无配置迁移。

Windows构建、隔离回归及生产`ModelWorkspace`/`UsageView`/`ClientHeader`等隐藏合成检查以[V1.0.4交付](../releases/v1.0.4.md)记录，不以合成图替代实际Hub验收。下面各节保留原版本实施依据。

## V1.0.3 活动与原生交互修复

按V1.0.2实机反馈，普通Codex入口从明确请求标识补来源，专用URL/受管配置优先；图片与compact活动路径补齐。运行图不因来源未知而丢弃真实模型/渠道。`/api/desktop/activity?live=true`只读本机运行证据，不等待远控、IM及天工桥；概览300毫秒轮询，完整客户端概览保持原读法。端内请求与根请求在慢读结束后取同一份快照。响应体/WS单轮结束和取消仍清理，等待超过5秒不继续流动，短于轮询的请求不承诺逐条动画。

`ResourceView::visible_for_client`结合真实using、受管managed及兼容compatible筛选；端内默认本端使用及异常项，“可添加”才看兼容未使用项。`resource_gallery.rs`取代列表详情为1–3列卡片，保留原use/edit/restore等业务入口。`provider_form.rs`提供真正生产六字段；TextCtrl最低高度按原生字体加12 DIP且不低于36 DIP，单行编辑器居中。`request_view.rs`提供真正生产日志布局，普通列填充正文，技术列仍可横向滚动。统计API新增全部历史筛选值，曲线鼠标命中按实际点位/DPI，悬浮显示原报告用量。

透明品牌SVG复用原三端并补齐新端；侧栏展开和收起增大/加图标，不改变稳定三端。原路由隔离、写配置保护、日志明细和输入输出统计口径保持，没有配置迁移。离线合成SSE链路、真实生产控件隐藏捕获、最终构建和包身份见[V1.0.3交付](../releases/v1.0.3.md)；不以合成图或字体点数测试冒充实机/DPI/macOS验收。

## V1.0.2 布局与控件修复（历史）

根据用户V1.0.1截图，移除品牌大标题及运行页重复的聊天/Skill/MCP快捷条，侧栏收至202 DIP。共用圆角选择器保留真实wxChoice数据/事件，输入外层为圆角Panel、内部仍为wxTextCtrl；全部业务按钮沿原回调接入ActionButton。程序设置选择不触发用户事件；原三端只读模型列表保留旧值与校验，不再混用ComboBox。列表去外框和网格，卡片使用圆角表面。

统计布局独立为`src/gui/usage_view.rs`：短工具栏可换行、模型/渠道字段进带标签弹窗，四卡76 DIP、趋势改端点有界Bézier。`client_header.rs`统一七端右上启动/下方读取；gui.rs将原Codex/WB/天工启动和读取句柄接入原模块，保留预检查、确认、禁用和错误处理，新四端仍用原open API。

vendor新增细滚动层，保留虚拟尺寸/滚轮/键盘/焦点滚动，并在隐藏系统bar时按滚动轴使用虚拟尺寸布局，避免短窗挤压图表；说明文字按父窗口宽重新换行。Windows列表表头的绘制通知统一文字/背景/字体，保留原生列事件，修复深色白条。独立`desktop_layout_review`只初始化绘图与隐藏生产组件，不调用产品初始化、配置或API；检查统计三个尺寸/深色、七端启动位置、选择/文本事件及滚动。Windows隐藏捕获递归真实控件布局，非Windows返回空；捕获临时DC不进入正常产品流程。准确验证、限制、接口和整体回滚见 [主题](../gui-theme.zh-CN.md)与本轮交付。

## V1.0.1 视觉修复（历史）

用户截图显示 V1.0.0 没有达到确认原型的视觉要求；原生实现和通过编译不能作为视觉符合的证据。本轮从共用层修复字体回退、青绿配色、图标侧栏、折叠和底部导航、按钮/页签、路由图留白及客户端概览形态，版本递增为1.0.1，不覆盖V1.0.0包。

新增 `src/gui/visual.rs`、`controls.rs` 与 vendor ActionButton/GraphicsDC/DIP/font API。原生控件与独立绘图调用同一 renderer；新增 `examples/desktop_visual_review.rs` 用合成数据检查浅深主题、缩放、多个路由的行配对和点击区域。隐藏 Shell 检查未调用 Hub 初始化、未显示窗口或启动事件循环；外围整页 PNG 是组合示意，不是实际运行窗口截图。具体字号、颜色、vendor生命周期与回滚要求见 [主题维护](../gui-theme.zh-CN.md)。

本轮不更改实际模型/资源/账户保存接口、路由隔离或日志口径；保留既有v13和V1所有未提交业务开发。初始窗口按原生DIP换算后约束到屏幕物理工作区，最小尺寸不超过可用大小，客户端概览支持短屏滚动；新增两项尺寸回归。常规旧编辑器仍用原生表单，真实效果需重新由用户检查。

本轮首轮完整回归为1153通过、1失败、5忽略；失败是`connection_diagnostics_limits_log_count_and_exports_log_tail`合成夹具假定后创建文件必定更新。核对本轮临时目录的7个文件修改时间完全相同，生产排序按时间降序、同时间按文件名升序，目标chain文件因此未进前4项。仅测试夹具改为显式不同时间，并断言条数及旧文件排除；未修改诊断导出逻辑，也未读取用户诊断资料。自绘Panel同时补尺寸变化整区刷新。修正后最终结果以V1.0.1交付记录为准。

## 工作区保护

- 唯一 main，HEAD 为 25d024b2baa6af25f66bd69d5b237e0caa35df80，加既有 v13 和本轮未提交修改。没有另建分支、提交、推送、触发 Actions 或发布。
- 实施前将 186 份既有源码/文档/原型及工作区差异保存到忽略目录 target/v1-implementation-20261108/baseline/，附 SHA-256；用于核对，不覆盖用户后改。
- 没有读取或修改 zhenduan/、docs/.obsidian/。站点源码只读参考，没有部署或修改天才小分队网站。
- 没有启动实际 Hub、客户端、IDE 或安装器，没有真实模型、MCP、站点登录或支付调用。合成服务绑定随机 localhost，配置在临时目录。

## 实际实现

| 范围 | 当前行为与入口 | 维护定位 |
| --- | --- | --- |
| 原生导航 | 一级概览、天才小分队、客户端、模型与渠道、Skill、MCP、聊天接入、日志、设置、帮助；稳定三个客户端位置，第三端下方展开，标题侧收起 | src/gui/shell.rs；src/desktop/clients.rs |
| 运行与统计 | 运行图优先；活动端 → Hub → 进行中模型 → 对应渠道。无活动端仅 Hub；新鲜活动才流动，过期等待停止流动。输入/输出各一个总量，整库 SQL 统计与分页下钻 | src/gui/overview.rs；src/desktop/activity.rs、runtime.rs；src/ai_gateway/request_log/desktop.rs |
| 七端工作区 | ChatGPT/Codex、WorkBuddy、天工 Work、VS Code Chat、DeepSeek Harness、ZCode、WebStorm。每端状态图、模型、Skill、MCP、接入设置；Cursor 暂缓 | src/gui/clients.rs；src/desktop/targets.rs、models.rs、model_operations.rs |
| 模型独立复用 | 选择已有来源/模型后真实保存本端配置，生成独立条目及路由；来源变化仅显式同步。普通、WB、天工、新端和各条目不混用 | src/web/desktop_models.rs；src/desktop/models.rs、harness_models.rs |
| 新端协议 | 显式客户端/条目命名空间，凭据与所属关系验证；Messages 流式/非流式转换、工具/停止/取消；同协议 Messages 保留签名及未知块 | src/ai_gateway/client_inbound.rs；providers/anthropic_messages/native.rs |
| Skill 管理 | 完整目录、ZIP、公开仓库导入；读取已有配置，选端直接复制/登记；拒绝越界和链接，不执行包内脚本；部分失败定向重试 | src/gui/resources.rs；src/desktop/skills.rs、resources.rs |
| MCP 管理 | 表单/JSON 共用草稿，HTTP/SSE/STDIO、认证头/环境/参数；保留未知字段，真实写入适配目标后读回。天工用原生服务登记，STDIO 仅由使用触发 | src/gui/mcp_editor.rs；src/desktop/mcp.rs、mcp_bridge.rs、tiangong_resources.rs |
| Dumpling-MCP | 复用原环境与 NVWA 三种认证、prepare/apply/detect/restore，保留原授权和工具摘要 | src/gui/nvwa.rs；src/nvwa/；MCP 页面入口 |
| WebStorm 双模式 | AI Assistant 辅助插件通过 IDE API 与 PasswordSafe 读写；Air/ACP 显式安装固定 OpenCode 组件后写 agent 和模型配置 | integrations/webstorm/；src/desktop/webstorm_native.rs、components.rs |
| 天才小分队 | 原生账户/仪表盘、密钥、用量、模型广场、充值订阅/订单、组织六权限、成员/部门/预算/限额/报告/分组授权、结算/预存/发票。验证与支付仅受限官方窗口 | src/gui/squad.rs、squad/、squad_form.rs；src/desktop/squad.rs、squad_quote.rs |
| 全局设置与维护 | 统一主题/字体/间距、滚动/屏幕约束、动画偏好、网络设置、配置根、帮助；保留 Codex 修复/历史、IM 会话与审批、托盘及后台恢复 | src/gui/preferences.rs、theme.rs、widgets.rs；原专题列出的旧接口 |

完整使用与验收路线见 [V1 用户测试指南](../v1-desktop-guide.zh-CN.md)。原型是交互参考，正式程序继续使用 wxWidgets 原生控件与自绘运行图，并非把 HTML 直接装入主窗。

## 状态与业务边界

普通页面以“使用中 / 未使用”表达配置选择。读取失败保持未知，写入失败不点亮；不把配置成功当作客户端已经加载或上游真实调用成功。客户端可能需要新聊天、重新加载或重启，只在相应入口提示。运行图由请求生命周期驱动，不能由资源开关伪造。

运行图模型节点打开精确 route/model 请求集合；渠道节点定位对应来源/专用副本。旧日志没有可信客户端归属时显示历史未标记，不依据模型名猜客户端。不新增首字统计；原日志明细的 TTFT、缓存、客户端流式与上游流式证据保留。缓存已在协议层合入输入，统计不再重复相加。

旧 Codex/WB/天工配置服务和恢复路径保留。配置 revision、锁、原子替换、备份、后改保护不绕过；新领域对文件和来源分别比较指纹。撤销仅恢复匹配的目标，保留其他客户端与后来修改；冲突时要求核对。VS Code 多 Profile 可能共享原生 Skill 根，这一原生共享关系不能声称物理独立。

账户凭据使用既有系统秘密存储；退出清理会话并保留本地模型渠道。401 单次刷新，会话代次/账号身份保护迟到结果；403 不当空数据。写入超时/5xx 保留待确认标记并阻止同操作重放；可只读核对或去站点确认后移除标记。订单支付以服务器查单为准，窗口关闭和跳转不算支付成功。

组织管理依站点权限：部门管理员与组织管理员的成员操作分开，财务手调须服务端 can_adjust。普通用户不获得站点 /admin 权限。部门排序、批量成员加入/限额/余额/权限、限额重置使用站点原接口；发票税费使用同站点的整数舍入，账户充值/订阅显示预计金额，最终订单金额仍以站点为准。

## 兼容、回滚与平台

新增领域状态在主配置相邻的 .desktop 目录，资源快照用 OS 凭据保护；旧主配置字段和旧 API 路径保留。关闭资源/撤销模型在其原页面执行；后改冲突不会静默覆盖。降级前保留完整配置与恢复快照，先撤销新增端的模型及资源写入；旧版不会管理新增的账户和资源领域。MSI 阻止直接安装较低版本，确需降级时由用户退出并卸载 V1 后安装旧包，不删除配置目录。

WebStorm 辅助插件按 AI Assistant 262.11879 SDK 合同构建，兼容区间限定 262.*，与其他版本兼容须另测。OpenCode 固定 1.2.21，归档哈希核验及安全解包；没有在开发时运行组件。官方配置源码要求 limit.context 和 limit.output 成对，已校验，未填写的能力不伪造厂商支持。参考与许可见根目录 THIRD_PARTY_NOTICES.md，并纳入两平台打包输入。

Windows 优先；本机完成 Windows 编译和静态打包。macOS 保留条件实现和 WebKit 打包链路，本机不能声明 macOS 构建、签名、登录或实机通过；未新增 Linux 支持。

## 验证归属与已知限制

V1.0.0 回归使用临时配置、合成上游、合成天工登记和合成 STDIO 服务；不使用用户账号或客户端。该版最终 Rust 结果为 1152 通过、0 失败、5 ignored，测试与 release 构建的825个产品输入逐项一致；MSI 静态核验及解包哈希通过。WebStorm Java合同测试验证读回、撤销、后改与部分失败补偿，JAR无测试桩。这些保留V1.0.0归属；V1.0.1必须记录本轮结果，不能直接继承旧验证。

v13 的原路径优先、明确 HTTP 失败后同源补 /v1 一次、有效结果才保存、并发配置保护和 WorkBuddy max 保留；v13 原 344 项结果仍属 v13，本轮需重跑才登记为 V1 验证。

WorkBuddy /wb 外部任务执行仍未接通；Codex 持续 EBADF 根因仍未确认。模型可选或档位可见不证明上游全部能力可用。原型确认、代码实现、离线回归、Windows 包静态核验、用户实机验收、正式发布分别记录。
