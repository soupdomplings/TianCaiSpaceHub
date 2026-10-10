# Hub 接入 NVWA MCP：当前开发交接

维护日期：2026-10-10。目标项目：TianCaiSpaceHub。关联 TC-014，适用 `0.4.30-5`；本文件路径保留首次交接日期。

本文按用户最新决定记录当前源码交接。操作、默认值、系统保护与回滚以 [NVWA MCP 专题](customizations/nvwa-mcp.md) 为准；同仓依据见 [NVWA 资料索引](nvwa/README.md)。2026-10-10 本轮将第二入口改为“认证服务连接”，应用参数移至主区域，并增加双语配置说明；**最终 Windows locked GUI build 通过（13.85 秒、38 条警告），新 EXE 构建及静态产物核对通过，未运行**。2026-10-08 及此前 2026-10-10 基础/高级界面阶段的编译/EXE 保留原归属，见 [v5 交付](releases/v0.4.30-5.md)。没有真实登录、MCP 调用、客户端测试或用户验收，未发布。

## 1. 已确认需求

管理登录的三种入口现在都要求随机 UUID `loginAttemptId`。取消可提交同一标识；服务按同一短临界区排序并保留 10 分钟、最多 4096 条取消记录，避免取消先到、旧登录后到。GUI 挑战重试生成新事务，取消期间晚到响应不能提交授权；不把取消解释为远端业务回滚。

- Hub 新增独立 NVWA MCP 页签，管理多个环境，分别接入本机 Codex、WorkBuddy、天工 Claw。
- 认证选择为“账号密码”和“认证服务连接”，默认 `password`。用户切入认证服务连接时默认勾选主区域的“使用共享应用代表指定用户”，使用 `application`、要求代表账号和 ClientID/ClientSecret；取消勾选才使用 `browser` 真实产品页面授权。勾选时也能切换认证选择，网络操作忙状态保护保留。
- 加载旧环境按已存模式显示：`password` 仍密码，`application` 为认证服务连接且勾选，`browser` 为认证服务连接但不勾选；不因新切入默认勾选而改变旧模式。旧 URL/header/digest 与 browser 非空账号限制均保留。
- 在认证服务管理添加应用服务，获取ClientID和ClientSecret。当前可共享一套真实注册 ID/密钥，不要求每位实施人员或每端客户端重新申请。Hub 不内置共享密钥；取消共享应用走浏览器时也需真实注册应用与本机回调。用户已确认共享应用密钥没有服务端用户名范围限制；应用签名能代表填写的用户，不证明人员已完成本人密码认证，配置说明明确该含义，UI 不新增服务端授权控制。
- 用户已确认 `Authorization` 登录 token 与 `authorization-ticket-token` ticket token 都可用、有效期不同。两类 token、使用头与 expiry 分别管理，不猜测互换或统一 TTL。
- 普通 profile 只存 secret 引用。Windows 使用当前用户 DPAPI，macOS 使用 Keychain；本轮不扩展 Linux。
- 用户负责真实测试。开发方只进行必要编译、产物及文档静态核对，不启动实际客户端、Hub/NVWA，不调用真实认证、MCP 或模型，不读取 `.local-data`、用户配置、凭据、私有日志或数据库。

先前浏览器作为唯一默认入口、密码登录仅预留、共享密钥用户名范围待核实、每客户端单独应用 ID 等建议，不作为当前开发指令。产品将来新增服务端范围限制时再更新真实契约，不用 UI 校验冒充服务端控制。

基础区显示环境名称、一个 NVWA 服务地址和认证选择。账号密码显示账号/遮罩密码/记住密码；认证服务连接主区域直接显示共享应用复选框、ClientID、遮罩 ClientSecret 和保存应用密钥，勾选时显示代表账号。应用注册字段不再藏在高级区，密码模式不显示它们。独立认证/MCP 地址、认证头、指定租户、密码登录机构和仅 application 的签名算法仍在默认折叠高级区；摘要默认 SHA-256，保留 SM3/MD5 兼容。

