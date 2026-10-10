# GUI 与 wxDragon 维护

维护日期：2026-10-10。当前基线 `0.4.30-5`，唯一 `main`。本文替代旧版 wxDragon 同步与资源优化执行计划；旧文档中的 `0.9.16`、临时脏文件和 stash 操作不再代表当前状态。

发布状态：用户对本轮本机测试版反馈“可以了”并授权发布 `v0.4.30-5`，当前准备标签、Actions 构建与 Release。该反馈只属于此次本机试用和可交付判断，不扩大为 Windows MSI、macOS 或系统退出全部场景验收；Windows/macOS 安装包由 GitHub Actions 生成，实际成功结果及产物身份由 [v5 交付](releases/v0.4.30-5.md) 后续登记。

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

本轮实现及事件 ABI、取消语义、启动并发收尾的静态复核已完成，最终 Windows locked GUI build 通过（22.88 秒、38 条警告），包含发布后关机握手；独立 EXE 版本、x64 PE、DLL 导入及复制哈希静态核对通过，最终身份见 [v5 交付](releases/v0.4.30-5.md)。开发方未实际关机/注销、启动 GUI 或执行测试；系统取消关机、普通 ×、强制关闭和自有/外部后台边界待用户验收。session hooks 在 Windows/macOS 注册，macOS 此轮仅条件编译下的源码核对，原生编译待 Actions，不宣称系统退出已验证；本轮不增加 Linux 支持。此修复不新增配置字段或数据迁移，回退 GUI 与整套窄事件扩展可能恢复关机阻止，回退后可在关机前从托盘“退出”。Windows MSI 安装成功页及其 Actions 构建边界另见 [桌面与交付](customizations/desktop-and-packaging.md#平台与产物)，本地 EXE 编译不验证安装向导。

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
