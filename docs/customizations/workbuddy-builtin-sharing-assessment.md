# WorkBuddy 登录内置模型跨客户端分享评估

维护日期：2026-11-11。源码审阅基线：`main / 149ee27`（V1.0.26 开发阶段）。关联 TC-002、TC-004、TC-018、TC-021。

状态：**本项为只读评估，未实现分享功能，不单独提升版本或生成测试包**。本轮核对仓库、官方公开文档和本机 WorkBuddy 安装资源；没有读取用户配置、登录凭据或会话，没有启动客户端、模型调用或安装操作。Windows 静态结果如下，macOS 未核对。

## 结论

**技术上存在底层通路，当前应标记“受限可行”，尚不能作为稳定的一键分享交付。** WorkBuddy 登录模型由官方云端服务执行，新版客户端已经有“由 WorkBuddy 保管登录凭据、给本机会话代发请求”的内部机制；因此无需预设必须导出账户 token。但现有本机入口主要服务于宿主技能和电脑操控，尚未证明能稳定地把当前账户允许使用的全部普通模型提供给其他客户端。

最小下一步是验证一个 **WorkBuddy 端内桥接**：先解决宿主授权、真实模型目录和请求透传，再接 Hub 现有协议转换与各端配置 writer。不能仅复制模型名、填写一个官方地址，或将已有自配 API 复用称为“登录内置模型分享”。

| 来源或路线 | 当前判断 | 与本需求的关系 |
| --- | --- | --- |
| 已购官方模型 API / Token Plan 的 API Key | 可沿普通模型渠道接入，受对应套餐限制 | 独立 API 凭据和额度，不能证明 WorkBuddy 登录权益已分享 |
| 用户在 WorkBuddy 自配的外部模型 | Hub 已能管理其自身配置的来源副本；其他自配项需用户提供来源 | 属于外部 API 复用，不是登录内置模型 |
| 官方 Open API 本地助理或云端任务 | 有公开接入路线；需要独立应用授权 | 调用整个 Agent，不是指定模型的 Chat Completions / Responses 服务 |
| WorkBuddy 原生会话代理 + Hub 端内桥接 | 有静态技术依据，受宿主会话和版本约束 | 最接近本需求；须先完成小范围验证 |
| 直接读取/复制登录 token 后发往官方服务 | 本轮未做，也不是建议方案 | 会把刷新、账号切换与秘密存储变成 Hub 的额外责任 |

## 仓库现状与天工差别

[WorkBuddy 配置](../../src/workbuddy_config.rs) 为每项生成 `workbuddy:<entryId>` 专用渠道与本机地址；`workbuddy-local` 是本机兼容值，不能代替官方登录凭据。[WorkBuddy 协议层](../../src/ai_gateway/workbuddy.rs) 和 [请求入口](../../src/ai_gateway/handler.rs) 处理的是 **WorkBuddy → Hub → 已配置来源**，现有专用入口没有实现反向借用 WorkBuddy 登录账户。

[跨端模型接口](../../src/desktop/models.rs) 可以复用 Hub 已知的来源；[路由](../../src/ai_gateway/router.rs) 继续隔离客户端保留渠道。仅将一个 WorkBuddy 条目复制给其他端，不会自动新增账户认证能力。

[天工内置模型](tiangong-builtin-models.md) 有明确的两个来源 ID、来源参数档案、固定官方模型地址与专用授权适配。[配置结构](../../src/ai_gateway/config.rs) 中也已有 `tiangongBuiltin` 绑定。WorkBuddy 的模型目录和账户权限则受产品配置、登录身份及套餐影响，不能沿用天工认证或假设原生免个人登录。

## 本机安装资源证据

本次只读取 `D:\Program Files\WorkBuddy` 内的程序资源，未进入 `.workbuddy`、`.codebuddy`、AppData 用户数据或凭据目录。没有执行安装包里的 JavaScript。

| 项目 | 核对结果 |
| --- | --- |
| Windows 程序版本 | `WorkBuddy.exe` 文件版本 `5.9.3`，产品版本 `5.9.3.0` |
| 安装包元数据 | `resources/app.asar` 的 `package.json` 为 `5.9.3`；主入口 `main/index.js` |
| 配套引擎 | 安装资源 `cli/package.json`：`@genie/agent-cli / 2.165.0` |
| 静态产品模型目录 | `cli/product.json` 中有 58 个模型定义；这是安装模板，不能据此声称当前用户拥有 58 个可用模型 |
| 历史边界 | [WorkBuddy 专题](../workbuddy.md) 中 5.6.2 / 2.147.0 属旧审阅结果；本次对 5.9.3 重新核对，没有据旧版直接推断新版接口 |

### 云端模型调用确实存在

