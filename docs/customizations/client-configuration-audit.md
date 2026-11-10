# 客户端配置路径与写入核对

日期：2026-11-10（UTC）。适用：V1.0.12，TC-017/018/020。状态：代码修正与隔离验证见 [本轮交付](../releases/v1.0.12.md)；真实客户端及 macOS 待用户验收，未发布。

本次针对用户提出的“是否完整借鉴 Magpie，是否会定位或写入错误”核对当前七端。**参考过配置格式不代表完整移植，更不代表对端实机通过。** 本文记录实际对齐、Hub 必须保留的差异和剩余边界，不能将 Magpie 全部客户端及测试计为 Hub 支持。

## 参考身份

固定 Magpie 提交：[`e6cdeac9c681ad0377b0f863af556d4c1d7be2a8`](https://github.com/yetone/magpie/tree/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8)。本轮重新下载该提交的 40 个源码/测试/许可文件，原文及逐文件 SHA256 保存在 `target/v1.0.12-workspace-20261110/magpie/manifest.json`。不把此前摘录的身份代替此次固定原文；没有执行 Magpie 或其 Go 测试。

主要依据：`internal/agent/{vscode,zcode,dsh,workbuddy}.go` 及对应测试，`internal/appdir/zcode.go`，`internal/library/{targets,mcp}.go`。派生代码保留原版权和 [第三方许可](../../THIRD_PARTY_NOTICES.md)。VS Code 原生 Skill/MCP 和 WebStorm 两种路线继续采用各自现行专题中的官方依据，它们不等于 Magpie Library 的同名目标。

## 七端核对结果

| 客户端 | 模型位置与写入 | Skill / MCP | 本轮处理与边界 |
| --- | --- | --- | --- |
| ChatGPT / Codex | 沿原 `codex_app_config` 的桌面目录、可见模型及别名服务 | 桌面目录 `skills/` 与 `config.toml` 的 `mcp_servers` | 保留已实现的桌面定位、独立路由、初始化/历史；**有意不把启动 Hub 进程继承的 `CODEX_HOME` 当作另一个桌面 App 的目录**，不照搬 CLI 场景。通用 MCP 仍转换 HTTP 头及启用字段，SSE 不冒充支持 |
| WorkBuddy | 默认 `.workbuddy/models.json`，本轮补 `WORKBUDDY_CONFIG_DIR`，Hub 原 `WORKBUDDY_CONFIG_PATH` 仍最高优先级 | 同根 `skills/`、`mcp.json` 的 `mcpServers` | 本轮补 `{models,availableModels,...}` 对象容器，保留未知顶层字段、其他模型及后改；添加/移除同步本条目可见性，恢复只还原本次可见性差额。远程 MCP 用原生 `type:http`。国际版、多实例和客户端信任仍不据此宣称全适配 |
| 天工 Work | 原 `gmclaw_config` 的 SQLite、专用条目与本机授权服务 | 原 `tiangong_resources` 的已授权 DesktopClient API | 不改成通用 JSON 文件写入，不尝试绕过本机认证；本轮审阅原专用路径与服务合同，没有来自 Magpie 的等价替换。真实桌面行为仍待用户验收 |
| VS Code Chat | 用户或便携 `User/chatLanguageModels.json`，JSONC 自有组 | `~/.copilot/skills`；用户/Profile `mcp.json` 的 `servers` | 按参考规则补便携程序/CLI、版本子目录、Scoop shim、macOS bundle 和 Profile 元数据；只认登记且实际存在的 Profile。继承默认模型的 Profile 不再新建无效模型文件，但保留其资源目标。仅当前 VS Code Chat，未新增 Insiders/VSCodium/Cursor/工作区全能力 |
| DeepSeek Harness | 原现代 `llm-pi-ai` provider、`.credentials.yaml` 与 `settings.yaml`，模型按原服务管理 | `DSH_HOME` 下 `skills`，MCP 改读实际 `profiles/*/cordis.patch.yml`，旧目录为 `config.yaml` | 修复错误的根 `cordis.patch.yml`；默认采用实际 web Profile（存在时），其余实际 Profile 单独选择，不默认联动覆盖。现代首次添加指向实际 desktop Profile；旧模型代际不冒充完整移植，见下文 |
| ZCode | 原位置 `.zcode/v2/setting.json` 的绝对 `dataBaseDir` 决定模型根，写新旧两种模型文件 | **仍在原 HOME 的** `.zcode/skills`、`.zcode/cli/config.json` 的 `mcp.servers` | Windows 尊重客户端实际优先的 `HOME`，保留 Hub 显式 `ZCODE_HOME`。迁移只变模型根，目标身份不变。远程 MCP 用 `type:http`。原端内手动模型规则继续优先；不把全套 Magpie 模型策略和原生可选参数都认定已等价 |
| WebStorm | AI Assistant Java 插件调用 IDE API/PasswordSafe；ACP 用固定 OpenCode 1.2.21 配置 | AI Assistant 的原生 MCP 位置；ACP 的 OpenCode MCP/Skill 位置 | 原两种路线与原合同保持，不把 Magpie 其他 IDE/CLI 适配混入。AI Assistant SDK 限定 262 系列；插件和真实 IDE 加载仍待验收 |

## 路径与保存的确定行为

### VS Code

- Hub 已保存的程序路径先参与识别，其次 PATH 中的 `code`；只读取程序及配置元数据，不运行命令。程序符号链接及 Scoop `.shim` 可用于只读定位，实际配置仍保留原链接/重解析点保护。
- Windows 的 `bin/code.cmd`、`Code.exe` 和版本子目录按同一安装根判断。便携包需有 `data` 且产品未标记安装目标；名称为其他编辑器的构建不借用。已有 `VSCODE_PORTABLE` 绝对目录兼容保留。
- macOS 从 CLI 或 `.app` 查产品定义的便携目录；无明确 CLI 且多个候选不一致时不猜测某个便携副本。只在临时布局测试中验证这些规则，未在 macOS 原生执行。
- Profile 读取 `globalStorage/storage.json` 的 `userDataProfiles`：支持位置名称及 URI 对象、保留显示名；不把残留目录算作实际 Profile。`useDefaultFlags.languageModels` 为真时模型选择沿默认位置，Skill/MCP 目标仍可用。旧已保存条目不自动删除，遇到继承关系变化时需在默认位置重新配置；显式停用只处理原 Profile 的旧受管组。

### ZCode

- Windows 的有效绝对 `HOME` 优先于 `USERPROFILE`；原 Hub 的绝对 `ZCODE_HOME` 仍可显式指定配置根。原根的 `v2/setting.json` 里 `dataBaseDir` 有效时，模型写 `<dataBaseDir>/.zcode/v2/`。
- 空白、相对或非字符串迁移值按客户端规则回原根；不修写 bootstrap 设置文件，不搬移用户内容。模型根可不同于资源根，使用可选 `modelRoot` 表示；原目标 ID 保持，原记录缺字段仍能读取。
- bootstrap 无法安全读取、迁移目录含链接或非规范父级路径时，停止自动目标读取并说明应核对的目录/权限；不把“无法确认”当作“没有迁移”而写回旧目录。修正配置位置后可重新读取。可读取但非合法 JSON 的 bootstrap 沿参考实现回默认位置。
- 发现目录迁移后，旧文件仍存在也不冒充模型已在新目录使用；核对后显式替换才写新位置，旧位置和备份不自动清理。普通配置检查与打开文件夹继续能看到模型根与资源位置。

### WorkBuddy 与 Harness

- WorkBuddy 的模型、Skill、通用 MCP 共用原专用模型服务解析出的根；Dumpling 原适配器已读取 `WORKBUDDY_CONFIG_DIR`，保留其独立预检/信任行为。模型文件允许数组、旧单模型对象及现代容器；非数组 `models` 拒绝保存。非空 `availableModels` 只改变本操作涉及的模型，恢复不覆盖后来新增的其他模型和未知字段，目标可见性被后改则停止恢复。
- Harness MCP 沿实际 Profile 文件定位：默认目标 ID 保持，其他 Profile 作为资源目标，模型目录仍由默认目标管理。Skill 目录仍共享同一 home，并沿原共享目录冲突检查。**不会把旧错误根文件的内容自动复制到新位置，也不会删除它**；重新选择使用才经原事务写入真实目标。

## 尚未扩大为已完成的范围

- Harness 的旧 `config.yaml` 模型接管、完整版本检测和 Magpie 各发行版策略，没有在本轮全部移植；不应只因能生成 YAML 就承诺旧客户端识别现代 provider。首次旧目录需先建立受支持的实际 Profile，再添加模型。MCP 的旧列表仍按可安全解析的原格式处理。
- WorkBuddy AI/多实例、VS Code 其他发行版及 Cursor 未新增；环境变量或显式目录不是“全部发行版已适配”。工作区和项目级 Skill/MCP 不自动扫描。
- 通用 OAuth、各客户端独立环境变量表达式、任意客户端版本的全部可选字段，不能由文件往返测试证明。表达式的跨端等价转换尚未完整覆盖，仍需逐端核对，不能登记为全部可用。
- Hub 的多条目独立路由、来源选择同步、显式替换与后改保护是已确认业务需求，不能替换为 Magpie 的整组共享模型/自动联动策略。

## 验证与回退

本轮采用固定参考场景的 Rust 临时目录测试：便携/已安装/同名 fork、CLI/EXE/Scoop、版本目录、macOS 合成 bundle、多配置继承和残留、Windows HOME/迁移值、模型/资源根隔离、旧记录缺省字段、Harness MCP Profile、WorkBuddy 容器/可见性/后改恢复，以及原生 MCP 类型名。**这些是 Hub 测试，没有在 Hub 里执行 Magpie Go 测试，也没有启动真实客户端。** 完整计数、源码身份、构建及 MSI 结果只以 [V1.0.12 交付](../releases/v1.0.12.md) 为准。

回退可恢复本轮路径/格式代码，旧目录和既有操作备份保留。新版未默认修改任何用户配置；实际使用后的恢复仍用原锁、revision、受保护快照和原子替换，不能整份回写覆盖后改。旧版本可忽略新增可选目标字段；若用户已在新版指定迁移位置，回退前应核对旧版不会重新写回默认目录。
