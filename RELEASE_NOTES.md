TianCaiSpaceHub v0.4.30-8：天工 Work 适配与接入页面统一（2026-10-27）

适配天工 Work 的任务收尾、主任务审批、IM/桌面模型绑定、默认流式上下文和后台运行/重连保护，兼容旧协议。Hub 不再设置天工默认模型，保存保留原状态。

WorkBuddy、天工 Work 统一管理模型、渠道名称和主要按钮，隐藏内部地址/Key/路径等展示；思考强度并入 WorkBuddy 连接卡片。两页与 Dumpling-MCP 共用只读/编辑底色，原路由、凭据和备份行为保留。

版本递增至 v0.4.30-8 / Windows 0.4.30.8。Windows 本地测试包已交付，用户初步反馈“暂时没问题”并授权发布；162 个不同 Rust 聚焦用例、20 个桌面同步 JS 用例、4 个实际函数离线用例、Windows GUI 编译及 MSI 静态核验通过。Windows x64、macOS universal 的分发包由 GitHub Actions 生成并上传本 Release，本地 debug MSI 不作为正式分发包；构建开始不等于产物完成，macOS 实机及完整边界仍待验收。源码身份、兼容/回滚和各阶段状态见 [v8 交付](https://github.com/soupdomplings/TianCaiSpaceHub/blob/main/docs/releases/v0.4.30-8.md)。