`cli/dist/codebuddy-lite-wb.mjs` 的 `axiosToFetchAdapter` 使用当前产品模型配置，处理 `messages`、工具字段及流式响应，并为内部模型附加当前会话身份和请求元数据；自定义模型另有 URL、协议和内部请求头处理。静态 `cli/product.json` 的默认产品地址是 WorkBuddy 官网，官方地址列表包含 WorkBuddy 与腾讯模型网关。

`main/server.js` 的 `VlmUpstream` 明确构造官方网关 `/v2/chat/completions`，从 WorkBuddy 当前认证管理器读取访问 token，在上游 401 时尝试由宿主刷新。默认网关回退地址为 `https://copilot.tencent.com/v2/chat/completions`。这证明安装包含实际调用路径，**不证明本次账户、任一模型或任一请求格式已经被服务端接受**。

### 已有会话代理能保留账户凭据在 WorkBuddy 内

`main/server.js` 中可检索 `VlmProxy`、`VlmSessionLedger`、`SkillVlmService`、`createSkillVlmService`：

- 首次需要时监听 `127.0.0.1` 随机端口；宿主为会话分配临时本机 token 和签名 context，上游账户 token 不交给调用方。
- 技能路径是 `/skills/vlm/chat`，通过 `WORKBUDDY_VLM_CHAT_URL`、`WORKBUDDY_VLM_AUTH_TOKEN`、`WORKBUDDY_VLM_CONTEXT`、`WORKBUDDY_VLM_MODEL` 注入宿主子进程环境；默认技能模型为 `kinfra-office-agent`。
- 代理把请求体和上游响应字节透传，具备流式传输通路；客户端断开、会话释放或停止代理会中断请求。当前默认上游超时 570 秒、请求体上限 32 MiB。
- 当前身份改变时撤销已有会话授权；`createSkillVlmService` 从认证管理器取当前身份，并由 WorkBuddy 负责刷新。宿主会话结束及产品退出后，不应继续使用旧引用。
- `main/daemon-bootstrap.js` 的电脑操控代理使用 `/computer-use/vlm/chat`，默认模型为 `kinfra-cua`，并有独立用户授权门槛。

在审阅的代理透传函数中未见把所有请求强制改为上述默认模型；这只说明本地转发层没有在该处改写模型，**不能推出上游允许任意模型，也不能把技能或电脑操控授权当成跨端调用授权**。当前没有核实一个供 Hub 长期订阅、枚举账户模型并续期的通用宿主接口，也没有为 Hub 取得这些会话引用。

### 另外两条内部路线不足以直接交付

`main/server.js` 的 `createWbRequestPipe` 注册 `wb.request / http.fetch`，由宿主补充官方身份，但方法类型是一次性 `call`：完整收集响应后返回 base64，默认超时 50 秒，响应限制 655,360 字节。它不是 SSE 通道，不适合把长对话、工具流和大上下文请求直接搬上去；能使用同一登录身份也不等于已有稳定外部授权入口。

同文件 `DaemonServer.start` 只设置进程内状态，`getHttpServer` 没有实现监听；桌面使用内部 RPC、stdio 等组合。安装包存在 `--acp` 或 `--serve` 相关代码，不代表独立启动引擎会继承桌面登录、模型权限或会话。`main/daemon-app-server-main.js` 的 `127.0.0.1:52741/v1/chat/completions` 属本地模型运行配置，不能拿它证明云端登录模型已开放。

## 官方公开接口边界

WorkBuddy 官方明确区分内置模型与用户提供 URL/API Key 的第三方模型，模型窗口、思考档位和消耗也按实际模型或任务设置变化。因此产品预设、自配 API、账户内置权益需要分别处理。[官方 WorkBuddy 模型说明](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model)

