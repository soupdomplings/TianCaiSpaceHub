# TianCaiSpaceHub V1.0.61 · 会话移动、项目修复与模型同步

会话移动显示选中数量和目标，点「确认移动」后执行；取消、Esc 或关闭不执行。确认窗按系统缩放和控件所需空间布局。

项目检查区分整库分组、可补卡片和待重分会话，支持当前 SQLite 活动账号侧栏及旧 JSON。无法补回时显示原因和下一步；SQLite 修复回执为第 6 版，撤销需使用本版或兼容版本。

空模型清单可直接跨渠道勾选保存，非空清单启用 Hub 网关；已接入本 Hub 时补齐缺失目录及发现设置，自定义目录或显式禁用分别提示，失败可重试。

Mac 请在本次 Release 的 Assets 中选择 `TianCaiSpaceHub-v1.0.61-macos-universal.dmg`，Apple 芯片与 Intel 使用同一份；Windows 可选 x64 MSI 或便携 ZIP。

本次也包含前几个本地测试版的更新：远程会话支持处理追问和审批、下载会话文件，目标电脑可以分别控制思考、工具、文件变更和产物的显示；桌面设置重新分组，Windows 支持开机启动。

13 项 Windows 定向回归和本地 MSI 载荷核验通过。真实安装、原项目恢复、会话移动及客户端模型菜单仍待实机验收。[完整验证及验收步骤](https://github.com/soupdomplings/TianCaiSpaceHub/blob/v1.0.61/docs/releases/v1.0.61.md)
