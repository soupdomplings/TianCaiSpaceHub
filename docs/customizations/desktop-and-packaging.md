# 天才空间品牌、桌面行为与交付

维护日期：2026-10-03。适用源码版本：`0.4.30-2`（本地开发版，未发布）。关联 TC-001、TC-007；网页导入另见 [专门说明](../hub-external-import.md)，上游合并见 [v0.4.30 整合](../upstream-v0.4.30-integration.md)，当前产物与验证见 [开发交付](../releases/v0.4.30-2.md)。

## 名称与兼容身份

显示品牌为 **TianCaiSpace Hub**，覆盖窗口、托盘、程序图标、Windows 安装与快捷方式、macOS App 名称及分发文件名。首次二开见提交 `76e58f3`。

| 内容 | 当前值或位置 | 维护规则 |
| --- | --- | --- |
| Rust crate / CLI | `codexhub` | 与显示品牌分离，不因改文案重命名协议和数据 |
| Windows MSI 程序 | `TianCaiSpace Hub\CodexHub.exe` | 保留安装路径与升级身份 |
| Windows 便携程序 | `TianCaiSpace Hub.exe` | 路径可含空格，协议注册命令必须正确引用 |
| Windows UpgradeCode | `42B4BF3C-E660-4C83-96AA-56A4426B96A2` | 保持升级连续性，不随版本生成新值 |
| macOS 显示名 / 内部程序 | `TianCaiSpace Hub` / `CodexHub` | `Info.plist` 与包内文件一致 |
| macOS Bundle Identifier | `com.codexhub.app` | 保留既有安装身份 |
| 图标资源 | [Windows 图标](../../packaging/icons/AppIcon.ico)、[macOS 图标](../../packaging/macos/AppIcon.icns)、[品牌原图](../../assets/tci-hub-icon.png) | 变更时同步各平台所需尺寸及资源引用 |
| 厂商图标 | [来源说明](../provider-logo-assets.zh-CN.md) | 保留来源和许可证 |

早期上游曾由 `codex-remote` 改名为 CodexHub。该迁移已经结束，不再执行旧分支合并、远端仓库改名或目录迁移步骤；原始记录可从 Git 历史追溯。

## 当前桌面行为

- 主窗口每次启动最大化到系统工作区；不要求用户手动拖大。
- 关闭窗口隐藏到托盘/菜单栏；需要结束程序时使用“退出”。退出流程停止 GUI 定时器并处理本次启动的后台进程。
- GUI 保持单实例。Windows 网页导入通过命名管道交给现有实例；macOS 使用系统 URL 事件，CLI/重复 App 通过同用户 Unix socket 转交，失败给出脱敏提示。macOS 重复 App 会短时维持隐藏事件循环接收初始 URL，随后退出。
- 自 `0.4.28-2` 起，帮助菜单和托盘中的“检查更新”已移除，启动也不自动检查。`0.4.30-1` 保留上游更新诊断源码及其导出覆盖；更新模块仍仅在 `cfg(test)` 下编译，生产入口继续关闭，不能据此宣称本版会主动生成更新诊断日志。既有导出过滤规则已经支持相应日志，本轮只合入新增导出夹具。
- 主题和语言沿用现有设置；主题实现见 [GUI 主题](../gui-theme.zh-CN.md)。

维护入口：[GUI](../../src/gui.rs)、[托盘](../../src/gui/tray.rs)、[浏览器打开](../../src/gui/browser.rs)。OAuth 仍需打开浏览器，不能在清理更新模块时一起删除。

`0.4.30-1` 合入上游 Windows 增强启动预检修复：主检测和备用检测共同核对官方安装包身份与桌面主程序路径，兼容 `Codex.exe`、`ChatGPT.exe`，避免把 npm Codex CLI、后台 app-server 或第三方客户端误认作官方桌面进程。该变化不恢复更新入口；新增界面文字继续使用 TianCaiSpace Hub 品牌。上游曾报告增强启动及插件列表恢复，该反馈属于上游历史，本地新版行为仍待用户验收。Chrome 认证兼容仍是独立未解决事项，详见 [认证说明](../auth-notes.zh-CN.md)。

