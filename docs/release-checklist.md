# 二开交付与发布检查表

维护日期：2026-10-03。开发交付和远程发布是两个动作；本表不自动授权推送、打标签、发布、安装或启动用户应用。当前用户负责测试，未执行的项记录为待验收。

## 文档与版本

- [ ] 对应专题写明当前行为、默认值、平台、配置兼容及撤销方式。
- [ ] [二开变更记录](customizations/CHANGELOG.md) 登记本次开发，实际行为变化同步到 [总表](customizations/README.md)。
- [ ] 新增/删除/替代文档同步 [索引](README.md) 和引用；过时内容按 [维护规范](development/documentation.md) 清理。
- [ ] 产品发版时核对 `Cargo.toml`、`Cargo.lock` 和产物版本，更新 `RELEASE_NOTES.md`、`UPDATE_NOTES.md`（最多四条）。纯文档变更不自行升版本。
- [ ] 分别写明实现、构建、测试、用户验收、发布状态，不复用旧版本测试数字证明新版本。

## 构建与交付

Windows 本地编译核对参考；Cargo package 为 `tiancaispacehub`，binary 为 `TianCaiSpaceHub`：

```powershell
cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub
cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub
```

Windows/macOS 发布安装包统一由 GitHub Actions 生成，本地不打发布包；构建测试代码不表示已执行测试。产品名、文件名与升级兼容规则见 [品牌与交付](customizations/desktop-and-packaging.md)。

- [ ] 记录具体平台与 profile，未构建的平台不写已验证。
- [ ] 提供源提交/工作区变更、版本、签名状态和 SHA-256；保留可恢复的源状态。
- [ ] 程序、App、CLI 帮助与安装资源使用 `TianCaiSpaceHub`；保留配置/环境变量/协议和升级身份，单独验收旧 MSI 升级和 macOS 旧名称 App 迁移。
- [ ] 不覆盖已交付的同名包或其构建清单；文档整理无需重建二进制。
- [ ] 检查配置、凭据、日志、私人截图及运行状态不进入代码提交或安装包。
- [ ] 保留现有 LICENSE 及第三方资源来源；不把构建缓存清空作为每次发布的默认步骤。

## 用户验收参考

按本次变更选择范围，结果由执行者实际登记。当前测试由用户完成，开发方不为了勾选此表主动启动 Hub、Codex 或真实付费调用。

| 涉及功能 | 建议覆盖 |
| --- | --- |
| 网页导入 | 首次/已运行唤起、注册及路径空格、来源确认、取消、过期、连续点击、四种协议、同源/同名冲突、空模型、并发保存、显式启用和可见模型 |
| WorkBuddy | 选择与保存、备份恢复、账号引用、模型/别名、思考强度、协议缓存、重试及流式边界 |
| 动态模型 | 手动输入、远端获取、同步、路由补齐、别名及能力覆盖、重启后保留 |
| 桌面/打包 | 当前品牌、最大化、托盘退出、主题、版本、升级/卸载与协议关联归属 |
| 上游整合 | [二开总表](customizations/README.md) 各保留项及新合入功能的衔接 |
| IM/remote-control | 连接、新建/恢复会话、回复及审批收口；仅在该链路受改动影响时纳入 |

## 发布前最后确认

- [ ] 用户已授权本次发布动作和目标，文档没有把“包已构建”写成“已发布”。
- [ ] 未验收问题和回滚方法已写入相关专题与交付说明。
- [ ] 需要清理分支时已核对合并关系、服务器现状和 worktree 占用；不删除有独有提交或正在使用的分支。
