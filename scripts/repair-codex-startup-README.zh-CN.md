# Codex「无法加载组织设置」检查与修复

适用于 Windows / macOS。先完整解压，三个脚本放在同一目录。

需要 Python 3.9+（含 sqlite3），推荐 3.11+；不会自动安装依赖。Python 3.9/3.10 且存在 config.toml 时，要额外明确指定实际状态库目录。

请先正常退出 ChatGPT/Codex、Hub 和同名后台进程；脚本会检查，不会替你强杀它们。

## Windows

在解压目录打开 PowerShell，先只检查：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\repair-codex-startup-windows.ps1
```

提示发现本脚本支持的迁移冲突后，执行修复：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\repair-codex-startup-windows.ps1 -Repair
```

指定实际目录或内置程序时，在命令末尾追加：

```powershell
-CodexHome "C:\实际Codex用户目录" -SqliteHome "C:\实际状态库目录" -CodexBin "C:\当前应用内置目录\codex.exe"
```

## macOS

在解压目录打开终端，先只检查，再根据报告决定修复：

```bash
bash ./repair-codex-startup-macos.command --diagnose
bash ./repair-codex-startup-macos.command --repair
```

也可双击 `.command`，选择 `1` 检查或 `2` 修复，回车默认只检查。若执行权限未保留，使用上面的 bash 命令。

指定实际目录或内置程序时，在命令末尾追加：

```bash
--codex-home "/实际Codex用户目录" --sqlite-home "/实际状态库目录" --codex-bin "/当前应用内置程序路径"
```

支持的新 ChatGPT macOS 内置程序路径：

```text
/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex
```

## 修复会做什么

默认只检查，不启动 Codex、不修改原库。显式修复会先备份 DB/WAL/SHM，在副本复现 SQLite 错误、处理已确认的表/字段冲突，再由当前应用的内置程序执行原生迁移。验证会话索引、远程控制记录和启动协议通过后才替换；未知问题停止，不自动删库，不清空 `.codex`。

该工具只处理已确认的 `state_5.sqlite` 迁移冲突；同样的界面提示还可能有其他原因。迁移编号低不等于数据库坏了。修复阶段最多进行三次副本验证，每次默认最多 30 秒；不会创建会话或调用模型。

成功后保留终端打印的备份目录，再打开 Hub/Codex 确认。此轮分发包是独立工具，未给旧 Hub 安装包加上防复发代码；旧 Hub 初始化仍可能重复制造冲突。待新版 Hub 包包含保护后再用其初始化/增强启动，当前实机效果由用户验收。

## 备份与恢复

备份在实际状态库目录下的 `hub-startup-repair-backups/<时间戳-随机码>/`，包含用户私有状态，只保留在本机。

如果修复后尚未重新打开应用、状态库和边车完全没有变化，可用刚才打印的完整目录恢复：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\repair-codex-startup-windows.ps1 -Restore "C:\实际备份目录"
```

```bash
bash ./repair-codex-startup-macos.command --restore "/实际备份目录"
```

只要状态库/边车发生变化就会拒绝覆盖，避免丢失新数据；恢复原库也可能恢复原错误。替换失败会尝试移回原文件，但多文件操作不保证断电原子性；异常断电或清单阶段中断时请保留整个备份目录进一步核对。

脚本已通过合成 SQLite 与模拟协议检查，未运行真实 Codex；Windows/macOS 目标机效果待用户确认。遇到停止提示时先保留报告，不要继续运行删库或清空整个用户目录的命令。
