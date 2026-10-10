# NVWA MCP 接入

维护日期：2026-10-10。适用版本：`0.4.30-5`。关联：TC-014。

实现状态：接入页、认证 provider、后台本机桥、管理接口与三客户端适配已实现；本轮调整基础/高级界面、认证切换、保存输入及可编辑 MCP 路径。2026-10-10 最终 Windows locked GUI build 通过（17.55 秒、38 条警告），新测试 EXE 已构建、复制并通过静态产物核对，未运行。2026-10-08 的编译/测试 EXE 保留原阶段归属，详见 [v5 交付](../releases/v0.4.30-5.md)。验收状态：待用户验收。发布状态：未发布。

本文描述 Hub 的当前实现；同仓 [NVWA 资料索引](../nvwa/README.md) 和 [开发交接](../hub-nvwa-mcp-integration-handoff-2026-10-08.md) 保存依据与跨项目边界。参考规范、旧脚本及静态源码核对不等于本次真实登录或客户端验收。

## 需求与边界

实施人员在自己电脑上的 Hub 配置 NVWA 环境，再分别接入 Codex、WorkBuddy、天工 Claw。普通认证选择只有本人账号密码 `password` 和浏览器个人授权 `browser`，默认 `password`。高级设置保留明确的“使用共享应用代表指定用户（仅管理员）”；启用后使用 `application`，不能作为本人密码登录证明。当前共享应用可使用同一套真实注册 ID/密钥；用户已确认服务端没有限制共享密钥可以声明的用户名范围。Hub 不内置共享密钥，界面中的管理员提示也不建立服务端尚不存在的个人隔离。

NVWA 继续通过产品宿主的真实 HTTP MCP 和 Java 服务执行业务。Hub 读取真实登录上下文、工具目录及响应，不复制工具、不生成权限、不自行开放管理员分析能力。产品必须已经部署 MCP 模块、启用接口并提供可达地址；Hub 不上传 JAR、不操作产品数据库、不切换部署，也不发起模型调用。

新增平台优先 Windows，其次 macOS；本轮不扩展 Linux。修改本机客户端配置不会接入 ChatGPT 网页或云端连接器，不静默安装、重启或批准客户端工具。

## 操作入口与状态

桌面的“NVWA MCP”页签先显示环境选择、环境名称、一个“NVWA 服务地址”和普通认证选择。账号密码方式显示账号、遮罩密码和“记住密码”；浏览器方式显示产品页面授权说明，应用注册参数放在高级设置。日常密码登录无需填写独立认证地址、MCP 地址、租户或应用注册参数。环境保存后再登录或授权。

“高级设置（特殊部署 / 管理员）”默认折叠，按认证方式显示相关内容：

| 设置 | 当前行为 |
| --- | --- |
| 认证服务地址 | 留空使用 NVWA 服务地址；特殊部署可填写独立地址，不强制同源 |
| MCP 地址 | 默认直接显示可编辑的 `/mcp`；可改为其它以 `/` 开头的路径或完整 HTTP(S) 地址 |
| MCP 认证头 | 按认证方式使用默认头；保留已保存的显式头配置 |
| 指定租户、登录机构 | 租户可选；登录机构仅账号密码方式显示 |
| 限定授权账号（可选） | 仅浏览器方式显示；新环境可留空，旧非空账号限制保留且可编辑，不静默删除 |
| 共享应用代表指定用户 | 仅通过明确的管理员复选框启用，启用后普通认证选择禁用，显示所代表的账号及真实应用 ID/密钥 |
| 应用签名算法 | 仅应用代表用户方式显示，默认 SHA-256，保留 SM3 和 MD5 显式兼容 |

NVWA 服务地址、独立认证地址和完整 MCP 地址必须为 HTTP(S)，不允许嵌入用户名、密码、查询或片段。MCP 留空或 `/mcp` 都使用服务地址的部署路径再追加 `/mcp`；其它路径也保留部署前缀后追加，完整 URL 则独立覆盖。例如占位服务地址 `https://nvwa.example.com/product` 与 `/mcp` 对应 `https://nvwa.example.com/product/mcp`，改为 `/tools/mcp` 则对应 `https://nvwa.example.com/product/tools/mcp`。保存服务地址变更后，相对路径跟随新服务地址，完整 URL 不自动改变。管理接口/后台状态返回 `resolvedMcpUrl`，用于核对实际 MCP 目标；当前 GUI 身份文字不显示此字段，返回地址也不能证明接口已启用或已连接。旧环境的完整 MCP 覆盖地址、认证头、摘要及认证模式继续保留，不因加载新版界面自动改写；旧 `application` 环境在管理员区域显示，可继续编辑和认证。

