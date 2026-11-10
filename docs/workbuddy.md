# WorkBuddy 多模型接入

## V1.0.16 天工内置来源参数

天工内置来源在 WorkBuddy 使用时，窗口、输出、温度、推理、流式与超时取自天工档案；不再套用通用 GPT 推理/图像默认。保存、已有副本同步仍走 WorkBuddy revision/文件锁/备份 writer，后改冲突停止。普通 WorkBuddy 模型参数规则不变，身份和压缩边界见 [天工专题](customizations/tiangong-builtin-models.md)。

## V1.0.12 配置兼容

维护：2026-11-10（UTC）。模型路径优先 Hub 原有 `WORKBUDDY_CONFIG_PATH`，其次客户端原生绝对目录 `WORKBUDDY_CONFIG_DIR`，最后是用户目录的 `.workbuddy/models.json`。通用 Skill/MCP 使用该模型文件所在目录，Dumpling 继续自己的独立目录预检规则；不据此扩大为国际版或全部实例自动适配。

保存兼容数组、旧单模型对象及 `{models,availableModels,...}` 对象容器，保留原容器、其他模型和未知顶层字段。非空 `availableModels` 随本条目添加/移除，受保护恢复只还原本次可见性差额，其他后改保留；本条目可见性已被后改则停止恢复。通用远程 MCP 使用客户端原生 `type:http`。目录发现只读，不在升级时自动改写配置。逐端差异与回退见 [配置核对](customizations/client-configuration-audit.md)，实际离线验证、构建和新包身份见 [V1.0.12](releases/v1.0.12.md)；真实客户端及 macOS 待用户验收。

## V1.0.10 工作区入口

维护：2026-11-09。WorkBuddy 工作区含状态图、模型、Skill、MCP 和统一“接入设置”；后者提供本端配置查看及有备份时的恢复，不需另开接入开关。原多条目保存/删除/受保护恢复/桌面启动服务保留，来源隔离和 v13 的 max 强度不变。跨端复用生成独立副本，不借用其他条目配置。

右上只保留“启动客户端”，进入和保存后自动调用原模块读取。点击“添加模型”在当前原列表末尾新增唯一“新建模型”草稿；右侧仍为完整来源/模型/协议/思考强度表单，保存才创建，放弃只丢内存。直接切已有项再回来保留输入，再次添加不重复创建；不再先用简化表单保存后才调参数。草稿保留创建 revision，普通后台状态不替换；显式重新读取才更新。列表复用原控件，字段真实溢出才滚动，保存/放弃固定可见，选择框按 DIP 高度显示。构建、隐藏完整编辑器及配置载荷回归、包身份见 [V1.0.10交付](releases/v1.0.10.md)，真实启动、原空白/闪退和客户端加载待用户验收。

真实Skill/MCP由共用资源领域写入并读回；使用状态不等于正在请求。/wb外部任务执行仍未接通，本轮没有宣称补齐。新入口、失败恢复与验收见 [V1指南](v1-desktop-guide.zh-CN.md)，实际构建/测试见 [V1交付](releases/v1.0.0.md)。下文历史结果保留其版本归属。

维护更新：2026-11-07，`v0.4.30-13` 修正 GPT 思考强度预设。Hub 原先仅给 Claude 提供五档，导致 WorkBuddy 页的 GPT 最高为 `xhigh`，与天工页面不一致；这不能据此认定是 WorkBuddy 客户端限制。当前 GPT-6、GPT-5.6 家族（含点号小版本和命名空间、经别名解析）增加 `max`，保存、重新加载和请求转换保留该值；旧 GPT 和未知模型沿用原四档。实际模型是否接受该值由上游决定。隔离验证及本地包状态见 [v13 交付](releases/v0.4.30-13.md)，实机待用户验收；以下历史阶段保留原版本归属。

