TianCaiSpaceHub v0.4.30-7 本地测试：补齐组织设置错误的程序路径修复（2026-10-20）

“检查与修复 → 无法加载组织设置 / 启动失败”现在分别检查程序路径与状态库。即使未发现迁移冲突，也能按实际安装位置重新绑定 CODEX_CLI_PATH，不再因数据库正常而禁用路径修复。

Windows 更新当前用户环境，macOS 更新当前登录会话；先备份原值，再核对来源和后续变化，支持在修复记录中恢复。路径和数据库修复分别反馈、分别备份，Hub 保持运行，只要求退出 ChatGPT/Codex。原 13 类修复中心、增强启动 v24、Commands 模式和会话保护保留。

本轮版本递增到 v0.4.30-7 / Windows 数字文件版本 0.4.30.7，源码已推送到 main（3b96fa6）。Windows x64 和 macOS universal Actions 均已确认开始，本轮生成 Actions artifacts，包结果和实机待确认，未创建标签或发布 Release。本地包、隔离测试、构建及运行链接见 [v7 交付](docs/releases/v0.4.30-7.md)，详细范围和回滚见 [启动修复](docs/customizations/codex-startup-repair.md)；[v6 测试包](docs/releases/v0.4.30-6.md) 保持原身份。