GUI 将默认显示的 `/mcp` 与原空 `mcpUrl` 统一视为默认值，保存表单及“先保存环境”检查使用同一规则，不因显示默认路径制造未保存差异。新环境默认保存空值；旧环境已保存为 `/mcp` 时保持原值，不因等价保存改成空值而撤销凭据。旧完整 URL 加载时原样显示，用户可直接修改该高级字段。

密码与应用密钥使用遮罩输入；“记住密码”和“保存应用密钥”默认不勾选，控制认证时是否交给系统保护存储。普通环境配置只保存引用，不保存密码、应用密钥或远端 token。点击“保存环境”后的刷新，只有返回的完整 profile 与本次保存快照及当前表单一致，才保留当前内存中的密码、密钥和记住选项，避免保存时吞掉尚待登录的输入；该保留不把秘密写入普通配置。

切换普通认证方式或管理员复选框会清空账号、密码、应用 ID/密钥、指定租户、登录机构、记住选项、双因子挑战、身份、工具目录及客户端检测结果，重置认证头/摘要默认值，保留环境名称和通用地址。对已保存环境同时提交包含旧 `loginAttemptId` 的取消请求，并使迟到结果失效；取消返回后保留新表单，不重新加载旧认证字段。新方式须保存并重新认证。

常用顺序如下：

1. 选择或新增环境，填写名称、NVWA 服务地址；默认账号密码方式填写账号和密码。保存环境后点击“登录 / 授权”。
2. 核对服务端返回的当前用户、稳定身份和租户。挑战、密码过期和失败都不会开放 MCP。
3. 点击“检测 MCP”，核对真实 `initialize` 和完整 `tools/list`；这一步不执行任何工具。
4. 分别检查目标客户端，核对实际路径，点击“预览接入”并确认该端目标操作。三个客户端各自显示结果，部分失败不会显示全部成功。
5. 在客户端完成原生刷新、信任及真实调用，由用户验收实际行为。

环境管理还提供刷新、删除环境、取消授权和退出登录；各客户端提供预览移除及预览恢复。网络和文件操作在后台执行，GUI 只接收当前操作及当前环境对应的结果。

登录等待期间仍可取消。每次管理登录带随机 `loginAttemptId`；取消登记有界、10 分钟的事务标记，并与登录开启按同一临界区排序，避免取消先到而旧登录后到重新开放授权。网络返回还须核对认证代次，界面不采用旧操作结果。此取消不证明已执行的远端业务被撤销。

GUI 状态区分后台是否可达、是否认证、待身份恢复核验、身份/租户、个人 token 与 MCP token 是否存在及各自有效期、Hub 检测工具数、客户端配置保存、等待客户端刷新或下一轮加载。管理接口另返回解析后的 `resolvedMcpUrl`，当前身份文字不展示此字段。未知有效期保持未知。“配置已保存”“Hub 检测通过”均不表示某个客户端已加载或已连接。Codex/WorkBuddy 的文件目标可检查、可配置并不证明应用已安装；天工缺少经核验的当前运行授权时明确不可用。

## 三种认证方式

| 方式 | 当前认证链路 | 默认 MCP 头 | 个人身份含义 |
| --- | --- | --- | --- |
| 账号密码 `password` | 动态公钥 -> `/nvwa/login` -> `/nvwa/getLoginContext` | `Authorization: <raw token>`，无 `Bearer` | 服务端完成密码及必要挑战后确认本人身份 |
| 浏览器个人授权 `browser` | 浏览器 `#/authorize` -> 本机一次性回调 -> 票据交换 -> 登录上下文 | `authorization-ticket-token: <ticket token>` | 浏览器中的实际个人授权身份；不能用打开页面代替换票和身份验证 |
| 应用代表用户 `application` | 应用签名申请一次性 ticket -> Basic 换票 -> 登录上下文 | `authorization-ticket-token: <ticket token>` | 共享应用代表填写的用户名；不是本人密码认证 |

