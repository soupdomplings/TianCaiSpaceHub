# TianCaiSpace Hub v0.4.30-1 上游整合

维护日期：2026-10-03。源码版本 `0.4.30-1`，开发中、未发布。已合入上游 `v0.4.30` 并保留 TC-001～TC-012；Windows GUI/测试代码编译与调试 EXE 构建通过，未执行测试，功能和交互待用户验收。

本页保存 `0.4.30-1` 的整合与历史产物记录；后续 WorkBuddy 多模型和用途标识、当前调试 EXE 的身份见 [0.4.30-2 开发交付](releases/v0.4.30-2.md)。下文哈希不代表后续构建。

## 来源与恢复点

| 角色 | 版本 / 提交 |
| --- | --- |
| 本地天工开发前 | `0.4.29-5` / `3b3f21338f8709f2906ec37437f1e336fe492b25`，包含 macOS 网页导入接入 |
| 本轮合并前保存点 | `9b9702ec2368eb0ec9956b9c8505569772ca8597`，天工多模型、流式聚合及外部消息执行端已提交 |
| 与本轮上游的共同基线 | `184ea454fc13df54616f35417bdbeaec59921536`，上次整合包含的 v0.4.29 发布后修复 |
| 上次本地整合 | `9d4a7f4e60a8698fa8095f3c43ef33c3b466a114`，详见 [v0.4.29 记录](upstream-v0.4.29-integration.md) |
| 本轮官方标签 | `v0.4.30` / `6520da49d55919a9f908c6c84c51efaf7e1a0a7b` |
| 本地整合结果 | `0.4.30-1` / `22ee85033e9e5f26f3a788e0f7a90e17c38c98c7`，随后仅补记文档追溯信息 |

