# 数据加载反馈

维护日期：2026-11-11。适用 V1.0.26，关联 TC-016、TC-017、TC-019、TC-023。源码已实现，管理会话已完成最终 Windows GUI 测试目标编译和本节两项用例；真实界面与 macOS 待验收。操作结果与确认窗口继续见 [统一操作反馈](operation-feedback.md)，页面切换见 [桌面视觉规范](desktop-visual-system.md)。

## 当前行为

真正开始异步读取或操作后，复用原有状态文字或刷新按钮，约 180 ms 后显示不定进度：状态文字使用少量循环小点，按钮使用原生圆点环。提前完成的操作不显示加载动画；完成立即结束，不设置最短等待，不延迟业务结果。组件不生成额外等待窗口、页面日志、缓存或百分比，也不逐行动画或重新布局列表。

读取期间保留已有数据、选择和编辑草稿。请求记录每 5 秒的后台刷新不重复提示，行数据、页码、技术视图和总数均无变化时不再重建表格，保留选区和滚动位置；实际筛选、翻页和进入页面仍提供加载反馈。小分队同一子页刷新继续保留现有内容，首次账户恢复及不同子页仍沿原恢复/占位流程，不能将静态占位误报为动态列表。

减少动画复用已有 `reduceMotion` 和系统动画偏好：文字保持静态说明，按钮显示静态圆点环，不启用循环动画计时器。页面隐藏、Hub 隐藏到托盘或最小化时停止动画；重新显示时仅仍在执行的操作恢复指示。后台请求及原结果消费继续按各自生命周期运行。

## 接线与覆盖

共用实现为 [loading.rs](../../src/gui/loading.rs)。只在 GUI 线程操作，使用原 busy、请求和结果边界，不新增重试、请求或任务队列。

```rust
let loading = LoadingIndicator::new(&page, status);
// 或复用当前页面的刷新/获取按钮：
let loading = LoadingIndicator::for_button(&page, reload);
loading.start("读取中…"); // 原请求已进入 busy 后调用
loading.finish();        // 写入实际结果前立即调用
```

`LoadingIndicator` 可克隆，不重复创建对应原生控件。相同 pending 文案不会重置延迟；结束只恢复自己仍持有的文字与显示状态，实际结果或页面已更新文字时不回写旧值。按钮的大小和选中状态保持。每个组件使用自身隐藏 Panel 托管 Timer，避免 wxDragon 同 owner 的 TIMER 回调影响业务轮询。文字小点周期 280 ms，按钮环每 50 ms 更新一帧，减少动画后只保留一次静态呈现。

主窗口已有 `on_idle` 只增加一次 `loading::refresh_pending()`，处理仍 pending 指示的可见性与动画偏好变化。组件不再次绑定根窗口 idle，不请求连续 idle，也不建立可见性扫描计时器。结束和销毁释放待显示状态与计时器。

| 入口 | 复用位置及完成边界 |
| --- | --- |
| [dialog_job.rs](../../src/gui/dialog_job.rs) | 页内异步任务、账户通用读取/操作和真实保存表单；实际完成先结束指示，错误仍保留草稿并沿共用反馈显示。准备 Server 等必要等待说明保留在原状态行。 |
| [squad.rs](../../src/gui/squad.rs) | 当前账户页读取复用刷新按钮，页内操作复用状态行；接受当前 serial 的结果后结束。账户和页面切换保护保持。 |
| [clients.rs](../../src/gui/clients.rs)、[gui.rs](../../src/gui.rs) | 客户端模型读取与渠道远端模型获取复用各自按钮；不写共享 Shell 状态行。后台客户端目录轮询保持安静。 |
| [request_browser.rs](../../src/gui/request_browser.rs) | 实际可见请求记录页的首读、筛选与翻页，结果消费后结束；后台刷新不重播。 |
| [gmclaw.rs](../../src/gui/gmclaw.rs)、[workbuddy.rs](../../src/gui/workbuddy.rs) | 既有 `begin_action` 与 `apply_result` 的 busy 边界，保留原配置与草稿处理。 |
| [nvwa.rs](../../src/gui/nvwa.rs)、[resources.rs](../../src/gui/resources.rs) | 既有操作或读取状态；只接已有异步入口，不扩展资源扫描及客户端业务。 |

同步缓存投影继续即时显示。模型按钮容器与渠道列表不添加逐行加载或进入动画，通用 Skill/MCP 的隐藏规则保持。

## 兼容、回滚与验证

没有新配置、API、持久化文件或数据迁移。回滚移除 `start`/`finish` 接线和唯一 idle 调用后恢复原状态文字即可，不影响已保存模型与账户资料。

新增定向用例两项，已由管理会话执行通过，纳入本批 57 项不同 Rust 检查：

- `gui::loading::tests::loading_delay_dots_and_reduced_motion_follow_real_elapsed_time`：延迟、时间阶段、减少动画及隐藏暂停的纯状态判断。
- `gui::loading::tests::hidden_loading_uses_a_private_timer_and_preserves_new_result_text`：隐藏原生夹具，不显示产品窗口、不发请求；核对私有 Timer、快速结束、pending 回收、按钮原值/选择恢复，以及不覆盖实际结果。

本开发会话已执行定向 `rustfmt` 和 `git diff --check`，未运行 Cargo、实际 Hub、客户端、Server、安装器或真实模型调用。最终构建、用例结果和唯一测试包身份以 [V1.0.26 交付](../releases/v1.0.26.md) 为准；Windows 真实动画、隐藏/恢复、缩放与减少动画效果待用户验收。
