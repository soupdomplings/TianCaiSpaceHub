# GUI 与 wxDragon 维护

维护日期：2026-09-30。适用工作区：`0.4.29-2`。本文替代旧版 wxDragon 同步与资源优化执行计划；旧文档中的 `0.9.16`、临时脏文件和 stash 操作不再代表当前状态。

## 依赖来源

[Cargo.toml](../Cargo.toml) 声明 wxDragon `0.9.17`，通过 `[patch.crates-io]` 指向 [vendor/wxdragon](../vendor/wxdragon)。升级时需要一并考虑 vendored 的 `wxdragon`、`wxdragon-sys`、`wxdragon-macros` 和底层构建，而不是只改版本字符串。

依赖同步应作为有明确目标的开发任务记录基线与差异，不在一般功能开发中顺带升级，也不执行旧文档针对当年工作区的提交/stash 命令。

## 当前刷新机制

- [gui.rs](../src/gui.rs) 当前 dashboard 刷新常量为 10 秒，请求日志刷新常量为 5 秒；这些是配置的轮询间隔，不是实测性能指标。
- `GuiTimers` 管理周期任务，托盘隐藏和恢复时调整刷新，退出时停止定时器。
- 网页导入的接收和预览使用独立 Panel 作为计时器 owner；模态预览期间接收继续工作，展示回调暂停以避免重入。模型获取对话框的短周期刷新仅服务于该临时操作。
- 单次或后台操作应避免阻塞 GUI 主线程；沿用异步任务和结果回传，不能在渲染回调中执行长网络请求。

维护入口：[GUI 与计时器](../src/gui.rs)、[托盘](../src/gui/tray.rs)、[导入预览](../src/gui/external_import.rs)、[dashboard API](../src/web.rs)。

## 后续优化记录

如再次处理 CPU/内存问题，专题中写明触发场景、实际采样依据、涉及刷新任务和最终行为；没有采样不填性能降幅。事件推送、虚拟列表等旧建议只是备选方案，不视为已经交付或自动排期。

测试安排遵守 [开发约定](development/documentation.md)，当前用户自行验收；本次文档更新未运行性能或 GUI 测试。