上游来源为 [happy-loki/codexhub](https://github.com/happy-loki/codexhub)，发布说明见 [官方 v0.4.30](https://github.com/happy-loki/codexhub/releases/tag/v0.4.30)。本地整合在 `integrate/upstream-v0.4.30` 完成；追溯和回滚以提交号为准，不依赖临时分支是否保留。上游相对共同基线改变 15 个文件，主要涉及模型目录、账号回调、增强启动、更新诊断和对应文档。

## 上游变化与本地衔接

- Windows 增强启动的主检测和备用检测共同核对官方安装包身份及桌面主程序路径，兼容 `Codex.exe` 和 `ChatGPT.exe`，避免把 npm Codex CLI、后台 app-server 或第三方客户端误判为官方桌面进程。保留 TianCaiSpace Hub 品牌；上游报告的插件列表恢复是上游历史反馈，本地仍待用户验收。
- ChatGPT 登录回调依次尝试 `127.0.0.1:1455`、`127.0.0.1:1457`，取消随机端口回退；两个端口均不可用时返回 `login_callback_port_unavailable`（HTTP 409），不关闭占用端口的其他程序。账号引用、刷新、模型发现和独立 OAuth 浏览器入口继续保留。
- 内置目录新增 `gpt-6.1-sol`，同步现有 GPT 条目和顺序，非 GPT 目录保持不变。新增模型是否可调用仍取决于来源渠道和账号权限，模型发现不等于真实调用验收。
- 本地未知模型仍按最长完整组件前缀、再按数字版本选择模板。新增目录使 `gpt-6-next`、`GPT-6-NEXT`、`vendor/gpt-6-next` 默认继承 `gpt-6.1-sol`，此前模板为 `gpt-6-astra`；精确内置模型仍使用自身目录，手动能力覆盖优先。相关默认上下文 `272000`、最大上下文 `872000` 未变，这些目录声明不代表厂商真实能力已核验。
- 保留上游更新诊断源码及其导出覆盖。更新模块在本地仍仅 `cfg(test)` 编译，不恢复帮助菜单、托盘或启动检查更新入口，不宣称本版会主动生成更新诊断日志。`diagnostics_export.rs` 本轮只有新增夹具，原过滤规则已经支持对应日志。
- 合入账号和插件核查资料；Chrome 插件的调用者认证兼容仍未解决。上游的测试、只读检查和用户反馈均保留来源，不计作本地本轮验收。

## 冲突取舍

| 文件 / 范围 | 处理结果 |
| --- | --- |
| `Cargo.toml` / `Cargo.lock` | 产品版本为 `0.4.30-1`；Windows feature 取双方并集，保留本地管道/授权功能并加入上游 Appx 身份检查所需能力；保留现有依赖和锁文件关系 |
| `src/ai_gateway/catalog.rs` | 同时保留上游新增 `gpt-6.1-sol` 与本地 `custom-model` 的目录断言；保留 WorkBuddy/GMClaw 保留渠道隔离、动态可见模型及手动覆盖；按既有模板规则修正未知 GPT-6 的测试期望 |
| `RELEASE_NOTES.md` | 保留本地完整历史，新增 `0.4.30-1` 开发中摘要；上游说明链接官方 Release，不用上游说明覆盖二开历史 |
| `UPDATE_NOTES.md` | 替换为本版最多四条短摘要，明确未发布及待用户验收 |
| 无文本冲突但需核对的桌面行为 | 保持启动最大化和生产检查更新入口关闭，保留独立 OAuth 浏览器启动；新增品牌文字使用 TianCaiSpace Hub |

## TC-001～TC-012 保留矩阵

以下“保留”指源码合并与差异核对结果，不等于已完成运行验收。天工、WorkBuddy、网页导入、配置保存、品牌打包和 Actions 核心源码相对 `9b9702e` 未变；目录与增强启动等上游触及位置另作衔接。

| 编号 | 必须保留的行为 | 本轮结果 / 定位 |
| --- | --- | --- |
| TC-001 | TianCaiSpace Hub 显示品牌及图标，`codexhub` 内部与安装升级身份，Windows/macOS Actions 产物 | 保留；新增 UI 文案使用本地品牌。[桌面与交付](customizations/desktop-and-packaging.md) |
| TC-002 | WorkBuddy 独立页签、Chat 入口、`workbuddy` 渠道、来源选择、备份与还原，排除普通路由 | 保留专用渠道隔离。[WorkBuddy](workbuddy.md) |
| TC-003 | WorkBuddy 502/503 最多额外 2 次、等待 1/2 秒，与传输重试共用预算；不重放已开始的响应 | 保留同一上游与请求的重试语义。[WorkBuddy](workbuddy.md) |
| TC-004 | 协议/模型/别名联动、Claude 五档与有效默认值，OpenAI 缓存优先级及 Anthropic 原生缓存 | 保留。[WorkBuddy](workbuddy.md) |
| TC-005 | 通用 `openai_chat`、旧 DeepSeek Chat 兼容和按渠道 `chatDisableReasoning` | 保留，不改成全局推理开关。[Chat Completions](openai-chat-completions.md) |
| TC-006 | 手填/同步/远端模型、选定渠道补路由、家族继承、手动覆盖、可见性与路由分离 | 保留；未知 GPT-6 默认模板随新增目录变化，动态 `custom-model` 断言保留。[动态模型](dynamic-codex-models.zh-CN.md) |
| TC-007 | 启动最大化，取消帮助/托盘/启动检查更新，保留取得新版包后主动安装 | 保留；更新模块只在测试配置编译，OAuth 浏览器独立保留。[桌面与交付](customizations/desktop-and-packaging.md) |
| TC-008 | Kimi、ChatGPT 账号引用/刷新/模型发现、OAuth 与二开的兼容衔接 | 保留并合入两端口回调、增强启动及新 GPT 目录；不把上游功能记为本地原创。[账号登录](ai-gateway-chatgpt-auth.zh-CN.md) |
| TC-009 | Windows 协议与 macOS URL/同用户 socket 接入，一次性票据兑换、HTTP/HTTPS 预览、默认禁用、一次一渠道 | 保留；macOS 待原生构建/验收，未修改现有协议关联。[网页导入](hub-external-import.md) |
| TC-010 | 保存重读、目标合并/指纹、全量 `_revision`、文件锁与原子替换，空模型不能事后直接启用 | 保留配置并发和导入约束。[配置](configuration.md) |
| TC-011 | 独立条目 ID/渠道/地址、同模型不同来源、保存/删除/默认切换/恢复、集合版本、厂商参数、JSON/SSE 聚合及旧入口兼容 | 保留 `gmclaw` 与整个 `gmclaw:` 命名空间隔离；账号登录来源仍不开放。[模型接入](customizations/gmclaw.md) |
| TC-012 | 飞书/微信/企微显式 `/gmclaw`，本机 Harness Token、发送者会话隔离、串行队列、文本及父会话审批 | 保留；共享目录不是文件沙箱，天工策略可能直接执行工具，不新增附件、主动取消、MCP CRUD 或 Telegram 天工执行。[外部消息执行](customizations/gmclaw-im.md) |

## 配置兼容与回滚

本轮上游整合没有新增用户配置迁移，既有账号引用、模型别名、手动能力覆盖及全部二开配置继续沿用。合并和编译不改写用户运行中的 Hub/天工配置、登录态、协议关联或已交付安装包。新增模型不会因目录存在而证明所选上游支持它。

回退本轮上游整合时，使用保存点 `9b9702ec2368eb0ec9956b9c8505569772ca8597` 对应源码/程序，并先备份配置、正常退出旧 Hub，避免两个版本同时写入。该保存点保留天工第二阶段；`3b3f213` 位于天工开发之前，不能当作仅撤销上游合并的恢复点。回到保存点后，新增内置模型和本轮增强启动/OAuth 修复不再存在，未知 GPT-6 模板也回到旧目录选择。

若需要进一步降级到天工第二阶段之前，先按 [模型专题](customizations/gmclaw.md#配置备份与并发) 处理多条目与备份元数据兼容，再按 [外部消息专题](customizations/gmclaw-im.md#回滚与维护定位) 停用桥接。恢复程序不等于恢复配置、撤销天工已执行任务或迁移新备份；本轮未实际执行回滚。

## 本轮验证与产物

- 已通过 `cargo fmt --check`。
- 差异空白及合并冲突标记核对通过；本轮 14 份 Markdown 文档的 273 处本地文件链接均可定位。模型目录静态比较为 16 → 17 项，唯一新增 `gpt-6.1-sol`，9 项非 GPT 条目与保存点逐字段一致。
- 已通过 `cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin codexhub`，编译测试代码；保留 51 条警告（含 12 条重复），未执行测试。
- 已通过 `cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin codexhub`，生成 Windows 调试 EXE；保留 23 条警告。
- 已静态核对 x64 PE、Windows GUI 子系统；24 个直接 DLL/DRV 文件存在，12 个 API-set 作为系统虚拟契约记录，未运行程序验证加载器。
- 未启动 Hub、Codex 或天工交互，未发起真实模型、OAuth 登录或 Harness 工具调用；未修改用户实际配置。macOS 尚待原生构建及实机验证，不新增 Linux 验收范围。

本版当前调试产物唯一身份记录：

| 项目 | 值 |
| --- | --- |
| 程序 | `target/x86_64-pc-windows-msvc/debug/codexhub.exe` |
| 大小 | `55,254,016` 字节 |
| SHA-256 | `78c78162e27f91c6bd6a67fe8f6dbcbcecd67f1aef5ee997788348ef99f122d5` |
| 清单 | `.build-tools/upstream-v0.4.30-build-manifest.json`，Git 忽略的本地核对记录 |
| 日志 | 同目录 `upstream-v0.4.30-check.log`、`upstream-v0.4.30-build.log`、`upstream-v0.4.30-fmt.log`、`upstream-v0.4.30-dependents.log` |

`9b9702e` 的历史 EXE 和更早单模型 EXE 仅属于各自阶段，见 [天工模型验证记录](customizations/gmclaw.md#验证状态)。上游 v0.4.30 所列 792 项测试、3 项忽略，以及其 OAuth 对照检查和插件恢复反馈，均不是本地本轮执行结果。

待用户验收包括 Windows 增强启动与插件可见性、登录回调占用/授权、模型目录和未知模型继承、WorkBuddy 与专用渠道隔离、网页导入并发、多模型/流式工具续轮、三类 IM 的授权/权限/审批/断流。旧单模型“保存成功且无需重启”的用户反馈不扩展为上述新增流程通过。

当前未发布 Release，未生成本地发布安装包。后续 Windows/macOS 安装包统一由 GitHub Actions 生成，构建成功、测试代码编译、用户验收和发布分别记录；根目录摘要只用于准备交付，不代表已经发布。