认证地址留空回退产品地址。MCP 高级字段默认直接显示可编辑的 `/mcp`，可改成其它以 `/` 开头的路径或完整 HTTP(S) URL；空值和 `/mcp` 都保留产品部署前缀后追加 `/mcp`，其它路径同样保留前缀，完整 URL 独立覆盖。路径随保存的服务地址变化，完整 URL 不自动改变。GUI 将空值和 `/mcp` 同样视为默认，保存和未保存检查使用同一规则；新环境默认保存空值，旧环境已存 `/mcp` 则保持原值，避免等价保存误撤凭据。旧完整 URL 原样加载，不因默认显示造成未保存差异。管理状态增加 `resolvedMcpUrl`，用于核对实际目标，当前 GUI 身份文字不显示该字段。服务主区域的“保存应用密钥”在认证时明确交给系统保护存储，普通 profile 仅持引用。

浏览器在高级区另有“限定授权账号（可选）”专属行，新环境可留空，无需日常填写；旧环境非空 `username` 限制保留并允许编辑，不因切换界面静默删除。填写限制后仍须与真实授权身份一致。

切换认证选择或共享应用复选框，清空账号、密码、ID/密钥、指定租户、机构、记住选项、挑战、身份、工具及客户端检测，重置认证头/摘要默认值，保留环境名称和通用地址。已保存环境取消请求携带旧 `loginAttemptId`，旧结果失效，取消完成后保留新表单、不重载旧认证输入。移除切换/重新检查状态文字及 application 身份横幅，真实错误、挑战和操作结果仍显示；原取消与代次保护保留。保存后的状态刷新仅在完整 profile 与保存快照及表单一致时保留当前内存密码/密钥和记住选项，避免保存吞输入；不写普通配置。

顶部“?”打开按界面语言显示中文或英文的原生配置说明对话框，覆盖环境和服务 URL、默认可编辑 MCP 路径、密码流程、在认证服务管理添加应用服务获取 ClientID/ClientSecret 与代表账号、取消勾选后的浏览器授权及回调、记住秘密，以及保存/登录/MCP 检测/各端预览的操作顺序。共享应用代表用户的身份含义放在说明内，不能误认成本人密码证明。

## 2. 两项目职责

```text
Hub GUI -> 后台本机管理接口 -> NVWA 密码/浏览器/应用认证
                                 |
                      真实 token、用户、身份、租户
                                 |
Codex / WorkBuddy / 天工 Claw -> 127.0.0.1:3849 的每端 HTTP 桥
                                 |
                        NVWA 产品宿主真实 /mcp
                                 |
                   产品 Java 业务、权限与审计
```

Hub 管环境、认证材料、系统保护、每端配置及 HTTP MCP 转发；NVWA 管真实身份、租户、权限、工具开放、写入幂等、冲突与审计。Hub 不复制 NVWA 工具、不添加 LLM/planner、不生成管理员权限、不上传 MCP JAR、不切换部署、不操作产品业务库。

NVWA MCP 必须已在产品宿主安装并启用。工具目录由该账号真实 `tools/list` 返回，不硬编码历史数量；模块缺失或产品权限拒绝，不在 Hub 伪造可用能力。

## 3. 认证与产品契约

### 3.1 本人账号密码

[AuthClient](../src/nvwa/auth.rs) 先 GET `/anon/framework/api/encrypt/key`。alias `3`：UTF-8 标准 Base64 后每 50 个字符分块，RSA PKCS#1 v1.5 加密，各块 Base64 拼接；alias `2`：原文 UTF-8、SM2 `C1C3C2`。与当前产品前端一致，POST `/nvwa/login` 发送 `username/pwd/tenant`、可选 `loginUnit`、受限 `extInfo`、`encryptType="3L"`。不硬编码公钥、不采用旧 AES 默认值，不因公钥失败降级明文，不裁剪密码。

密码方式指定租户留空时，请求的 `tenant` 使用产品默认选择值 `__default_tenant__`；最终仍须真实 `/nvwa/getLoginContext` user/identity/tenant，不将该占位值视为已核验租户。显式租户仍须与返回身份相符。

