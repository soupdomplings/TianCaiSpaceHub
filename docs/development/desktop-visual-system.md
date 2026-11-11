# V1 桌面视觉与组件规范

## V1.0.26 页面与公共弹窗短转场

用户要求功能切换和弹窗不再一闪跳变。共用 `src/gui/motion.rs` 对整个原生容器作短距离位置过渡，不逐字段重排或缩放正文，不保存页面截图或新增等待窗口。入场使用三次缓出曲线；页面及表单为 150 ms / 8 DIP，结果提示为 130 ms / 12 DIP。选中状态、业务回调、请求和模态结果仍立即按原流程执行，动画不等待网络，也不延迟确认或关闭结果。

本批覆盖路径：

| 入口 | 实际覆盖 |
| --- | --- |
| `shell.rs::Shell::navigate` | 左侧模块、客户端主入口与已有跳转，完成同步布局和原 changed 回调后整体内容入场。 |
| `clients.rs::select`、`overview.rs` | 客户端状态/模型/接入设置页签，概览运行状态/使用统计；原草稿、选择和设置更新保持。 |
| `squad_view.rs` | 账户运行期间正文在加载摘要、表格、账户概览与购买页之间切换；相同正文页后台刷新不重复播放。 |
| `request_log_detail.rs` 的原生 Notebook | 请求、上游、响应及错误详情页签，继续使用原生页签和编辑控件。 |
| `SquadDialog::new`、`squad_form::select`、组织 `workspace_dialog` | 小分队表单/选择、组织和成员复合页面共用入场。 |
| `feedback::build_prompt` | 共用删除/安装/移除确认与说明详情表单入场。 |
| `chatgpt.rs`、`local_assistance.rs`、`codex_tab.rs`、`usage_view.rs`、`preferences.rs`、`session_history.rs`、`onboarding.rs` | 渠道配置、本机协助、增强启动等待关闭、用量筛选、客户端配置位置、会话目标选择及四种聊天登录配置自绘窗口。 |
| `feedback::Notice` | 成功和失败结果卡片入场、自动到期和手动收回；新结果可打断旧退出，并从当前进度继续。 |

主渠道编辑、关于、请求浏览和 ChatGPT 检查修复窗口已由管理会话在 `gui.rs` / `request_browser.rs` / `startup_repair.rs` 接入同一 `install_dialog`，最终集成结果以 [V26 交付](../releases/v1.0.26.md) 为准。助手浮层及[数据加载忙态](loading-feedback.md)由各自入口维护，不把它们描述为本节公共模态自动覆盖。系统文件/目录、文字输入和单选窗口沿系统行为；隐藏的通用 Skill/MCP 管理页及旧通用等待 Dialog 没有扩展动画，其他未接公共入口的原生窗口也不宣称已覆盖。

`theme::motion_enabled()` 统一控制：用户选择减少动画或现行 Windows 系统设置关闭动画时直接呈现终态；隐藏、最小化与销毁停止相关计时，隐藏窗口不会因动画完成重新显示。Timer 只由独立隐藏 Panel 托管，不占用业务轮询 owner；同一页面再次被选择不重播，快速切页以最后逻辑目标为准。连续结果替换从当前路径续接，旧收回不能隐藏新提示；用户实际移动窗口或父布局重新定位时停止过渡，尊重新位置。模态首版只有入场，关闭即时执行原结果，避免延迟保存和选择流程。

本次 `ui-ux-pro-max` 定向查询 `animation continuity reduced-motion desktop` 命中 Reduced Motion、Excessive Motion、Duration Timing 和 Motion Sensitivity：采用统一短时长、少量整体运动及减少动画终态；没有套用 CSS 或网页滚动效果。无持久格式、业务接口或依赖迁移，回退 `motion.rs` 与接线即可恢复即时显示，保存配置和用户成果保留。

新增纯路径用例 `gui::motion::tests::interrupted_position_path_continues_from_current_point_and_finishes_latest_target`；两项隐藏原生夹具 `hidden_native_motion_skips_timers_and_reduced_motion_uses_final_state`、`hidden_page_switches_keep_latest_target_and_new_notice_cancels_pending_exit` 检查私有 Timer、隐藏/减少动画终态、连续页签最终选择及旧提示退出被替换。三项均在 `gui::motion::tests`，隐藏夹具不显示 Frame、不发键鼠输入、不调用真实服务。开发阶段仅定向格式化和差异检查，编译、定向执行和唯一包由管理会话统一记录；真实 Windows 动态观感、DPI/多屏和 macOS 待用户验收，没有扩展 Linux。

管理会话已完成最终 Windows GUI 测试目标编译及上述三项用例，均通过；退出过程中被外部定位打断时仍结束隐藏，入场被打断则保留实际位置和可见性，两分支沿原隐藏夹具补验。整体 57 个不同 Rust 用例及最终构建、包身份以 [V26 交付](../releases/v1.0.26.md) 为准。

## V1.0.25 助手保存后的列表刷新与专用渠道名称

用户反馈搭档已报告新增 WorkBuddy 模型，但 Hub 需要多次切换页面才显示，同时模型、渠道页面感觉迟滞。本轮复用已有配置 `revision`：助手状态返回 `configurationRevision`，界面在首次取得有效值及后续变化时通知客户端页和概览刷新；缺少该字段时忽略，不创建额外计数、哈希或磁盘扫描。当前可见客户端立即读取通用列表和对应原生列表，隐藏页只标记待更新，下一次进入先消费已完成响应再请求最新状态，不必靠反复导航释放旧的忙碌状态。

通用模型读取现在独立返回，不再等活动发现和资源库读取全部完成后才交给界面；后两项只在客户端概览可见时请求。相同模型快照不重建列表。概览配置读取共用一个运行中的 worker，期间的多次更新合并为后续最新读取；WorkBuddy、天工原生编辑器也只保留一次待刷新，明确的“载入更新”优先于普通通知。完成读取和清除忙碌状态采用同一份刷新状态，避免旧读取覆盖新保存或失去刷新请求。

