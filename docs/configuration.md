# Configuration

There are two separate config surfaces:

- `codexhub` config, usually this repository's `config.toml`
- Codex App config, usually `~/.codex/config.toml`

Do not mix them. `codexhub` stores IM channel and bridge settings. Codex App stores model provider, auth, and `chatgpt_base_url`.

## TianCaiSpace customization fields and concurrent saves

维护日期：2026-10-03；当前 `0.4.29-5` 开发中。天工多模型和外部消息执行端的最终编译、用户验收状态见 [二开总表](customizations/README.md) 与对应专题。

| Configuration area | Current contract | Detail |
| --- | --- | --- |
| `aiGateway.codexVisibleModels` | Saved model IDs displayed to Codex; visibility and provider routing are separate | [Dynamic models](dynamic-codex-models.zh-CN.md) |
| `aiGateway.codexModelProfiles` | Explicit model capability overrides take priority over inferred family defaults | [Dynamic models](dynamic-codex-models.zh-CN.md) |
| Provider `compatibility` / `chatDisableReasoning` | `openai_chat` identifies general Chat Completions; disabling reasoning is per provider | [Chat Completions](openai-chat-completions.md) |
| Reserved provider `workbuddy` | Dedicated WorkBuddy configuration, excluded from ordinary Codex routing | [WorkBuddy](workbuddy.md) |
| Reserved providers `gmclaw` / `gmclaw:<entryId>` | 旧天工专用渠道与多模型条目渠道，排除普通 Codex 路由、可见模型同步和网页导入；每项按独立地址路由，同模型可使用不同来源；来源配置改变后需重新保存相应条目 | [天工 Claw](customizations/gmclaw.md) |
| `aiGateway.providers[].gmclawParameters` | 仅天工专用渠道使用；`reasoningEffort` 省略时跟随上游，`temperatureMode` 为 `auto`（默认）/`omit`/`preserve`；经天工页签保存并随专用渠道备份恢复，不写天工 `extra_params` | [厂商适配](customizations/gmclaw.md#hub-的厂商适配) |
| GMClaw API `entryId` / `makeActive` / `revision` | 条目身份与模型名分开；空 ID 新增，已有 ID 更新；`makeActive` 默认 `true`；集合 revision 防止覆盖任一管理条目及默认选择的后续改动，不写 Hub TOML | [模型 API 与备份](customizations/gmclaw.md#本地配置-api) |
| `gmclawBridge` | 第二阶段外部消息执行端：默认禁用，本机 Harness、用户提供的 Token、绝对工作目录、可选天工模型 ID 及执行步数；不等同于原 `bridge` 的 Codex 连接 | [天工外部消息](customizations/gmclaw-im.md) |
| Provider `importSource` | Imported identity survives renaming; nested fields are `origin`, `key_id`, `site_name`, `key_name` | [Web import](hub-external-import.md) |
| API `_revision` | Read from `GET /api/config`, send back unchanged with `POST /api/config`; never persisted in TOML | [Web import and save behavior](hub-external-import.md) |

The full-config save endpoint requires `_revision`; missing or stale versions return HTTP 409. Reload the latest configuration, review the intended changes, and submit again. Do not blindly retry the old full document, as that could overwrite newer settings.

`POST /api/external-import/commit` is the local GUI save operation after confirmation, not a public deep-link receiver. It rereads the latest file, merges only the selected provider, and checks the preview target fingerprint before updating an existing provider. Deep links arrive through Windows IPC. Both save paths use a file lock and an atomic replacement; `_revision` is the content revision used for conflict detection. Older Hub versions do not participate in this protection, so do not edit the same file from old and new versions at once.

An enabled imported provider must have models, and its alias targets must exist in that list. This rule also applies when enabling it later through the normal configuration editor. The GUI should preserve `importSource` during ordinary edits.

The source of truth for these fields is [provider configuration](../src/ai_gateway/config.rs), [config storage](../src/config.rs), and [local API](../src/web.rs). Back up configuration before downgrade or external edits; remove a newly imported channel or restore its prior backup to undo an import.

### 天工配置边界

模型页通过 `GET/POST /api/gmclaw/config` 读取和保存管理条目，`activate/delete/restore` 操作使用同一集合版本；原顶层状态字段表示当前选中条目，新列表位于 `entries`。新增条目的数据库 ID 为 `tiancaispacehub-<entryId>`，网关地址为 `/ai-gateway/gmclaw/<entryId>/v1`；旧 `entryId=legacy` 保留原行、渠道和地址。保存、删除、设为默认均保留一次定点撤销；新元数据 v2 兼容读取旧 v1，旧程序不能保证理解新备份和多模型路由。

用户已确认上一轮模型保存后可以直接选择使用，无需完全退出重开天工；本轮 API 不再要求重启。新增多模型操作和上游流式聚合仍待用户验收。模型客户端继续接收完整 Chat JSON，OpenAI Responses 上游流式响应由 Hub 聚合，不意味着天工客户端改为逐字显示。

`gmclawBridge` 经现有 `GET/POST /api/config` 和 `_revision` 保存。界面后台重读最新配置，核对桥接字段仍等于加载值后只合并桥接字段，使用最新整体版本提交；模型保存或其他无关配置变化不会必然造成冲突。启用时由 `AppConfig::save` 校验，配置字段如下：

| 字段 | 默认值与含义 |
| --- | --- |
| `enabled` | `false`；显式启用外部消息执行 |
| `endpoint` | `http://127.0.0.1:7861`；只接受本机回环根地址或 `/v2/chat` |
| `authToken` | 空；由用户提供，与天工启动环境 `GMCLAW_AUTH_TOKEN` 相同；不自动读取进程 token |
| `projectPath` | 空；启用时必须为本机已存在的绝对目录，不创建目录、不自动补相对路径 |
| `modelId` | 省略；填写天工 `model_configs.model_id`，留空使用天工默认选择 |
| `maxSteps` | `30`，范围 `1–200` |

`GMCLAW_AUTH_TOKEN` 是启动天工执行端时的授权环境变量；`GMCLAW_CONFIG_PATH` 是 Hub 定位既有模型数据库的路径覆盖，两者用途不同。桥接 Token 保存在用户本地 Hub 配置中，不应写入仓库、截图或日志。外部消息执行使用当前 IM 账号允许名单及 `/gmclaw` 显式绑定，不扩展原 Codex 会话卡片、附件、主动取消或 MCP 配置管理。参数校验、指令、审批、停用与回滚详见 [TC-012](customizations/gmclaw-im.md)。

## `codexhub` Config

Use an explicit config path for predictable behavior:

```powershell
codexhub --config D:\path\to\config.toml daemon
```

Example:

```toml
bind = "127.0.0.1:3847"
statePath = "codexhub-state.json"

[outboundProxy]
mode = "system"
url = ""

[feishu]
appId = ""
appSecret = ""
mentionOnly = true
allowedOpenIds = []
allowedChatIds = []

[telegram]
botToken = ""
allowedChatIds = []

[wechat]
accountId = "wechat"
botToken = ""
baseUrl = ""
userId = ""
botType = "3"
allowedUserIds = []

[bridge]
enabled = true
accountId = "default"
sendStreaming = true
```

Paths relative to the config file are normalized at startup.

### `bind`

HTTP bind address for the local backend API and remote-control websocket.

Default:

```toml
bind = "127.0.0.1:3847"
```

Keep this on localhost. Do not expose it directly to a network.

### `outboundProxy`

Controls only requests that CodexHub sends to external services such as model providers,
WeChat, Telegram, Feishu HTTP APIs, and update endpoints. It does not change the operating
system proxy or the environment of other applications.

```toml
[outboundProxy]
mode = "system" # system | direct | custom
url = ""
```

- `system` follows the operating system proxy and proxy environment variables.
- `direct` disables proxy discovery for CodexHub HTTP requests.
- `custom` uses `url` as an explicit HTTP, HTTPS, SOCKS5, or SOCKS5H proxy.

Example for a local Clash mixed port:

```toml
[outboundProxy]
mode = "custom"
url = "http://127.0.0.1:7890"
```

The desktop GUI exposes the same setting under `Network` and applies it immediately while the
daemon is running. Local GUI-to-daemon requests always bypass proxies. A VPN implemented as a TUN or Network Extension may still route traffic below
the HTTP proxy layer; configure loopback exclusions in that VPN when necessary.

### `statePath`

Path to the persisted state JSON file.

This stores local bridge state such as IM conversation bindings. It should not be committed.

## Feishu

```toml
[feishu]
appId = ""
appSecret = ""
mentionOnly = true
allowedOpenIds = []
allowedChatIds = []
```

### `appId` / `appSecret`

Feishu app credentials. The desktop GUI onboarding flow can populate these automatically.

Do not commit real credentials.

### `mentionOnly`

When `true`, group messages are ignored unless the bot is mentioned. Direct messages are still accepted.

### `allowedOpenIds`

Optional allowlist of Feishu user `open_id` values.

Empty means no user-level allowlist.

### `allowedChatIds`

Optional allowlist of Feishu chat ids.

Empty means no chat-level allowlist.

## Telegram

```toml
[telegram]
botToken = ""
allowedChatIds = []
```

### `botToken`

Telegram Bot token from BotFather. `bot_token` is also accepted for hand-written config.

This is the private-chat bot flow: create your own bot with BotFather, then send messages to that bot from your Telegram account. It does not require Telegram `api_id`, `api_hash`, phone login, or an MTProto user session.

Group chats are intentionally ignored for now. This prevents other group members from controlling the host machine through the bot.

Existing configs may still contain `mentionOnly`; it is kept for compatibility but is not used while Telegram group chats are disabled.

### `allowedChatIds`

Allowlist of Telegram private chat ids as strings.

Empty means "bind the first private chat". After the first private Telegram message is accepted, `codexhub` writes that chat id into `allowedChatIds` and rejects other private chats.

For stricter setup, prefill this list before starting the bridge:

```toml
allowedChatIds = ["123456789"]
```

## WeChat

```toml
[wechat]
accountId = "wechat"
botToken = ""
baseUrl = ""
userId = ""
botType = "3"
allowedUserIds = []
```

WeChat config is normally written by the GUI QR onboarding flow. The implementation follows the OpenClaw WeChat bot path: QR login through `https://ilinkai.weixin.qq.com`, bot type `3`, long polling through `ilink/bot/getupdates`, and text replies through `ilink/bot/sendmessage`.

### `accountId`

Local label for the WeChat bot account. It is used in route keys and persisted state.

### `botToken`

WeChat bot token returned by QR onboarding. Do not commit real tokens.

### `baseUrl`

WeChat iLink API base URL. Leave empty unless the QR flow returns a redirected host.

### `userId`

The WeChat user id returned by onboarding. It is stored for display and allowlist defaults.

### `botType`

Current bot type. The default is `3`.

### `allowedUserIds`

Optional allowlist of WeChat user ids.

Empty means no user-level allowlist.

## WeCom (Enterprise WeChat)

```toml
[wecom]
enabled = true
accountId = "wecom"
botId = ""
secret = ""
displayName = "企业微信机器人"
websocketUrl = "wss://openws.work.weixin.qq.com"
allowedUserIds = []
allowedChatIds = []
```

The GUI QR flow normally writes `botId` and `secret`. CodexHub then subscribes to the official WeCom AI Bot WebSocket and supports direct/group text, streaming and final replies, initial/history thread routing cards, image/file input and output, and interactive approval template cards. Empty allowlists accept all users and chats. Keep `secret` private.

## Bridge

```toml
[bridge]
enabled = true
accountId = "default"
sendStreaming = true
```

### `enabled`

Controls whether the IM bridge should run.

When disabled, Feishu and WeCom websocket listening, Telegram polling, and WeChat polling stop, and IM messages are not forwarded to Codex.

### `accountId`

Local label used to build route keys:

```text
feishu:<accountId>:<chatId>
telegram:<accountId>:<chatId>
wechat:<accountId>:<userId>
wecom:<accountId>:<userId-or-groupChatId>
```

### `sendStreaming`

Controls whether assistant deltas are streamed into Feishu cards.

## Codex App Config

Codex App must point ChatGPT backend traffic at the local daemon:

```toml
chatgpt_base_url = "http://127.0.0.1:3847/backend-api"
```

This belongs in the Codex App config home, usually:

```text
~/.codex/config.toml
```

Third-party model provider keys stay in the Codex model provider section. Example:

```toml
model_provider = "llmx"
model = "gpt-5.5"

chatgpt_base_url = "http://127.0.0.1:3847/backend-api"

[model_providers.llmx]
name = "llmx"
base_url = "https://ai.llmx.cloud"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "your-third-party-key"
```

`chatgpt_base_url` is not the model API base URL. It is the ChatGPT backend-shaped URL used by Codex App features such as remote-control enrollment.
`codexhub` does not manage Codex App runtime settings such as `[features]`, `[windows]`, `[desktop]`, `[mcp_servers]`, or per-plugin `enabled` flags.

When CodexHub injects its default local AI Gateway provider, it keeps ChatGPT-shaped authentication enabled so Codex App retains its account-backed model catalog and Remote Control state:

```toml
web_search = "live"

[model_providers.ai-gateway]
name = "ai-gateway"
base_url = "http://127.0.0.1:3847/ai-gateway/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "dummy-token"
```

The provider identity remains `ai-gateway`, so `provider.is_openai()` is false and OpenAI-only remote compaction, request compression, and private metadata behavior stay disabled. The managed provider intentionally does not use Actor Authorization by default.

Actor Authorization requires `requires_openai_auth = false`, which makes the provider account API return no account. In Codex App 26.707.8479 this also causes the frontend to apply the official Statsig `available_models` allowlist; custom CodexHub models then disappear even though `/ai-gateway/v1/models` returns them. For that reason the native `web.run` provider gate remains disabled in the default configuration, and GPT-5.6 uses CodexHub's hosted `web_search` compatibility path instead. The `/alpha/search` proxy remains available for future Codex versions or explicit experimental configurations.

## Codex App Auth

Remote-control requires ChatGPT-compatible auth. API-key-only auth is rejected before the websocket connects.

For this local backend, use `chatgptAuthTokens` in Codex App's `auth.json`:

```json
{
  "auth_mode": "chatgptAuthTokens",
  "OPENAI_API_KEY": null,
  "tokens": {
    "id_token": "<local ChatGPT-shaped JWT>",
    "access_token": "<local ChatGPT-shaped JWT>",
    "refresh_token": "",
    "account_id": "acct_codexhub_local"
  },
  "last_refresh": "2026-05-26T00:00:00Z"
}
```

The local JWT needs to parse as a JWT and include the ChatGPT-shaped auth metadata Codex reads:

```json
{
  "email": "codexhub-local@example.local",
  "https://api.openai.com/auth": {
    "chatgpt_account_id": "acct_codexhub_local",
    "chatgpt_user_id": "user_codexhub_local",
    "user_id": "user_codexhub_local",
    "chatgpt_plan_type": "pro",
    "chatgpt_account_is_fedramp": false
  }
}
```

This identity is local bridge identity only. The model provider key controls the actual model provider.

The desktop GUI provides Codex App configuration controls that write the local Codex App config for you.

The CLI equivalent is:

```powershell
codexhub --config config.toml configure-codex-app
```

Optional provider fields:

```powershell
codexhub --config config.toml configure-codex-app --provider-name llmx --provider-base-url https://ai.llmx.cloud --provider-key sk-... --model gpt-5.5
```

When provider fields are supplied without `--provider-name`, `llmx` is used as the provider name.

The daemon does not modify Codex App config on startup. It writes these files only when the desktop GUI or CLI command is used.

## Feishu App Requirements

For a manually created Feishu app, enable bot messaging and websocket event delivery. Subscribe to:

```text
im.message.receive_v1
card.action.trigger
```

Typical permissions:

```text
im:message
im:message:send_as_bot
im:resource
```

Depending on Feishu app type and tenant policy, additional scopes may be required for card updates or attachment downloads.

## Local Files To Keep Private

These should stay ignored:

```text
config.toml
codexhub-state.json
*.log
.im/
target/
target-verify/
reference/
```

Do not commit Codex App `auth.json`, third-party provider keys, Feishu credentials, open ids, or chat ids.