成功需明确业务状态和 token，再 GET `/nvwa/getLoginContext` 核对真实用户、身份、租户与登录名。支持 `context` 包装、`contextUser`/历史 `conetxtUser`；显示占位租户不算已验证上下文。默认 MCP 采用 `Authorization: <raw login token>`，无 Bearer。

| 产品状态 | Hub 当前处理 |
| --- | --- |
| `0` / `200` | 仍需有效 token 与真实上下文才完成认证 |
| `203` | 有有效 token/上下文时可作为到期提醒，不扩大到其他 2xx 状态 |
| `201` / `202` | 首次/过期改密，返回产品页面必要操作；Hub 无改密表单 |
| `204` | 双因子阶段，保留产品会话，仍不能开放 MCP |
| `402` | 需要有效图形验证码，不跳过、不伪成功 |
| 错误账号、锁定、停用、过期、登录限制、维护等 | 固定安全提示，不回显原响应或认证 header |

provider 挑战字段仅 `verifyId`、`verifyCode`、`validCode`、`twofactorSessionId`。GUI 仅在 `204` 后显示双因子验证码输入和发送按钮，session 内部维护。发送使用 POST `/anon/nvwa-nros/v1/msg/send`；第二次登录带 `extInfo.validCode/twofactorSessionId`，由产品继续校验。不能借附加字段覆盖 username、`checkPwd` 或加密模式。

GUI 已移除图形验证码 ID/码输入。获取方法、图片 schema 尚未核实，当前**没有验证码图片获取或展示**；`402` 提示到产品页面处理或选择已配置的浏览器授权，不保证产品页面操作后 Hub 密码登录必然成功，也不自动复用浏览器会话。完整集成仍需 NVWA 提供准确方法、参数、图片/ID、有效期和一次性契约并由用户验证；provider 保留受限字段兼容，本版不猜返回值。

### 3.2 浏览器个人授权

认证服务连接取消共享应用勾选后，走真实浏览器页面授权；首次需要真实注册的 ClientID/ClientSecret 及本机回调，Hub 无内置共享密钥。密钥可由用户明确记住后交给 SecretStore。当前构造产品 `#/authorize?response_type=code&client_id=...&redirect_uri=...&state=...`。本机 `127.0.0.1` 回调含随机 64 hex state 路径，事务 10 分钟、单次消费，失败也消费。只接一个 `code`/`ticket`/`ticketId`，若带 state 参数必须匹配路径；还核对 profile 指纹与登录代次。新登录、配置改变、取消和退出均撤销旧事务，晚到或重复回调不能重新开放授权。

通过校验后用应用 ID/密钥交换 ticket，并读取真实上下文。打开网页或收到回调不是认证完成。当前共享应用是否接受本机回调地址/端口、部署是否采用 Hash 路由、个人票据交换后能否调用 MCP，均待用户实测。不假设 OAuth discovery、PKCE、refresh token 或任意 redirect URI 已支持；回调 query/票据不进日志和错误。

### 3.3 应用代表用户

认证服务连接勾选共享应用时使用 `application`，该勾选为用户切入服务方式的默认选择，旧环境按已存模式加载。使用 `authorization-cer-client` GET `/nvwa-certification/v1/ticket/apply`；默认 SHA-256，可选 SM3 或 MD5 兼容。SHA-256/SM3 在签名串分别加入 `1`/`2`，小写 hex；MD5 按复制连接器的大写 hex。

申请 `data.id` 是一次性 ticket，经 `authorization-client-basic: Base64(clientId:clientSecret)` POST `/nvwa-ticket/v1/ticket/{encodedTicketId}` 交换，返回 token 用 `authorization-ticket-token`。Basic 材料和密钥不是 MCP token。必须明确用户名，并验证返回用户/租户；不自动使用管理员或空用户名。

密码和浏览器认证失败时不自动改用此方式。共享密钥目前可代表任意用户名是已确认服务端能力边界，不能将 application 写成已验证本人登录。