刷新列表时继续保留新建草稿。已有条目存在未保存输入时，同样保留所选渠道、模型和支持的参数，以及原保存基准；外部配置已改变时显示“载入更新”，明确载入后采用最新配置，避免把旧输入直接写回新的保存基准。外部移除正在编辑的条目时保留输入、取消列表选中并暂停保存，不能将其悄悄转为另一条目、新建模型或天工内置模型。新建草稿的显式载入仍沿既有流程保留输入并更新保存基准。没有改变后端原生 writer 的 revision 核对。

模型／渠道页中的客户端专用渠道采用真实显示名称：普通桌面副本先用已保存的标签，再用模型；WorkBuddy、天工专用渠道用真实上游模型，兼容仅有别名映射的旧配置。别名键可能是原生条目 UUID，不能当模型显示。内部渠道名、条目身份、路由及复制来源继续按原标识操作；复制模型的来源选项显示“客户端 · 模型”，不暴露内部 UUID 作为主名称。

WorkBuddy、天工的启动、保存、删除和撤销结果接入统一成功／错误反馈，取消同一结果在页底重复出现；连接是否配置、输入校验、读取错误、草稿及“载入更新”等业务状态继续保留。共用反馈的样式和其他页面整理由本批统一反馈实现记录，不以此次刷新修改扩大其范围。

关键入口：`src/assistant/tools.rs::public_status`、`src/gui/assistant.rs::on_configuration_changed`、`src/gui.rs::force_dashboard_refresh`、`src/gui/clients.rs`、`src/gui/configuration_refresh.rs`、`src/gui/workbuddy.rs`、`src/gui/gmclaw.rs` 与 `src/gui/ai_gateway.rs::provider_display_name`。只增加助手状态响应字段及内存刷新／编辑状态，不迁移持久配置；旧状态接口不带该字段仍可显示原有内容。回退相关 GUI 和状态通知字段即可，保留所有已保存渠道、客户端模型和用户成果。

新增定向用例：`gui::configuration_refresh::tests::repeated_invalidations_merge_and_explicit_rebase_is_not_lost`、`gui::ai_gateway::display_tests::dedicated_channels_display_real_models_and_keep_internal_identity`。原隐藏合成夹具 `gui::native_model_review::native_existing_model_lists_keep_complete_unsaved_drafts_and_original_revisions` 增加两端已存条目编辑遇到外部新增／删除、显式更新及旧 revision 保留检查，沿用完整草稿和跨端副本断言，不调用实际服务。开发阶段 `rustfmt --check`、定向 `git diff --check` 已通过；GUI 编译、用例执行和唯一测试包由管理会话统一登记到 [V25 交付](../releases/v1.0.25.md)。此处不将源码就绪写作实机通过，真实助手保存后的显示、交互延迟、Windows 缩放和 macOS 待用户验收。

本批管理会话已完成 Windows GUI/release 构建，新增刷新合并和真实模型名称两项纯用例均通过，唯一 MSI 静态核验通过；整体 31 项 Rust 结果见 [V25 交付](../releases/v1.0.25.md)。扩展的原生模型编辑夹具本批未运行，原 V24 结果保持历史归属，不将源码断言当作新执行结果。

## V1.0.24 导航与模型同步

左栏固定展示五个可见客户端，不再维护最近三个位置、更多或收起，旧 `recent-clients.json` 留在本机但不读取/写入。Nav、Tab、Segment 的逻辑选中立即切换，原生按钮自持 160 ms 颜色/指示线过渡；不移动按钮边界，不延后业务动作，可从当前值反向。普通按钮不套用该动画，减少动画设置直接取终态；隐藏或销毁停止计时。入口为 `controls.rs` 和 vendor `button.cpp`。

Codex 模型选择以最近实际读取值为基准，无草稿时接收搭档/API 的保存；存在未保存勾选或手填文字时保留并显示“载入更新”。点击立即应用最近模型列表，不等待下一次 dashboard 刷新。仅顺序变化时同步原始保存基准，避免无意义的 revision 冲突。入口为 `clients.rs`、`codex_tab.rs` 和 `codex_models.rs`，不迁移配置，不撤销已保存模型。

小分队导入结果使用原生三列表格。联调同时修复 vendor `ListCtrl::get_item_text` 的 UTF-8 缓冲长度：C++ 返回字节数不含 NUL，Rust 分配额外终止位再按返回长度取正文，避免中文回读末尾被截断；显示数据和原生提交协议不变。必要验证及浅深/窄窗结果统一记录在 [V24 交付](../releases/v1.0.24.md)。

## V1.0.24 天才小分队弹窗统一（本轮开发）

用户要求小分队弹出页面统一整理，避免冗长标题、整屏文字和连续选择窗口。本轮共用 `src/gui/squad_dialog.rs::SquadDialog` 承载简短标题／说明、主题卡片、独立滚动正文、固定反馈与底部主次动作；尺寸按 DIP 换算，并受 Hub 客户区和系统工作区约束。沿既有 `theme` 字号、浅深色、圆角和 `controls::Button`，不另造网页风格或用原始 JSON 代替业务内容。

共用编辑表单 `squad_form.rs` 按基本信息、渠道与状态、资金与额度、用量与限额、有效期、访问验证、开票资料及成员分配整理字段；字段名用简短中文，数值、选择、开关与逐行列表保留原生输入。表单直接复用外层正文滚动，固定保存和错误反馈；现有完整成员／额度页仍使用自己的单一滚动与固定动作，避免重复标题和嵌套底栏。非业务用的未知复杂结构保留于原请求，界面不提供原始 JSON 编辑器。原金额编辑、精度、未知选项保留、`expected_*` 并发字段和验证器保持，视觉分组不改变提交合同。详情按既有业务字段白名单显示主题卡片，不列内部身份或私有凭据。

