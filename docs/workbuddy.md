# WorkBuddy 多模型接入

维护日期：2026-10-03。适用版本：`0.4.30-2`（本地开发版，未发布）。关联 TC-002、TC-003、TC-004、TC-013；历史验证见 [二开变更记录](customizations/CHANGELOG.md)，本轮构建与产物见 [开发交付](releases/v0.4.30-2.md)。

TianCaiSpace Hub 在 WorkBuddy 页签逐条管理多个模型。每条可以选择自己的来源渠道、模型、协议和思考强度；WorkBuddy 调用本机 Chat Completions 地址，Hub 经该条目的专用渠道转发。同名模型可绑定不同来源，保存一个条目不再以单元素数组覆盖其他模型。

## 推荐配置

1. 先打开“大模型接入”页签，保存至少一个 provider，并在该 provider 中填写上游 Base URL、API Key 和模型列表。
2. 打开“WorkBuddy 接入”页签，刷新渠道与模型配置。选择已有管理条目，或进入新增模式；页面只列 Hub 管理条目，用户自己添加的其他模型保留，不自动接管数组首项。
3. 在“上游提供商（来自 AI Gateway）”中选择刚才保存的 provider。
4. 在模型下拉框中选择该 provider 已配置的模型或模型别名。URL、凭据配置状态和模型列表会随 provider 自动带出；协议默认按 provider 自动选择，特殊兼容服务仍可手动调整。
5. 使用自动生成的条目地址和本机 Key（示例使用默认端口）：

   ```text
   http://127.0.0.1:3847/ai-gateway/workbuddy/<entryId>/v1
   ```

   WorkBuddy 在该地址后追加 `/chat/completions`。地址随 Hub 当前固定监听端口生成，IPv6 回环用 `[::1]`。默认 Key 为 `workbuddy-local`，只用于客户端非空字段兼容，当前端点不校验它，不是上游凭据。旧条目 `entryId=legacy` 保留 `/ai-gateway/v1` 地址及 `workbuddy` 渠道。
6. 思考强度改为下拉选择，随 provider、模型（含别名实际指向）和协议自动刷新。Claude 统一提供 `low, medium, high, xhigh, max` 五档，不为旧版 Claude 单独分档；OpenAI 等其他模型默认提供 `low, medium, high, xhigh`。GLM 的 Anthropic 兼容配置沿用适配器的 `high, max` 两档。默认值初始为 `high`，切换时保留仍然适用的选择，不适用则回退为 `high`。请求未携带强度时，Hub 补充保存的默认值；Claude 原生请求转换为 `thinking.type=adaptive` 和 `output_config.effort`。
7. OpenAI 协议的固定缓存键由 provider 自动生成 `workbuddy:<provider>`，无需手填。Anthropic 原生协议显示“不需要（使用原生缓存）”，保存时省略 `cacheKey`，由 Hub 自动添加 `cache_control` 标记。Claude 经 OpenAI 兼容协议接入时，缓存仍按所选协议处理。
8. 保存当前条目。新增时生成 UUID 条目 ID、WorkBuddy 模型 ID `tiancaispacehub-<entryId>`、专用渠道 `workbuddy:<entryId>` 和对应地址；编辑沿用身份并保留其他条目、未知字段与未在页面编辑的能力选项。新保存条目的上游 Key 写入 Hub 专用渠道，在 `models.json` 及状态响应中清空；同目录的撤销备份仍可能包含渠道凭据，详见下文。来源渠道的导入元数据不复制到专用渠道。
9. 删除只移除选中模型及其渠道；“还原备份”撤销最近一次成功保存或删除，恢复前另存 `models.json.before-restore.bak`，成功后消耗该次操作记录。新备份定点还原，不覆盖其他条目的后续修改；损坏文件及旧备份的限制见下文。

按 WorkBuddy 自身方式重新加载配置后，在客户端选择所需模型。本机未定位到 WorkBuddy 安装资源，本轮未运行客户端；客户端对 UUID ID、多条同名模型和独立 URL 的识别待用户验收，不声称已验证其默认模型机制。Hub 为 `id/name/providerModel` 建立到来源实际模型的映射，避免向厂商发送 UUID；来源选择为别名时先解析真实目标。请求使用别名、条目 ID 或真实模型名时，默认思考和缓存参数均按该条目的专用渠道解析，不从另一个同名条目读取。