### 3.4 两类有效期与恢复

`AuthResult` 内含真实 `VerifiedIdentity`、可选 `personal_token` 与 `mcp_token`。token 不作为普通 DTO Debug/Serialize；仅专用系统保护记录含值。可见状态分别报告个人/MCP expiry，未知为未知。password 默认个人 token 直接用于 MCP，browser/application 默认交换 token。

expiry 只从该 token 响应的 `expiresAtMs` 或明确 `createTime+validTime`（秒）取得，apply ticket 的 600 秒不继承给交换 token。profile 可显式选择两个已允许头名，但不能推定所有部署令牌可互换。

后台保留系统保护的认证记录和每端本地凭据，绑定 profile 指纹。恢复后首次使用必须重新核验真实身份，运行期核验缓存最长 60 秒；状态明确 `identity_verification_pending`，分别显示两 token 是否存在及 expiry。明确过期或真实 401 后，只有此前显式保存该方式凭据时，下一次请求前 password/application 可重新认证并核对相同 user/identity/tenant；captcha/MFA/改密需人工。browser 无已确认 refresh，过期重授权。原请求特别是 `tools/call` 绝不因换 token 自动重放。

## 4. 本机桥与每端配置

[服务](../src/nvwa/mod.rs) 与 [bridge/runtime](../src/nvwa/bridge.rs) 由 daemon 承载，默认 `127.0.0.1:3849`。每 profile、每 client 独立本地 Bearer，真实 NVWA token 只后台发往产品；管理端另用系统保护实例引导并核对 Host/Origin。旧公开网关管理 API 不挂载 NVWA 凭据操作。

initialize 后本地 `Mcp-Session-Id` 绑定环境、client、凭据、登录代次与协议，不传给 stateless NVWA。RPC ID 独立映射后回原 ID，拒同 session 重复活动 ID；取消只定位该 session 请求，DELETE 仅释放本地映射，不证明远端写入已撤销。最多 512 session、24 小时空闲期限、256 活动 RPC、8 MiB 每远端响应。

首版只支持 stateless JSON HTTP MCP POST/DELETE 和 `2025-11-25`、`2025-06-18`、`2025-03-26`，不实现独立 SSE GET 流。需正确 JSON-RPC、ID、initialize serverInfo/tools 能力；检测通知需 202，分页 tools/list 最多 100 页/10000 项，拒循环 cursor、重复工具及无效 schema，零工具目录也可合法。检测不调用工具，不能代替某客户端真实连接证据。

已发送 `tools/call` 断连、超时、解析/ID 失败等明确未知结果、不可自动重试，不伪造成功或权限。认证和桥自身错误使用固定安全提示/HTTP业务 code，不回显原始 msg/body/header；产品协议和工具业务失败保留其语义。

[适配器](../src/nvwa/adapters/mod.rs) 固定受管名 `nvwa-<SHA256(profileId)前16hex>`。检查/预览后带最新指纹定向保存、移除、恢复；不存在目标为 `missing`。同名不受管拒接管，客户端只含本机 URL 与每端本地 Bearer。

| 客户端 | 当前范围与生效边界 |
| --- | --- |
| Codex | 桌面用户 home `.codex/config.toml` 单一 `mcp_servers`，可显式绝对路径；保留其他 TOML。旧会话加载与原生信任待用户确认，文件目标可配置不证明已安装 |
| WorkBuddy | config dir 或用户 `.workbuddy[-实例]/mcp.json` 的单项 `mcpServers`；品牌/专享版核对显式路径。保留其他 JSON 和 approvals，原生 refresh/重启/信任待用户 |
| 天工 Claw | [DesktopClient 窄 CRUD](../src/gmclaw_desktop.rs)，必须经核验官方桌面运行授权；不指定 DB、不直接写 SQLite。保存未连接/空目录，管理 detect 可定向严格核验后更新本端目录；弱 test/discover ok 不是严格证据 |

