# GUI 主题维护

维护日期：2026-09-30。适用工作区：`0.4.29-2`。这是沿用的上游 UI 基础能力；天才空间桌面定制见 [品牌与桌面](customizations/desktop-and-packaging.md)。

当前已经具备系统、浅色和深色三种主题。旧设计稿中“没有暗色模式、还需创建主题模块”的描述已过时，由本文替代。

## 当前实现

- [theme.rs](../src/gui/theme.rs) 提供 `ThemeMode`、配色及字体/间距等公共设计值；新增页面应复用这些值。
- `AppConfig.theme` 保存 `system`、`light` 或 `dark`；`auto` 作为 system 的兼容输入。
- [gui.rs](../src/gui.rs) 在创建窗口前应用 wxWidgets appearance 并初始化主题；菜单切换保存偏好后提示重启。
- 原生控件外观与自绘颜色共同适配；新增窗口不能只改背景而遗漏文字、边框或禁用状态。

## 修改时的约束

`set_appearance` 必须在创建窗口前调用。保持主题和语言偏好独立，不把主题切换写成已经支持运行中即时切换。布局尺寸应允许中文和较长模型/服务地址显示，必要时使用滚动区域。

代码入口：[主题](../src/gui/theme.rs)、[通用控件](../src/gui/widgets.rs)、[GUI](../src/gui.rs)、[中英文文案](../src/gui/text.rs)。具体 UI 验收由用户完成；本次仅依据代码更新文档，未新增 GUI 测试。