浏览器个人授权首次仍需管理员提供真实注册的应用 ID、应用密钥及可接受的本机回调配置。Hub 没有内置共享密钥，也不将“已打开产品页面”视为认证成功。高级区可在一次授权时勾选“保存应用密钥”交给系统保护存储，后续按同环境引用使用；保存引用和浏览器身份核验分别处理。账号密码方式不显示应用注册与签名参数。

用户已确认登录 token 与 ticket token 两条链路可用，但有效期不同。内部分别保留 `personal_token` 与 `mcp_token` 及各自 expiry；password 首版使用登录 token 直接访问 MCP，browser/application 使用交换后的 ticket token。允许 profile 显式选择 `Authorization` 或 `authorization-ticket-token`，不推定两种令牌在任意部署中可互换；指定头能否匹配部署由用户核对。其他头名被拒绝，token 不放入 URL。

成功必须有 token 且 `/nvwa/getLoginContext` 返回真实 `user_id`、`identity_id`、`tenant_id` 和登录名。支持上下文 `context` 包装及 `contextUser`/历史 `conetxtUser` 字段。账号密码的指定租户留空时，登录请求发送产品默认选择值 `__default_tenant__`；返回上下文仍必须提供真实租户，不能把该占位值视为已验证身份。填写账号或指定租户与返回上下文不一致时拒绝认证。重新登录、环境修改或退出使旧客户端授权、session 和工具缓存失效，进行中请求不切换为另一人的身份。

### 账号密码与登录挑战

Hub 先获取 `/anon/framework/api/encrypt/key`，按服务端真实公钥与 alias 加密账号和密码：alias `3` 使用 UTF-8 标准 Base64 后每 50 个字符分块的 RSA PKCS#1 v1.5，各块 Base64 密文拼接；alias `2` 使用原文 UTF-8 的 SM2 `C1C3C2`。与当前产品前端一致，登录请求发送 `encryptType="3L"`，不固定公钥、不使用旧 AES 默认值，不因获取公钥失败改为明文。密码原值不裁剪空格。

成功业务状态 `0`、`200` 和有有效 token/上下文的 `203` 才可完成认证。`201` 首次登录改密、`202` 密码过期返回必须完成的产品页面操作；本版不实现 Hub 改密表单。`204` 返回双因子会话，仍未认证，不能检测或调用 MCP。仅收到此挑战后，GUI 显示验证码输入和“发送双因子验证码”按钮，session 由后台状态内部管理，不要求手填。管理 provider 使用 `/anon/nvwa-nros/v1/msg/send`；收码后第二次登录带 `extInfo.twofactorSessionId` 与 `extInfo.validCode`，继续由产品校验。发送渠道和实际到达仍待用户验收。

本轮移除 GUI 的图形验证码 ID/码输入。当前验证码获取方法、图片 schema 未从真实契约确认，**没有集成验证码图片获取或展示**。`402` 提示到 NVWA 产品页面处理，或切换到已配置的浏览器个人授权；产品页面操作不保证后续 Hub 密码登录能够完成，也不自动复用浏览器会话。provider 仍保留受限 `verifyId/verifyCode/validCode/twofactorSessionId` 契约兼容，不借附加字段覆盖用户名、加密模式或服务端密码校验设置。

账号密码错误、锁定、停用、账号过期、在线数或多终端限制及维护状态分别使用固定安全提示。所有挑战和错误均不把旧规范“0～400 都登录成功”作为判断依据。

### 应用签名与浏览器回调

应用取票使用 `authorization-cer-client`；SHA-256 为默认摘要，SM3 可选，MD5 仅显式兼容。SHA-256/SM3 按规范在签名串中分别加入 `1`/`2`，digest 为小写 hex；MD5 沿用参考连接器的大写 hex。新摘要支持取决于部署版本，服务端时间校验也要求双方时钟正确。成功取得的 ticket 通过 `/nvwa-ticket/v1/ticket/{ticketId}` 与 `authorization-client-basic: Base64(clientId:clientSecret)` 交换，Basic 材料不是 MCP token。

