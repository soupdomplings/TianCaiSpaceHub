# 天工 Work 内置模型、来源参数与跨端分享

维护：2026-11-10（北京时间）。关联 TC-006、TC-011、TC-018、TC-021；继续当前 V1.0.16，保留前两次 V16 产物。已实现本轮修正，最终构建、离线验证和新 MSI 身份见 [本版交付](../releases/v1.0.16.md)。真实服务权限、登录、调用、压缩、客户端加载与 macOS 仍待用户验收。

## 当前操作

### V1.0.26 入口收纳

天工日常模型页移除内置分享区域、两个内置分享条目和对应账号操作，不再在进入／刷新时自动请求 `/tiangong-models/read` 或以 `native:true` 接入来源。普通外部模型编辑、保存、启动和已有运行服务保持。通用客户端、渠道及概览 UI 未发现额外调用上述接口；没有为隐藏入口短路已启用模型的授权或请求服务。

入口移到天才搭档，默认隐藏。用户明确说“打开天工内置模型分享”或同义明确请求后，才显示专用面板和本次运行内可再次打开的按钮；说明、否定或引用不触发。可见性不持久化为服务开关，不改变来源、分享副本、凭据和目标选择；清空搭档对话或重启不撤销已保存分享。

首次打开面板只读已有模型／参数／分享状态；原生接入、个人登录、身份测试和跨端分享均由显式按钮进入原服务和完整表单。未点击不登录、不接入、不新增分享，尚未接入时提供原生接入按钮。实际分享仍沿原 revision、参数同步、凭据保护和目标 writer。日常自动同步移除后，用户需在此面板主动接入／读取；已保存模型继续使用原档案。

实现为 `src/assistant/personality.rs::opens_tiangong_share`、运行期可见状态、`src/gui/assistant.rs` 和 `src/gui/tiangong_models.rs::show_builtin_share`；日常页移除范围是 `src/gui/gmclaw.rs`。无需配置／凭据迁移，回退 UI 不应顺带启停或重写已有分享。新增隐藏状态夹具 `hidden_builtin_share_panel_reads_state_without_preparing_or_changing_existing_shares`，格式和差异检查已完成，编译／离线执行由管理会话统一登记；真实服务、用户界面及 macOS 待验收。下列旧操作保留历史版本归属，当前入口以上述收纳为准。

1. 进入 **天工 Work → 模型**，只读发现两个内置模型并导入来源参数。原生方式不要求个人登录，不发额外的账号核验请求；已有有效个人会话则使用其个人 ID，没有时不附带个人 ID。模型仍携带已安装天工适配所需的模型认证，能否使用由服务端决定。
2. 需要关联个人账号时，点击 **天工登录**，读取天工已有登录，或在 Hub 输入账号密码登录一次。显式登录仍核验身份，密码不保存；登录 token 与模型认证分开。普通页面无需个人 PAT。
3. 选择 **天工内置-外网 / 天工内置-内网 → 分享给其他客户端**。成功才点亮；重复分享复用原条目，不产生重复副本。ChatGPT 删除可见模型后再次分享，恢复原别名；不会因来源仍存在误显示为“已使用”。首次使用 Codex 仍需其原有接入设置；未读取新配置的会话需重新选择模型、新建会话或重开客户端。

个人 ID 只来自有效保存/本机会话，正常调用不随机生成，也不接受客户端伪造。会话过期时原生请求缺省个人 ID；显式账号登录仍报告失效并给重新登录入口。原个人 PAT 后端兼容。

**接入测试**为用户主动触发的服务端调试入口：选择模型及“原生方式 / 当前登录账号 / 随机 ID”，点击发送一次简短真实请求。随机 ID 每次新生成，只影响本次请求头，界面可查看测试 ID；不落配置、不替换账号、不自动重试。没有有效个人登录时不能选择账号模式完成测试。开发方只测试合成请求合同，未点击真实测试。

天工 Work → 接入设置 → **下载天工 Work** 提供官方 Windows x64、Mac Apple 芯片、Mac Intel 芯片和下载官网。2026-11-10 前轮核对的 1.0.4 三个直链 HEAD 返回 200；下载在默认浏览器进行，Hub 不自动安装。

