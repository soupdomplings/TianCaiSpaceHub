# GUI 与 wxDragon 维护

V1.0.13：使用现有 wxWidgets 原生栈统一主题 token、Shell/页签/按钮状态、模型表单、资源图标、统计数字面、日志表面与二级表单 DIP 留白。生产窗口、事件处理、配置 writer、键盘/原生输入和真实溢出语义保持。隐藏生产页面截图、主题对比度测试和 MSI 身份见 [本轮交付](releases/v1.0.13.md)，组件规则及平台边界见 [桌面视觉规范](development/desktop-visual-system.md)。

V1.0.8：账户按配置复用HTTP连接池、短时成功GET内存缓存，刷新/保存/会话变化失效，付款查单始终实时；精简八项入口并修复密钥导入Origin。详见 [账户专题](customizations/tiancai-squad-account.md) 和 [交付](releases/v1.0.8.md)，配置文件、锁及原日志不变。以下V1.0.7/V1.0.6记录保留版本归属。

V1.0.6维护：模型新增为内存草稿，保存才调用原接口；revision/文件锁/备份/后改保护保持。列表更新不重建当前控件，客户端模型仅字段区按需滚动，底层virtual size按当前内容重算。Windows登录回传必须用AtDocumentStart并核对结果；账户确认后列表失败留原生页重试。新增公共Skill缓存可刷新/切仓库，32 MiB单文件与64 MiB完整包有明确处理入口，详见 [公共Skill](customizations/skill-marketplace.md)、[本轮验证](releases/v1.0.6.md)；不以隐藏合成检查代替真实客户端验收。


<a id="v106-运行图与-mcp-分支"></a>

## V1.0.7 运行图与 MCP 分支

运行图取消原 430/560 DIP 高度上限，生产 `visual::runtime_surface` 以可增长比例填满视口。最小宽 620 DIP，纵向按节点数计算真实内容；多节点溢出仍可滚动，重新进入空场景后虚拟尺寸随内容缩小。MCP 在 Hub/模型下方每行两项，Hub 分别连到模型和 MCP；空场景仅 Hub。模型与渠道保持完整路由配对，客户端、模型、MCP 各自使用本次页面出现顺序，不因刷新重排。

`gui/runtime_scene.rs` 持有本次访问的场景，300 毫秒快照更新节点证据，60 毫秒绘图不做磁盘/网络操作。请求结束立即停流，亮度保留 3 秒，用 2 秒缓动降至约 40%，节点及点击入口保留；再次调用原身份恢复。切到使用统计或其他页面后，再进入运行状态创建新场景，清理先前已结束节点；仍打开/使用中的项目依据新快照显示。显示槽每类最多 256，避免单次长期访问无限增长，不影响业务请求。

模型活动 `snapshot()` 仍仅返回进行中项；新增 `recently_completed()` 保留最多 256 条/5 秒内存结束证据，使两次轮询间的短请求也能短暂呈现。`/api/desktop/activity` 新增 `recentRequests`、`mcpCalls`，不写数据库，不加入统计、不补写原日志。页面重进不会将之前已结束的短时记录重新点亮。5 秒无流数据停止动画并等待，8 秒状态快照过期停流并淡出，均不标为成功或错误。

V1.0.7 起，通用 MCP 与 Dumpling 都仅由 `mcpCalls` 中的实际工具调用证据创建节点。已配置、已使用或客户端已打开均不创建 MCP 节点；结束后沿原 3 秒保亮和 2 秒暗淡保留到重进。重进不会带入之前已结束调用。`desktop/runtime.rs` 原有两秒本地配置摘要仍为兼容保留，但运行图不消费它创建节点；不启动服务或探测上游。

`desktop/mcp_activity.rs` 仅观测 Hub 已转发的 `tools/call`，不记录参数、结果、令牌或工具内容。天工 Streamable HTTP→STDIO 在认证及写入子进程成功后开始；Dumpling 在已准入的原远程任务内持有观测，原有断线后完成响应/不重放合同不变。初始化、工具目录和未经授权的请求不做调用动画；客户端原生直连外部 MCP 不经过 Hub，无法观察其流动。结束证据最多 5 秒，仅用于展示。点击通用 MCP 进入 MCP 管理，Dumpling 进入环境管理；模型查看对应路由请求集合，渠道直达来源配置。