## 协议规则

- AI Gateway 的 `OpenAiResponses` provider 自动映射为 `openai-responses`。
- ChatGPT 账号登录渠道同样使用 `openai-responses`。先在“大模型接入”完成登录并配置模型，WorkBuddy 会复用账号凭证引用和自动刷新能力，无需填写账号令牌；上游固定使用官方账号接口。
- `AnthropicMessages` provider 自动映射为 `anthropic-messages`。
- DeepSeek、Grok、Chat Completions 和其他 provider 类型自动按 `openai-chat` 处理。
- 普通 API 来源的协议仍可在 WorkBuddy 页签手动调整；ChatGPT 账号来源只允许 Responses，保留账号引用及刷新。

## 502 / 503 自动重试

WorkBuddy 专用渠道收到上游 HTTP `502` 或 `503` 时，Hub 会在返回错误前自动重试，最多额外重试 2 次，分别等待 1 秒、2 秒。重试使用同一上游地址、模型、请求体和缓存键，不会切换提供商；Responses、Chat Completions 和 Anthropic Messages 协议均适用。HTTP 错误和已有的传输错误重试共用次数上限，避免叠加重试。

该规则只针对尚未成功建立响应的上游 HTTP 请求。成功返回流式响应后，即使流中报错或中途断开，也不会重新发送已经开始的请求。持续失败时保留最后一次上游错误；`401`、`403`、`422` 等其他 HTTP 状态不触发这项重试。运行日志中的 `retrying upstream HTTP error` 会记录状态码和重试次数，不记录密钥或请求正文。

## 配置文件

默认路径：

```text
Windows: %USERPROFILE%\\.workbuddy\\models.json
macOS/Linux: $HOME/.workbuddy/models.json
```

也可以用 `WORKBUDDY_CONFIG_PATH` 指定完整路径。数组中的非目标模型与未知字段保留，旧单对象在增加第二个条目时转换为数组。WorkBuddy 地址、本机 Key、上游模型与缓存键由页签维护；实际凭据由后端从最新普通来源渠道取得，页面不显示真实 Key。Linux 路径仅保留历史资料，不纳入本轮新增支持。

备份均与模型文件同目录：`models.json.bak` 保存最近操作前完整文件（原文件不存在时为空数组）；`models.json.before-restore.bak` 保存还原前安全副本；新增 `models.json.tiancaispacehub-backup.json`（元数据版本 `1`）记录目标行、渠道快照与操作后指纹；`models.json.tiancaispacehub.lock` 为协作锁。元数据可能含渠道凭据，旧文件备份也可能含旧版上游 Key，应按私密配置保管，不提交仓库。

页面 `revision` 前缀为 `workbuddy-v2`，覆盖模型文件和全部 WorkBuddy 专用渠道。GET 和写入重读 Hub 配置；旧页面或无版本请求需刷新。写入使用文件锁、原子替换、前后指纹核对及失败补偿；WorkBuddy 自身不遵守 Hub 锁时仍有外部写入竞争，模型文件和 Hub TOML 不是崩溃原子事务，错误后需核对两端。

新备份只撤销最近目标行/渠道，目标已被后续修改时拒绝覆盖，其他条目保持；成功撤销不提供再次撤销。旧 `.bak` 无元数据时仍支持原整文件还原，但当前或备份存在新多模型条目时拒绝。损坏 JSON 的自动自救仅适用于符合条件的旧备份；新定点备份无法核验损坏文件中的目标状态时停止还原，需用户核对备份，不以空配置继续保存。

## 本地 API

| 入口 | 字段与行为 |
| --- | --- |
| `GET /api/workbuddy/config` | 可选 `entryId`；无参优先旧条目、其次首个管理条目，空字符串表示新增 |
| `POST /api/workbuddy/config` | `{ entryId?, revision, model }`；省略/空 ID 新增，已有 ID 更新；来源及模型按最新普通渠道复核，不信任提交的上游 URL/Key |
| `POST /api/workbuddy/config/delete` | `{ entryId, revision }`；删除单个模型和专用渠道 |
| `POST /api/workbuddy/config/restore` | `{ revision }`；撤销最近操作或兼容旧备份 |

