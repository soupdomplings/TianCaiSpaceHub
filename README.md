# TianCaiSpaceHub

[English](README.en.md)

[文档总入口](docs/README.md) · [二开功能总表](docs/customizations/README.md) · [二开变更记录](docs/customizations/CHANGELOG.md)

当前发布候选 `v0.4.30-4`：天工桌面同步已移除 `1.1.1` 版本号、固定界面文件名和指纹限制，改为识别已安装天工与实际界面能力；兼容接口和字段的新版本可直接同步，并核对实际写入状态和会话缓存的可写能力。用户已授权更新 GitHub 并重新运行 Windows/macOS Actions，源码、构建及附件状态见 [本版交付](docs/releases/v0.4.30-4.md)。两平台实机兼容待用户验收，上一版 `v0.4.30-3` 的已有安装包仍保留旧限制。实现和回退见 [天工外部消息](docs/customizations/gmclaw-im.md)。

上一版 `v0.4.30-3` 已推送到 `main` 并创建 [GitHub 预发布版](https://github.com/soupdomplings/TianCaiSpaceHub/releases/tag/v0.4.30-3)，Windows/macOS 安装包已由 Actions 构建、上传并下载静态核验通过；本版纳入天工外部消息完整接入、平台会话与审批交互、桌面消息同步、自动重连、完整项目目录和临时回复状态，以及统一的 `TianCaiSpaceHub` 程序名称。Codex、WorkBuddy、天工专用模型入口隔离和多模型配置继续保留，上游仍为 CodexHub `v0.4.30`。当前本机测试用户反馈未发现新增问题，其他平台与安装升级待验收；构建、源码和发布状态见 [版本交付](docs/releases/v0.4.30-3.md)，上游来源见 [整合记录](docs/upstream-v0.4.30-integration.md)。源码只维护 `main`，完整二开版本通过标签和 Releases 管理。

已发布 `v0.4.30-3` 安装包由 GitHub Actions 构建 Windows x64 MSI/便携 ZIP 和 macOS Apple Silicon/Intel universal DMG/App ZIP。该发布版的 Windows GUI/测试代码编译、两平台 Actions 构建及附件核验通过；Windows 包未签名，macOS 为 ad-hoc 签名且未公证，详见版本交付记录。本版新增修改的构建与验收另见 [当前开发记录](docs/customizations/desktop-and-packaging.md#当前交付状态)。上一版 `v0.4.30-2` 包和哈希保持历史归属，不以旧构建替代本版结果。网页导入沿用平台协议、URL 事件及同用户实例转交，见 [导入说明](docs/hub-external-import.md)。

当前天工外部消息使用与 Codex 相同的平台会话入口、目录表单和模型选择流程。启用后后台自动重连；IM 创建真实桌面任务、保存正文并恢复全部原生场景历史，保留会话目录、模型与记忆身份。飞书/企微使用按钮审批，微信使用文字选项；桌面消息局部同步按天工安装身份与实际运行能力识别，首次仍从 Hub 启用本机通道。正式回复结束临时等待状态；其他平台与异常场景待验收。已发布 `v0.4.30-3` 包的固定版本限制不因源码修改自动解除，完整使用、构建与回滚见 [天工外部消息](docs/customizations/gmclaw-im.md)、[模型接入](docs/customizations/gmclaw.md) 及 [已发布版交付](docs/releases/v0.4.30-3.md)。

## 产品预览

| 功能 | 说明 |
| --- | --- |
| 远程和本地同屏操作 | 支持飞书、微信、Telegram 远程连接本地 Codex App、Codex VS Code 插件和 Codex CLI，同一个 Codex 会话可以在 IM 和本地客户端之间同步操作。 |
| 本地 Codex 接入 | 不修改任何 Codex 前端代码，通过本地 backend 连接 Codex App、VS Code 插件和 Codex CLI。 |
| Codex 会话管理 | 在 GUI 中管理 Codex 历史会话；切换 provider 或接入 AI Gateway 后，可以把旧会话移动到当前入口，让 Codex App 左侧继续看到。 |
| 支持 IM 端管理 Codex 会话 | 利用 Codex 原生 remote-control 协议，在 IM 里创建会话、恢复会话、处理审批。 |
| 内置 AI Gateway | 让 Codex App 继续使用原生 Responses 入口，同时可以在本地 GUI 中接入 OpenAI、DeepSeek、Anthropic/Claude、智谱 Anthropic（API / Coding Plan）等模型渠道。 |
| WorkBuddy 多模型 | 逐条新增、编辑、删除与撤销，独立渠道和地址，保留原生配置中的其他模型；大模型列表显示各渠道的适用接入点。详见 [WorkBuddy](docs/workbuddy.md) 和 [用途与隔离](docs/customizations/client-channel-scope.md)。 |
| 天工 Claw 接入 | 独立页签管理多个模型，适配厂商参数及流式响应，支持撤销配置；保存模型无需重启天工。`v0.4.30-3` 纳入天工复用飞书、微信、企业微信原有会话流程、完整目录/模型设置、同步与审批；`/wb` 外部任务暂不支持，其他平台及异常场景待验收，详见 [模型接入](docs/customizations/gmclaw.md) 和 [外部消息接入](docs/customizations/gmclaw-im.md)。 |

<p align="center">
  <img src="docs/assets/product/main.png" alt="TianCaiSpaceHub GUI 状态和配置界面" width="900">
</p>
<p align="center">
  <img src="docs/assets/product/codex-app-chat.png" alt="Codex App 会话同步和图片结果" width="900">
</p>
<p align="center">
  <img src="docs/assets/product/deepseek.jpg" alt="Codex App 通过 AI Gateway 使用 DeepSeek 模型" width="900">
</p>

AI Gateway 是 `TianCaiSpaceHub` 内置的本地模型入口。Codex App 仍然按它熟悉的方式发送请求，`TianCaiSpaceHub` 在本地把请求转到你配置的模型渠道，并把返回结果整理回 Codex 能消费的格式。渠道、模型列表、模型映射、请求日志和生图工具过滤都可以在 GUI 里完成。

<p align="center">
  <img src="docs/assets/product/feishu-mobile-image.jpg" alt="飞书移动端展示 Codex 图片结果" width="360">
  <img src="docs/assets/product/tg.jpg" alt="Telegram 移动端创建 Codex thread" width="360">
</p>
<p align="center">
  <img src="docs/assets/product/syn.png" alt="飞书 IM 与本地 Codex CLI 同步会话" width="900">
</p>


## 快速使用

Codex App 和 VS Code 插件通常只需要：下载程序 -> 配置 AI Gateway -> 写入 Codex 配置 -> 重启 Codex。只有需要飞书、微信、Telegram 远程控制时，才需要接入 IM。Codex CLI 需要按第 7 步单独启动 app-server。

### 0. 前置条件

- macOS、Windows 或 Linux 设备
- Codex App、Codex VS Code 插件或 Codex CLI
- 不需要 ChatGPT 账号，也不需要“加速网络”
- 至少一个模型服务 API Key：OpenAI Responses、DeepSeek、Anthropic/Claude、智谱 Anthropic（API / Coding Plan）或其它兼容渠道
- 可选 IM 通道：只有需要飞书、微信、Telegram 远程控制时才需要

### 1. 安装

从 GitHub Releases 下载对应版本的 Windows MSI/便携 ZIP 或 macOS DMG/App ZIP。macOS 将 App 拖到 Applications 后打开，系统提示来自互联网时按提示确认；Windows 便携包解压后打开其中的 EXE。当前源码统一使用 `TianCaiSpaceHub.exe` 和 `TianCaiSpaceHub.app`；已发布 `v0.4.30-2` 包仍保留当时的 `TianCaiSpace Hub.exe`、MSI 内 `CodexHub.exe` 及 `TianCaiSpace Hub.app`，更名未回写旧包。这个 App 不安装开机启动项，也不自动常驻后台。

Linux 仅保留历史用法参考：旧包 `TianCaiSpace Hub Linux x86_64.AppImage` 可用 `chmod +x "TianCaiSpace Hub Linux x86_64.AppImage"` 赋予执行权限。当前二开只发布 Windows/macOS，本轮不扩展 Linux。

当前二开版已取消帮助菜单、托盘和启动时的更新检查。升级时使用指定版本的安装包或便携包，先退出旧 Hub 并备份配置；详见 [桌面与交付说明](docs/customizations/desktop-and-packaging.md)。

### 2. 打开应用

打开 `TianCaiSpaceHub`。GUI 会自动启动本地 backend，并在退出时关闭本次启动的 backend。

状态概览显示本地服务运行后继续下一步。

### 3. 接入 IM 通道（可选，远程控制时需要）

切到“聊天工具接入”页面，选择一个通道：

- 飞书：点击“扫码使用新机器人”，按二维码流程完成接入。
- Telegram：填写 BotFather 提供的 Bot Token，点击“保存并接入”。当前支持私聊文本、图片/文件输入、原位菜单与审批状态更新、同一 turn 的命令和子代理协作进度聚合，以及 Agent 回复草稿流式展示；群聊不会接入。
- 微信：点击“扫码连接微信”，使用微信扫码确认。
- 企业微信：点击“添加企业微信机器人”，使用企业微信扫码确认。支持私聊/群聊文本、流式与最终回复、图片文件、初始/历史会话选择卡片和审批模板卡片。

接入成功后，状态概览里的“IM 通道”会显示可用。之后正常使用不需要反复扫码或重新填 token；只有更换机器人时才需要重新接入。

自 `v0.4.30-3` 起，天工外部消息直接从聊天接入：在已连接的飞书、微信或企业微信中发送 `/tg`，Hub 自动准备授权、检查连接，并在需要时启动天工。天工页只保留模型配置与使用指引，无需填写桥接表单。选择当前平台原有的“新建会话”，或发送 `/tg new` 进入同一设置流程。飞书使用原卡片中的目录下拉、自定义目录和创建按钮；微信、企业微信沿用各自原有交互。选好目录与模型后，按当前界面确认创建；缺失目录仅在创建会话时建立，每个会话固定自己的目录和模型 ID。

模型可选择天工已有配置及 Hub 管理条目。天工表单只开放目录和模型；思考参数沿用所选模型接入配置，工具权限沿用天工策略。默认目录优先使用合法旧配置，否则使用天工数据目录下的 `workspace`；完整默认值、会话隔离与限制见 [外部消息接入](docs/customizations/gmclaw-im.md)。

可先正常打开天工，再发送 `/tg`。接入启用后，Hub 每 5 秒后台检查并自动识别同一用户官方天工的新运行授权；天工重启后等待就绪即可继续，无需重复切换命令。临时运行授权只留在内存，不写回配置或显示在聊天中。后台重连、普通消息和概览不会自行启动桌面或重放任务；需要启动时仍通过显式 `/tg`，最多等待 30 秒处理退出或初始化，不强退程序、不重复启动占用端口的实例。模型保存也不要求重启。

新会话会在天工创建真实场景/任务及会话元数据，用户消息、答复和审批提示保存到该任务。恢复入口展示全部桌面场景的任务，现有允许名单中获准的发送者均可浏览；同一会话一次只能由一个 IM 发送者认领。`/q` 释放认领，`/gpt` 成功切离时保留当前会话但释放认领；`/tg` 返回原会话时重新认领，若被他人占用可用 `/tg new` 新建。恢复沿用原目录、模型和记忆身份；最新一轮事件与保存状态共同判断是否可恢复，避免把天工正常结束后残留的 `processing` 误认为仍在执行。待审批、没有可确认结束记录或 Hub 结果未知的会话仍不能普通恢复。Hub 同时持久保存会话关联与状态，旧版本仅内存中存在且没有桌面消息的历史无法补全。已打开窗口通过局部能力适配同步消息；天工界面或接口缺少所需能力时说明具体原因，已保存正文保留，不强制重载或重启客户端，详见 [外部消息接入](docs/customizations/gmclaw-im.md)。

2026-10-06 用户已确认前阶段新版能自动连接天工；这是自动连接的有限验收，不替代历史恢复、审批、模型调用和桌面显示的验收。本轮历史恢复修复的构建与独立程序身份见 [品牌与交付](docs/customizations/desktop-and-packaging.md#当前交付状态)。

状态概览随 Codex、WorkBuddy、天工接入页签切换，公共页保留最近视角；切页不改变 IM 的 `/tg`、`/gpt` 选择。天工模型“已连接”表示当前 Hub 已收到对应本地入口请求、配置地址匹配且天工仍运行；没有请求时显示等待，退出或重启后重新识别。这个状态与 IM Harness 授权独立，也不代表上游模型调用已经验收。请求日志分别记录客户端与实际上游流式模式，上游流式聚合为完整 JSON 时显示 `Streaming (Upstream)`。WorkBuddy 外部任务、天工 Telegram 继续标明不支持。细节见 [模型接入](docs/customizations/gmclaw.md)、[请求日志](docs/ai-gateway-request-log-detail-patch.zh-CN.md) 与 [外部消息接入](docs/customizations/gmclaw-im.md)。

### 4. 配置 AI Gateway

切到 “Codex 接入” 页面，在 AI Gateway 区域添加模型渠道。GUI 会提供常用服务商模板，也可以手工填写：

- 渠道名称
- 服务商类型
- 第三方 Base URL
- API Key
- 模型列表

如果上游模型名和你希望在 Codex 里看到的名字不一致，可以在“编辑模型映射”里把一个上游模型映射成一个或多个 Codex 可见模型。例如上游要求 `GLM-5.3`，Codex 里可以显示成 `glm-5.3`。

如果渠道不支持 Codex 请求里的生图工具，勾选“过滤生图工具”即可实时移除 `image_generation` 工具，不需要再改 Codex 配置。

### 5. 写入 Codex 配置

在 “Codex 接入” 页面点击“写入 Codex 配置”。这一步会让 Codex App 和 Codex VS Code 插件连接到本机 `TianCaiSpaceHub`，并把模型请求交给本地 AI Gateway。

写入后如果想回到原来的 Codex 连接方式，点击“恢复 Codex 原有配置”即可。GUI 只在已经写入过配置时显示恢复入口，避免第一次使用时误操作。

### 6. 打开 Codex

正常启动 Codex App 或 Codex VS Code 插件，并打开 remote-control / 控制这台电脑。

连接成功后，`TianCaiSpaceHub` 里会看到 Codex 控制通道变为已连接。

不需要在 Codex App 的“连接”设置页里看到远程连接设备列表。这个项目走的是本地 backend + IM bridge，只要 `TianCaiSpaceHub` 的状态概览都正常，就可以直接在已接入的 IM 里使用。

如果 Codex App、Codex VS Code 插件和 Codex CLI 同时连接到 `TianCaiSpaceHub`，IM 端新建或恢复会话时会按固定优先级选择执行端：Codex App > Codex VS Code 插件 > Codex CLI。会话绑定后，后续消息会继续发给当时选中的执行端，直到该 IM 会话退出或重新绑定。

### 7. 使用 Codex CLI

如果希望 Codex CLI 和飞书 / Telegram / 微信交互，不需要替换 `codex` 命令，也不需要安装包装脚本。macOS、Windows 和 Linux 都按下面三步操作。

1. 打开 `TianCaiSpaceHub` 桌面程序，完成 IM 通道和 Codex 接入，并保持程序运行。

2. 在要操作的项目目录打开终端，启动 Codex app-server：

```bash
codex app-server --listen ws://127.0.0.1:3849 --remote-control
```

3. 再在同一个项目目录打开一个终端，连接本地 Codex TUI：

```bash
codex --remote ws://127.0.0.1:3849
```

完成后可以在 IM 里给机器人发消息，也可以在本地 Codex TUI 里继续使用同一个 Codex app-server。端口 `3849` 被占用时可以换成其它本机端口，但第 2 步和第 3 步里的地址必须一致。

### 8. 在 IM 里开始使用

在飞书、Telegram 私聊、微信或企业微信里给机器人发消息。

如果当前 IM 会话还没有绑定 Codex thread，机器人会先让你选择新建 thread 或恢复已有 thread。选择后，后续对话就会进入对应的 Codex thread。

微信链路依赖客户端下发的 context token。长任务或手机端长时间不活动时，微信客户端可能让 token 过期，导致本地 backend 暂时无法继续发送消息。遇到这种情况，在微信里发送 `!` 或 `?` 可以刷新 token；这两个激活消息只用于恢复发送链路，不会转发给 Codex。

## 网络与代理

TianCaiSpaceHub 的“网络”菜单提供三种出站模式：跟随系统代理、强制直连、自定义 HTTP/SOCKS5 代理。该设置只影响 TianCaiSpaceHub 访问模型服务、微信、Telegram、飞书 HTTP API 和更新地址，不会修改 macOS `launchctl`、Windows 用户环境变量或其它应用的网络设置。

使用 Clash、V2Ray 等本地代理时，可以选择“自定义 HTTP/SOCKS5 代理”，填写 `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:1080`。daemon 正在运行时设置会立即生效。本地 GUI、Codex App、VS Code 与 TianCaiSpaceHub 之间的回环通信不会使用这个出站代理。

TUN / Network Extension 类型的 VPN 工作在 HTTP 代理层以下。如果它拦截回环流量，仍需要在 VPN 软件中排除 `localhost`、`127.0.0.1` 和 `::1`。

## AI Gateway

AI Gateway 解决的是“Codex 只认原生模型入口，但用户想用更多模型渠道”的问题。你在 GUI 里配置渠道后，Codex App 看到的仍然是普通模型列表；真正的上游请求由 `TianCaiSpaceHub` 负责转发和转换。

当前重点能力：

- OpenAI Responses 渠道：适合原生 Responses 或兼容 Responses 的模型服务。
- DeepSeek Responses 渠道：原生对接 DeepSeek `/v1/responses`，支持官方 hosted web search、function 和 `apply_patch`。
- DeepSeek Chat / Chat Completions 渠道：保留旧接入方式，把 Codex 请求转换成 Chat Completions，再把返回结果转换回 Codex 可消费的格式。
- Anthropic Messages 渠道：用于 Claude / Anthropic 兼容模型，支持文本、图片、工具调用、思考输出和 web search 的协议转换。
- 智谱 Anthropic 渠道：普通智谱 API 与 Coding Plan 统一走 Anthropic Messages；模型列表使用 API Key 从智谱独立目录自动获取，并处理 GLM web search 的返回差异。
- 模型映射：解决上游模型名大小写、别名、第三方转发命名不一致的问题。
- Codex 可见模型：控制 Codex App 模型列表里展示哪些模型。
- 请求日志：记录 Codex 原始请求、发给上游的请求、返回结果、错误、token、缓存、耗时和请求包大小，方便排查首帧慢、超时和协议转换问题。
- 过滤生图工具：默认关闭；打开后 AI Gateway 会从请求中移除 Codex 的 `image_generation` 工具，适合不支持生图工具的渠道。

这些能力都在 GUI 中操作，不需要用户手写配置文件。

## 交流与支持

有问题可以提 GitHub issue，也可以关注公众号后直接发消息给我。

<img src="docs/assets/wechat-public-account.jpg" alt="微信公众号" width="220">

## IM 命令

`v0.4.30-3` 中，飞书、微信和企业微信使用 `/tg` 首次启用或切换到天工、`/gpt` 返回 Codex；启用后的天工重启会自动恢复连接，`/wb` 暂不可用并保持当前模式。天工模式下 `/q` 退出并释放当前会话认领，保留天工模式并回到该平台原有的新建/恢复入口；`/tg new` 直接打开同一会话设置。按当前卡片按钮或文字菜单选择目录、模型并创建真实桌面任务，恢复列表读取全部桌面场景任务，沿用所选任务的会话、目录与记忆身份。`/s` 会明确提示尚未停止任务，需要回天工桌面处理。执行中、待审批、已被其他 IM 发送者认领或状态未知时有保护，详见 [天工外部消息](docs/customizations/gmclaw-im.md)。Telegram 继续使用 Codex 路径。

以下为原 Codex 模式命令，按卡片提示操作：

```text
/q         中断并清除当前绑定
```

审批卡片在选择后会高亮并标记为已处理，避免聊天里堆了很多卡片后分不清哪些已经操作过。

## 恢复 Codex 原有配置

GUI 里点击“恢复 Codex 原有配置”即可恢复写入前的 Codex 连接方式。恢复后，Codex App 不再通过本地 AI Gateway 发模型请求。

这一步不会卸载 Codex，也不会删除 Codex 的会话历史。

## 项目边界

`TianCaiSpaceHub` 只支持干净的 Codex remote-control 路径。

它不会：

- 安装 `codex` 包装命令
- 替换 Codex CLI
- 通过 shim 启动 Codex App
- 安装登录项或开机启动项
- 自动常驻后台
- 替换 Codex App、Codex CLI 或 VS Code 插件的原始可执行文件

本地 backend 只会在用户明确打开 GUI 或主动从开发工具启动时运行。

## 技术说明

主链路：

```text
Codex App / Codex VS Code 插件 / Codex CLI app-server
  |
  | chatgpt_base_url = "http://127.0.0.1:3847/backend-api"
  | 用户打开 remote-control，或启动 codex app-server --remote-control
  v
官方 Codex app-server
  |
  | outbound remote-control websocket
  v
TianCaiSpaceHub 本地 backend
  |
  | 飞书 websocket 事件 / 消息卡片 API
  | Telegram long polling / Bot API
  | 微信 iLink long polling / sendmessage
  | 企业微信 AI Bot WebSocket / aibot_send_msg
  v
IM 通道
```

本项目实现官方 remote-control endpoint：

```text
POST /backend-api/wham/remote/control/server/enroll
GET  /backend-api/wham/remote/control/server
```

Codex remote-control 要求 ChatGPT 兼容的 auth mode。这个项目采用本地 `ChatgptAuthTokens` 形态，用来通过 Codex 客户端的 remote-control 账号检查。API-key-only auth 不能启动 remote-control。

Thread 绑定模型：

- Codex app-server 仍然维护 thread 生命周期和历史
- 一个 IM 会话同一时间只绑定一个 Codex thread
- 如果 IM 会话还没绑定 thread，bridge 会先给出新建或恢复 thread 的入口
- 从 IM 恢复某个 thread 后，会订阅这个 thread 后续的 remote-control 事件
- IM 发起的 turn 会按 turn id 记录来源，避免 userMessage 回显

## 开发

Cargo package 为 `tiancaispacehub`，binary 为 `TianCaiSpaceHub`。当前功能测试由用户负责；以下仅检查编译与生成调试 EXE，不启动应用或执行测试。Windows/macOS 发布安装包统一由 GitHub Actions 生成。

```powershell
cargo fmt --all -- --check
cargo check --locked --target x86_64-pc-windows-msvc --features gui --tests --bin TianCaiSpaceHub
cargo build --locked --target x86_64-pc-windows-msvc --features gui --bin TianCaiSpaceHub
```

daemon 运行时常用状态接口：

```text
GET http://127.0.0.1:3847/api/status
GET http://127.0.0.1:3847/api/remote-control/status
GET http://127.0.0.1:3847/api/remote-control/backend-status
GET http://127.0.0.1:3847/api/events
```

## 安全说明

- daemon 默认只绑定 `127.0.0.1`，不要直接暴露到公网
- 本地保存的 IM token、模型 API Key 和 Codex 认证信息都是 secret，不要提交
- 飞书和 Telegram 附件会分别下载到本地状态目录旁边的 `.im/attachments/feishu/` 与 `.im/attachments/telegram/`
- 真正使用时建议配置 `allowedOpenIds` 和 / 或 `allowedChatIds`
- bridge 可以替 IM 用户向 Codex 提交审批决定，所以飞书 / Telegram / 微信 / 企业微信访问权限应视为等价于本地 Codex 审批权限

## 更多文档

- [架构](docs/architecture.md)
- [WorkBuddy 接入](docs/workbuddy.md)
- [OpenAI Chat Completions 接入](docs/openai-chat-completions.md)
- [微信集成计划](docs/wechat-integration-plan.zh-CN.md)
- [认证说明](docs/auth-notes.zh-CN.md)
- [排障](docs/troubleshooting.md)

## License

Apache-2.0