浏览器 URL 使用产品地址的 `#/authorize`、`response_type=code`、`client_id`、已编码 `redirect_uri` 和随机 `state`。本机回调为 `/callback/<64位hex随机state>`，仅绑定 `127.0.0.1`，事务 10 分钟、最多 64 个，必须匹配当前环境指纹与登录代次。有效回调在换票前单次消费，换票失败也不能重用；若回传 state 参数必须匹配路径，取消后晚到回调不能重新开放授权。支持单一 `code`/`ticket`/`ticketId` 票据，重复票据字段拒绝，兼容性 `tokenId` 不作为第二个票据使用。回调 code、ticket 与应用密钥不进入日志或可见错误。当前共享应用能否接受该本机回调 URL/端口、部署是否使用 Hash 路由，以及换票后 MCP 行为，均待用户真实验收；不假设标准 OAuth discovery、PKCE 或 refresh token 已存在。

### 有效期、恢复与重新认证

token expiry 只采用该 token 响应的 `expiresAtMs`，或明确 `createTime + validTime`（秒）组合；没有明确字段时为未知。一次性申请 ticket 的 `validTime=600` 不能作为交换后 token 的寿命。个人与 MCP token 分别显示，不能把某个 token 的过期推定为另一 token 也过期。

后台把认证状态和每端本地连接凭据通过系统保护存储保留；重启后恢复记录还要核对 profile 指纹及真实上下文身份，不能直接放行旧 token。运行期同身份核验缓存最长 60 秒。明确已过期或真实 `401` 后，只有此前显式保存了该方式凭据，password/application 才可在下一次请求发送前重新认证，并严格保持原 `user_id`、`identity_id`、`tenant_id`；遇验证码、双因子或改密状态需人工继续。browser 无已确认 refresh 契约，过期要求重新浏览器授权，不切换到共享应用代表用户认证。

真实 MCP 返回 `401` 使授权失效；原请求不会在取新 token 后自动重放。`403` 或网络超时不被当成同义的“重新登录即可”，业务状态和请求结果分别处理。

## 本机 HTTP 桥与 MCP 行为

桥由 Hub 后台承载，默认 `127.0.0.1:3849`，与模型网关/IM 的既有入口分开。每个 profile 和客户端都有独立随机本地连接凭据；客户端只配置 `/mcp/<profileId>/<codex|workbuddy|tiangong>` 的本机 URL 与本地 `Authorization: Bearer ...`，真实 NVWA 密码、密钥和 token 不写入客户端配置。管理接口另用受系统保护的实例凭据，并校验本机 Host 与存在时的 Origin；旧公开网关管理 API 不挂载这些 NVWA 操作。

本机管理 GET `/manage/status` 返回配置和安全状态；POST `/manage/profile/save`、`profile/delete` 使用 `expectedRevision`，登录为 `login/password`、`login/browser`、`login/application`，另有 `login/twofactor/send`、`login/cancel`、`logout`、`tools/detect`。每端使用 `client/inspect`、`client/preview`、`client/apply`、`client/remove`、`client/restore`，字段为 `profileId/clientKind/overridePath?`，实际修改还需最新 `expectedFingerprint`，恢复需当前 `backupRef`；所有操作均需要后台管理凭据。成功包装为 `{ok:true,data:...}`，失败为安全 `{ok:false,error:{code,message}}`，不返回 token/原条目。本地请求体最多 2 MiB。

NVWA 宿主采用 stateless HTTP MCP。本地桥支持 POST JSON-RPC、DELETE 本地 session；GET/独立 SSE 流不是本版传输实现。客户端 Accept 需包含 JSON 与 event stream，远端响应仍须为有效 JSON，SSE/HTML/重定向或 HTTP 200 下的协议 error 均不会被当成成功。支持协议版本 `2025-11-25`、`2025-06-18`、`2025-03-26`，初始化还核对 `serverInfo`、tools 能力及 RPC ID。

initialize 成功后给客户端本地 `Mcp-Session-Id`。session 绑定 profile、client、本地凭据、登录代次与协议；本地 session 不传给 stateless NVWA。RPC 原 ID 转为独立上游 ID，响应再映回原 ID，同一 session 的重复活动 ID 被拒绝。取消通知只映射该 session 的活动 ID，不转发可能含私有正文的 reason。DELETE 仅释放本地 session/映射，不宣称取消已经到达远端的业务写入。

