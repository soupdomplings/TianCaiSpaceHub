# WorkBuddy 登录内置模型跨客户端分享

维护日期：2026-11-11。实现基线：`main / 3daf9ee`，V1.0.27 开发批次。关联 TC-002、TC-004、TC-018、TC-021。

状态：**已实现 Windows 5.9.3 宿主桥接与分享接口，GUI 测试目标编译和本项离线检查通过，真实启动待用户验收**。此前 V1.0.26 只做评估；本轮按用户授权落实端内桥接，不再停留于评估。不单独产包，release、源码提交及唯一测试包由管理会话在 [V27 交付](../releases/v1.0.27.md) 登记。

## 行为与入口

天才搭档可通过 `hub_open_page({"page":"workbuddy-builtin-share"})` 打开默认隐藏的分享面板。打开只读取，由用户点击“启用分享”后准备并启动宿主。没有加入通用 Skill/MCP 管理，也不将 IM `/wb` 当作模型推理接口。

读取成功保持安静，批量保存集中反馈一次；需要先退出 WorkBuddy 时显示具体提示，不弹“启用成功”。宿主已启动但登录或目录未就绪时保留等待说明，只有 `ready=true` 才显示分享已就绪。

| 操作 | 实际行为 |
| --- | --- |
| 打开、重新读取 | 返回进程内状态、当前已确认账户模型目录和目标客户端；不启动 WorkBuddy，不读取其用户资料或凭据 |
| 启用分享 | 检查安装版本、唯一宿主锚点和启动能力；已有普通 WorkBuddy 进程时提示用户正常退出后再启用，绝不自动关闭任务 |
| 选择并分享 | 当前账户和临时引用就绪后，从当前目录选模型及目标，生成 Hub 来源并复用既有客户端 writer |
| 重复分享同模型 | 复用同账户来源与目标副本，保留客户端独立参数；Codex 已移出可见目录的副本重新加入原条目 |
| 停用 | 关闭 Hub 回调 listener；宿主短时内释放本项独占 ledger/listener。保存的模型配置保留，后续调用明确提示停用；再次启用需先退出 WorkBuddy |
| 切号、切换组织、退出登录 | 宿主立即同步身份并撤销旧 ledger，Hub 随心跳更新；旧副本不自动借用新账户，需重新选择当前账户模型 |
| 真正退出 Hub | 回调连接消失，宿主结束本项临时服务；不强杀 WorkBuddy，不影响其原生技能/电脑操控会话 |

费用、限流与权益沿 **WorkBuddy 原账户/组织**，不属于天才搭档管理员承担的客服额度。目录只代表当前可选配置，不能据此保证每项额度或推理已通过；401/403、429 等仍以上游实际结果为准。

## 宿主与凭据边界

关键代码：[分享域](../../src/desktop/workbuddy_builtin/mod.rs)、[只读安装准备](../../src/desktop/workbuddy_builtin/prepare.rs)、[进程内 bootstrap](../../src/desktop/workbuddy_builtin/host-bridge.cjs)、[模型转发](../../src/ai_gateway/providers/workbuddy_builtin.rs)、[本机操作 API](../../src/web/workbuddy_builtin.rs)。

1. 显式启用时，只读安装 `resources/app.asar` 的 `package.json`、`main/server.js`，核对 5.9.3、唯一 `createSkillVlmService` 调用处和执行程序的 Electron FuseV1 启动能力。没有修改安装 ASAR、程序、签名或用户登录文件。
2. Hub 在自身桌面状态目录 `workbuddy-host/host-bridge.cjs` 保存小型自有适配资源，为本次用户触发的启动追加 `NODE_OPTIONS --require`。只拦截经核对的绝对 server 路径，在 `Module._compile` 中内存接线；锚点不唯一时报告不兼容，不假报就绪。
3. 以原生 `SkillVlmService` 构造器及其 `deps` 创建**本项独占实例**。它复用宿主身份、刷新与模型代发，但拥有独立临时会话、ledger 和 listener；停止时不关闭原生技能实例。
4. access/refresh token 始终留在 WorkBuddy。Hub 仅在内存持有原生 `/skills/vlm/chat` 的 loopback URL、随机临时 token 与签名 context；这些字段不写入来源、目标客户端、诊断或操作记录。
5. 宿主每秒向随机 loopback 回调更新租约和目录。Hub 最后有效心跳超过 4 秒后拒绝引用；宿主回调超时为 2 秒，连接失败/租约关闭时释放独占实例。实际关闭时间受调度及在途回调影响，不承诺固定毫秒数。
6. 命中接线后移除该进程环境中的 Hub 回调授权，并恢复原 `NODE_OPTIONS`，避免传播到宿主工具。适配文件本身不含秘密；保留它是为了已启动主进程重建 stdio 宿主时不会引用被删除的 preload，属于适配资源而非会话缓存。

普通单空格路径受支持；当前明确拒绝引号、换行和连续两个空格的 preload 路径，保留普通启动。这源于 WorkBuddy 子进程会按空白归一 `NODE_OPTIONS`。没有修改其发行时安全开关。

## 目录、来源与配置兼容

只用宿主 `coordinator.getConfirmedRemoteModelSnapshot()`：其自身要求当前身份一致、远端模型已确认、fresh 且无刷新在途。本项再次要求 fresh，不以安装模板 58 项填充账户目录。

只投影 `id`、`name` 及确实存在的 `contextWindow`、`maxOutputTokens`、`supportsToolCall`、`supportsImages`、`supportsReasoning`。`maxInputTokens` 不当作输入加输出总窗。未知思考档位、默认值及厂商协议不猜测，配置对象的 `url`、`apiKey` 不返回 Hub。