关键代码：`src/gui/overview.rs`、`src/gui/runtime_scene.rs`、`src/gui/visual.rs`、`src/desktop/activity.rs`、`src/desktop/mcp_activity.rs`、`src/desktop/runtime.rs`、`src/desktop/mcp_bridge.rs`、`src/nvwa/bridge.rs`、`src/web/desktop.rs`。无新增持久配置字段，旧文件及日志兼容；回退这些观测/场景改动会恢复原先快速消失行为，不需数据迁移。本轮调用门槛、淡出与重进回归及隐藏组件布局见 [交付](releases/v1.0.7.md)，实际 MCP、动画节奏、系统缩放及 macOS 待用户验收。

## V1.0.5 二级页面与登录入口

Dumpling标题前提供34 DIP的←，返回实际来源；资源管理客户端、未读取项目页有各自←。主页面保持侧栏导航，已有可关闭弹窗不重复加入页面返回。Skill/MCP全局卡片以使用摘要和“管理客户端”替代直接切换图标；二级页紧凑网格只改草稿，底部确认后应用，操作期间禁用选择及返回。只读文件视图替代原“配置位置”单选框，网格不随大窗口纵向拉长。

读取问题按资源类型/客户端显示少量入口，详情列原因、处理建议和文件夹；不再跨所有资源页铺红字。天才小分队未登录直接使用内嵌官网登录容器，确认后切原生工作区；离开模块清理私有视图和消息通道。状态API无本地会话时不等待网络。安全边界、离线与隐藏检查范围及MSI身份见[V1.0.5交付](releases/v1.0.5.md)，真实加载/登录、DPI与macOS待用户验收。

## V1.0.4 导航与运行状态维护

维护：2026-11-08。固定页签索引已由 `src/gui/shell.rs` 的 PageId 和注册表承接，切页按 PageId 调度；原后台健康/恢复、托盘隐藏及会话结束停止计时器的生命周期保留。新 Overview/ClientPage/ResourceUi/RequestBrowser/SquadPage 使用可见性、请求代次和忙锁接收后台结果；隐藏/切页后不把迟到结果当作新上下文。

V1.0.4区分持久recent和显示槽位：展开期间点击按冻结槽位映射，收起再更新三个位置；不能只冻结文字而点击仍读实时recent。隐藏侧栏滚动指示不关闭滚轮、键盘和焦点滚动。统计用Panel动态切趋势/分布，不再保留虚拟溢出；仅错误才占用页底状态区域。工作区页签索引仍保留0–4，WorkBuddy隐藏第4项且图中客户端节点改进模型页，避免导航到空页。

原三端模型读取回调在进入和添加成功后触发，不定时覆盖未保存参数；新四端轮询只更新状态，编辑row/revision快照不变，发现配置更新给显式载入入口。Skill/MCP进入自动discover，十秒内合并，刷新强制再读；discover只收录到Hub库，实际use/edit/restore仍明确操作。恢复入口移至设置且只有可恢复记录时启用，后改保护保持。[V1.0.4交付](releases/v1.0.4.md)记录本轮验证及包身份；以下旧结果保留版本归属。

V1.0.3将概览活动改为300毫秒轻量查询，完整客户端概览保留远控/聊天读取；活动API两组请求用同一最新快照。来源识别仅作观测，不授予路由权限。未识别客户端的真实Hub→模型→渠道仍显示；新请求证据写入日志，旧来源不回填。端内Skill/MCP按本端使用/受管异常与可添加兼容项筛选，资源、厂商、日志生产布局可独立隐藏检查。维护与回滚详见[V1.0.3交付](releases/v1.0.3.md)。

请求活动由 `src/desktop/activity.rs` 独立于日志开关持有，HTTP 响应体生命周期、WebSocket 单轮和取消清理各自负责；并发同名模型按路由/条目区分。运行证据过期停止流动并显示未知/等待，不能用缓存快照长期显示客户端正在运行。全库日志查询分页不再依赖原 recent200 轮询填充新列表；清理日志后刷新可见列表。

V1.0.2在不改变上述业务生命周期的前提下继续修复布局和共用控件：保留ActionButton/GraphicsDC、DIP/字体、稳定三端、完整路由容量与双排状态图；去重复快捷块，短筛选/平滑用量/小指标卡、圆角输入选择、溢出细滑块。七端启动在读取上方，原三端回调仍属原模块；错误与说明换行。隐藏生产组件检查不调用产品初始化，也不验证客户端已经加载。V1.0.0/V1.0.1视觉未通过，本包真实效果待验收；vendor类型/捕获/滚动/绘制与整体回滚见 [主题](gui-theme.zh-CN.md)。

[使用与验收](v1-desktop-guide.zh-CN.md) · [实施](development/v1-implementation.md) · [本轮交付](releases/v1.0.4.md) · [V1.0.3](releases/v1.0.3.md) · [V1.0.1](releases/v1.0.1.md) · [V1.0.0历史](releases/v1.0.0.md)。下文原维护记录保留旧版本身份，不将旧页面索引当作新导航合同。

