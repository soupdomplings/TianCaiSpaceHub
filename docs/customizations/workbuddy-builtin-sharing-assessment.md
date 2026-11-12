# WorkBuddy 登录内置模型跨客户端分享

维护日期：2026-11-12。本轮测试交付：**V1.0.30**，源码基线为 `main / b2a2ef9` 加未提交修改。关联 TC-002、TC-004、TC-018、TC-021。

状态：**用户已反馈 V1.0.29 WorkBuddy 独立分享可以使用**，本轮补齐接入设置常驻入口、账号／授权及真实积分查询，V30 的 Windows GUI/release 编译和 40 项不同受影响 Rust 检查通过，新增积分与界面待用户实机验收。V29 的 53 项 Rust 和 17 项 Node 保持原版本归属，不重复计入本轮执行；用户反馈不扩大为真实续期、所有账户、积分或 macOS 已验收。首次显式启用仍需 WorkBuddy 完成登录及目录同步；显示“独立分享已就绪”后可以真正退出，由 Hub 继续转发。

V1.0.28 用户已反馈能获取 WorkBuddy 内置模型目录，确认该范围的宿主连接与目录读取；该反馈不覆盖真实推理、权限和费用。V28 的 45 项 Rust、6 项 Node、Windows GUI/release 及唯一 MSI `005946` 静态核验保持原归属，见 [V28 记录](../releases/v1.0.28.md)。V26 仅评估、V27 首次实现但等待宿主，历史结果不替代 V29 验收。本批按用户要求暂不推送。

## 当前行为与入口

WorkBuddy 首次揭示和启停／分享仍限定为**软萌猫娘**，以本机已保存性格为准。用户明确要求打开时，`hub_open_page({"page":"workbuddy-builtin-share"})` 持久揭示入口，继续沿实际桌面 `opened` 回执打开原分享面板；揭示本身不等于已显示、已授权或启用。之后可从 **WorkBuddy → 接入设置** 直接查看状态和管理分享，不必每次发消息；重启保留入口，切换性格不隐藏已揭示的设置，启用／停用／保存分享继续复核原资格。

用户取消接入设置中的“内置模型分享”勾选时，**只隐藏入口和相关按钮**，保留当前分享、本机授权和模型配置；下次在对话明确要求打开才再次揭示。显隐偏好独立于分享运行，保存为 Hub 自有 `workbuddy-direct/preferences.json` 的 `visible`（缺省 false），不改用户原配置或 WorkBuddy 登录。显隐接口只支持隐藏，不能用 `visible=true` 绕过明确请求。已授权的账号／积分查询是只读操作，可在已揭示设置中查看，不扩展为新授权。

打开只读缓存，不因进入设置而启动 WorkBuddy 或装载受保护凭据。用户点击“启用分享”后恢复已有授权，或准备 WorkBuddy 获取新授权。不加入通用 Skill/MCP 管理，不将 IM `/wb` 当作指定模型推理接口。

首次启用时，Hub 为本次用户请求打开 WorkBuddy，从运行中的当前会话导入维持模型转发和续期所需的最小授权，保存在本机系统保护存储中；不读取或复制 WorkBuddy 用户目录、登录数据库和历史会话。若已有普通 WorkBuddy 进程，提示用户正常退出再启用，不自动关闭其任务。已有可恢复授权和模型目录时，显式启用直接恢复，不需要再次启动 WorkBuddy。

读取成功保持安静，批量保存集中反馈一次。登录或目录未确认、未取得独立授权时，给出对应提示；只有授权与非空目录均就绪且没有已知授权错误，才显示独立分享就绪。该状态不代表每项模型已真实调用。宿主接线握手最多等待 30 秒，失败显示实际阶段，不无限显示“正在等待宿主连接”。

