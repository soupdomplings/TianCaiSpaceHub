TianCaiSpaceHub V1.0.19：Harness 状态与四端启动修复（2026-11-11）

修复 DeepSeek Harness 已打开但 Hub 显示“未打开”的问题，识别带空格的桌面主程序名；启动器按安装登记定位桌面程序，支持自定义盘符和目录，保留旧 CLI 入口。模型成功导入后可直接启动客户端，无需额外账号登录；本机 Harness 0.2.2-rc.1 的欢迎页凭据规则已静态核对，既有模型 Profile 和凭据引用格式保持。

参考固定 Magpie 提交补齐四端共用 Windows 安装发现。VS Code 支持 PATH 的 code.cmd/code 入口，复用便携版、产品身份、版本目录和 Scoop 解析；ZCode、WebStorm 补齐已登记的自定义安装位置，WebStorm 支持用户 Programs 和 JetBrains 版本目录。AI Assistant Bridge 与 ACP 组件仍沿各自适配路线。

保留 V1.0.18 天才搭档、共享 Server、新版 Ultra/Fast 等累计开发；不迁移或覆盖用户客户端配置。32 个不同定向离线用例已通过，未变化的 V1.0.18 检查沿用原归属，不重复累计。Windows release 与唯一新 MSI 的实际构建、静态核验和包身份见 [V1.0.19 交付](docs/releases/v1.0.19.md)，实现及配置兼容见 [客户端适配](docs/customizations/desktop-client-adapters.md#deepseek-harness-状态与四端启动修正)。

实际安装、四端打开/模型选择/调用及 macOS 待用户验收；未运行产品、客户端或安装器，未提交、推送、触发 Actions、创建标签或 Release。历史 V1.0.18 包保留原身份。