维护日期：2026-10-10。当前基线 `0.4.30-5`，唯一 `main`。本文替代旧版 wxDragon 同步与资源优化执行计划；旧文档中的 `0.9.16`、临时脏文件和 stash 操作不再代表当前状态。

发布状态：用户对本轮 Windows 本机测试版反馈“可以了”并授权发布；[v0.4.30-5 Release](https://github.com/soupdomplings/TianCaiSpaceHub/releases/tag/v0.4.30-5) 已创建（非草稿 Pre-release、Latest=false），[Windows Actions](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/38052269127) 与 [macOS Actions](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/38052269076) 已启动、尚未完成，本轮停止核对时为 0 个附件，未执行发布产物核验。按用户要求在开始构建后停止等待，构建成功后由 Actions 自动上传。该反馈只属于此次 Windows 本机试用和可交付判断，不扩大为 MSI、macOS 或系统退出全部场景验收；源码/标签、运行和 Release 身份见 [v5 交付](releases/v0.4.30-5.md)。

## 依赖来源

[Cargo.toml](../Cargo.toml) 声明 wxDragon `0.9.17`，通过 `[patch.crates-io]` 指向 [vendor/wxdragon](../vendor/wxdragon)。升级时需要一并考虑 vendored 的 `wxdragon`、`wxdragon-sys`、`wxdragon-macros` 和底层构建，而不是只改版本字符串。

依赖同步应作为有明确目标的开发任务记录基线与差异，不在一般功能开发中顺带升级，也不执行旧文档针对当年工作区的提交/stash 命令。

## Windows 系统结束会话

用户报告关机被 Hub 阻止。底层 wxWidgets 3.3.2 MSW 默认 `OnQueryEndSession` 尝试关闭全部顶层窗口，原关闭处理拒绝关闭并隐藏托盘，导致会话结束查询被拒绝。当前窄扩展不升级依赖：C 枚举增加 `QUERY_END_SESSION=390`、`END_SESSION=391`，C++ 映射 wxApp 事件，App→wxEvtHandler 由 `static_cast` 正确取得基类指针；Rust `EventType` 与 `App::on_query_end_session/on_end_session` 复用已有 `EventToken` 和回调生命周期，枚举、头文件、C++ 和 Rust 必须同步。

Windows 查询处理通过 `set_can_veto(false)` 明确允许，并用 `skip(false)` 消费 query 事件，避免进入 wxWidgets 默认关闭/提示路径。此时不隐藏窗口、不弹框、不变更授权或退出状态；Windows 取消关机的 `WM_ENDSESSION(false)` 不产生真正 end-session 回调，Hub 继续使用。

真正 end-session 时标记 `session_ending/quitting/closing`，使用 `try_borrow/try_borrow_mut` 尽力停止 GUI 计时器，避免系统消息从模态循环重入时因已有借用而崩溃；移除托盘，并仅对本 GUI 当前持有的 `Child` 及可通过 `try_lock` 取得的启动中 `Child` 发出终止请求。不调用后台网络停止、不扫描外部 PID、不等待线程/子进程或阻塞取得锁，也不结束独立天工/WorkBuddy 桌面进程。最后 `skip(true)` 保留 wxWidgets 原生 session 终止处理：Windows 完成本机 app 清理并退出，macOS 默认强制关闭主窗口。

后台启动线程在发布 `Child` 后先释放 `pending_startup_child` 锁，再复查 `closing`。若系统结束回调恰在首次检查后到达、因锁被占用而跳过，线程自行重新取得槽并收尾晚到的自有 Child；若结束发生在发布解锁和复查之后，系统回调可直接取得已发布 Child。线程收尾前释放槽锁，先观察最多 250 毫秒，再按需终止并回收子进程；250 毫秒不是整个线程收尾的硬期限，GUI 不等待该线程。紧急系统终止仍可能打断尽力清理，不承诺后台业务完成或重放。

普通未主动退出、可 veto 的 × 仍隐藏托盘；不可 veto 的强制 close 不再走隐藏/提示分支，真正销毁窗口和结束事件循环，执行同一快速自有资源清理。用户主动菜单/托盘“退出”保持原有退出流程；系统查询不会借用这一流程。

维护入口：[GUI 会话结束](../src/gui.rs)、[自有后台](../src/gui/daemon.rs)、[Rust App](../vendor/wxdragon/rust/wxdragon/src/app.rs)、[事件映射](../vendor/wxdragon/rust/wxdragon/src/event/mod.rs)、[C ABI](../vendor/wxdragon/rust/wxdragon-sys/cpp/include/core/wxd_app.h)、[C 枚举](../vendor/wxdragon/rust/wxdragon-sys/cpp/include/wxd_types.h)、[C++ App](../vendor/wxdragon/rust/wxdragon-sys/cpp/src/app.cpp)、[C++ 事件](../vendor/wxdragon/rust/wxdragon-sys/cpp/src/event.cpp)。合并依赖时保留或以等价上游 API 替代，不能只删枚举或单边改 ABI。

本轮实现及事件 ABI、取消语义、启动并发收尾的静态复核已完成，最终 Windows locked GUI build 通过（22.88 秒、38 条警告），包含发布后关机握手；独立 EXE 版本、x64 PE、DLL 导入及复制哈希静态核对通过，最终身份见 [v5 交付](releases/v0.4.30-5.md)。开发方未实际关机/注销、启动 GUI 或执行测试；系统取消关机、普通 ×、强制关闭和自有/外部后台边界待用户验收。session hooks 在 Windows/macOS 注册，本轮产品功能以 Windows/macOS 兼容为目标，macOS 条件编译及平台分支静态复核通过；本次 Mac 原生编译和包构建已由 Actions 启动、尚未完成，Mac 实机及系统退出验收另行进行；本轮不增加 Linux 支持。此修复不新增配置字段或数据迁移，回退 GUI 与整套窄事件扩展可能恢复关机阻止，回退后可在关机前从托盘“退出”。Windows MSI 安装成功页及其 Actions 构建边界另见 [桌面与交付](customizations/desktop-and-packaging.md#平台与产物)，本地 EXE 编译不验证安装向导。

## 当前刷新机制

- [gui.rs](../src/gui.rs) 当前 dashboard 刷新常量为 10 秒，请求日志刷新常量为 5 秒；这些是配置的轮询间隔，不是实测性能指标。
- `GuiTimers` 管理周期任务，托盘隐藏和恢复时调整刷新，退出时停止定时器。
- 网页导入的接收和预览使用独立 Panel 作为计时器 owner；模态预览期间接收继续工作，展示回调暂停以避免重入。模型获取对话框的短周期刷新仅服务于该临时操作。
- 单次或后台操作应避免阻塞 GUI 主线程；沿用异步任务和结果回传，不能在渲染回调中执行长网络请求。

维护入口：[GUI 与计时器](../src/gui.rs)、[托盘](../src/gui/tray.rs)、[导入预览](../src/gui/external_import.rs)、[dashboard API](../src/web.rs)。

## macOS URL 事件接入

`0.4.29-5` 使用已有 `App::on_open_url` 回调，在原生回调中只解析、入队，不进行网络查询或模态交互；主窗口的导入定时器负责展示，隐藏窗口收到有效导入时通过 `activate_app` 恢复置前。

vendored [app.cpp](../vendor/wxdragon/rust/wxdragon-sys/cpp/src/app.cpp) 增加最多 16 个、每个最多 8192 字符的回调前 URL 暂存，回调注册后交付并清空；过量或超长输入使用无原文的错误信号。Rust 端仍按 8192 字节上限严格校验。队列只在内存中，不能将完整 URL 写入日志。

该队列覆盖已交给 `MacOpenURL` 的事件。当前依赖 wxWidgets 3.3.2 的 `OSXStoreOpenURL` 在自身 `OnInit` 完成前仅保存一个 `m_getURL`，随后由 `CallOnInit` 交付；不能声称本层队列解决了更早阶段连续 URL 的覆盖问题。首次启动应等待主窗口出现后继续导入；冷启动与连续唤起仍需 macOS 实机验证。

重复 App 使用独立隐藏 Frame 与 Timer，在后台线程执行 socket 转交；有界事件队列与失败提示防止单实例退出吞掉 URL。该事件桥使用公共 wxDragon 接口，可在 Windows 的测试代码编译中核对 Rust 类型，不能据此宣称 macOS 原生运行或 Unix IPC 验收通过。具体路径、超时及回退见 [网页导入](hub-external-import.md)。

共享导入队列将已占用名额的导入失败与启动/原生链接解析错误分开处理；仅完成预览或导入失败时扣减在途数量，避免畸形链接影响并发限制。

## 后续优化记录

如再次处理 CPU/内存问题，专题中写明触发场景、实际采样依据、涉及刷新任务和最终行为；没有采样不填性能降幅。事件推送、虚拟列表等旧建议只是备选方案，不视为已经交付或自动排期。

测试安排遵守 [开发约定](development/documentation.md)，当前用户自行验收；本次文档更新未运行性能或 GUI 测试。
