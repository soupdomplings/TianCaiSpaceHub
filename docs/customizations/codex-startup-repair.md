# Codex「无法加载组织设置」诊断与状态库修复

维护日期：2026-10-20。适用开发基线：`v0.4.30-5`、唯一 `main`，关联 TC-007。

实现状态：Windows / macOS 共用的 Hub 原生修复按钮、状态库防复发保护已实现。按钮位于“ChatGPT(Codex) 接入”的本地配置区域，Hub 保持运行，仅要求 ChatGPT/Codex 完全退出；无需解压脚本、终端或 Python。必要编译、本地 Windows 测试 MSI 与验收边界见 [按钮交付](../releases/v0.4.30-5.md#2026-10-20-hub-内置组织设置修复按钮)。源码未提交/推送，Windows/macOS 实机待用户验收；此前独立脚本包保持原身份。

## 原因与证据范围

「无法加载组织设置」是桌面初始化失败后的上层提示，单凭这句话不能判断网络、账号或数据库哪个出错。本次 macOS 用户反馈：保留旧状态库时失败，换成新用户目录或只放配置、认证文件可启动；仅放回旧 `state_5.sqlite` 又出现 `failed to initialize sqlite state runtime`。删除该库后可启动，说明问题集中在旧状态库；尚未取得该机旧库和完整原始 SQL 错误，不能把具体冲突当作已经实机证实。

源码核对发现一条吻合的冲突路径：旧 Hub 在初始化时直接创建 `remote_control_enrollments`，并在缺失时添加 `remote_control_enabled`。原生 SQLx 迁移第 24 步也会建此表，第 37 步也会加此列；旧库未记载对应迁移时，后续原生升级可能出现表/列已存在。修改原生 schema 而不由原生迁移负责，才是这里要避免的行为。

用户截图中的「23 条迁移、当前程序 61 条」用于定位差异，**不是删库标准**。旧库应由当前内置程序正常迁移，迁移数量少不能直接证明损坏。脚本不把 61 或客户端版本号作为通过条件，也不伪造 `_sqlx_migrations` 回执。

已核对的上游依据固定到 [Codex 源码 revision 8196bcdc](https://github.com/openai/codex/tree/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/state)：[迁移 24](https://github.com/openai/codex/blob/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/state/migrations/0024_remote_control_enrollments.sql)、[迁移 37](https://github.com/openai/codex/blob/8196bcdc189ad2c189d631b321fa54b8e09b9bb0/codex-rs/state/migrations/0037_remote_control_enrollments_enabled.sql)。协议与路径参考 [官方 app-server](https://learn.chatgpt.com/docs/app-server)、[环境变量](https://learn.chatgpt.com/docs/config-file/environment-variables) 和 [排障说明](https://learn.chatgpt.com/docs/reference/troubleshooting)。

## Hub 内置按钮与使用

1. 在 Hub 的“ChatGPT(Codex) 接入”页，本地配置区域点击 **修复无法加载组织设置**。无需手动寻找安装目录或运行脚本；Hub 本机服务未连接时也能使用。
2. 按钮立即在 Hub 内打开检查窗口，后台自动定位当前安装的内置程序与实际状态目录。ChatGPT/Codex 仍在运行时，提示正常退出（包括托盘和后台），然后点击“重新检查”；不替用户强杀程序，也不关闭 Hub。
3. 只在确认本文支持的迁移冲突后，自动完整备份、在副本复现错误，让当前内置程序迁移、保留历史及远程记录；验证通过且原文件没有变化，才应用修复。未知问题显示停止原因，不清空状态库或 home。
4. 窗口显示当前步骤、最终结果和已有备份位置，提供“重新检查”“打开备份”“恢复本次修复”“关闭”。恢复需另行确认且文件未再变化。处理结束后关闭此窗口，再从 Hub 使用增强启动打开 ChatGPT/Codex；Hub 全程保持原进程和网关服务。

修复包含状态库检查，不能理解为只改启动快捷方式或路径。安装程序路径由按钮自动发现；`CODEX_CLI_PATH` 失效/指向其他程序、存在多个安装候选、桌面/进程 home 不一致时，明确报告并停止，不猜测修改全局变量。

| 入口 | 当前职责 |
| --- | --- |
| 初始化 / 更新配置 | 不创建原生状态库、表或列；原生迁移就绪后才写远程控制偏好 |
| 增强启动 | 保留基础启动流程；启动成功后再补试一次延后偏好写入，不自动扫描、修复或重建旧库 |
| 修复无法加载组织设置 | 显式按钮，Hub 内后台检查和处理已知冲突；Hub 不退出，只要求关闭 ChatGPT/Codex |
| 独立 Windows / macOS 脚本 | 保留上一轮可单独分发的排障工具，依赖 Python；不是安装包按钮的运行依赖 |

修复与恢复期间，GUI 使用独立模态进度窗口，Hub 守护进程及其他实例对配置、原生偏好、增强启动和会话迁移的操作通过同用户跨进程文件锁协调。已在执行的相关操作会使检查提示稍后重试，修复持锁后新操作立即提示等待；不停止其他网关/MCP 服务。锁自动随任务结束或进程关闭释放，不新增 Hub 配置字段。旧版 Hub 不认识这把锁，混用旧进程无法获得相同协调保护。

运行中的修复窗口暂不接受普通关闭或 Esc，防止中断文件替换；系统关机不被该窗口阻止。备份和阶段清单用于处理中断，不声称多文件替换具有掉电原子性。

## 独立脚本与前置条件

以下是此前独立脚本的用法，供手动排障参考；安装用户直接使用上面的 Hub 按钮即可，无需执行本节。该工具仍按旧流程要求 Hub 和 ChatGPT/Codex 都退出，与原生按钮保持 Hub 运行的方式不同。

三个文件必须放在同一文件夹并完整解压：

| 文件 | 职责 |
| --- | --- |
| [repair-codex-startup-windows.ps1](../../scripts/repair-codex-startup-windows.ps1) | Windows PowerShell 入口；默认只检查 |
| [repair-codex-startup-macos.command](../../scripts/repair-codex-startup-macos.command) | macOS Bash 入口；双击时可选只检查或修复，默认只检查 |
| [repair-codex-startup.py](../../scripts/repair-codex-startup.py) | 两平台共用的标准库引擎 |
| [简明使用说明](../../scripts/repair-codex-startup-README.zh-CN.md) | 随脚本包分发的中文步骤 |

需要 Python 3.9+ 且带 `sqlite3`，推荐 3.11+。脚本不自动安装依赖；Windows 跳过 Microsoft Store 的 Python 执行别名。Python 3.9/3.10 缺少标准库 TOML 解析器，存在 `config.toml` 时，须显式指定实际 `SqliteHome` / `--sqlite-home`，避免误判自定义状态库位置；3.11+ 自动解析。

检查和修复前均需正常退出 ChatGPT/Codex、Hub 和同名后台进程。脚本只报告仍在运行的进程，不替用户强杀。修复时仅在私有临时目录里启动当前桌面应用自带的 app-server 做协议检查，不创建会话/回合或调用模型；只结束自己创建的探测进程。开发方本轮未实际运行安装的 Codex。

### Windows

在完整解压的目录打开 PowerShell，先检查：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\repair-codex-startup-windows.ps1
```

确认报告的是本工具支持的表/字段迁移冲突后，仍保持应用退出，再修复：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\repair-codex-startup-windows.ps1 -Repair
```

此处 ExecutionPolicy 仅影响这次 PowerShell 进程，不修改系统执行策略。需要恢复刚才修复前的库时，使用终端打印的完整备份目录：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\repair-codex-startup-windows.ps1 -Restore "C:\实际状态库目录\hub-startup-repair-backups\本次备份目录"
```

可追加 `-CodexHome`、`-SqliteHome`、`-CodexBin` 和 `-TimeoutSeconds 30`。目录和程序路径均填实际绝对路径；`CodexBin` 指当前应用内置的 `codex.exe`，避免误用 PATH 中的独立 CLI。

### macOS

在完整解压目录的终端中先检查，再按报告决定是否修复：

```bash
bash ./repair-codex-startup-macos.command --diagnose
bash ./repair-codex-startup-macos.command --repair
```

也可双击 `.command`，选 `1` 检查或 `2` 修复。若文件执行权限没有被解压工具保留，直接使用上面的 `bash` 命令。恢复使用终端打印的目录：

```bash
bash ./repair-codex-startup-macos.command --restore "/实际状态库目录/hub-startup-repair-backups/本次备份目录"
```

可追加 `--codex-home`、`--sqlite-home`、`--codex-bin` 和 `--timeout 30`。新版嵌套安装支持以下路径；同时兼容 ChatGPT.app / Codex.app 的平铺 `Contents/Resources/codex`，以及用户目录下的 Applications：

```text
/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex
```

自动发现多份不同的内置程序时要求明确选择，不按版本号或时间戳猜测。尚未提供非商店 Windows 安装器、自定义任意目录的全盘搜索；可用程序路径参数指定。

## 检查、修复与备份行为

默认检查只复制 `state_5.sqlite` 及存在的 `-wal`、`-shm` 到私有临时目录，再读取副本；不启动产品、不打开原库执行 SQL。实际状态目录按显式参数、`config.toml` 的 `sqlite_home`、桌面/终端的 `CODEX_SQLITE_HOME`、当前 Codex home 解析。桌面与终端的 home 覆盖不一致、无法确定相对路径时停止，要求指定；不假设所有机器都使用 `~/.codex`。

只支持两类明确冲突：

1. 表已存在，但原生迁移 24 尚未完成且最高回执仍低于 24。
2. 原生建表迁移已完成，`remote_control_enabled` 已存在，但原生迁移 37 尚未完成且最高回执仍低于 37。

显式修复执行以下步骤：

1. 再次确认相关程序已退出、源文件哈希未变，完整复制原 DB/WAL/SHM 并核对 SHA-256。
2. 从物理备份的工作副本生成包含已提交 WAL 的一致快照，不使用 SQLite 打开字节备份本身，保留其哈希。
3. 在副本上运行当前内置 app-server，要求复现 SQLite 启动错误；当前程序已能读取原库副本时，不替换原库。
4. 仅在另一副本上移开已确认提前出现的表/列，保存远程控制行；由当前内置程序执行原生迁移，不补写回执、不按 61 条强制升级。
5. 恢复远程控制记录，检查数据库完整性、原历史会话 ID 和远程控制行保留，再运行一次 `initialize → initialized → config/read`，严格匹配 JSON 响应 ID、结果、错误和超时。
6. 全部通过且源文件未变时，才以候选库替换。检查出未知 schema、额外索引/触发器/引用、未完成迁移、非 SQLite 错误、记录丢失或探测失败均停止，原库保持不变。

按钮每次协议检查最多 30 秒（另有有界进程收尾），最多三次；独立脚本默认 30 秒、可设 5～60 秒。只复制配置、认证、installation_id 到私有临时 home 用于本机验证，不上传它们；清单仅记录路径、哈希、阶段和固定结果类型，不保存配置内容、令牌或原始探测日志。

备份位于**实际状态库所在目录**，不是脚本目录：

```text
hub-startup-repair-backups/<时间戳-随机码>/
  manifest.json       # 本机路径、哈希与执行阶段
  original/           # 不再由 SQLite 打开的原始物理备份
  displaced/          # 替换时移出的原库全家，供 --restore 使用
  before-restore/     # 恢复时保留的修复后库
```

这些是用户私有资料，应保留在用户本机，不提交仓库或打入分发包。替换时遇到普通异常会尝试恢复移出的文件，但多文件搬移不具备跨文件、掉电原子性；断电后 `stage=replacing` 等中断状态需人工核对，脚本不会自动猜测恢复方向。

恢复仅处理清单为 `applied`、备份哈希正确，且当前 DB/WAL/SHM 与刚修复后完全一致的情况。原生按钮在搬移前先记 `restoring`，失败尝试退回，成功标为 `restored`；清单更新异常时明确提示保留备份、不重复恢复。重新打开应用、其他访问产生边车或新写入都会导致拒绝覆盖；这是数据保护，不代表备份失效。此时保留备份进一步处理，不强制覆盖新数据。

退出码：`0` 表示检查未命中已知冲突或修复流程正常结束；`2` 表示只检查发现待修复冲突；`1` 表示停止/失败；`130` 表示用户中断。`0` 不等于桌面产品已实机验证。检查不会全面诊断同文案的其他原因；`CODEX_CLI_PATH` 异常只提示，不修改全局环境变量、不清空 `.codex`、不修复未知物理损坏，也不处理其他名称的状态库。

## Hub 源码与维护定位

[codex_app_config.rs](../../src/codex_app_config.rs)：`upsert_official_remote_control_enrollment` 不再创建 `state_5.sqlite`，以只读写既有文件模式打开；`remote_control_enrollments_schema_ready` 要求原生 24/37 成功回执及必需列齐全，未就绪记录 `official_enrollment_deferred` 并跳过。只有就绪后才创建 installation_id 并写入偏好，不做建表、补列或伪造迁移。

[codex_app_enhanced.rs](../../src/codex_app_enhanced.rs)：基础注入成功后再尝试一次延后偏好写入，原生 schema 仍未就绪或发生可选写入错误只记诊断，不启动修复。初始化、TOML 对话模式受管规则和 Fast/Ultra 能力适配保留；Windows/macOS 共享此源码，未使用版本号分支。本次没有全面调整 Hub 自身的自定义 SQLite 路径定位，独立脚本可显式选择实际目录。

[codex_startup_repair.rs](../../src/codex_startup_repair.rs)：安装包内 Rust / bundled SQLite 引擎负责定位、物理备份、副本探测、替换和恢复；不下载运行时、不需要 Python。macOS 同时支持平铺与嵌套内置程序路径，未设置的 launchctl 环境变量视为缺省。

[gui/startup_repair.rs](../../src/gui/startup_repair.rs)、[gui/codex_tab.rs](../../src/gui/codex_tab.rs)、[gui/text.rs](../../src/gui/text.rs)：按钮与 Hub 内模态窗口，后台工作线程通过有界步骤反馈进度，不退出或重启 Hub。[codex_access.rs](../../src/codex_access.rs) 在系统用户数据目录维护共享/独占锁；[codex_session_history.rs](../../src/codex_session_history.rs) 的整段迁移以及配置和增强启动的关键操作遵守该锁。

迁移与回退：没有新增 Hub 配置字段。防复发修改不自动修复此前冲突库；旧 Hub 仍可能再次提前创建结构，应在正式产品包含保护后使用初始化/增强启动。恢复旧状态库可能恢复旧错误；不建议回退到会继续改原生 schema 的旧实现。回退脚本只需移除分发文件，已实际修复的库通过备份规则处理。

验证分轮记录：此前 Python 24 项合成库与模拟协议测试、PowerShell/Bash 语法通过，旧脚本 ZIP 未修改；本轮新增 Rust 原生引擎与文件锁合成测试，覆盖两类冲突、提交 WAL、历史/偏好保留、失败不替换、重新启动/原库变化保护、恢复保护和严格协议结果。当前编译、测试数量与新 Windows MSI 身份以 [按钮交付](../releases/v0.4.30-5.md#2026-10-20-hub-内置组织设置修复按钮) 为准。未运行实际 Hub/Codex、内置 app-server、安装器或真实模型调用；macOS 未原生编译，Windows/macOS 实机与恢复效果均待用户验收。