路径必须绝对且文件名正确，拒 symlink/Windows 重解析点。`managed.json` 只存引用/指纹；目标前像分块系统保护，不备整份第三方配置。写前 journal、写后只读核对，未知写入不重放。文件锁、双重读盘、原子替换保留无关项；天工无 CAS，不能承诺跨进程原子保护。恢复只允许最新备份、同目标及写后指纹未变，不覆盖用户修改。

管理 `tools/detect` 带 `clientKind="tiangong"` 时，严格目录检测后仅当前受管、未修改、启用且本地凭据匹配条目可更新原生目录，返回 `tiangongDirectoryUpdated`。临写前第二次读取完整连接，与 inspect 使用同一归一化指纹规则核对完整目标，并再次确认启用、本地 Bearer 和认证代次；DataServer 没有 CAS，最后一次读与写之间仍有外部并发窗口。GUI 顶部“检测 MCP”只检测 Hub 上游链路，每端“检测本机桥”传对应 clientKind 并真实核对该端本机凭据、initialize/session、分页 tools/list 与本地 DELETE；目录更新不代表真实工具成功，不接管或启用用户条目。

移除/恢复撤销该端授权。有未移除受管条目时拒绝删除 profile；退出撤销环境认证/session/事务但不自动改客户端配置。改端口、迁移作用范围和回旧版需先定向移除或恢复，细节见 [专题](customizations/nvwa-mcp.md)。配置、加载/信任、Hub 检测、客户端连接和业务成功分别记录。

新增 MCP 路径不被此前只接受完整 URL 的 NVWA 版本理解。回退路径功能前，先将已存 `/mcp` 或其它路径解析成明确完整 HTTP(S) 地址并保存，再处理授权与客户端条目；默认空值保持兼容，旧完整覆盖地址不自动改变。

## 5. 代码与系统存储

| 路径 | 职责 |
| --- | --- |
| [GUI](../src/gui/nvwa.rs) | 认证服务主区域、共享应用/浏览器选择与旧模式加载、双语配置说明、基础/高级显隐、切换清空/取消、保存后一致输入保留、204 挑战与每端预览；后台结果代次隔离 |
| [types](../src/nvwa/types.rs)、[config](../src/nvwa/config.rs) | 非秘密 DTO、默认值、URL 校验、集合 revision 和原子保存 |
| [auth](../src/nvwa/auth.rs)、[secrets](../src/nvwa/secrets.rs) | 实际认证、动态 RSA/SM2、身份/TTL、Windows/macOS 系统保护 |
| [mod](../src/nvwa/mod.rs)、[server](../src/nvwa/server.rs) | 生命周期、管理 API、一次性 browser、系统保护恢复与代次 |
| [bridge](../src/nvwa/bridge.rs)、[runtime](../src/nvwa/runtime.rs) | HTTP MCP、session/RPC 映射、无重放、分页目录 |
| [adapters](../src/nvwa/adapters/mod.rs) | 每端定向编辑、预览指纹、归属、系统保护备份和恢复 |

以实际 Hub 主配置父目录/文件 stem 派生 `<stem>.nvwa.json`、`<stem>.nvwa.lock`、`<stem>.nvwa-secrets`。集合 version `1`、默认 bridgePort 3849、最多 64 profiles/1 MiB，`_revision` 不落盘，profile 仅 `credentialSecretRef`。集合/profile 未知非敏感字段透传保留，递归拒 password/secret/token/authorization/cookie/ticket 等普通配置中的敏感字段，限制 16 层/10000 节点，防扩展覆盖标准字段。

凭据、token、后台引导和备份进入 SecretStore。Windows 为用户域 DPAPI 密文文件（不使用 LOCAL_MACHINE）；macOS 为用户 Keychain service `TianCaiSpaceHub.NVWA`，account 用 root/key hash。普通 ledger 仅引用/指纹，多份主配置隔离，复制 root 不承诺跨用户恢复。没有用命令行参数写入秘密，也不修改旧模型/IM 配置来保存 NVWA 材料。

## 6. 同仓依据与待验收契约

