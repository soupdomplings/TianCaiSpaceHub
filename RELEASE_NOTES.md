TianCaiSpaceHub V1.0.33：天工 Work 1.1.0 兼容测试版（2026-11-16）

根据本机实际 Work 1.1.0 和官方清单核对，修复大版本更新引入的本地会话账号、renderer 请求头、协助 IPC 和运行协议差异。未登录时通过 Hub 增强启动使用已保存外部模型；AI Gateway 与官方登录独立，官方云端资源继续按真实账号权限使用。

历史管理适配任务／项目 owner_id、默认任务独立工作目录和后台运行状态；迁入仅处理选中历史，跨账号项目复制项目壳，撤销恢复原绑定，保留原 session／agent／project 和记忆、附件。模型列表采用原生显示名，不改变真实 ID；保存前核对输出与上下文预算，内置分享和 MCP 基础接口继续复用。

IM 与远程续聊承接原账号、模型参数、目录和会话版本，分离子智能体内容和审批，补检查点等待／继续及真实停止。协助通过原生 IPC 钩子接入，保持单次提交和原生审批，不覆写冻结 electronAPI 或修改安装资源。

本次仅提供同批一份 Windows 本地 MSI，由用户自行安装测试。请真正退出旧 Hub 与天工后安装，再从 Hub 增强启动。静态／合成检查、Windows 构建与包核验分别登记；macOS 原生资源和实机待验。未触发 Actions、标签或 Release。

完整依据与回滚见[天工当前核对](docs/customizations/tiangong-work-compatibility-assessment.md)，实际构建、包身份和验收重点见 [V1.0.33 交付](docs/releases/v1.0.33.md)。