| 操作 | 实际行为 |
| --- | --- |
| 打开、重新读取 | 返回本次 Hub 已启用的运行状态和目录；不启动 WorkBuddy，不因读取而载入受保护授权 |
| 取消接入设置勾选 | 持久隐藏分享入口；运行、授权和模型保留，直至下次对话明确打开才恢复入口 |
| 账号／积分 | 区分授权账号、可续期／已失效等本机状态；显式启用后查询实际积分，可见接入设置按需更新并提供手动刷新 |
| 首次启用／重新授权 | 没有可恢复授权时，检查安装和启动接线，打开 WorkBuddy 获取当前账户授权；未登录时等待用户正常登录 |
| 选择并分享 | 从当前账户目录选择模型及多个目标，生成 Hub 来源并复用既有客户端 writer；目标只持有 Hub 连接及自身 local token |
| 重复分享同模型 | 复用同账户来源与目标副本，保留客户端独立参数；Codex 已移出可见目录的副本重新加入原条目 |
| 真正退出 WorkBuddy | 独立分享就绪后仍可使用；模型调用与授权续期由 Hub 处理，目录改用最近同步的缓存 |
| 真正退出 Hub | 结束本次内存运行和宿主回调，不强杀 WorkBuddy；保留受保护授权及目录，重启 Hub 后需再次显式启用才能恢复 |
| 停用 | 撤销本次回调、停止在途请求并删除 Hub 保存的本项授权及其目录缓存；已保存的模型配置保留，后续调用提示未启用。再次启用需重新授权 |
| 授权失效 | 显示“重新授权”入口；该动作撤销旧授权，再走一次显式获取流程，不静默借用旧 loopback 引用 |
| 宿主仍连接时切号、切组织、退出登录 | 通过独立授权事件先撤销旧授权及在途请求，再接受新账户；旧副本不自动改用新账户 |

费用、限流与权益沿 **WorkBuddy 原账户／组织**，不属于天才搭档管理员承担的客服额度。WorkBuddy 已退出时，Hub 无法即时观察其他设备退出、服务端撤销或本机其他普通实例的账户变化；服务端拒绝生成或续期时才发现并提示处理，不能承诺远端撤销实时同步。

## 模型目录与积分倍率

旧桥接直接投影 `getConfirmedRemoteModelSnapshot().configuration.models`，只排除 `disabled`。这是产品完整目录，还包含补全、图像、视频及内部任务等用途；任务模型选择器实际沿 `sessions.models.list` 的主 Agent 规则构建可选项，因此旧列表比 WorkBuddy 选择器多。

[目录投影](../../src/desktop/workbuddy_builtin/model-catalog.cjs) 仅在宿主 fresh 配置下，选择带 `cli` 标签的默认 Agent；没有 default 时取首个非 plugin Agent。按该 Agent 的 `models`（存在具体 `declaredModel` 时优先）逐项先 ID、再名称、再 aliases 解析，并追加正常聊天模型。按真实 ID 去重，排除禁用项及官方标记为自配 API 的 `configurable/custom/custom:/custom-local:` 项；原始模型 ID 继续用于分享和推理。相同名称但不同 ID 可能有不同权限或积分倍率，不能按名称合并，界面增加 ` · ID` 区分。

静态核对链：confirmed snapshot 保留完整 Agent；`workbuddy-product-config.js` 的按名合并和自动档位转换保留 `tags/models`，JSON 归一化不删除它们；CLI bundle 的 `AgentManager.computeInitializeMetadataSnapshot/computeModelsFromRaw` 定义主 Agent 选择和解析顺序。`declaredModel` 是可选运行时字段，缺失时使用 `models`。不把安装模板的完整目录或固定数量当作当前用户目录，没有可确认的 Agent 目录时返回待加载。

宿主连接时，将当前账户确认的目录同步到本项受保护记录；同账户令牌轮换恰逢目录刷新时保留其上一次已确认目录。完全退出 WorkBuddy 后和 Hub 重启显式恢复后，使用该账户最近缓存的目录，标记 `catalogCached=true`，不伪装成实时 fresh；“刷新状态”不会另行抓取远端目录。需要重新同步时可先停用，再启用并在 WorkBuddy 完成目录加载。旧保存但不再列入当前目录的模型保留，调用时提示重新选择，不自动删除用户配置。

公开计费字段来自当前目录的 `credits`，投影为 `billing:{unit:"credits",display?,multiplier?}`：`display` 保留非空原说明，只有完整 `x0.51` 或 `x0.51 credits` 一类明确格式才解析为倍率。列表显示“模型名 · 积分 ×0.51”（示例，不是固定价格），详情说明按 WorkBuddy 账户积分扣费；其他目录说明在列表简写、详情保留全文。缺失倍率明确“未提供”，不猜为 1 倍或免费；0 倍保留为目录倍率，不承诺实际免费。