限制包括最多 512 个本地 session、24 小时空闲期限、最多 256 个活动 RPC；远端响应最多 8 MiB。Hub 检测只发 `initialize`、`notifications/initialized` 与分页 `tools/list`，最多 100 页、10000 工具，并核对名称、输入 schema、cursor 循环及响应 ID；零工具目录合法，工具数未知与零分别显示，不硬编码历史数量、不调用 `tools/call`。

任何已经发送的 `tools/call` 在断连、超时、无法解析、ID 不匹配等情况下返回 `outcomeUnknown=true`、`retryable=false`，不自动重试。产品返回的工具结果和权限 error 保留业务语义，不伪造成功或提升权限。认证及桥自身错误只含安全阶段、HTTP/业务 code 和固定提示，响应 body、认证 header、密码、密钥、回调 query 不进入诊断。

## 三客户端配置与预览

受管名固定为 `nvwa-` 加 profile ID 的 SHA-256 前 16 位 hex。每个客户端使用独立本地凭据，同名条目不属于 Hub 时拒绝接管或覆盖。

| 客户端 | 实际配置范围 | 生效与证据 |
| --- | --- | --- |
| Codex | 桌面用户 home 的 `.codex/config.toml`，仅 `[mcp_servers.<受管名>]`；不盲从启动 Hub 进程的 `CODEX_HOME` | TOML 保留其他表和设置；已有会话是否加载、原生信任与真实调用待用户确认 |
| WorkBuddy | 优先 `WORKBUDDY_CONFIG_DIR`/`CODEBUDDY_CONFIG_DIR` 下 `mcp.json`，否则用户 `.workbuddy[-实例号]/mcp.json`，仅 `mcpServers.<受管名>` | JSON 保留其他根项与 MCP 条目；品牌或专享版需显式核对路径；需原生刷新或重启，并保留信任流程 |
| 天工 Claw | 经核验的官方桌面 DataServer `/data/mcp/connections` 窄 GET/POST/PUT/DELETE API；复用现有运行授权，不指定数据库文件 | 保存保持 `is_connected=0`、空工具目录；严格检测后可仅更新当前受管/未改动/启用且凭据匹配的目标目录；弱 `test/discover ok` 不当连接证明；每轮加载待用户验收 |

Windows 标准路径分别形如 `%USERPROFILE%\.codex\config.toml` 与 `%USERPROFILE%\.workbuddy\mcp.json`；macOS 为用户目录下同名文件。GUI 可以显式指定 Codex/WorkBuddy 的绝对文件路径，只允许相应文件名，拒绝符号链接与 Windows 重解析点；不能悄悄同时改全局和项目配置。天工只用原生 API，不直接读写 SQLite，也不修改运行审批。

管理 `tools/detect` 可带 `clientKind="tiangong"`：严格 Hub 目录检测通过后，仅当前拥有且启用、未被改动、本地凭据匹配的天工受管条目才更新 `is_connected/tools_json`，返回 `tiangongDirectoryUpdated`。临写前第二次读取完整连接，按同一归一化规则核对 URL、配置、状态等完整目标指纹，再次确认启用、本地 Bearer 和认证代次；DataServer 没有 CAS，最后一次读与写之间仍有外部并发窗口。该更新不接管同名用户条目、不自动启用禁用项，也不证明真实工具调用已经成功；GUI 顶部“检测 MCP”只检测 Hub 上游链路；每端“检测本机桥”传对应 clientKind，真实经过该端本机凭据、initialize/session、分页 tools/list 及本地 DELETE。天工目录未更新时单独提示，不显示客户端真实调用已通过。

检查及预览返回目标路径、受管归属、目标指纹、是否有外部修改、启用状态、最新备份引用和加载/连接状态；GUI 确认框读取嵌套预览详情、警告与备份引用，不回传原条目或本地 Bearer。实际接入、移除、恢复必须使用最新预览指纹，保存前再读目标；GUI 登录和客户端操作前核对表单与已保存环境一致，修改过的字段先保存，避免显示环境与后台实际环境不一致。外部文件侧加锁、核对完整读入文件后原子替换，保留无关项；不协作编辑器仍有竞态窗口，不能宣称跨进程原子 CAS。天工原生接口也没有 CAS，只能前后核对，失败不重放。