同仓资料：

- 本机 `docs/nvwa/all-in-one.md` 规范快照：登录约 25347、状态约 42590、浏览器约 54887、票据约 54923、摘要约 55247 行；含未核对凭据示例，仅本机保留不纳入 Git，不抄示例值，旧章节不能覆盖实际实现。
- [连接器参考](nvwa/nvwa-mcp-auth-connector.mjs)：MD5 apply、Basic exchange、ticket header 可作证据；未鉴权 proxy 与通用 401 重发不作为执行方案。
- [项目规则](../AGENTS.md)、[文档入口](README.md)、[二开总表](customizations/README.md)、[维护规范](development/documentation.md)。

NVWA 邻仓 `D:\traeworkspace\dumpling-nvwa` 的公开静态来源仅作来源路径说明，不再创建指向本仓不存在文件的相对链接：

- `bud-std/bud-std-server/src/main/resources/static/static/index-DLgEX3fU.js`：动态 key、RSA/SM2、登录包装、个人 authorize。
- `nvwa-agent-backend/mcp-nvwa-server/src/main/java/com/jiuqi/dumpling/nvwa/mcp/server/NvwaMcpTrustedHttpContextExtractor.java`：真实 Subject/NpContext user/identity/tenant。
- 同模块 `NvwaMcpAuthenticationHeaderFilter.java`：单一产品认证头，拒 query/trust 自报身份。
- `nvwa-agent-backend/mcp-analysis/src/main/java/com/jiuqi/dumpling/nvwa/mcp/analysis/AnalysisProductContextGuard.java`：真实身份一致与管理员约束。

客户端官方入口：[Codex MCP](https://developers.openai.com/codex/mcp/)、[Codex 配置](https://developers.openai.com/codex/config-reference/)、[WorkBuddy MCP](https://www.codebuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/MCP-Guide)。客户端配置参考不替代本版验收；天工静态依据为官方 DataServer/harness adapter 公开资源，没有读取用户 token 或私有任务库。

交回 NVWA 的待验收契约：浏览器注册地址/Hash 路由、两类 token 实际寿命与字段、getLoginContext 稳定 identity/tenant、动态 RSA/SM2、captcha 获取 schema、MFA 发送与二次校验、普通/管理员工具权限。不能把这些待验收项写成已确定产品缺陷或本轮已经通过。

2026-10-08 Windows locked GUI check（6.27 秒、38 条警告）及原 `outputs/nvwa-v0.4.30-5-windows/` EXE 保留当时源码归属。此前 2026-10-10 基础/高级与 MCP 路径阶段 check（27.23 秒、38 条警告）及 build（17.55 秒、38 条警告），对应 `outputs/nvwa-v0.4.30-5-windows-ui-20261010/`，版本/x64 PE/导入/哈希已核对、未运行；均不能验收本轮认证服务主区域和帮助说明。

本轮仍为 `0.4.30-5`、唯一 `main`，**最终 Windows locked GUI `cargo build` 通过（13.85 秒、38 条警告）**；已独立输出到 `outputs/nvwa-v0.4.30-5-windows-auth-service-20261010/`。新 EXE 为 60,942,336 字节，版本/x64 PE/DLL 导入和复制前后哈希核对通过，未运行，不覆盖此前文件。源码及产物身份由 [v5 交付](releases/v0.4.30-5.md) 分阶段记录。

本轮认证服务默认勾选/取消后的真实浏览器授权、旧 password/browser/application 加载保持、认证选择始终可切、主区域应用字段、中文/英文配置说明及切换状态文字调整待用户验收；认证切换清空/晚到结果、保存环境保留输入、默认 MCP 地址/默认与指定真实租户、402 提示、204 挑战、真实登录、MCP/模型、三端加载信任、写断连未知、并发保存和系统恢复仍待用户。开发方未运行测试、实际 Hub/客户端或业务，不读取私有资料；安装包只由 GitHub Actions 生成，Windows/macOS 构建、产物核验、实机验收、源码推送和发布分开记录。