账户安全改用主题选择卡。通知邮箱合并主邮箱、其他邮箱列表和当前行启停／移除动作，减少先看详情、再选动作、再选邮箱的连续窗口；添加仍走发送验证码与原验证接口，删除保留确认。密钥详情使用单一密钥卡和固定复制／关闭，完整密钥默认遮盖、可再次隐藏。资金顺序使用带实际启用状态的序号列表，上移／下移按当前选择禁用，底部一次保存。通用单选／批量选择继续使用现有可搜索主题组件，保留已选身份和自身底部操作。

支付建单、继续原订单和受限付款／验证 WebView 使用同一外框，订单内容与预计支付分卡，金额有独立层级，仅显示当前用途需要的金额或方案。发票列表分金额概览、右对齐金额表格、操作和分页；部门排序也归入主题外框。组织、成员、分配的完整复合页面保留内建标题／滚动／底栏，仅统一宿主、字体和 20 DIP 边距。站点接口、账户代次、权限、财务口径、一次下单、继续原订单和查单确认保持。导入多模型与客户端的一键配置由本批另一专题记录，不以弹窗整理代替其保存或测试结果。

使用 `ui-ux-pro-max` 定向 `dialog form grouping scroll footer` 查询，结果中 Submit Feedback 的 All 平台建议适用：原位置展示保存／错误，保持草稿并可继续。Web 锚点平滑与横向滚动示例不作为 wxWidgets 规则；卡片、短标签和固定底部依据本产品已采用的桌面体系。没有新增持久配置或站点改动；回退相应 GUI 文件即可，保留账户、密钥、模型和原支付订单。

修改入口为 `src/gui/squad_dialog.rs`、`squad_form.rs`、`squad.rs` 与 `squad/{payment,organization,finance}.rs`。隐藏定向夹具 `gui::squad_form::native_review_tests::squad_forms_keep_footer_visible_and_preserve_unedited_server_fields`（显式 `--ignored --exact --test-threads=1`）检查浅深、大小窗口的正文滚动、保存始终可见、字段横向边界，以及金额精度／并发字段／未知结构原样保留；设置 `TIANCAI_NATIVE_REVIEW_DIR` 后保存四张合成组件图供人工检查。管理会话已通过 Windows GUI 编译和该原生夹具，四张浅深/大小窗组件图已人工查看。实际小分队新弹窗、支付／邮箱／密钥操作、Windows 缩放和 macOS 待用户验收；没有启动实际产品、真实账户、支付或安装器。

## V1.0.24 天才搭档消息滚轮、Markdown 与展开动画（本轮开发）

用户反馈对话浮层无法用滚轮翻阅。根因在 `src/gui/assistant.rs::MessageCard`：旧回调只匹配 `WindowEventData::General`，而本仓库 wxDragon 的 `wxd_IsMouseButtonEvent` 把 `wxEVT_MOUSEWHEEL` 包装为 `MouseButton`；因此正文转交分支未执行，原生只读多行文本默认处理了滚轮，外层消息区没有滚动。旧分支还固定每格三行，没有采用系统行数和高精度累积。

消息正文、气泡边距、卡片空白及结果标题现在把一次普通竖向滚轮交给消息区自己的原生 `wxScrollHelper`。采用事件副本并转换坐标、对象和 ID，目标同步处理后明确消费原事件，避免默认处理再滚一次；不保留事件指针或引入后台任务。新增 `ScrolledWindow::forward_mouse_wheel` 只在显式绑定时启用，不改全局事件分类。系统每格行数、整页滚动及不足一格的累积沿用 wxWidgets 原生逻辑；Ctrl+滚轮和横向滚轮放行，输入框仍自行处理多行滚动。消息区空白沿用 ScrolledWindow 默认滚轮路径，欢迎内容的子控件也有同样转交。

消息正文接入 `src/gui/assistant_markdown.rs::MessageBody`，使用可选择和复制的原生块控件显示段落、1–6 级标题、有序／无序列表、围栏代码、行内代码及粗体。表格使用原生标题与单元格，沿现有主题区分表头、行和分隔；每列宽度不足 76 DIP 时转为逐记录的标签／值排列，避免横向裁切。长网址和代码按字符换行；链接与 HTML 保留为普通文字，不新增自动打开行为。未完成的流式尾块可继续更新，已完成块和兼容表格单元格复用原控件及选区；结构变化只替换受影响尾部。新块、表格文字和空白继续转交消息区滚轮，并保留 Esc 收起和原生复制按键。

展开／收起采用 160 ms cubic ease-out 短位移，最多使用最终矩形上方的 14 DIP 空余空间。只移动原生子 Panel，正文、表格与输入尺寸不随每帧重排，焦点和发送不等待动画；小窗口没有可用位移空间或现有减少动画设置开启时直接到达终态。连续点击可从当前位置反向，收起末尾隐藏，重新展开保留对话和草稿。清空、切换临时对话／账户及停用即时停止过渡；主窗最小化或隐藏时停止计时并保留应有展开状态，销毁时释放动画 Timer。动画 Timer 由 chat 独立持有，与模块页状态轮询分开，避免当前 vendor 按 owner 绑定 Timer 回调时互相触发。

消息身份和自动跟随计算保持；上滚时的新消息继续保留原阅读位置，回到底部后才随新内容。未改后端、配置格式、用户资料及 V23 浮层裁剪。回滚 assistant、新 Markdown 模块及注册和三个 vendor ScrolledWindow 文件即可恢复原行为，不迁移用户配置。

使用 `ui-ux-pro-max` 的 UX 定向查询 `nested chat mouse wheel scrolling`，结果为 Web 水平溢出及 IME Enter；按技能要求收窄为 `scroll hijacking nested scrolling`，结果为 Web 锚点平滑和动效敏感。没有核对到适用于本原生事件转交的规则，故未套用 Web CSS/滚动动画。事件处理依据为本机实际 wxWidgets 3.3.2 的 `scrlwing.cpp::HandleOnMouseWheel` 与官方 [滚轮消息说明](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel)。

Markdown 查询 `markdown narrow text readability` 核对 Text Reflow／Contrast，沿现有字号、颜色与原生控件实现。动画查询 `reduced motion animation interruption` 命中 All 平台的 Reduced Motion、Excessive Motion 和 Duration Timing；只采用尊重现有动画偏好、限制移动范围及保持反馈及时的建议，160 ms 是本组件取值，不当作全部交互的固定要求，也不移植 CSS 媒体查询。