## 配置、凭据与备份

NVWA 不复用模型 API Key、IM 授权或普通网关渠道。普通 profile 字段为 `id/name/productBaseUrl/certificationBaseUrl/mcpUrl/authMode/clientId/username/tenant/loginUnit/mcpAuthHeader/signatureAlgorithm/credentialSecretRef`；缺失字段用兼容默认值，默认 password、SHA-256、桥端口 3849。`certificationBaseUrl` 留空回退产品地址。`mcpUrl` 接受空值、`/mcp`、其它以 `/` 开头的路径或完整 HTTP(S) URL；空值和 `/mcp` 都为默认，新环境保存空值，旧环境已存 `/mcp` 则保持原值，避免等价保存撤销凭据。路径解析保留产品部署前缀，完整 URL 独立覆盖。`resolvedMcpUrl` 是管理状态中解析后的地址。`tenant` 留空不等于已核验租户，密码请求使用产品默认选择值后仍须核对真实上下文。集合与 profile 保留未知非敏感扩展字段；递归拒绝密码、密钥、token、认证 header、cookie、票据和登录挑战值等敏感字段，扩展最多 16 层/10000 节点，不能用扩展影子覆盖标准字段。集合版本 `1`，最多 64 环境、配置 1 MiB；`_revision` 只作为管理 API 比较依据，不落盘。

以实际 Hub 主配置 `<目录>/<stem>.toml` 为作用范围，旁边派生：

| 文件或存储 | 内容 |
| --- | --- |
| `<目录>/<stem>.nvwa.json` | 环境集合、非秘密字段及 secret 引用；文件锁与原子保存 |
| `<目录>/<stem>.nvwa.lock` | 集合操作锁 |
| `<目录>/<stem>.nvwa-secrets/managed.json` 与 `managed.lock` | 定向客户端操作的归属、路径、指纹和备份引用；不包含明文 Bearer/远端 token |
| Windows：`<stem>.nvwa-secrets/<scope-key-hash>.dpapi` | 当前 Windows 用户域 DPAPI 加密值，不使用 `LOCAL_MACHINE`；原子保存 |
| macOS：用户 Keychain，service `TianCaiSpaceHub.NVWA` | 以配置 root 与 key 的 SHA-256 派生 account 保存字节；不通过命令行参数暴露秘密 |

系统存储包括显式保存的登录凭据、认证会话、本地客户端凭据、后台 GUI 实例引导和定向备份。scope hash 防止多份 Hub 主配置串用同名 key。敏感目标备份只保存待改的一个 MCP 条目前像，按 32 KiB 块交给 SecretStore 保护；不备份整份第三方配置。普通元数据仅持有引用与指纹。

写入前登记操作恢复记录，写后只读核对实际目标。写入响应不明确时不重放，读取确认一致才报告配置保存。元数据提交失败，只在目标仍等于本次写入结果时补偿还原；并发变更则保留记录并要求人工核对，不能覆盖新修改。

## 移除、退出、迁移与回滚

“预览移除”只操作选中的客户端和该 profile 受管条目，并撤销该端本地授权/session；恢复也撤销该端当前凭据，其他客户端独立处理。“退出登录”使该环境授权、本地 session、工具缓存、系统保护的认证会话和在途浏览器事务失效；远端 logout 尽力执行并单独返回是否确认，不代表远端业务写入回滚。已保存登录凭据仍按用户记住选项保留，客户端配置仍需分别移除。存在未移除受管条目时拒绝删除环境；成功删除才清除其凭据引用和认证记录，不以删 profile 掩盖配置移除失败。修改已保存环境的字段会撤销旧认证及其保存凭据，需重新核对/认证。

“预览恢复”只允许当前受管目标的最新 `backupRef`，同时核对最新预览指纹及备份记录的写后指纹。仅恢复该条目并保留其他设置；用户已修改的目标不能自动还原。移除前也保留系统保护的定向备份，恢复结果仍需按原生刷新/信任方式生效。

桥端口改变需要后台重新绑定并重新核对/接入各客户端保存的 URL，不能把旧地址仍然当作可用入口。配置 root 改名、复制到其他 Windows 用户或电脑、回滚到不认识 NVWA 的旧版本，均不能推定 DPAPI/Keychain 引用会跟随恢复。回滚前先通过本版定向移除或恢复所需受管条目、退出授权，再保存非秘密 profile；旧版不负责加载本机桥，新版存储不并入旧模型配置。

