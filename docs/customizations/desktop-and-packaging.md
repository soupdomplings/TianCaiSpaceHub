# 天才空间品牌、桌面行为与交付

维护日期：2026-09-30。适用版本：`0.4.29-2`。关联 TC-001、TC-007；网页导入另见 [专门说明](../hub-external-import.md)。

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
- GUI 保持单实例。普通重复启动退出；Windows 网页导入入口会把导入请求交给现有实例，无法转交时明确报错。
- 自 `0.4.28-2` 起，帮助菜单和托盘中的“检查更新”已移除，启动也不自动检查。历史更新模块或 CI 更新清单存在，不代表当前 GUI 有更新入口。
- 主题和语言沿用现有设置；主题实现见 [GUI 主题](../gui-theme.zh-CN.md)。

维护入口：[GUI](../../src/gui.rs)、[托盘](../../src/gui/tray.rs)、[浏览器打开](../../src/gui/browser.rs)。OAuth 仍需打开浏览器，不能在清理更新模块时一起删除。

## 平台与产物

新开发以 Windows 为先，macOS 为后续优先级，Linux 不纳入。历史 CI 仍有三平台工作流；本次保留它们，不将其存在解释为新增功能已完成多平台测试。

- [Windows 工作流](../../.github/workflows/release-windows.yml)：MSI、便携 ZIP；有签名凭据时签名。
- [macOS 工作流](../../.github/workflows/release-macos.yml)：DMG、App ZIP；保留签名/公证流程和无 Developer ID 凭据时的测试包路径。ad-hoc 签名不等于 Developer ID 签名或公证。
- [Windows 本地打包](../../scripts/package-hub-import.ps1)：基于已构建的程序生成导入测试包，拒绝覆盖同名产物，并记录版本、构建 profile、基线提交、本地变更、签名状态和 SHA-256。

Windows 本地构建与打包：

```powershell
cargo build --locked --release --features gui --bin codexhub
./scripts/package-hub-import.ps1
```

`0.4.29-2` MSI 的三段 ProductVersion 为 `0.4.29`，二开后缀由程序版本和分发文件名标识；现有 WiX 配置允许同 ProductVersion 升级。安装升级结果必须单独验收，不能只依据构建成功判断。

## 更新与回滚

升级通过取得指定版本安装包或替换便携程序完成。先正常退出旧 Hub 并备份配置，避免两个版本同时写入同一配置。恢复旧程序时同时检查配置兼容，按需恢复备份。

网页导入 MSI 使用系统级关联，便携版使用当前用户关联；用户级关联可能优先，移动便携程序后需重新注册。只解除当前程序拥有的关联，详见 [网页导入](../hub-external-import.md)。

根目录 `RELEASE_NOTES.md` 和 `UPDATE_NOTES.md` 仍被发布工作流读取，不能当作旧残留删除；后者保持最多四条。新包必须记录实际构建来源和是否签名，不自动发布或覆盖已交付文件。

## 当前交付状态

`0.4.29-2` 首批本地 MSI/ZIP 保留原样；GitHub 交付采用按提交源码重新打包的 Windows 预发布产物，见 [版本交付说明](../releases/v0.4.29-2.md)。包未签名，主站联调、安装升级卸载和浏览器唤起由用户验收；macOS 网页导入未实现。