验证入口包括以下定向用例；原生夹具需显式 `--ignored --exact --test-threads=1`：

- `gui::assistant::native_overlay_tests::assistant_transcript_wheel_routes_native_children_and_preserves_reading`：显示隔离合成窗口，用 Windows `WM_MOUSEWHEEL`／`WM_MOUSEHWHEEL` 覆盖正文、气泡边距、行空白、结果标题、消息区空白、半格累积、修饰键、选区、阅读／跟随及输入框自身滚动。
- `gui::assistant_markdown::tests::native_markdown_reflows_tables_and_keeps_streamed_prefix_selection`：隐藏合成窗口中的标题、列表、长代码／网址、宽窄表格、流式追加与前块／单元格句柄和选区保留；另有三个 `markdown_` 纯解析用例。
- `gui::assistant::layout_tests::chat_motion_reverses_continuously_and_respects_reduced_motion`：纯状态检查打断连续性、边界及减少动画直接到位。
- `gui::assistant::native_overlay_tests::assistant_chat_motion_keeps_layout_and_handles_interruptions`：隔离合成窗口检查焦点、尺寸、裁剪、快速反向、实际 Timer 收尾、收起续聊、减少动画、隐藏、切号、停用和销毁。原 V23 浮层屏幕夹具只补显式结束收起过渡，避免把动画期间仍显示当成错误；不扩大为本轮重新通过旧版全部屏幕验收。

上述夹具不启动用户 Hub、后台、账号或模型。管理会话已统一编译并执行这些定向用例，原生滚轮、Markdown、动画及菜单检查通过，最终输入与复用范围见 V24 交付。真实 Hub 滚轮、Markdown 观感／复制、Windows 多屏／DPI、动画与 macOS 仍待用户验收，不能以源码就绪替代构建或实机结果。

## 2026-11-11 客户端入口精简（本轮开发）

按用户要求隐藏 VS Code 和 WebStorm。侧栏最近三个位置、展开列表、客户端页面构建、配置位置选择、添加模型的来源候选、天工内置模型的跨端分享统一使用 `ClientId::VISIBLE` 或其 `is_visible` 判定。该列表保留 ChatGPT / Codex、WorkBuddy、天工 Work、DeepSeek Harness 和 ZCode；后台 `ClientId::ALL` 仍含全部七端。

读取旧 `recent-clients.json` 时，仅把隐藏端所在的槽位替换为尚未占用的可见端，保留其他槽位的顺序及最近使用信息；正常后续导航按原方式保存。已知隐藏端的旧跳转和助手导航统一返回概览并正常完成导航确认。概览活动图在 GUI 投影中移除隐藏端及其请求/MCP 连接，ChatGPT 状态区隐藏 VS Code 卡片及对应连接线；历史请求日志、来源筛选和统计归属继续保留，避免丢失既有记录的说明。

本轮不删除客户端配置、程序位置、模型副本和安装资料，不停用后台发现、writer 或 API。没有新增持久开关或配置格式迁移；恢复 GUI 可见列表及投影后，原有两端资料可继续展示。`ui-ux-pro-max` 两次定向查询没有命中适合原生桌面隐藏目的地的规则，未采用 Web 溢出或层级建议；本次沿现有原生导航处理。新增离线用例覆盖旧最近记录迁移、隐藏端导航回退、概览活动投影及后台身份保留。管理会话最终GUI测试目标编译及本批57项定向回归通过，包含新增及受影响的可见性用例，见 [V23交付](../releases/v1.0.23.md)。用户实机、Windows多屏及macOS待验收。

## 2026-11-11 操作反馈与二级弹层精简

状态：纳入 [V1.0.20 统一测试交付](../releases/v1.0.20.md)，Windows GUI 测试目标离线编译及本批 14 项定向用例通过，范围及记录见 [变更记录](../customizations/CHANGELOG.md)；这些文件/协议用例不代替真实界面验证，真实操作、Windows 缩放和 macOS 待用户验收。用户反馈恢复模型配置时出现重复“撤销更改”标题、无正文且只留“重试”的窗口，本次从共用任务交互及客户端调用处处理。

客户端页的打开客户端、保存程序位置、读取连接配置、保存模型、移除模型、恢复配置、导出 WebStorm 插件和安装 ACP 组件统一使用页内异步反馈。等待、完成和具体错误留在当前页面底部，不为这些动作单开进度窗口或再弹成功提示；执行时禁用相关编辑及提交区域，页面导航仍可使用。请求完成后恢复控件，失败保留未保存草稿；离开当前页仍完成收尾，后台读取结束不在别的页面上突然弹出模型编辑器。

真正的文件位置选择、连接配置表单、删除或覆盖确认继续保留。恢复只有一条记录时直接确认一次；有多条时选择一次，条目使用“最近第几次更改”和中文状态，不把内部 ID 作为主要说明。确定恢复后在页内显示结果。没有可恢复记录时同样采用页内提示。

维护入口为 `src/gui/dialog_job.rs::InlineJob` 与 `src/gui/clients.rs::action`：后台线程仅执行 HTTP，原生 Panel 的 Timer 接收结果后停止，UI 线程更新反馈和执行一次完成回调；同一任务执行中再次提交不会重放。为仍需同步取得结果再打开真实表单的旧入口保留 `run`，去掉正文重复标题、禁用整个进度窗口及固定“重试”按钮；失败展示具体原因与“关闭”，空原因提示先重新读取实际状态。不要把关闭错误窗口解释为请求已取消，也不自动重新发送写入请求。

采用 `ui-ux-pro-max` 的 UX 定向查询 `nonblocking action feedback modal` 与 `loading feedback`，核对其中 Loading Indicators / Confirmation Messages：反馈与等待相称，保留布局和焦点，并给简短完成说明。沿现有原生主题与字号，不移植 Web toast/ARIA 或移动触控尺寸。此调整不改变配置格式、writer、后改保护、账号权限和既有安装包；回退恢复相应 GUI 实现即可，用户配置及成果不删除。

