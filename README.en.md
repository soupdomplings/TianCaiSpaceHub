# TianCaiSpaceHub

[中文说明](README.md)

[Documentation index](docs/README.md) · [Customization inventory](docs/customizations/README.md) · [Customization changelog](docs/customizations/CHANGELOG.md) (maintained in Chinese)

Release candidate `v0.4.30-4` checks the installed GMClaw application and runtime capabilities instead of requiring version `1.1.1`, a fixed script filename, or a renderer hash. Compatible versions can synchronize, with writable state and cache checks. The user authorized GitHub publication and fresh Windows/macOS Actions builds; see the [delivery record](docs/releases/v0.4.30-4.md) for source, builds, and assets. Native compatibility awaits user acceptance. Existing `v0.4.30-3` packages retain the old restriction; see [GMClaw IM](docs/customizations/gmclaw-im.md) for behavior and rollback.

Previous version `v0.4.30-3` is committed to `main` and has a [GitHub pre-release](https://github.com/soupdomplings/TianCaiSpaceHub/releases/tag/v0.4.30-3); Windows and macOS Actions builds succeeded; installers are uploaded and downloaded copies passed static validation. Windows packages are unsigned; the macOS app is ad-hoc signed and not notarized. It is based on upstream CodexHub v0.4.30. It includes the complete GMClaw IM session workflow, desktop history and message synchronization, automatic reconnection, approvals, full project selection, temporary reply status, and unified `TianCaiSpaceHub` program naming. Existing model isolation and WorkBuddy/GMClaw multi-model configuration are retained. The user reports no new issue in the latest local checks; other platforms and installation upgrades await acceptance. See the [delivery record](docs/releases/v0.4.30-3.md) for source, Actions, and publication status. Only `main` is used for source development; full customization versions use tags and Releases.

GMClaw integration uses `/tg` to initially enable or select GMClaw through the existing platform session workflow. `/gpt` returns to Codex, `/q` returns to create/restore, and `/wb` remains unavailable without changing the executor. Enabled GMClaw reconnects after desktop restarts; IM sessions use real desktop tasks and original memory identities. Feishu and WeCom show temporary reply progress and platform approval controls; WeChat sends final text and numbered approval choices. Desktop synchronization checks the installed GMClaw identity and runtime fields; its local channel is first enabled by launching from Hub. If an old desktop inherited port 3847, exit it normally once before launching it with the new Hub. Windows is prioritized, followed by macOS; this change does not add Linux support. Installers are built by GitHub Actions, and prior build results retain their own version identity. See [GMClaw IM](docs/customizations/gmclaw-im.md) and [naming and packaging](docs/customizations/desktop-and-packaging.md#当前交付状态).

## Product Preview

| Feature | Description |
| --- | --- |
| Remote and local side by side | Use Feishu, WeChat, and Telegram to control local Codex App, the Codex VS Code extension, and Codex CLI. The same Codex session can stay synchronized between IM and local clients. |
| Local Codex access | Does not modify Codex frontend code. Connect Codex App, the VS Code extension, and Codex CLI through the local backend. |
| Codex session management | Manage Codex session history from the GUI. After switching providers or enabling AI Gateway, move old sessions into the current entry so they still appear in the Codex App sidebar. |
| Manage Codex sessions from IM | Use the native Codex remote-control protocol to create and resume Codex sessions from IM. |
| Built-in AI Gateway | Keep Codex App on its native Responses entry while routing model calls to OpenAI, DeepSeek, Anthropic/Claude, and Z.AI Anthropic (API / Coding Plan) from the local GUI. |

<p align="center">
  <img src="docs/assets/product/main.png" alt="TianCaiSpaceHub GUI status and config UI" width="900">
</p>
<p align="center">
  <img src="docs/assets/product/codex-app-chat.png" alt="Codex App session sync and image result" width="900">
</p>
<p align="center">
  <img src="docs/assets/product/deepseek.jpg" alt="Codex App using DeepSeek through AI Gateway" width="900">
</p>

AI Gateway is a local model entry built into `TianCaiSpaceHub`. Codex App keeps sending normal Responses-style requests, while `TianCaiSpaceHub` routes them to the provider you configured and converts the result back into the shape Codex expects. Providers, visible models, model aliases, request logs, and image-generation-tool filtering are managed in the GUI.

<p align="center">
  <img src="docs/assets/product/feishu-mobile-image.jpg" alt="Feishu mobile Codex image result" width="360">
  <img src="docs/assets/product/tg.jpg" alt="Telegram mobile Codex thread creation" width="360">
</p>
<p align="center">
  <img src="docs/assets/product/syn.png" alt="Feishu IM and local Codex CLI synchronized session" width="900">
</p>

## Quick Start

For Codex App and the VS Code extension, the usual flow is: download the app -> configure AI Gateway -> write Codex config -> restart Codex. Connect an IM channel only when you need Feishu, WeChat, or Telegram remote control. Codex CLI still requires starting its own app-server in step 7.

### 0. Prerequisites

- macOS, Windows, or Linux device
- Codex App, the Codex VS Code extension, or Codex CLI
- No ChatGPT account and no acceleration network required
- At least one model API key: OpenAI Responses, DeepSeek, Anthropic/Claude, Z.AI Anthropic (API / Coding Plan), or another compatible provider
- Optional IM channel: needed only for Feishu, WeChat, or Telegram remote control

### 1. Install

Download the Windows MSI/portable ZIP or macOS DMG/App ZIP for the chosen version from GitHub Releases. On macOS, copy the App to Applications and confirm the system prompt if shown. On Windows, extract the portable ZIP and run its EXE. Current source uses `TianCaiSpaceHub.exe` and `TianCaiSpaceHub.app`; the published v0.4.30-2 packages retain `TianCaiSpace Hub.exe`, `CodexHub.exe` inside the MSI, and `TianCaiSpace Hub.app`. Renaming source does not alter previously published files. The app does not install startup items or start in the background automatically.

Historical Linux reference only: use `chmod +x "TianCaiSpace Hub Linux x86_64.AppImage"` for that older AppImage. Current customization releases cover Windows and macOS; this iteration does not add Linux support.

This customization removes update checks from Help, the tray, and startup. Upgrade using the chosen version's installer or portable package after exiting the old Hub and backing up configuration. See [desktop and packaging](docs/customizations/desktop-and-packaging.md).

### 2. Open The App

Open `TianCaiSpaceHub`. The GUI starts the local backend automatically and stops the backend it started when the GUI exits.

Continue when the status overview shows the local service is running.

### 3. Connect An IM Channel (Optional, For Remote Control)

Open the `消息接入` page and choose one channel:

- Feishu: click `扫码使用新机器人` and complete QR onboarding.
- Telegram: paste the BotFather token and click `保存并接入`. Private chats support text, image/file input, in-place menu and approval updates, aggregated command and subagent progress per turn, and streamed agent reply drafts; group chats are ignored.
- WeChat: click `扫码连接微信` and confirm in WeChat.
- WeCom: click `添加企业微信机器人` and confirm by scanning with WeCom. Direct/group text, streaming and final replies, image/file transfer, initial/history thread selection cards, and interactive approval template cards are supported.

After a channel is connected, the `IM 通道` status panel becomes available. Normal use does not require scanning or entering the token again unless you switch bots.

The GMClaw integration in `v0.4.30-3` starts in chat: send `/tg` in connected Feishu, WeChat, or WeCom. Hub prepares authorization, checks the connection, and starts GMClaw when needed. The GMClaw tab contains model settings and a short guide; no bridge form is required. Choose the platform's existing **Create session** entry or send `/tg new` to open the same settings workflow. Feishu uses its existing directory dropdown, custom path field, and create button; WeChat and WeCom retain their existing interactions. Select a directory and model, then confirm through the current interface. Missing directories are created only when the session is created; each session keeps its directory and model ID.

Model choices include native GMClaw settings and Hub-managed entries. GMClaw exposes directory and model selection; reasoning parameters follow the selected model configuration, and tool permissions follow GMClaw policy. The default directory uses a valid legacy setting when available, otherwise the `workspace` folder under GMClaw's data directory. See [GMClaw IM](docs/customizations/gmclaw-im.md) for defaults, isolation, and limits.

You can open GMClaw normally, then send `/tg`. Once integration is enabled, Hub checks every 5 seconds and automatically identifies new runtime authorization for the same user's official desktop after a restart; another `/tg` is unnecessary. Runtime authorization stays in memory and is never saved back to configuration or shown in chat. Background reconnection, ordinary messages, and overview refreshes do not launch the desktop or replay tasks. An explicit `/tg` can start it when needed, waiting up to 30 seconds for shutdown or initialization without terminating processes or launching onto an occupied port. Model saves do not require a restart.

The user confirmed automatic connection on 2026-10-06; history restoration, approvals, model calls and desktop display remain pending acceptance. This iteration fixes history restoration rejecting finished native sessions with a stale `processing` flag: it checks the last complete Harness turn, pending approvals, timestamps and stable repeated observations. Incomplete or unknown turns remain blocked; it does not rewrite desktop status or replay a task. Build and artifact identity are recorded in [naming and packaging](docs/customizations/desktop-and-packaging.md#当前交付状态).

New IM sessions create real desktop scenarios/tasks and session metadata; user messages, replies, and approval notices are saved to the task. Restoration lists tasks across all desktop scenarios for senders admitted by the existing allowlists. Only one IM sender can claim a session at a time: `/q` releases it, while a successful `/gpt` switch retains the current session but releases its claim. Returning with `/tg` claims the retained session again; if another sender has claimed it, use `/tg new` to create a new session. Restoration preserves the original directory, model, and memory identity; running, approval-pending, or uncertain sessions cannot be restored normally. Hub persists session links and status, but cannot reconstruct older sessions that existed only in Hub memory without desktop messages. Open windows receive scoped message updates when the required runtime capabilities are available; otherwise saved messages are retained and the reason is reported, without forcing a reload or restart. See [GMClaw IM](docs/customizations/gmclaw-im.md).

The overview follows the Codex, WorkBuddy, or GMClaw integration tab and retains that view on shared tabs. This does not change IM executor selection. GMClaw model **Connected** means this Hub has received a request on the configured local model route and the desktop is still running; no request shows a waiting state, and a desktop exit or restart requires new evidence. This status is separate from IM Harness authorization and upstream model acceptance. Logs distinguish client streaming from the actual upstream request; upstream streaming aggregated into JSON is labeled `Streaming (Upstream)`. WorkBuddy external tasks and GMClaw Telegram remain unsupported. See [model integration](docs/customizations/gmclaw.md), [request logs](docs/ai-gateway-request-log-detail-patch.zh-CN.md), and [GMClaw IM](docs/customizations/gmclaw-im.md).

### 4. Configure AI Gateway

Open the `Codex 接入` page and add a model provider in the AI Gateway area. The GUI includes common provider templates, and you can also enter provider details manually:

- Provider name
- Provider type
- Third-party Base URL
- API Key
- Model list

If the upstream model name differs from the name you want to expose in Codex, use `Edit Model Aliases`. For example, the upstream model can be `GLM-5.3` while Codex shows `glm-5.3`.

If a provider rejects Codex's image generation tool, enable `Filter image generation tool`. It takes effect immediately and removes `image_generation` from outgoing AI Gateway requests.

### 5. Write Codex Config

Click `Write Codex Config` on the `Codex 接入` page. This points Codex App and the Codex VS Code extension at the local `TianCaiSpaceHub` service and routes model requests through the local AI Gateway.

To go back to the previous Codex connection, click `Restore Codex Config`. The restore action is shown only after Codex config has been written.

### 6. Open Codex

Open Codex App or the Codex VS Code extension normally, then enable remote-control / control this computer.

When connected, `TianCaiSpaceHub` shows the Codex control channel as connected.

You do not need to see a remote device list in Codex App's connection settings. This project uses a local backend plus IM bridge. If the `TianCaiSpaceHub` status overview is normal, you can use it directly from the connected IM channel.

If Codex App, the Codex VS Code extension, and Codex CLI are connected to `TianCaiSpaceHub` at the same time, new or resumed IM sessions choose the execution endpoint by fixed priority: Codex App > Codex VS Code extension > Codex CLI. After a session is bound, later messages keep using the selected endpoint until the IM session exits or binds again.

### 7. Use Codex CLI

If you want Codex CLI to work with Feishu / Telegram / WeChat, you do not need to replace the `codex` command or install a wrapper. Use the same three-step flow on macOS, Windows, and Linux.

1. Open the `TianCaiSpaceHub` desktop app, finish IM channel setup and Codex access, and keep it running.

2. Open a terminal in the project directory and start Codex app-server:

```text
codex app-server --listen ws://127.0.0.1:3849 --remote-control
```

3. Open another terminal in the same project directory and connect the local Codex TUI:

```text
codex --remote ws://127.0.0.1:3849
```

After that, you can message the bot from IM, and you can also keep using the same Codex app-server from local Codex TUI. If port `3849` is already in use, choose another local port, but keep the addresses in step 2 and step 3 identical.

### 8. Use IM

Send a message to the bot in Feishu, a Telegram private chat, WeChat, or WeCom.

If the IM chat is not bound to a Codex thread yet, the bot first asks you to create a new thread or resume an existing one. After selection, the chat is bridged to that Codex thread.

The WeChat path depends on a context token issued by the WeChat client. During long tasks or when the phone client has been inactive for a while, the token may expire and the local backend may temporarily be unable to send messages. If this happens, send `!` or `?` in WeChat to refresh the token. These activation messages are only used to recover the send path and are not forwarded to Codex.

## Network and Proxy

The Network menu provides three outbound modes: use the system proxy, connect directly, or use a custom HTTP/SOCKS5 proxy. This setting only affects requests TianCaiSpaceHub sends to model providers, WeChat, Telegram, Feishu HTTP APIs, and update endpoints. It does not modify macOS `launchctl`, Windows user environment variables, or networking for other applications.

For a local Clash or V2Ray proxy, select the custom proxy option and enter `http://127.0.0.1:7890` or `socks5://127.0.0.1:1080`. The setting applies immediately while the daemon is running. Loopback communication between the GUI, Codex App, VS Code, and TianCaiSpaceHub does not use this outbound proxy.

TUN and Network Extension VPNs operate below the HTTP proxy layer. If such a VPN intercepts loopback traffic, exclude `localhost`, `127.0.0.1`, and `::1` in the VPN application.

## AI Gateway

AI Gateway solves one practical problem: Codex expects its native model entry, but users often want to use more model providers. After providers are configured in the GUI, Codex App still sees a normal model list; `TianCaiSpaceHub` handles provider routing and protocol conversion locally.

Current highlights:

- OpenAI Responses providers for native or compatible Responses services.
- DeepSeek Responses providers for the native DeepSeek `/v1/responses` API, including hosted web search, function tools, and `apply_patch`.
- DeepSeek Chat / Chat Completions providers retain the existing conversion path back to Codex-compatible Responses output.
- Anthropic Messages providers for Claude / Anthropic-compatible models, including text, images, tool calls, thinking output, and web search conversion.
- Z.AI Anthropic for both the standard API and Coding Plan, with API-key model discovery from Z.AI's separate model catalogs and GLM web search normalization.
- Model aliases for case differences, provider-specific names, and third-party relay names.
- Codex visible model selection.
- Request logs with original Codex request, upstream request, response or error, tokens, cache usage, cost, latency, TTFT, and request body size.
- Image generation tool filtering, disabled by default.

All of this is configured from the GUI. Users do not need to hand-edit config files.

## Community And Support

For questions or feedback, open a GitHub issue or message me through the WeChat public account.

<img src="docs/assets/wechat-public-account.jpg" alt="WeChat public account" width="220">

The WeChat group is for issue feedback, usage discussion, and feature suggestions.

<img src="docs/assets/wechat-group.png" alt="AI-Agent technical discussion group" width="260">

## IM Commands

In `v0.4.30-3`, Feishu, WeChat, and WeCom accept `/tg` to initially enable or switch to GMClaw and `/gpt` to return to Codex; an enabled integration reconnects automatically after desktop restarts. `/wb` reports unavailable and keeps the current mode. In GMClaw mode, `/q` leaves and releases the current session while retaining GMClaw mode and returns to the platform's existing create/restore entry. `/tg new` opens the same session settings directly. Follow the current card buttons or text menu to choose a directory and model, then create a real desktop task. Restoration lists tasks across all desktop scenarios and preserves the selected task's session, directory, and memory identity. `/s` reports that the task has not been stopped and directs you to GMClaw desktop. Running tasks, approvals, claims by another IM sender, and unknown execution states have protective checks. See [GMClaw IM](docs/customizations/gmclaw-im.md). Telegram retains the Codex path.

The following commands apply to the original Codex mode. Follow the card prompts for other actions.

```text
/q         interrupt and clear the current binding
```

Approval prompts are updated after selection where the platform supports it.

## Restore Codex Config

Click `Restore Codex Config` in the GUI to restore the Codex connection from before setup. After restore, Codex App no longer sends model requests through the local AI Gateway.

This does not uninstall Codex and does not delete Codex session history.

## Project Boundary

`TianCaiSpaceHub` only supports the clean official Codex remote-control path.

It does not:

- install a `codex` wrapper
- replace Codex CLI
- launch Codex App through a shim
- install login items or startup agents
- run as a background service automatically
- change Codex model, sandbox, approval policy, cwd, or environment

The local backend starts only when the user opens the GUI or explicitly starts it from development tooling.

## Technical Notes

Runtime path:

```text
Codex App / Codex VS Code extension / Codex CLI app-server
  |
  | chatgpt_base_url = "http://127.0.0.1:3847/backend-api"
  | user enables remote control, or starts codex app-server --remote-control
  v
official Codex app-server
  |
  | outbound remote-control websocket
  v
TianCaiSpaceHub local backend
  |
  | Feishu websocket events
  | Feishu message/card APIs
  | Telegram long polling
  | Telegram Bot API
  | WeChat iLink long polling
  | WeChat sendmessage API
  | WeCom AI Bot WebSocket / aibot_send_msg
  v
IM channel
```

The project implements the official remote-control endpoints:

```text
POST /backend-api/wham/remote/control/server/enroll
GET  /backend-api/wham/remote/control/server
```

Codex remote-control requires a ChatGPT-compatible auth mode. This project writes local `ChatgptAuthTokens` to satisfy Codex App's remote-control account check. API-key-only auth does not start remote control.

Thread binding model:

- Codex app-server remains the source of truth for thread lifecycle and history.
- One IM chat binds to one Codex thread at a time.
- If the IM chat has not bound a thread yet, the bridge asks whether to create or resume a thread.
- Resuming a thread from IM subscribes to that thread's future remote-control events.
- IM-origin turns are tracked by turn id to avoid `userMessage` echo.

## Development

The Cargo package is `tiancaispacehub` and the binary is `TianCaiSpaceHub`. The user currently handles functional testing. These commands compile code and create a debug EXE without launching the app or executing tests. Generate Windows/macOS release packages through GitHub Actions.

```powershell
cargo fmt --all -- --check
cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub
cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub
```

Useful status endpoints while the daemon is running:

```text
GET http://127.0.0.1:3847/api/status
GET http://127.0.0.1:3847/api/remote-control/status
GET http://127.0.0.1:3847/api/remote-control/backend-status
GET http://127.0.0.1:3847/api/events
```

## Security Notes

- The daemon binds to `127.0.0.1` by default. Do not expose it publicly.
- Locally saved IM tokens, model API keys, and Codex auth data are secrets; do not commit them.
- Attachments from Feishu and Telegram are downloaded to local state-adjacent `.im/attachments/feishu/` and `.im/attachments/telegram/` directories.
- Restrict access with `allowedOpenIds` and/or `allowedChatIds` for real usage.
- The bridge can send approval decisions to Codex. Treat Feishu / Telegram / WeChat / WeCom access as equivalent to local Codex approval access.

## More Docs

- [Architecture](docs/architecture.md)
- [WorkBuddy integration](docs/workbuddy.md)
- [WeChat integration plan](docs/wechat-integration-plan.zh-CN.md)
- [Auth notes](docs/auth-notes.md)
- [Troubleshooting](docs/troubleshooting.md)

## License

Apache-2.0