2026-10-27 发布状态：用户初步反馈本地包正常；产品提交 `f322634`、完整 `v0.4.30-8` 标签和 Pre-release 已发布，Windows/macOS Actions 已确认开始后停止等待。准确源码、运行链接和尚待确认的包及验收范围见 [v8 交付](releases/v0.4.30-8.md#2026-10-27-源码与-release-已发布两平台-actions-已开始)。下方未发布或待用户反馈的表述保留开发阶段归属。
维护日期：2026-10-22。当前开发 `v0.4.30-8`；本轮统一 WorkBuddy 与天工 Work 接入页的字段、按钮、说明和只读/编辑底色，未变更模型文件格式或路由。关联 TC-002、TC-004、TC-007、TC-013、TC-014。隔离验证、本地测试包及未发布状态见 [v8 交付](releases/v0.4.30-8.md)，两平台实机待用户验收。历史产物保持原身份。

TianCaiSpaceHub 在 WorkBuddy 页签逐条管理多个模型。每条可以选择自己的来源渠道、模型、协议和思考强度；WorkBuddy 调用本机 Chat Completions 地址，Hub 经该条目的专用渠道转发。同名模型可绑定不同来源，保存一个条目不再以单元素数组覆盖其他模型。

## Dumpling-MCP 独立接入

通过“Dumpling-MCP”页签完成远端认证后，下拉选择 WorkBuddy，预览并确认写入 Hub 受管的本机 HTTP MCP。当前三端使用保存的环境名；旧 hash 名只按归属和显式确认迁移。它与本页 `models.json` 模型条目独立，使用 `mcp.json`，保留其他连接和原审批设置。

MCP 路径优先 `WORKBUDDY_CONFIG_DIR` / `CODEBUDDY_CONFIG_DIR`，否则为用户 `.workbuddy[-实例号]/mcp.json`；`WORKBUDDY_CONFIG_PATH` 仍只管理模型。客户端只保存本机桥与本地凭据，远端密码/密钥由 Hub 系统保护。认证方式、预览、备份、移除/恢复及按需续认证见 [TC-014 完整专题](customizations/nvwa-mcp.md)。Hub 需运行，原生刷新/信任和真实工具调用待用户验收。

## 外部消息执行端边界

`v0.4.30-3` 为 IM 保留 `/wb` 指令。用户已选择先明确提示“WorkBuddy 外部任务执行暂不支持”，并保持当前执行端与会话；它不会发送模型请求、创建 WorkBuddy 任务或悄悄改用 Codex。`/tg`、`/gpt` 与天工会话菜单见 [外部消息执行端](customizations/gmclaw-im.md#指令会话和权限)。本页已有模型接入负责 WorkBuddy → Hub → 模型厂商的调用，不能据此推定 Hub 已能反向操控 WorkBuddy 桌面任务。

2026-10-03 对本机 WorkBuddy **5.6.2** 的安装程序资源进行只读核对，确认内置 agent-cli **2.147.0** 与 ACP 执行能力，但尚未找到可供 Hub 授权连接、创建和选择现有桌面会话的稳定外部接口。核对限于程序资源，不读取用户凭据或私有会话，不启动应用。具体依据：

- `resources/app.asar` 中 `main/server.js` 的 `DaemonServer` 不开放 HTTP listener；桌面会话记录由其数据库服务维护。
- `main/index.js` 的桌面 daemon 使用进程内调用或父子进程 stdio，Renderer 使用 Electron MessagePort；这些内部传输不是可填写地址及令牌的公共会话接口。
- `main/code-cache.js` 的 HTTP sidecar 使用进程内随机密码；本轮不发现或提取该密码。WBIPC 管道目前注册 `wb.request/http.fetch`，没有本地任务创建、选择和提示提交方法。
- 独立运行内置 `cli/bin/codebuddy --acp` 不代表复用桌面会话；桌面另外提供模型选择、提示、MCP/连接器及安全宿主能力，本轮没有将独立引擎代替桌面接入。

官方 [WorkBuddy Open API](https://open.workbuddy.cn/docs/openapi) 提供 OAuth 授权后的本地助理消息收发，但现有公开文档未列出本地多会话新建/选择接口。[CodeBuddy HTTP API](https://cloud.tencent.com.cn/document/product/1831/137064) 描述的是独立 CodeBuddy Code 服务，不能作为普通 WorkBuddy 桌面已开放同等接口的依据。本轮不创建开放平台应用、不发起 OAuth 或真实调用；`/wb` 保留入口没有新增凭据和配置字段。

## 启动 WorkBuddy 桌面

「WorkBuddy 接入」页新增「启动 WorkBuddy」按钮，通过后台线程调用 `POST /api/workbuddy/runtime/start`；未配置模型时也可使用。点击先识别已有桌面进程，已运行时不重复启动；同页操作忙时按钮暂不可点，服务端对全部启动请求串行处理，并在一次启动后提供 30 秒冷却，避免初始化期间重复启动。Windows 检查 `WorkBuddy.exe` 与配套 `resources/app.asar`，通过 App Paths/卸载注册表和常见安装目录定位，支持用户提供的 `D:\Program Files\WorkBuddy` 自定义盘符安装；macOS 检查完整 `WorkBuddy.app` 并用系统 `open -a` 打开。辅助进程不显示控制台，不改系统关联或安装资源。

通常无需手填路径；自动发现失败时明确提示安装未找到。特殊安装可在启动 Hub 前设置 `WORKBUDDY_DESKTOP_PATH` 为完整桌面程序或安装目录（macOS 可用 App 或其所在目录）。该变量只用于启动定位，不改变 `WORKBUDDY_CONFIG_PATH`、模型配置或备份。已有进程优先处理；需要启动新进程时，无效覆盖值拒绝启动，不静默改用其他安装。GUI 请求最多等待 20 秒，界面区分“已发起启动”和“已运行”，不据此声明模型连接或 `/wb` 已接通。按钮不创建桌面会话、不执行任务；Hub 退出不会通过该功能关闭 WorkBuddy。

本轮不新增持久配置字段或迁移；回退程序会移除按钮，已启动的 WorkBuddy 仍由用户管理。关键代码见 [桌面启动与安装发现](../src/web/workbuddy_launch.rs)、[GUI](../src/gui/workbuddy.rs)、[GUI API](../src/gui/api.rs)。Windows 本轮编译与产物身份见 [品牌与交付](customizations/desktop-and-packaging.md#当前交付状态)，重复点击、已有实例和自动路径发现待用户验收；macOS 本版 Actions 原生构建通过，实机行为仍待用户验收，不扩展 Linux。本轮未启动 WorkBuddy、读取用户模型凭据或会话，未执行测试/真实调用。

## 推荐配置

1. 在“模型与渠道”保存并启用渠道，填写厂商地址、凭据和模型列表。
2. 打开“WorkBuddy → 模型”，自动读取已有项；新增点击“添加模型”，在同一窗口选择渠道或其他端来源并保存。这里只管理对应Hub条目，其他自建模型保留。
3. 选择“大模型接入渠道”和“模型”。“接口类型”跟随渠道，特殊兼容服务可调整；“思考强度”已放入同一连接卡片。
4. 点击“保存模型”，在 WorkBuddy 中重新加载配置并选择该模型。地址、本机 Key、上游 URL/Key、内部渠道和配置路径不显示，也无需在此填写。
5. “删除所选模型”只删除当前项及其专用渠道；有备份时“恢复配置”恢复最近保存/删除，保留其他条目和后改保护。启动和自动读取不保存模型。

只读或停用字段灰底，可编辑项在浅色主题白底，深色主题用对应卡片色；与天工 Work、Dumpling-MCP 使用同一规则。日常使用添加、保存和删除；启动在工作区右上，读取自动衔接，恢复有备份时才显示。

思考强度随模型（含别名）和协议更新。Claude、GPT-6 和 GPT-5.6 家族提供 `low/medium/high/xhigh/max`，其他模型保留 `low/medium/high/xhigh`；GLM Anthropic 沿用 `high/max`。初始 `high`，切换保留仍适用的档位，否则回退 `high`。请求未指定强度时使用此值。已保存的 GPT 条目需重新选择并保存一次，才能把新增档位写入 WorkBuddy 的模型文件；升级 Hub 不主动批量改写客户端配置。缓存仍自动管理，移除独立展示不改变缓存策略；OpenAI 默认键为 `workbuddy:<provider>`，Anthropic 省略该键并使用原生标记。

后台继续生成 UUID、`tiancaispacehub-<entryId>` 模型、`workbuddy:<entryId>` 渠道和 `/ai-gateway/workbuddy/<entryId>/v1` 地址，旧 `legacy` 保留。客户端只持本机兼容 Key，真实上游凭据由 Hub 转发。保存保留其他模型、未知字段和未展示的能力选项；通过别名/ID/实际模型调用仍精确路由到该条目，不把 UUID 发往厂商。厂商地址/密钥及来源调整需在“大模型接入”管理后重新保存对应条目。

本轮 UI 精简不迁移配置；原备份、独立渠道与定向撤销规则保留。真实客户端重新加载、同名多模型和两平台使用仍由用户测试，不宣称已验证默认模型机制。

## 协议规则

- AI Gateway 的 `OpenAiResponses` provider 自动映射为 `openai-responses`。
- ChatGPT 账号登录渠道同样使用 `openai-responses`。先在“大模型接入”完成登录并配置模型，WorkBuddy 会复用账号凭证引用和自动刷新能力，无需填写账号令牌；上游固定使用官方账号接口。
- `AnthropicMessages` provider 自动映射为 `anthropic-messages`。
- DeepSeek、Grok、Chat Completions 和其他 provider 类型自动按 `openai-chat` 处理。
- 普通 API 来源的协议仍可在 WorkBuddy 页签手动调整；ChatGPT 账号来源只允许 Responses，保留账号引用及刷新。

## 502 / 503 自动重试

从 `v0.4.30-13` 起，普通 API 地址缺少版本时另支持原地址失败后补 `/v1` 一次，确认有效结果后保存；双失败返回原错误。完整范围、流式时点、配置保护及回滚见 [TC-015](customizations/provider-url-recovery.md)。来源和专用副本各自验证，不批量同步。

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
| `POST /api/workbuddy/runtime/start` | JSON `{ "launch": true }`；仅显式启动桌面，返回 `detail/running/launched`；不接入 IM 任务 |

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

`v0.4.30-2` 已通过 Windows/macOS Actions 原生构建和打包，并发布预发布安装包；Windows 未签名，macOS 为 universal、ad-hoc 签名且未公证。当前用户负责测试，本轮未执行功能测试、启动 WorkBuddy 或发起真实调用。待验收：多模型加载、同名不同源、思考/协议切换、删除/撤销、其他模型与字段保留、冲突与隔离，以及两平台安装升级与实机交互。已有回归属于原版本，前版构建见 [原版本交付](releases/v0.4.30-2.md)，当前 `v0.4.30-3` 的两平台构建、包静态核验与发布结果见 [本版交付](releases/v0.4.30-3.md)。