来源新增可选 `workbuddyBuiltin`，只含用户/组织非秘密身份散列、固定 `modelId` 和投影能力。旧配置无此字段时行为不变；天工授权与模型来源仍独立。来源使用本机占位地址，执行时才取内存引用；目标客户端仍只接 Hub 及自身 local token。

来源/副本指纹不含租约 token，轮换不会制造模型配置变化。普通渠道编辑转到分享面板；通用配置保存不可新增/伪造 binding 或误删已有 binding，删除整条及客户端独立参数修改走既有入口。每次真实请求检查当前账户、固定模型 ID 与有效引用。

新账户显式分享时，只停用其他账户的普通 WorkBuddy 来源，避免同模型 ID 的普通路由命中旧账户；旧客户端副本及唯一 Codex alias 保留，使用时明确提示账户已切换。心跳不写配置。

## 协议与接口

原生 `VlmUpstream` 从宿主认证管理器取当前授权，向官方网关 `/v2/chat/completions` POST 原始 body，不在该处覆盖显式 `model`。当前目录 ID 可以进入同一代发链，服务器是否接受仍需实测。

Hub 使用独立 provider 分支，清除客户端/普通渠道认证与 Origin，只向 loopback 发送临时 ledger 授权、context 和 JSON。该连接不走外网代理、不跟随重定向、不自动重发生成。文本 JSON/SSE、工具声明、调用 ID、工具结果续轮复用既有 Chat/Responses 转换；工具由目标客户端执行。取消时断开宿主连接，由原生代理 abort 上游。非成功 HTTP 错误只保留状态与脱敏提示，不留可能回显认证的原始错误体。

**本轮不提供 WorkBuddy Responses `/compact`。** Codex 长会话的远端压缩收到明确暂不支持提示，原历史不替换；可新建会话或用客户端本地摘要。文本/工具通路不等同完整 Responses、加密压缩、所有多模态或专用思考档位验收。

| 本机 POST 路径 | 请求 | 响应 |
| --- | --- | --- |
| `/api/desktop/workbuddy-builtin/read` | `{}` | 当前 catalog，不启动 |
| `/api/desktop/workbuddy-builtin/enable` | `{revision}` | 新 catalog，已有普通进程则 `needsRestart=true` |
| `/api/desktop/workbuddy-builtin/disable` | `{revision}` | 停用后的 catalog |
| `/api/desktop/workbuddy-builtin/share` | `{revision,modelId,targets}` | 新 catalog 及逐目标 `outcomes` |

catalog 为 `revision/enabled/ready/loggedIn/accountName?/needsRestart/detail/models/targets`。只有登录、fresh 目录和本机引用均有效才 `ready=true`；未确认时 `models=[]`。接口沿 Hub 现有可信本机请求规则，share 复用 [既有分享 helper](../../src/web/desktop_models.rs) 与各端 writer。

## 静态证据与实际验证

本轮只读 `D:\Program Files\WorkBuddy`：WorkBuddy 5.9.3，`@genie/agent-cli` 2.165.0。没有读取 `.workbuddy`、`.codebuddy`、AppData 用户资料、凭据或会话，没有执行安装 JS、启动客户端或发真实模型请求。

独立只读审阅确认：

- `main/server.js` 的 `bootstrapWorkbuddyAppServer` 唯一 `let ye=createSkillVlmService(...)`，持有同一身份/产品管理器、fetch、coordinator。
- `SkillVlmService` 保存构造 `deps`；`resolveEnv()` 返回四项 `WORKBUDDY_VLM_*`；`syncIdentity()` 撤销旧身份 ledger；独占实例 `close()` 停止自有代理。
- confirmed snapshot 来自 `main/workbuddy-auth-product-coordinator.js`，现有 `daemon-app-server-main.js` 已使用；会话路径为 `session.auth.accessToken`、`session.account.uid/enterpriseId`。
- `main/main-bootstrap.js` 用 `spawn(process.execPath,...)` 启动 stdio daemon，继承环境及 `ELECTRON_RUN_AS_NODE=1`；sanitize 保留 `--require` 与引号，归一连续空白；in-main 同样加载 server。
- 真实程序 FuseV1 的 runAsNode/nodeOptions 均启用，安装 compile cache 没有替换 `_compile`。启动开关及 wire schema 依据 [Electron 官方说明](https://www.electronjs.org/docs/latest/tutorial/fuses)。
- `VlmUpstream.send` 原样 body，响应流透传且 client-gone/release 中断请求；`kinfra-office-agent` 仅为技能默认值，不覆盖本项显式模型，也不证明普通模型权限。

| 检查 | 本子任务状态 |
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

停用后正常退出当前 WorkBuddy 再普通打开，即回原启动方式；原安装没有修改，无需还原 ASAR/签名。来源/副本移除走现有模型管理或恢复入口，不直接删用户客户端文件。确认 WorkBuddy 已退出后可移除 Hub 自有 `workbuddy-host/host-bridge.cjs`，不删除其用户配置。升级需重新核对受影响宿主契约和夹具，不能仅放宽版本限制后声明兼容。

## 官方能力区别

普通 API/Token Plan 使用独立 API Key 与套餐，WorkBuddy 中自配外部模型也属于普通来源，不等同本项登录内置权益。[模型说明](https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model)、[Token Plan](https://cloud.tencent.com/document/product/1831/137451)

Open API 本地助理/云端任务及 CodeBuddy `--serve` 属完整 Agent 接口；本项没有借其替代指定模型推理协议，也不循环创建占位任务续授权。[Open API](https://open.workbuddy.cn/docs/openapi)、[第三方应用接入](https://open.workbuddy.cn/docs/third-party-app)、[CodeBuddy HTTP API](https://cloud.tencent.com.cn/document/product/1831/137064)