## 来源参数是权威配置

普通模型跨端副本继续允许各端独立调整。**天工内置模型例外**：窗口、输出、采样、推理、流式及超时沿天工配置；换客户端只做协议和字段映射，不套 GPT 的 xhigh、图像或窗口默认值。路由、客户端副本 ID、别名和名称继续独立。

| 项目 | 来源与行为 |
| --- | --- |
| 模型身份 | 只接受实际 `model_configs` 中 `tiangong-builtin` / `tiangong-builtin-intranet`，底层模型从原行读取，不固定猜测模型家族 |
| 上下文 | 天工 1.0.4 runtime 缺省 128,000；明确配置按 `model_tuning.context_window`、`context_window` / `contextWindow`、发现值读取，不把 `max_tokens` 当上下文 |
| 输出、采样 | 读取原行 `max_tokens`、`temperature`；字段缺省分别 4,096、0.7。本机静态审计两行输出字段均为 128,000，属于客户端设置，未实测服务端输出容量 |
| 其他生成参数 | 白名单读取 `request_options`，按 `reasoning_protocol` 映射原生四种思考协议；保留源停止词、采样及推理，不把目标端传入的通用推理/温度回流到来源 |
| 流式、超时 | 读取原 `extra_params`，缺省流式、600 秒；上游请求按源设置，下游 JSON / SSE 仍符合目标端合同，必要时收集或转换，工具和用量保留 |
| Codex 元数据 | 窗口及最大窗口映射来源，effective window 100%，`auto_compact_token_limit` 为源窗口 70%（默认 89,600）；不展示未经来源确认的 GPT 推理/图像能力 |

白名单不包含账号/认证字段。未知请求参数、参数范围或推理协议不兼容时停止导入并给原配置修正入口，不静默丢弃。明确保存的档案先校验再调用；旧 V16 绑定缺少档案时仅按本机确切模型 ID 和上游名只读回填，不能借用另一条目。

重新读取时更新来源及同一内置身份副本的档案和超时，通用端已有模型能力由原生事务 writer 同步，WorkBuddy 由专用 revision/锁/备份 writer 同步。后改冲突停止该目标并提示到目标模型页核对后保存，不覆盖外部修改；其他已成功目标保持。普通来源及天工原始内置数据库不修改。

## Codex 压缩兼容范围

天工自身在运行会话中以窗口 70% 触发摘要，不能通过仅修改模型名字把其整个会话机制移植到 Codex。本轮为 **unary `/responses/compact`** 提供文本摘要兼容：在同一来源模型发非流式摘要，使用源采样/思考，仅覆写天工摘要输出预算；默认输入预算 45%、初始输出不超过 4,096、最大不超过 16,384，并受源输出上限/窗口约束。完整历史分段后逐级合并，最多 128 请求、3 层拆分、8 轮合并。

保留完整最近用户/工具轮次，跨边界工具结果回溯至对应调用；单个过长轮次完整参与摘要，不直接截断。空、拒绝、工具输出、截断到上限或上游失败均不生成替换历史；外部加密压缩状态无法解读时明确拒绝。响应是 `response.compaction` 与普通 assistant 文本摘要和保留尾部，**不伪造 `encrypted_content`**。摘要请求集合、真实上游用量和响应保留在原请求详情中，不用本机估算量冒充账单。

此方式参照 Codex `rust-v0.115.0` unary compact 的 ResponseItem 消费合同。较新 streamed compaction v2 要求专门 compaction item，本轮没有实现其加密状态合同；不能据此声称所有新版 Codex 压缩已修复。用户实际安装版本是否接受明文摘要、压缩后续轮、长上下文与工具链仍需验收。