V29 只带入目录积分说明与倍率；V30 新增实际余额查询，不从模型倍率估算积分。仍不计算本次具体扣分、不将积分换算成人民币／美元或每 Token 单价，也不套用同名 GPT 的公开价格。动态促销不在本轮解析范围，实际扣费、模型权限及额度以上游为准。上游返回的普通 Token 用量继续记录；Hub 请求日志的金额保持未知，不能由目录倍率推出金额。

### 账号、授权与真实积分（V1.0.30）

接入设置显示当前已启用授权的账号、本机授权状态、实际剩余积分及最近成功更新时间。WorkBuddy 宿主退出不意味着登录授权失效，界面不以“宿主未连接”误报退出登录；仅本机恢复而未向上游查询的授权不宣称已实时验证。`authorization.status` 使用 `available/refreshable/expired/missing/rejected`；access 到期且 refresh 仍可用时为可续期，需要重新授权时给出入口。

查询复用已导入授权和串行续期，只读原生 `DesktopAccountRepo.billing()` 的接口：企业 `POST /v2/billing/meter/get-enterprise-user-usage {}`，读取 `limit_num/used_num`；个人 `POST /billing/meter/get-user-resource-summary {}`，读取 `Packages[]` 的周期总量、剩余量和已用量。按真实账号／组织选择，不循环猜地址，不混用网页的 camelCase 口径。显示单位沿原生数值，资源中的 `100` 是低余额告警阈值，不是换算系数。

企业 `limit_num=-1` 显示不限额度，不伪造剩余 0；有明确总量与已用量时按其精确差额显示剩余。个人多桶保留各自周期额度，仅完整真实字段可汇总，缺失不补 0。缓存只在内存，绑定当前账号，换号／撤销时清除；刷新失败保留同账号旧值和成功时间并注明未更新。

首次启用授权就绪后尝试查询一次；进入可见接入设置且已有授权时按需更新，持续可见最多每 60 秒更新，并提供手动刷新。普通 `read` 和隐藏页面仅读取已有缓存，不自动装载凭据或抓取积分；更新失败不重试生成，也不撤销已经启用的分享。调用积分查询不产生模型请求。

## 独立授权、保存与续期

关键代码：[分享域](../../src/desktop/workbuddy_builtin/mod.rs)、[独立运行与续期](../../src/desktop/workbuddy_builtin/direct.rs)、[宿主授权投影](../../src/desktop/workbuddy_builtin/host-bridge.cjs)、[系统保护存储](../../src/nvwa/secrets.rs)。

1. 只有本次显式启动的适配器开启授权导入。普通启动和旧宿主分享不导出账户授权；导入只包含当前账户／组织、access/refresh token 及有效期、认证／模型服务地址、必要产品与版本上下文，不复制完整 session 或用户数据。
2. 授权经本次随机 loopback 回调和租约交给 Hub，不经模型对话工具返回。Windows 将授权与目录元数据保存在 Hub 桌面状态目录下 `workbuddy-direct/protected/` 的专用记录，采用绑定当前系统用户的 DPAPI，不明文写入通用配置。macOS 虽有共用系统凭据存储实现，本项启动和整体链路仍未适配／验收。
3. 来源与客户端副本只保留非秘密账户身份散列、固定模型 ID 及能力，包括积分说明；access/refresh token 不进入来源、客户端、对外状态、诊断或请求日志。旧 loopback 的短期 URL/token/context 仍只存在内存，它们不承担独立续期。
4. Hub 直接向导入的官方模型服务发送 JSON/SSE 请求，替换客户端和普通渠道认证，带入原账户／组织及产品上下文。当前独立运行只接受官方 `https://www.workbuddy.cn` 服务地址，其他部署／认证类型可能提示不兼容。
5. 已知 access token 到期时，先续期再生成；收到首次 401 时，合并并发续期，其他同版本请求复用续期结果，核对续期后的账户／组织和授权范围，再允许认证失败请求重发一次。不自动重试网络失败、429 或已开始生成的内容。
6. 缺少或过期的 refresh token、续期被拒绝、续期后仍 401，或账户／组织／授权域变化时，提示重新授权；失效运行保持错误，不退回过时宿主引用。正常退出 Hub 保留授权，停用或重新授权才主动清除本项记录。
7. 宿主仍连接时，账户／组织／认证端点变化先发送撤销，再导入新授权；同授权范围令牌轮换仅替换一次。授权事件按序处理，晚到的旧目录或回调不能恢复已撤销授权，停用后也不能通过旧回调补回。
8. 切号、停用及重新授权会中止受本项生命周期管理的在途请求；成功响应体也保持停止／取消和截止时间检查。宿主已退出后无法即时订阅远端撤销，实际生成和续期结果仍是权限判定依据。