公开 Open API 支持 OAuth 应用授权后的本地助理消息、云端任务和 ACP。当前公开任务创建字段是任务指令及名称，未列出任意登录模型的独立推理接口；本地助理回复通过消息查询取得。云端 ACP 有流式更新、工具授权和会话，但这些是 Agent 交互，不能把工具执行记录直接当作目标客户端的 `tool_calls`。应用凭据需要服务端保护，不能内置公共 `client_secret`。[官方 Open API](https://open.workbuddy.cn/docs/openapi)、[第三方应用接入](https://open.workbuddy.cn/docs/third-party-app)

另有 `codebuddy --serve` 的 REST/ACP 文档，其产品位置为 CodeBuddy Code。它可以作为独立 Agent 接入研究来源，不能替代 WorkBuddy 桌面原有登录的兼容证据，也没有把其 `/runs` 当通用模型接口。[官方 CodeBuddy Code HTTP API](https://cloud.tencent.com.cn/document/product/1831/137064)

账户内置模型的服务端权限与实际计费仍未验证。即使透传成功，也应按 WorkBuddy 原账户/组织的实际权益计费；不能承诺免费、无限额或自动转为管理员承担。官方 API / Token Plan 则使用另行取得的 API Key 和对应套餐，不能把两类额度合并解释。[官方 Token Plan 配置说明](https://cloud.tencent.com/document/product/1831/137451)

## 最小方案与上线前验证

建议先限定为同一用户、同一电脑、一个普通文本模型的宿主桥接验证，不同时开发全部客户端或完整 Agent 代理：

1. 由 WorkBuddy 当前已登录进程持有账户认证，先确认可用的授权扩展/宿主接入口。现成 VLM 会话机制可作为参考；若必须做增强启动适配，应明确绑定安装版本，不把内部符号当长期公开 API。
2. 只向 Hub 交付本机受限调用引用与真实可用模型/能力元数据，不读取或导出账户 access/refresh token，不把临时 VLM token 写入普通渠道配置或目标客户端。模型目录必须来自本轮账户，不能以静态 58 项填满列表。
3. 先验证普通模型确能经授权桥接完成文本 JSON/SSE，随后验证一轮工具调用、目标客户端执行工具、工具结果续轮。工具应由请求该模型的目标客户端执行，避免 WorkBuddy 再执行一遍。
4. 前述成立后复用 Hub 现有 Chat Completions/Responses 转换及单端 writer，目标客户端只连接 Hub。首批不扩展远程公开服务，也不默认启动 WorkBuddy 或创建占位任务维持授权。

| 必要验证 | 判定要点 |
| --- | --- |
| 宿主接入与模型权限 | 取得明确授权的宿主引用；普通模型确实可用；静态配置、账号目录、服务端接受三者分开记录 |
| 协议与流式 | JSON/SSE 的角色、结束、推理、用量和错误保真；断开及时停止，不重放已开始生成 |
| 工具续轮 | 名称、参数、调用 ID 和工具结果关联准确；WorkBuddy 不替目标客户端执行工具 |
| 账号与生命周期 | 退出登录、切换用户或组织、刷新、会话释放、WorkBuddy/Hub 退出后旧引用失效；重新连接显式就绪 |
| 额度与错误 | 401/403、无模型权限、429、额度耗尽按真实结果呈现；不因同名模型自动换来源，不伪报免费 |
| 目标端能力 | 仅声明已确认的窗口/输出/推理/图像；Codex 压缩协议另验，不能因可发文本就声明完整兼容 |
| 版本与平台 | 先限定 Windows 5.9.3；升级后重核宿主契约；macOS 在独立核对前标记未验证 |

以上是下一阶段必要验证清单，本轮均没有真实执行。若宿主未提供可维护的普通模型入口，应停留在受限评估，不通过复制用户凭据或循环维持任务来补齐。

## 可复现静态核对与变更状态

可在上述安装版本中只读解析 ASAR 目录，分别读取 `package.json`、`cli/package.json`、`cli/product.json` 和表列 JavaScript，核对版本及符号。解析过程不需 `require`、`import` 或执行安装脚本；读取 unpacked 条目时仅按 ASAR 内已登记相对路径进入对应安装资源。

| 静态条目 | 可检索定位 |
| --- | --- |
| `main/server.js` | `DaemonServer`、`getHttpServer`、`VlmUpstream`、`VlmProxy`、`VlmSessionLedger`、`SkillVlmService`、`createWbRequestPipe` |
| `main/daemon-bootstrap.js` | `ComputerUseVlmProxy`、`ComputerUseSessionEnv`、`kinfra-cua` |
| `main/daemon-app-server-main.js` | `resolveConversationAgentExtraEnv`、`skillVlm.resolveEnv`、`52741` |
| `cli/dist/codebuddy-lite-wb.mjs` | `axiosToFetchAdapter`、`shouldApplyInternalModelRequestHeaders`、`normalizeChatCompletionsUrl` |
| `cli/product.json` | `models`、`authentication.type`、`officialEndpoints`；仅核对结构和公开元数据 |

本轮已完成上述静态定位、官方资料核对和文档差异检查。未做 Cargo/Node 产品运行、账号接入、真实推理、计费、安装或 macOS 检查；没有创建缓存副本或反复计算安装树哈希。

本项评估只新增本文件，未因此修改产品配置格式、代码、客户端文件或现有包；因此无需配置迁移或产品回滚。同批其他功能由管理会话按 V1.0.26 集成并统一交付，状态见 [V26 交付](../releases/v1.0.26.md)。索引、总表及变更记录由管理会话统一登记，建议状态为“WorkBuddy 登录内置模型分享：受限可行，仅评估”。