状态保留 `path/exists/backupPath/backupExists/model`，新增 `entries/selectedEntryId/revision/error`。`model` 表示当前选择，条目包含 `entryId/model/configured/localUrl/sourceProvider`；返回模型清空上游 Key。文件或元数据损坏通过 `error` 显示，不误报为空的新配置。

## 缓存

OpenAI 协议的缓存键优先级为：请求体中的有效 `prompt_cache_key`、`x-workbuddy-session-id`、对应条目来源派生的 `workbuddy:<provider>`、旧版 `cacheKey`（仅在没有 provider 时兼容）、最后按条目和模型回退。缺失、`null`、空字符串和纯空白均视为未提供。相同 provider 的条目可共享默认键，不意味不同模型能共享实际缓存；路由与会话命名空间仍按条目隔离，默认思考/缓存不会读到另一同名模型条目。

Anthropic Messages provider 会按 Claude 的 `cache_control` 规则标记 system、tools 和消息尾部，使用原生默认缓存期限。WorkBuddy 请求有无缓存键都能使用这些标记；上游 Messages 请求不发送 OpenAI 的 `prompt_cache_key` 或 `prompt_cache_retention`。Hub 内部仍保留稳定的会话标识。

OpenAI Responses / Chat Completions provider 会发送 `prompt_cache_key`，并沿用 AI Gateway provider 的缓存保留时间设置。缓存键或缓存标记均不保证命中，实际取决于上游支持、前缀长度和前缀是否相同。可通过响应 usage 中的缓存读取 token 统计判断；OpenAI 通常对应 `cached_tokens`，Anthropic 原生对应 `cache_read_input_tokens`。

## 维护与关联功能

代码入口：[界面](../src/gui/workbuddy.rs)、[配置与备份](../src/workbuddy_config.rs)、[API](../src/web.rs)、[请求转换](../src/ai_gateway/workbuddy.rs)、[重试](../src/ai_gateway/providers/mod.rs)、[分派](../src/ai_gateway/handler.rs)、[路由](../src/ai_gateway/router.rs)。普通 Codex 请求排除整个 `workbuddy:` 与 `gmclaw:` 命名空间；独立入口按条目精确选渠道，缺失、停用或模型不匹配时失败，不借用其他同名模型。渠道列表和编辑器明确用途，见 [TC-013](customizations/client-channel-scope.md)。

网页导入创建普通渠道，不自动写 WorkBuddy 配置；启用后可在本页选择并保存。网页导入及各接入页来源列表排除 `workbuddy`、`workbuddy:`、`gmclaw`、`gmclaw:` 全部保留名称。来源渠道的 Key、协议或参数变更后，需要重新保存相应 WorkBuddy 条目同步副本。

2026-10-03：共享 Chat Completions 转换在非流式 Responses JSON 返回 `status=failed/cancelled` 或非空 `error` 时返回网关错误，保留失败日志，不再将它转换成空的成功回复。该行为同时用于 WorkBuddy 和天工 Claw；原有 WorkBuddy 协议、思考与缓存配置继续保留。

同日用户反馈修复新增的 SSE 聚合仅用于天工专用渠道的非流式客户端请求，不改变 WorkBuddy 的流式转发方式；详见 [天工流式兼容](customizations/gmclaw.md)。

降级到 `0.4.30-1` 前，先在当前版本撤销或删除新增 WorkBuddy 条目并确认对应渠道移除，再备份两端配置。旧版不认识 `workbuddy:<entryId>`，可能把它当成普通 Codex 候选，不能仅替换程序而保留新渠道；旧保存功能也可能覆盖多模型数组。不要让新旧 Hub 同时写配置。

当前用户负责测试；本轮只做必要编译及静态核对，不启动 WorkBuddy 或发起真实调用。待验收：多模型加载、同名不同源、思考/协议切换、删除/撤销、其他模型与字段保留、冲突与隔离。已有回归属于原版本，当前构建结果见开发交付记录。