独立 HTTP 客户端目前**尚未接入 Hub 自定义出站代理设置**；不能据此宣称需该代理的网络环境已支持。客户端不跟随重定向，错误只返回 HTTP 状态和脱敏说明，不保留可能回显账户凭据的上游原始错误体。

## 首次宿主初始化与兼容路径

首次获取授权仍沿 [只读安装准备](../../src/desktop/workbuddy_builtin/prepare.rs) 与 [进程内 bootstrap](../../src/desktop/workbuddy_builtin/host-bridge.cjs)。本轮没有改变 WorkBuddy 窗口／托盘／退出行为，也不分离或保留原 daemon 冒充独立运行。

1. 首次获取或重新获取授权时，只读安装 `resources/app.asar` 的 `package.json`、`main/main-bootstrap.js` 和 `main/server.js`，核对 Windows 5.9.3、`main/index.js` 主入口、两个唯一锚点及原生 `processExecArgv` 能力。执行程序要求 Electron FuseV1 的 runAsNode 与 nodeCliInspect 允许。没有修改安装 ASAR、程序、签名、Fuse 或用户登录文件。
2. Hub 在自身桌面状态目录 `workbuddy-host/host-bridge.cjs` 保存小型自有适配资源。新建 WorkBuddy 使用临时随机 `--inspect-brk=127.0.0.1:<port>`；唯一发现来源是**本次持有的 Child.stderr** 发布的原生随机 URI，严格匹配本机地址、端口及 UUID。不扫描其他调试目标，其他 stderr 仅排空丢弃，不保存为协议日志。
3. 初始 waiting-for-debugger 阶段 Node 的 `process.pid/env` 尚未初始化；先启用 Runtime/Debugger，设置精确主入口断点，发送 `Runtime.runIfWaitingForDebugger`，保留并消费 `scriptParsed/paused`，经过内部启动暂停直到原 `main/index.js` 第一行执行前。此时再核对 PID，安装内存接线。新通知处理限定在 WorkBuddy 启动辅助，不改变其他已验收客户端的共享 renderer CDP 过滤。
4. 主进程只拦截精确的 `main/main-bootstrap.js`，在唯一 `new DaemonAppServerProcessManager({env:u,requestHandler:` 处补原生 `processExecArgv:["--require",bootstrap]` 和本项租约环境。保持原 stdio-fork 管理器、身份初始化、原环境/参数、stdio 与重建流程，不改全局 spawn 或切换到 in-main。原生 RUN_AS_NODE 子进程以真实 CLI 参数载入适配器，再精确拦截 `main/server.js`。
5. 主进程加载完成或适配器加载失败，都安排恢复原入口、移除本次调试参数并关闭临时 inspector。仅**收到本次原生 URI、确认该新进程仍处于调试等待且从未尝试放行**时，通道连接/初始化失败才可通过所持 Child 句柄结束这次暂停启动，并恢复普通启动。每个 run/resume 在发送前记录“已尝试放行”，响应丢失也不再结束或替换进程。无 URI 的超时/EOF 不自动结束进程：真实 Node 端口绑定失败仍可继续原入口，不能假定已暂停。既有 WorkBuddy 进程始终不处理。
6. 以原生 `SkillVlmService` 构造器及其 `deps` 创建**本项独占实例**，用于初始化、原生临时 ledger 和兼容转发；停止时不关闭 WorkBuddy 原生技能实例。V29 显式导入的最小账户授权交由独立运行端保护保存，provider 优先独立运行；尚无独立授权的兼容路径才使用原生 `/skills/vlm/chat` 本机引用，未取得独立授权时不显示可以退出宿主。
7. 宿主每秒向随机 loopback 回调更新租约和目录。最后有效心跳超过 4 秒后只使旧本机引用失效，不撤销已取得的独立授权；宿主回调超时为 2 秒，连接失败／租约关闭时释放其独占实例。启动阶段按进度排序，授权与目录事件另行处理；30 秒握手从启动接线完成后计时。
8. 子进程预加载立即清理本项 `TIANCAI_WORKBUDDY_*` 环境及自身 `process.execArgv` 中的 `--require`，避免工具再继承租约或预加载。原生 NODE_OPTIONS 不改写。适配文件本身不含秘密，保留供原管理器重建子进程时使用，属于适配资源而非会话缓存。