本轮新增的 MCP 路径值不被此前仅接受完整 URL 的 NVWA 版本理解。回退此路径功能前，应将已存的 `/mcp` 或其它路径解析为明确的完整 HTTP(S) MCP 地址并保存，再按原流程处理授权和客户端条目；默认空值继续兼容。旧完整覆盖地址不需要自动改写。

## 维护定位与验证

| 入口 | 职责 |
| --- | --- |
| [GUI](../../src/gui/nvwa.rs) | 基础/高级分区、按方式显隐、认证切换清空与取消、保存后保留一致的内存输入、204 挑战及每端预览；后台结果代次过滤 |
| [服务生命周期](../../src/nvwa/mod.rs)、[管理 API](../../src/nvwa/server.rs) | 后台桥、受保护管理引导、profile 管理、浏览器事务、认证恢复和定向操作 |
| [类型](../../src/nvwa/types.rs)、[配置](../../src/nvwa/config.rs) | 非秘密 DTO、默认值、URL 校验、集合 revision 和原子保存 |
| [认证](../../src/nvwa/auth.rs)、[系统存储](../../src/nvwa/secrets.rs) | 动态 RSA/SM2、取票/换票、身份与 TTL、Windows/macOS 平台实现 |
| [桥](../../src/nvwa/bridge.rs)、[运行态](../../src/nvwa/runtime.rs) | RPC/session/ID 隔离、工具检测、未知结果及禁止重放 |
| [适配器](../../src/nvwa/adapters/mod.rs)、[桌面窄 API](../../src/gmclaw_desktop.rs) | 定向配置、指纹、保护备份、天工原生接口 |

2026-10-10 完成界面简化、默认地址/租户请求、认证切换清空、保存输入保护及可编辑 MCP 路径的源码与文档调整。前阶段 Windows locked GUI check 通过（27.23 秒、38 条警告）；最终 locked GUI build 通过（17.55 秒、38 条警告），包括浏览器限定账号和最新 MCP 路径。版本仍 `0.4.30-5`，只维护 `main`，源码指纹及最终产物身份见 [v5 交付](../releases/v0.4.30-5.md)。macOS 原生编译待 GitHub Actions。未执行测试、未启动 Hub/Codex/WorkBuddy/天工/NVWA、未访问真实认证/MCP/模型接口、未读取实际用户配置/凭据/私有日志或业务数据库。本地不生成发布安装包；后续 Windows/macOS 包由 GitHub Actions 生成，源码推送、构建、用户验收和发布分别记录。

2026-10-08 的 Windows x64 debug EXE 保留在本机 `outputs/nvwa-v0.4.30-5-windows/TianCaiSpaceHub.exe`，其版本/哈希/PE 导入核对只属于当时源码，不能验收本轮界面。本轮新 EXE 已独立复制至 `outputs/nvwa-v0.4.30-5-windows-ui-20261010/TianCaiSpaceHub.exe`，60,940,800 字节；完整版本、x64 PE、DLL 导入及源/复制文件哈希静态核对通过，未运行，不覆盖此前文件。实际哈希与源码身份见 [v5 交付](../releases/v0.4.30-5.md#本机-windows-测试程序)。测试时先从托盘正常退出旧 Hub，再双击新 EXE，× 只隐藏旧窗口；本机测试 EXE 与 Actions 发布安装包分别记录，实际界面/登录/客户端行为仍待用户验收。

用户验收重点为基础/高级显示、默认 `/mcp` 可编辑与表单一致性、自定义路径/完整覆盖及服务地址变化、认证切换清空和迟到响应、保存环境保留当前内存输入、动态 RSA/SM2、本人密码与默认/指定真实租户、402 产品页面提示、204 双因子、201/202 改密、浏览器首次应用配置/系统保护引用、两 token 实际有效期、注册地址与晚到/重放 callback、后台恢复时同一身份、本机凭据端隔离、分页工具目录与权限失败、各端原生刷新/信任、并发目标保护、移除/恢复及写调用断连后的未知结果。以上是待验收范围，不是开发方已执行的测试清单；图形验证码图片接口仍未集成。