参考：[OpenAI compact](https://developers.openai.com/api/reference/resources/responses/methods/compact)、[Codex unary 源码](https://github.com/openai/codex/blob/rust-v0.115.0/codex-rs/core/src/compact_remote.rs)。只读研究副本及脱敏安装证据在 `target/v1.0.16-tiangong-parameters-20261110/`、`target/v1.0.16-tiangong-audit-20261110/`，不是实际客户端验收证据。

## 配置与接口

- 发现沿 `GMCLAW_CONFIG_PATH` / 天工目录，SQLite 只读查询两个确切模型的身份和参数。个人会话只读唯一 `claw_oauth_session` 键；缺库不创建，不读任务/聊天或普通模型密钥。
- `tiangongBuiltin` 保留 `modelId/upstreamModel/credentialRef`，新增可选 `parameters`。个人会话保存在原系统保护 SecretStore（Windows DPAPI / macOS Keychain），不保存密码、安装共享认证或把认证导出给目标端。
- 模型请求只从已安装 `app.asar` 的精确适配函数读取模型认证，只发固定 `https://tiangong.jiuqi.com.cn/api/v2/tg/chat/completions`。不执行安装脚本、不调用私有 runtime API、不抓浏览器 Cookie；正常请求不再联网调用 `getLoginContext`。
- revision、目录摘要、文件锁、备份、原子替换和后改保护保留。账号轮换更新受保护引用；失败清理本次暂存凭据，旧引用保留供备份恢复。
- 原生模型禁用重定向、普通 `/v1` 修正和自动模型重发；传输及响应读取受源超时约束。入站身份头被过滤，认证头脱敏，上游失败原文不回显；401/403 给账号/服务权限入口，404 重新读取，429 稍后重试/核对额度。

本机 API 均沿原可信管理授权与来源检查：

| POST 路径 | 行为 |
| --- | --- |
| `/api/desktop/tiangong-models/read` | 只读账号、模型、来源参数及分享状态，不触网、不返回凭据 |
| `/api/desktop/tiangong-models/connect` | `revision` + `native:true` 原生接入不联网登录；或读取会话 / `username/password` 显式登录并核验，导入模型和来源参数 |
| `/api/desktop/tiangong-models/share` | `revision/modelId/targets`，逐端沿 writer 使用，Codex 删除后恢复原可见别名 |
| `/api/desktop/tiangong-models/test-identity` | `revision/modelId/mode(native/account/random)`，用户点击后单次测试，不持久化测试身份 |
| `/api/desktop/tiangong-models/enable` | 保留旧 PAT API 兼容，普通 UI 不以此为前提 |

维护：`src/desktop/tiangong_models{,/parameters,/session,/diagnostics}.rs`、`src/web/desktop_models.rs`、`src/desktop/models.rs`、`src/workbuddy_config.rs`、`src/ai_gateway/{catalog,handler,workbuddy}.rs`、`src/ai_gateway/providers/{deepseek_chat,chat_json_stream,tiangong_compact}.rs`、`src/gui/{gmclaw,tiangong_models}.rs`。

## 迁移、回滚和验证

本版进入天工模型页会补齐缺失来源档案；不批量改天工数据库、不删除旧配置。撤销分享在目标客户端移除模型；Codex 删除后保存可见模型列表。回退前先停用新原生/会话授权来源和副本，再按原 writer 备份恢复仍未后改的目标片段；旧 V16 不认识完整参数/原生凭据，不应让它继续使用新绑定。不要整库回滚或删除凭据文件。

本轮验证使用隔离 HOME/客户端目录、临时 SQLite、合成 ASAR/RSA/会话与回环服务，覆盖原生免个人登录、参数映射/同步、身份测试合同、压缩分段/失败保历史、工具转换、配置后改保护和隐藏原生浅深布局。最终数量、Windows 构建和 MSI 静态核验以 [交付记录](../releases/v1.0.16.md) 为准；前两轮测试和包只保留原归属。

## 交付后复核：参数、压缩与登录依赖

2026-11-10 前轮登录包交付后的静态审计确认：该包没有完整读取源参数，Codex 未知动态模型默认 32,768、仅有 Responses 来源压缩，并额外强制个人登录及每次请求联网核验。天工 1.0.4 原端缺省上下文 128,000、压缩阈值 70%、个人 ID 可缺省。本轮产品修正上述参数和额外登录依赖，并增加有限 unary 文本摘要兼容；旧包 `161126` 没有本轮修正，真实服务能力未因审计或离线验证而获得确认。