真实 CLI 参数按 argv 项传递，不再依赖 NODE_OPTIONS 的引号／空白解析；含连续两个空格的路径已纳入真实 Node 双进程夹具。V29 首次授权在正式包中的 ASAR、Electron 初始化及回调仍待 Windows 用户验收；V28 用户已读取目录的反馈仅归属原宿主分享版本。

### V27 等待宿主的根因与修正边界

WorkBuddy 5.9.3 使用 Electron 37.10.3。V27 只核对 nodeOptions Fuse 和 JS 子进程继承，漏掉正式包原生层的额外限制：packaged Electron 过滤 NODE_OPTIONS 的 `--require`，并把过滤结果写回环境，因此主进程没有载入 bootstrap，RUN_AS_NODE 后台也继承不到。Fuse 允许不等于正式包支持该选项，单纯延长等待或重试无法修复。[Electron 环境变量](https://www.electronjs.org/docs/latest/api/environment-variables/)、[37.10.3 原生过滤](https://github.com/electron/electron/blob/v37.10.3/shell/common/node_bindings.cc#L423-L450)

V28 改用官方主进程调试启动能力进行一次临时接线，V29 首次授权沿用该路径，子进程仍以显式 argv 加载。Node 的两个暂停阶段分别处理：初始 runIfWaiting 释放运行时初始化，入口暂停使用 Debugger.resume 恢复；不能在初始阶段读取尚未初始化的 PID。[Electron 主进程调试](https://www.electronjs.org/docs/latest/tutorial/debugging-main-process)

## 目录、来源与配置兼容

新目录来自宿主 `coordinator.getConfirmedRemoteModelSnapshot()`，要求当前身份一致、远端模型已确认、fresh 且无刷新在途；独立运行恢复其最近已确认的缓存，不用安装模板填充账户目录。

公开模型投影包括 `id`、`name`、`billing` 及确实存在的 `contextWindow`、`maxOutputTokens`、`supportsToolCall`、`supportsImages`、`supportsReasoning`；能力在 Hub 中使用对应 `supportsTools/supportsThinking` 名称。`maxInputTokens` 不当作输入加输出总窗，未知思考档位和默认值不猜测。完整产品配置中的 `url`、`apiKey` 不经模型目录返回 Hub，独立授权只走前述专用导入路径。

来源可选 `workbuddyBuiltin` 只含用户／组织非秘密身份散列、固定 `modelId` 和投影能力，`billing` 随能力保存。旧配置无此字段时行为不变，旧能力缺失倍率时按未知显示；天工授权与模型来源仍独立。来源保留本机占位地址，执行时优先使用账户绑定的独立授权，否则走有效宿主引用；目标客户端只接 Hub 及自身 local token。

来源／副本指纹不含账户令牌或租约 token，轮换不会制造模型配置变化。普通渠道编辑转到分享面板；通用配置保存不可新增／伪造 binding 或误删已有 binding，删除整条及客户端独立参数修改走既有入口。每次真实请求检查绑定账户、固定模型 ID、当前可用目录和授权状态。

新账户显式分享时，只停用其他账户的普通 WorkBuddy 来源，避免同模型 ID 的普通路由命中旧账户；旧客户端副本及唯一 Codex alias 保留，使用时明确提示账户已切换。心跳不写配置。

## 协议与接口

原生 `VlmUpstream` 向官方网关 `/v2/chat/completions` POST body，不覆盖显式 `model`。V29 [独立运行端](../../src/desktop/workbuddy_builtin/direct.rs) 复现所需账户／组织和产品上下文，沿该 Chat 协议直接访问上游；仍需用户实测当前目录模型的实际权限与服务端接受情况。

[provider](../../src/ai_gateway/providers/workbuddy_builtin.rs) 优先选用独立授权，不再只有 loopback 一条路径。独立请求替换全部客户端／普通渠道认证；兼容路径仍只向 loopback 发送临时 ledger 授权、context 和 JSON，并禁用该本机连接的外网代理。两条路径均不跟随重定向、不自动重试一般生成错误；独立路径仅有前述 401 认证续期后的单次重发。文本 JSON/SSE、工具声明、调用 ID 和工具结果续轮复用既有 Chat/Responses 转换，工具由目标客户端执行；取消时断开对应上游响应。非成功 HTTP 错误保留状态与脱敏提示，不留可能回显认证的原始错误体。

**本轮不提供 WorkBuddy Responses `/compact`。** Codex 长会话的远端压缩收到明确暂不支持提示，原历史不替换；可新建会话或用客户端本地摘要。文本/工具通路不等同完整 Responses、加密压缩、所有多模态或专用思考档位验收。

| 本机 POST 路径 | 请求 | 响应 |
| --- | --- | --- |
| `/api/desktop/workbuddy-builtin/read` | `{}` | 当前 catalog，不启动 |
| `/api/desktop/workbuddy-builtin/enable` | `{revision}` | 优先恢复受保护授权；无可恢复授权时准备宿主，已有普通进程冲突则 `needsRestart=true`；授权失效时执行重新授权 |
| `/api/desktop/workbuddy-builtin/disable` | `{revision}` | 撤销运行和 Hub 本项保存授权后的 catalog，保留客户端模型 |
| `/api/desktop/workbuddy-builtin/share` | `{revision,modelId,targets}` | 新 catalog 及逐目标 `outcomes` |
| `/api/desktop/workbuddy-builtin/account-refresh` | `{}` | 仅查询当前已启用授权的真实积分，返回新 catalog；无授权不启动客户端 |
| `/api/desktop/workbuddy-builtin/visibility` | `{visible:false}` | 只隐藏入口，保留分享／授权／模型；true 提示先在对话明确打开 |

catalog 保留 `revision/enabled/ready/loggedIn/accountName?/needsRestart/detail/models/targets/builtinShareAllowed` 和 `independent/hostConnected/catalogCached/reauthorizationRequired`，V30 新增 `featureVisible/account`，不返回账户令牌。`account` 中授权只含状态和到期时间，积分为 `status:unknown|ready|error`、`unit:credits`、实际 `remaining/total/used/unlimited`、`updatedAt`（Unix 毫秒）及必要分桶和脱敏说明。`builtinShareAllowed` 来自 Hub 当前性格，不由请求提交；启停／保存分享仍只允许软萌猫娘，已授权查询与隐藏不受该资格限制。独立 `ready=true` 表示有本机授权、非空账户目录且没有已知授权错误；兼容路径还要求有效本机引用，不能据此宣称每个模型已通过调用。已失效时可保留目录用于说明，但 `ready=false` 并显示重新授权。接口沿 Hub 现有可信本机请求规则，share 复用 [既有分享 helper](../../src/web/desktop_models.rs) 与各端 writer。

## 验证与历史记录

### V1.0.30 当前验证边界

- Windows GUI 测试目标与 release 编译通过，保留 78 条编译告警；不以编译代替真实积分或客户端验收。
- 40 项不同受影响 Rust 检查通过：分享域 28（含账户状态／积分 8 项）、本机接口 2、原分享面板 4、常驻设置 3、助手揭示／实际导航／取消 3。覆盖持久显隐、隐藏不撤销、凭据不因读取加载、真实个人／企业数值、未知／不限量／失败旧余额、账户切换和迟到结果、隐藏原生浅／深色组件及导航回执。
- V29 未变的目录与桥接 17 项 Node、provider、共用 writer 和其余助手检查复用原结果，没有为文档或出包重跑。唯一测试包身份及版本／升级／载荷核验见 [V30 交付](../releases/v1.0.30.md)。
- Windows 真实账号和积分包、续期后余额、停用与隐藏的实际界面效果，以及 macOS 均待用户验收。开发方没有启动实际产品、WorkBuddy、安装器或真实模型。

### V1.0.29 历史验证

| 检查 | 当前状态 |
| --- | --- |
| [目录投影 Node](../../src/desktop/workbuddy_builtin/model-catalog.test.cjs)：主 Agent／ID／名称／aliases 解析、过滤和去重、同名不同 ID、公开积分／倍率、未知不推断及凭据排除 | 7 项通过，复用目录子任务已通过的工具输出；通过后 helper 和用例未改，未为文档再跑 |
| [桥接 Node](../../src/desktop/workbuddy_builtin/host-bridge.test.cjs)：初始化、最小授权投影、显式授权开关、撤销／轮换及异步事件边界 | 授权子任务报告 10 项通过；复用原报告，不重复计数 |
| 独立运行 Rust、GUI 倍率呈现、集成编译／release／唯一 MSI | Windows GUI/release、53 项不同 Rust 定向检查通过，含本域 19、provider 2、倍率和面板 4；唯一 MSI 状态及报告见 [V29 交付](../releases/v1.0.29.md)，不挪用旧版本结果 |
| Windows 真实首次导入、完全退出 WorkBuddy 后推理、长流／工具续轮／取消、401 续期、Hub 重启显式恢复、切号／撤销／停用及积分扣费 | 待用户实机验收；合成 Node 和静态代码审阅不能替代服务端验证 |
| macOS | 本项启动未适配，整体未构建／验收 |

开发审计只读安装目录 `D:\Program Files\WorkBuddy`：WorkBuddy 5.9.3、Electron 37.10.3，`@genie/agent-cli` 2.165.0；未读取 `.workbuddy`、`.codebuddy`、AppData 用户资料、凭据或会话，未执行厂商安装 JS、启动实际客户端或发真实模型请求。Node 子进程只执行 Hub 自有合成夹具及虚构账户／目录。当前必要集成检查和统一测试包由管理会话登记，不在本专题文档任务中追加测试或单独产包。

独立只读审阅确认：

- `main/server.js` 的 `bootstrapWorkbuddyAppServer` 唯一 `let ye=createSkillVlmService(...)`，持有同一身份/产品管理器、fetch、coordinator。
- `SkillVlmService` 保存构造 `deps`；`resolveEnv()` 返回四项 `WORKBUDDY_VLM_*`；`syncIdentity()` 撤销旧身份 ledger；独占实例 `close()` 停止自有代理。
- confirmed snapshot 来自 `main/workbuddy-auth-product-coordinator.js`，现有 `daemon-app-server-main.js` 已使用；会话路径为 `session.auth.accessToken`、`session.account.uid/enterpriseId`。
- `main/main-bootstrap.js` 正常模式固定 `evalModeEnabled:false`，走 stdio-fork；唯一原生管理器支持 `this.options.processExecArgv`，在 `daemon-app-server-entry.js` 前传递 argv，并保留环境及 `ELECTRON_RUN_AS_NODE=1`。没有强行切到初始化条件不同的 in-main。
- 真实程序 FuseV1 的 runAsNode/nodeCliInspect 允许，安装 compile cache 没有替换 `_compile`。nodeOptions Fuse 本身不能覆盖 packaged 环境过滤，V27 对它的判断不足已纠正。wire schema 依据 [Electron 官方说明](https://www.electronjs.org/docs/latest/tutorial/fuses)。
- `VlmUpstream.send` 原样 body，响应流透传且 client-gone/release 中断请求；`kinfra-office-agent` 仅为技能默认值，不覆盖本项显式模型，也不证明普通模型权限。

### V1.0.28 历史检查

V28 交付时尚无独立授权，以下结果只证明当时宿主／loopback 路径：

| 检查 | 当前状态 |
| --- | --- |
| Node 原有目录/凭据边界、精确 server 接线及新 main 锚点变化时原入口保留 | 3 项通过 |
| 真实 Node inspector → 用户主入口暂停 → 原生样式 manager → 真正 child argv preload → server → ready；保留原生 env、清理工具继承、连续双空格路径、关闭调试端口 | 1 项通过 |
| preload 加载失败后恢复原主入口、写出原始结果并关闭调试端口 | 1 项通过 |
| inspector 端口已占用时无原生 URI 但原入口仍实际执行 | 1 项通过；据此收紧无 URI 的进程保留行为 |
| Rust ASAR 双锚点/能力、只读 stderr URI、真实入口事件保留、加载失败后恢复、放行响应丢失、仅已确认新暂停 Child 回退，以及账户/租约/阶段乱序 | Windows GUI 测试目标编译及 8 项定向执行通过，无真实客户端 |
| rustfmt 与限定差异空白 | 已通过；未执行全树哈希或重复全量测试 |
| Windows 实际 WorkBuddy 启动、ASAR、daemon、登录/目录及模型权限 | 交付时待验；后续用户确认宿主连接及目录读取，其余不因此视为通过 |
| macOS | 本项未适配/构建/验收 |

V28 Node 检查以 5 项主轮和后续单独新增的端口占用 1 项合计 6 个不同用例。检查摘要登记在已忽略 `target/v1.0.28-unified-20261112/`，完整 GUI、release 和包身份见 [V28 记录](../releases/v1.0.28.md)。V28 后目录校正曾单独完成目录 5 项、桥接 6 项 Node 与 GUI 测试目标编译，当时独立路线仍处评估；这 11 项阶段结果不与 V29 的 7＋10 项再次累加，旧包未改名或重打。

### V1.0.27 历史检查

下表保留 **V27 历史离线结果**，用户实测已揭示其未覆盖正式包 native 过滤，不能据此宣称 V27 宿主启动可用：

| 检查 | V27 历史状态 |
| --- | --- |
| Node 合成宿主：fresh 投影、秘密字段排除、目录失效、退出登录、独占服务关闭、精确路径/重复锚点拒绝 | `node --test src/desktop/workbuddy_builtin/host-bridge.test.cjs`，2 项通过 |
| Rust ASAR 版本/锚点、账户目录/租约、凭据不入配置/防伪造 | 3 项定向用例通过 |
| Rust 本机 HTTP JSON、SSE、工具结果续轮、取消、错误脱敏/不重放 | 2 项定向用例通过，只有合成本机 HTTP，无真实模型 |
| 既有天工分享和客户端 writer 回归 | 共用 helper 的 3 项定向回归通过 |
| 分享表单、只读打开回执、选择身份与重启指导 | 3 项通过，含隐藏浅/深主题表单，不发键鼠输入 |
| 新文件 rustfmt、限定差异空白 | 已通过；未全树哈希或无变化重跑全部检查 |
| Windows 真实启动、登录、普通模型权限/额度、长流/真实工具/切号/退出 | 待用户验收，不以夹具替代 |
| macOS | 本项未适配/构建/验收，接口明确 Windows 优先边界 |

集成构建、定向测试、源码及统一包身份以 [V1.0.27 交付记录](../releases/v1.0.27.md) 为准。

## 回滚与维护

V30 的显隐偏好独立于授权，回退 V29 后该文件可保留，旧版不读取，不因此停用分享；积分缓存只在内存，退出后清空。回退到仅宿主分享的更早旧版前，先在本版停用，以结束在途请求并删除 Hub 本项受保护授权；不要只删来源而遗留授权。已保存来源／副本保留，后续移除走现有模型管理或恢复入口，不直接删用户客户端文件。正常退出当前 WorkBuddy 再普通打开即回原启动方式，原安装未修改，无需还原 ASAR／签名。

只有确认 WorkBuddy 已退出且本项停用后，才可整理 Hub 自有 `workbuddy-host/` 适配资源，不删除 WorkBuddy 用户配置。源码回滚需同时处理 `direct.rs`、宿主授权投影、分享域／provider、目录 helper 及其内嵌引用，不能只恢复一个文件。后续 WorkBuddy 升级需重核受影响初始化、授权／续期／账户接口及模型目录契约，不能仅放宽版本限制后声明兼容。

## 官方能力区别

普通 API/Token Plan 使用独立 API Key 与套餐，WorkBuddy 中自配外部模型也属于普通来源，不等同本项登录内置权益。[模型说明](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model)、[Token Plan](https://cloud.tencent.com/document/product/1831/137451)

Open API 本地助理/云端任务及 CodeBuddy `--serve` 属完整 Agent 接口；本项没有借其替代指定模型推理协议，也不循环创建占位任务续授权。[Open API](https://open.workbuddy.cn/docs/openapi)、[第三方应用接入](https://open.workbuddy.cn/docs/third-party-app)、[CodeBuddy HTTP API](https://cloud.tencent.com.cn/document/product/1831/137064)