## 平台与产物

新开发以 Windows 为先，macOS 为后续优先级，Linux 不纳入。`0.4.30-2` 发布起，版本标签只自动构建 Windows 和 macOS；Linux 历史工作流保留手动入口，macOS 发布不再等待 Linux 清单。不将历史工作流存在解释为新增功能已完成多平台测试。

- [Windows 工作流](../../.github/workflows/release-windows.yml)：使用 Cargo 锁文件构建 MSI、便携 ZIP，ZIP 包含导入说明；有签名凭据时签名。
- [macOS 工作流](../../.github/workflows/release-macos.yml)：DMG、App ZIP；包含 `tiancaispacehub` 协议声明和导入说明，签名前检查 plist 和双架构程序。保留签名/公证流程和无 Developer ID 凭据时的测试包路径。ad-hoc 签名不等于 Developer ID 签名或公证。手动对分支构建只上传 Actions artifact，标签触发才发布 Release。
- 标签带 `-` 的二开版本沿用 Pre-release 标记，不自动设为 Latest；macOS 发布前最多等待 30 分钟确认同版 Windows 清单。
- [Windows 本地打包](../../scripts/package-hub-import.ps1)：基于已构建的程序生成导入测试包，拒绝覆盖同名产物，并记录版本、构建 profile、基线提交、本地变更、签名状态和 SHA-256。

用户已要求所有发布安装包通过 GitHub Actions 生成；Windows 和 macOS 分别运行对应工作流，手动对分支构建时从 artifact 获取安装包。以下本地打包命令仅保留为历史维护参考，不用于本次及后续发布：

```powershell
cargo build --locked --release --features gui --bin codexhub
./scripts/package-hub-import.ps1
```

`0.4.29-2` MSI 的三段 ProductVersion 为 `0.4.29`，二开后缀由程序版本和分发文件名标识；现有 WiX 配置允许同 ProductVersion 升级。安装升级结果必须单独验收，不能只依据构建成功判断。

## 更新与回滚

升级通过取得指定版本安装包或替换便携程序完成。先正常退出旧 Hub 并备份配置，避免两个版本同时写入同一配置。恢复旧程序时同时检查配置兼容，按需恢复备份。

网页导入 MSI 使用系统级关联，便携版使用当前用户关联；用户级关联可能优先，移动便携程序后需重新注册。只解除当前程序拥有的关联，详见 [网页导入](../hub-external-import.md)。

macOS 保持 `com.codexhub.app` 的安装身份，完整 App 声明导入协议；菜单重新注册只更新 Launch Services，不改写签名内容。关联随 App 由系统管理，移除旧副本或从保留副本重新注册，不提供 Windows 式解除注册。

根目录 `RELEASE_NOTES.md` 和 `UPDATE_NOTES.md` 仍被发布工作流读取，不能当作旧残留删除；后者保持最多四条。新包必须记录实际构建来源和是否签名，不自动发布或覆盖已交付文件。

## 当前交付状态

`0.4.29-2` 首批本地 MSI/ZIP 保留原样；GitHub 交付采用按提交源码重新打包的 Windows 预发布产物，见 [版本交付说明](../releases/v0.4.29-2.md)。后续 Windows 本地包及用户反馈见 [导入专题](../hub-external-import.md)。`0.4.29-5` macOS 网页接入代码已补齐，尚未在 Mac 构建或实机验收，也未生成新版 macOS 分发包。

`0.4.30-1` 已完成 Windows x64 GUI/测试代码编译和调试程序构建，该记录属于前轮整合版本；后续 WorkBuddy 多模型及用途标识的 `0.4.30-2` 本地及 Actions 产物单独登记在 [交付记录](../releases/v0.4.30-2.md)。用户已授权本次 GitHub 发布；功能测试、桌面交互和安装升级仍待用户验收，构建成功不代表这些项目通过。本轮不修改用户运行中的 Hub 配置、协议关联或已交付包。