## 2026-11-11 天才搭档原生遮挡修复（本轮开发）

用户确认 V1.0.22 已能对话，但小分队账户页的卡片、图表仍覆盖浮层和输入区。管理会话在 1168×754 实际窗口复现。原因在 wxWidgets 3.3.2 的 `wxWindowMSW::MSWGetStyle`：它以普通子窗不重叠为前提，不把 `wxCLIP_SIBLINGS` 映射到真正的 `WS_CLIPSIBLINGS`。此前在 `SimpleBook` 设置同名 wx 样式和调用 `Raise`，只能改变顺序，不能阻止嵌套页子控件在相交区域绘制。依据为本机本轮使用的 wxWidgets 源码及 [Microsoft 子窗口裁剪说明](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#clipping)。

本轮增加仅供重叠子窗显式调用的 `WxWidget::raise_child_overlay`，由 `src/gui/assistant.rs` 在展开、重新定位和页面切换后使用；普通窗口的 `raise` 行为保持。Windows 支撑把该浮层宿主的直接子窗设置为真正的原生兄弟裁剪，宿主启用子级裁剪，使用不激活的 `SetWindowPos(HWND_TOP)` 保持浮层在业务页分支之上。只在样式或顺序发生变化时使相交区域重绘，不扫描所有后代，不新增轮询计时器、桌面顶层窗或持久开关。纯图标入口、无产品标题、收起续聊和模型来源保持。macOS 走原生子级抬升，未据此宣称完成实机验收。

vendor 范围限于 `rust/wxdragon/src/window.rs`、`rust/wxdragon-sys/cpp/src/window.cpp` 与 `cpp/include/core/wxd_window_base.h`，不全局改写所有 Panel/Book 的创建样式。移除 Shell 上没有原生效果的同名样式调用。回滚相关 GUI 与此三个 vendor 改动即可，不迁移或删除用户配置、会话目录及已保存成果。

验证入口为 `gui::assistant::native_overlay_tests::assistant_overlay_stays_above_nested_native_pages_on_screen`，需显式 `--ignored --exact --test-threads=1`。该用例短暂显示隔离合成窗口，包含两层 `SimpleBook`、`ScrolledWindow`、原生多行 `TextCtrl` 和醒目图表 `Canvas`；覆盖浅深色、1168×754 / 900×560、顶层和嵌套切页、滚动、重新绘制、输入与发送命中、收起、最小化及隐藏恢复。断言检查真实 `HWND` 风格/父级/命中、业务叶级 DC 可见裁剪及屏幕像素；`TIANCAI_NATIVE_REVIEW_DIR` 指向忽略的 `target/` 子目录时保存实际 `ScreenDC` 图像，区别于旧隐藏 `WM_PRINT` 合成图。既有 `assistant_account_switch_and_pending_actions_fit_the_hub_overlay` 隐藏夹具补非零尺寸与原生层级检查。管理会话已统一编译和执行通过，详见下段；真实Hub新包复测、Windows多屏/DPI与macOS另列验收状态。

管理会话前两轮屏幕用例均在首个像素采样失败，原生层级及命中已通过；人工核对截图，红色业务图表在浮层边界截止，消息及输入完整。Rust 测试 EXE 未带正式 GUI 的 DPI manifest，夹具已增加 `PerMonitorV2` 线程上下文并在结束后恢复。第二轮元数据记录卡片宽 630 px，但合成屏幕中的卡片约 620 px、全窗边缘有额外留白，符合首显 DWM 过渡期间的整体缩放；不能据此按固定偏移修正坐标。[DwmFlush](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmflush) 仅等待已排队更新呈现，不保证窗口过渡完成。本轮仅在隔离夹具首次显示前设置 `DWMWA_TRANSITIONS_FORCEDISABLED`，避免动画表面与 HWND 最终矩形不同，生产窗口不作此设置。客户区/浮层/采样点、请求的 DPI 上下文及真实窗口 DPI 保存为元数据；原右沿采样点、纯色容差、叶级 DC 裁剪和命中断言保持，不放宽颜色阈值。修正后的用例已由管理会话重编并通过，约13秒完成16组屏幕截图及几何/像素/命中/DC检查，人工查看浅深色和大小窗口的4张代表图通过，见 [V23交付](../releases/v1.0.23.md)。前两轮失败记录保留；产品代码不因夹具调整改变，后续追加需求未修改浮层产品代码，复用本结果。

## V1.0.22 天才搭档窗口内对话浮层（2026-11-11）

对话改为 Hub 内容区内的普通子 `Panel`，不再创建独立 `Frame`、系统标题栏或另一项任务栏窗口。右下角入口只有 44×44 DIP 的 `Chat` 线条图标，中文名称与用途保留在原生名称和提示中；浮层内不重复产品名。顶部只放简短就绪/处理状态、清空和收起图标，详细状态仍在模块页，错误原因可通过状态提示查看。沿用既有浅深色 tokens、线条图标及焦点样式，新清空图标为 `visual::Icon::Trash`，不用 emoji。

浮层期望为 420×560 DIP，按内容区可用空间收窄和降低；上边界位于 44 DIP 主标题栏下，侧栏和底部预留入口行保持可用。位置随父内容区变化重新计算，受父级裁剪；页面切换保留展开状态。最小化、托盘隐藏及恢复随主窗控制，收起只隐藏消息区；重新打开继续同一临时对话和草稿。展开时焦点进入输入，Esc 收起并返回入口，浮层首尾动作的 Tab 形成可退出的本区键盘顺序。

消息采用可滚动的独立条目：用户消息靠右使用强调色，回复靠左使用轻表面；操作结果和提示有短标签。每条正文为可选择和复制中文的原生只读文本，不再把整段对话拼为日志编辑器。轮询沿稳定消息 ID 保留已有控件，流式输出只更新对应内容和尺寸；上滚阅读时保留滚动位置，已在末尾才跟随新内容。空态只显示轻量欢迎和两项可填入草稿的示例，点击示例不自动执行。

输入和底部动作固定，Ctrl+Enter 发送；空闲显示发送、运行中显示停止。清空、切换账户/临时对话和退出的既有内容边界保持，待删摘要和凭据动作在独立限高区域展示，凭据仍在本机密码表单输入。V1.0.21 的单次安装确认及 GUI 消费操作结果后解除忙碌状态保持。

沿用之前 `ui-ux-pro-max` 定向 `focus not obscured floating chat` 查询的焦点可见建议；本子任务未重跑该查询，本轮管理会话另已核对 `focus obscured overlay keyboard`。使用既有原生视觉体系，未新建 Web 设计系统或套用 ARIA/CSS。改动入口是 `src/gui/assistant.rs`、`shell.rs` 与 `visual.rs`。纯几何用例覆盖正常、小内容区和缩放输入；既有隐藏合成界面用例同步检查子级归属、纯图标入口、原生消息选区、切号草稿及动作边界。本次子任务只做格式和静态差异检查，编译与纯几何运行由管理会话集成，未运行原生 GUI、Hub、Server 或模型。真实观感、IME、滚轮、键盘、托盘和 Windows 多屏/DPI，以及 macOS 均待用户验收。无新持久配置，回退相关 GUI 源码即可恢复旧界面，不删除用户配置或主动保存的成果。

管理集成结果：V1.0.22 GUI 测试目标及 Windows release 编译通过，纯几何和安装提醒 4 项离线用例通过。初轮宽泛子串误选隐藏原生夹具，未先初始化父级布局导致零尺寸失败；只修该测试函数并重编，原生夹具未再次运行，不计入通过。正式界面观感和交互继续待用户验收，详细检查归属与唯一包见 `docs/releases/v1.0.22.md`。

## 2026-11-11 天才搭档与独立 Dumpling 入口（V1.0.18 历史）

V1.0.18 沿现有 wxDragon 原生主题实现天才搭档模块和 400×560 DIP 非模态对话小窗，最小 360×440 DIP，可缩放；该独立窗口在 V1.0.22 改为上节的窗口内浮层。浅深色使用原文字、背景、强调色和控件样式。侧栏显示“天才搭档”，通用 Skill/MCP 入口、客户端页签/概览卡片/管理跳转隐藏；Dumpling-MCP 独立导航，没有返回隐藏 MCP 页的二级返回按钮。

右下角对话按钮仅在启用且共享运行端就绪时出现，在内容底部预留一行位置，避免盖住正在编辑的配置或键盘焦点。小窗独立于主页面，收起或关闭只隐藏；重新打开继续同一临时对话。输入为原生多行编辑器，Ctrl+Enter 发送、Escape 收起，收起后焦点返回入口。停止、清空保持可见的文字动作；执行中展示简短状态，后台准备/错误保留输入。密钥只在带可见标签的原生密码表单填写；删除集中显示待删摘要与确认/取消按钮。

使用 `ui-ux-pro-max` 定向检索 `focus not obscured floating chat`，核对 `Focus Not Obscured (Minimum/Enhanced)` 后采用不遮挡焦点的布局建议；其 Web WCAG/CSS 示例不移植为 wxWidgets 机制。实现与状态边界见 [管理专题](../customizations/desktop-capability-management.md#2026-11-11-天才搭档与入口收敛)。本轮正在集成，尚未声明构建或离线布局通过；真实多屏/DPI、浮窗与托盘行为、安装确认及 macOS 由用户验收。

## V1.0.17 协助 MCP 真实模型连接（2026-11-10）

路由运行保留天工为实际发起端，真实本机协助模型请求另显示“协助处理 MCP”，从 MCP 节点连至实际模型与渠道。复用既有表面、MCP 线条图标、青绿色和模型列旁的留白，不增加普通客户端卡片；连接按 `route + model` 配对，同名外部模型不串线。节点详情显示“1 个模型请求”等带语境状态，不改变键盘焦点；这沿用 `ui-ux-pro-max` 的 `contextual live badge updates` 定向原则，Web ARIA 示例不移植为 wxWidgets 机制。

协助发起工具结束后，仍有真实模型响应则保留 MCP 节点和精确模型边；仅配置/目录读取/进度轮询不构成模型请求。等待时停流，完成或取消后按原三秒保持、两秒缓动暗淡，本次页面保留，重进清理；系统减少动画时仅保留状态证据。点击协助 MCP 节点进入该来源日志，列表、详情、筛选与用量分布统一中文名称；来源筛选增加此项，现有客户端次序保持。实现及验证边界见 [实施记录](v1-implementation.md#v1017-协助模型请求来源2026-11-10)。本轮未启动真实 Hub 或客户端，实机连线/缩放及 macOS 待用户验收。

本机协助设置扩为 620×650 DIP、最小 600×630 DIP，增加 CLI 准备入口和可选完全访问；完全访问开启后原项目写入选项禁用但保留其原值。沿既有主题、24 DIP 边距及固定底部动作，准备中保留状态且防止重复点击，失败保留草稿供重试。两张浅深离屏图的几何检查及管理人工查看通过，新增控件、说明和底部动作完整；对应定向回归、两个示例编译、Windows release 与唯一 MSI 见 [V17 交付](../releases/v1.0.17.md)，不扩大为真实运行或多屏缩放验收。

## V1.0.16 菜单与主题日期增补（2026-11-10）

主窗口不再挂载“文件 / Language·语言 / 主题 / 网络 / 帮助”旧菜单条，避免挤占或遮住一体化顶栏。外观/语言/网络、Dumpling和网页导入在设置；关于及“导出连接诊断…”在帮助，保留另存位置、异步导出及完成反馈。关闭/最小化/退出沿窗口及托盘；Windows系统右键菜单、贴靠/缩放与macOS交通灯保持。

日期触发器152×34 DIP，线条日历图标、浅深主题、轻描边、14 DIP内距、4/8 DIP日空间；点击年月切月份选择，选中与今天分别表达。原生按钮保留Tab/方向键/Enter/空格；Escape与外部点击关闭，无模态阻塞循环。`squad_view/calendar.rs`承载内容，vendor仅补父窗口管理的`TransientPopup`；账户/组织共用，ISO日期及默认今天/校验保持。沿用ui-ux-pro-max定向`date picker calendar`（ux）的明确日期格式建议及现行焦点、键盘、主题规则，没有另建视觉系统。

本批统一回归生成54张隐藏生产组件图，人工查看本机协助浅深设置及深色日历共3图，其余沿既有几何/流程断言；开发会话的7图复核及107张整页结果保留原验证快照归属。Windows release和唯一新MSI静态核验通过，见 [V16统一交付](../releases/v1.0.16.md)。隐藏合成检查不等于实机验收，外部点击、原生弹层焦点、多屏/缩放和macOS待用户验收。

V1.0.16 本机协助：Hub设置采用600×520 DIP原生对话框、24 DIP边距、浅深主题及固定底部动作，失败保草稿。天工内存提示层提供短进度/按需详情/单次人类询问、键盘焦点和reduced-motion。设置以隐藏合成截图和几何断言核对；浏览器阻止file预览后停止，真实天工提示层观感待验收。[专题](../customizations/tiangong-local-assistance.md)、[最终验证](../releases/v1.0.16.md)。

适用版本：V1.0.13 起，当前 V1.0.16。本轮为现有 wxDragon/wxWidgets 原生界面的视觉打磨，保留 V1 已实现的页面、业务服务和配置保护。当前实现及验证状态以 [本轮交付](../releases/v1.0.16.md) 为准；合成图不代表实机验收。

## 方向与依据

采用轻表面、紧凑分组、明确文字层级和单一青绿色强调色。让模型、记录和配置成为主体，减少表面之内再加表面的层层框线。Windows 主窗口采用与应用内容融合的标题栏，保留系统拖动、缩放、菜单和托盘关闭语义；macOS 保留原生交通灯和窗口。仍用 wxWidgets 原生输入及组件，不迁移 Web 前端。

按用户要求使用本机 `ui-ux-pro-max`：系统级设计检索后，进一步核对 `minimal enterprise dashboard`（style）、`visual hierarchy spacing consistency` 和 `focus visible keyboard`（ux）。采用经核对适合管理工具的 Minimalism & Swiss Style 中的网格、文字层级、低装饰、焦点和对比度建议。自动生成结果中的营销落地页、古典衬线字体不适合本项目，未采用，也未保存为产品设计依据。技能没有 wxWidgets 专项栈；其 Web/Mobile 示例不直接变成本项目实现，未将手机触控尺寸套到全部桌面控件。

## 主窗口外框（V1.0.16）

Windows 标题区与侧栏品牌区对齐为 44 DIP，右侧放页面位置、本机服务、账户及三个 46×44 DIP 的线条窗口控件；主标题及业务内容在其下。品牌图沿用既有海豚图案，不使用 T 字替代，EXE/安装器/托盘原图文件保持。顶栏使用同一内容面颜色，侧栏和顶栏自然连成整体；去掉额外系统标题文字和底部状态条，版本只保留侧栏底部。关闭悬停为告警色，其他窗口操作为轻底色；键盘焦点和中文可访问名称保留。每个业务页面复用同一个 Shell，不各自模拟窗口。

仅主窗口显式启用 vendor `WxdIntegratedFrame`；其他窗口保持原生。Windows 保留 `WS_CAPTION`、`WS_THICKFRAME` 等系统样式，只延展标题区；拖动空白与交互控件各自命中，双击、右键及 Alt+Space 沿系统，最大化按钮提供 `HTMAXBUTTON` 供系统贴靠。自绘按钮关闭进入原 `wxEVT_CLOSE`，不能绕过托盘/退出流程。边缘按当前 DPI 留缩放命中区域，最大化保留系统侧边和底边及工作区顶部边距。系统 DWM 管理阴影和 Windows 11 圆角；旧系统忽略可选圆角属性。macOS 继续原生标题栏/交通灯，不宣称已验证同款 Windows 外观。

实现：`src/gui/shell.rs`、`controls.rs`、`visual.rs`、`src/gui.rs` 和 vendor `frame.rs`、`frame.cpp`、`integrated_frame.h`。无配置格式迁移、无新开关；回退旧包恢复原外框和旧模型广场入口，不删除账户或模型数据。原生标题命中、负屏幕坐标、交互控件、可否决关闭和浅深小窗以隐藏生产组件检查；DWM 系统阴影、真实贴靠/多屏/缩放手感由用户验收。实现依据为 [Microsoft 自定义窗口框架](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe) 和本地 wxWidgets 3.3.2 源码；本轮技能定向检索没有匹配原生窗口框架规则，未套用其 Web 导航建议。

## 共用尺度

V1.0.16 天工增补：账号入口嵌在天工模型页的短行中；登录窗使用有可见标签的账号/密码输入、页内恢复提示及固定底部操作。分享通过二级窗口的三列客户端图标进行确认，已使用端排前，成功才保持点亮；目标区可在内容超过窗口时滚动，底部关闭/确认固定。模型列表沿原组件，内置详情代替不适用的保存/删除按钮，保留未保存草稿。按 UI/UX Pro Max 的 `accessible authentication error recovery` 定向建议使用可粘贴输入和可恢复错误；不把其 Web 栈示例套入 wxWidgets。浅深生产控件图和离线回归见本版增补交付，真实交互待用户验收。

V1.0.14 补充：分段按钮用强调色描边和底色表示真实选中，未选中的键盘焦点只用短标识，避免统计默认近 7 天却像选中今天。小分队正文只伸展当前页面；日期采用原生日历并默认今天。该版本的模型广场已在 V1.0.16 按用户要求移除；充值选择与支付预览分区，订阅用有层级的方案面板；继续遵守本规范的字阶、轻表面和 DIP 间距。定向检索 `date range picker defaults`（ux）的日期格式建议及 `comparison ranking pricing`（chart）的精确比较使用数据表建议；前一次 `pricing comparison checkout hierarchy` 返回泛化 Web 导航建议，未用作本轮设计依据。原生截图与边界见 [V1.0.14 交付](../releases/v1.0.14.md)。

| 项目 | 当前规范 | 落实位置 |
| --- | --- | --- |
| 页面边距 | 20 DIP；标题与主体使用同一水平起点 | `theme::PAGE_GUTTER`、Shell |
| 内容面内边距 | 常规 16 DIP，紧凑操作间隔 8 DIP | `PANEL_INSET`、模型和资源表单 |
| 间距 | 4 / 8 / 12 / 16 / 24 DIP | theme tokens |
| 普通操作 | 默认高 34 DIP；已有密集页按实际字体增高，不硬裁字 | controls、ClientHeader |
| 圆角 | 按钮 7 DIP；共用内容面 12 DIP；没有多层描边 | `BUTTON_RADIUS`、`SURFACE_RADIUS` |
| 主标题 | 16 pt、粗体 | `TextRole::PageTitle` |
| 分组标题 / 数字 | 12 / 20 pt、粗体 | `Title` / `Metric` |
| 正文 / 辅助 | 10 / 9 pt | `Body` / `Caption` |
| 中文字体 | Windows Microsoft YaHei UI；macOS PingFang SC | 原平台字体选择保留 |

点字号由 wxWidgets 按显示缩放处理；Canvas 使用逻辑像素并在绘制边界换算。不能再次乘 DPI，也不能在表单中混用物理间距和 DIP。输入框仍按真实原生编辑器 best size 加至少 12 DIP 上下空间，字体放大时随之增高。

## 表面、颜色与层级

- 应用底为很浅的中性色，内容面为白色；深色使用同一组蓝灰色相，表格不再突然换成另一种灰色。卡片通常靠表面和间距分组，只有输入、选中或焦点等有明确用途的部位保留细边。
- 正文、标签、辅助文字分三级，辅助文字也保持可读。浅色强调色略加深，选中底上的文字不因浅淡失去对比；主题回归检查正常文字、辅助文字、强调文字和主按钮、告警文字的 4.5:1 对比目标。
- 选中导航同时有浅色底、字重及短竖标；页签采用短下划线和选中文字，补足悬停和独立焦点描边。日期选中为填色片，键盘焦点另有描边/内标记，不能把焦点当成当前日期。
- 不添加装饰性持续动画。原运行流动、延迟淡出、系统减少动画偏好、状态证据及错误规则保持。

## 页面与操作

**窗口和导航**：顶部与内容底融合，浅层页面不再重复“工作台 / 当前标题”；显示所属区域。侧栏文字与图标统一间距，保留三个固定槽位、展开冻结顺序、下方展开和仅图标收起；当前窗口行为和原生系统外框不变。

**客户端模型**：页内添加、模型列表、配置正文使用同一组起点。字段区一层内容面，减少灰底套白底和任意外边距。原 WorkBuddy/天工完整编辑器与通用模型表单使用一致底色、标签间距及底部操作。模型页“添加模型”和“启动客户端”为次级操作，“保存模型”为主操作；状态概览仍突出启动。新增依旧是同一列表里的唯一内存草稿，未保存不写配置，按端实际能力显示参数。

**统计和日志**：请求、成功率、输入、输出合成一条紧凑数字区；不新增缓存或首字指标。筛选尺寸按内容收敛，曲线、悬浮和短窗切换保持。日志工具条与记录面分开，表格使用有内边距的轻表面，铺满剩余区域；普通/技术列、分页、实际请求数据和最小可读列宽不变。

**Skill/MCP**：使用主题线条图标和浅强调底，资源名称、使用状态、说明与底部动作分层。图标是展示元素，不用“禁用按钮”冒充图标。仍先进入二级选端、确认后真实应用；成功反馈、使用范围、部分失败和受保护恢复不改变。

**二级表单**：统一标题、正文/标签字号、字段间距、左右边距和固定操作区。保留原生输入、密码、IME、只读、事件、验证与业务反馈；长表单只让内容区滚动。现有专用对话框逐项沿用共享控件，不宣称所有系统弹窗都已定制重写。

按钮显式指定 `Secondary` 时不再因标题以“添加/保存”等开头被提升成主按钮。未指定样式的原有操作仍沿原默认规则，避免漏掉既有主要入口。该修改仅改变表现和层级，不改变事件或默认提交语义。

## 验证、兼容与回滚

主要检查使用 `examples/desktop_layout_review.rs` 中的**生产 Shell、表单和页面组件**；隐藏窗口、合成数据，保存 PNG 并检查布局边界。另用 `desktop_visual_review` 检查共用绘图、按钮各状态及绘制缩放；它不是实际全页布局。`native_model_review` 在隔离临时配置中使用实际七端控制器及旧完整编辑器，检查新增草稿、参数和原 revision。

覆盖 900×560 外窗最小尺寸、960×640 / 1366×900 / 1920×1080 客户区、浅深色、长中文、放大字体、键盘焦点、停用态及真实溢出。截图记录运行环境缩放；合成绘制缩放和原生字体放大不等于真实 Windows 多显示器 DPI 切换验收。macOS、用户已有数据和真实客户端由用户验收。

本轮不增加持久配置、不迁移目录、不变更模型/Skill/MCP writer、账户 API、路由、统计口径或日志。revision、锁、备份、原子替换、后改保护及 V1.0.12 的路径修复保持。回滚可重新安装原测试包或还原本轮 GUI 及发布元数据，不删除用户数据、旧包、历史报告或整个工作区未提交改动。

主要代码：[主题](../../src/gui/theme.rs)、[控件](../../src/gui/controls.rs)、[Shell](../../src/gui/shell.rs)、[模型](../../src/gui/model_fields.rs)、[原生组件](../../src/gui/widgets.rs)、[资源](../../src/gui/resource_gallery.rs)、[统计](../../src/gui/usage_view.rs)、[日志](../../src/gui/request_view.rs)、[二级表单](../../src/gui/squad_form.rs)。
