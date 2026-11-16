# 远程会话首版协作契约

维护日期：2026-11-15。状态：源码集成及 Windows 离线检查完成，尚未实机验收。当前源码基线 `7c6d3d5` / 1.0.31；本批不发版或打包。最终检查范围见[集成验证](../customizations/remote-connection.md#验证记录)。

## 分工与边界

- 管理：原生导航和设置页、AppState/main/web 集成、Cargo 依赖、静态资源嵌入、CI 与文档索引。
- 网络：`src/remote_network/`，EasyTier 2.6.4 准备和生命周期、HTTPS、设备配对与撤销、本地浏览器代理。
- 会话：`src/remote_sessions/` 及必要 Codex 事件分流，共用原运行端的会话、事件、附件和审批。
- 网页：`web-remote/` 源码及 `assets/remote-web/` 构建资源，两套客户端视图及离线检查。
- 各开发方不独立打包、不提交或发布、不启动真实 Hub/客户端/模型；同文件修改先交接。原有未提交文档变动保留。

## Rust 集成

- `AppState.remote_network: remote_network::NetworkState` 和 `AppState.remote_sessions: remote_sessions::SessionService` 均支持 `Default`，由管理加入。
- 网络公开 `management_router() -> Router<SharedState>`、`startup(SharedState) -> anyhow::Result<()>`、`shutdown(&SharedState)`。
- 会话公开 `router() -> Router<SharedState>`，其中路由相对 `/api/remote/v1`；仅由隔离网关承载，不合入现有管理 Router。
- 网页公开 `remote_frontend::router() -> Router<SharedState>`，管理负责嵌入资源；主页 `/remote/`，资源 `/remote/assets/...`。
- 浏览器本地网关提供下述 API；`X-Hub-Target` 标识配对目标。设备凭据仅在 Hub 代理持有，SSE 使用 fetch 读取以支持目标 header。

## 会话 API（camelCase JSON）

统一错误 `{error:{code,message}}`，正文包含可读原因。`clientId` 是稳定运行端标识，`kind` 是 `codex` / `tiangong`。

| 方法与路径 | 请求／响应 |
| --- | --- |
| GET `/devices` | 网络提供 `{devices:[{id,name,online,paired,connectionType}],localDeviceId}` |
| GET `/clients` | `{clients:[{id,kind,name,ready,reason?,capabilities,models:[]}]}` |
| GET `/clients/{clientId}/projects?cursor=&limit=` | `{projects:[{id,name,path}],nextCursor?}`；客户端按项目 id 合并多页 |
| GET `/clients/{clientId}/sessions?cursor=&limit=` | `{sessions:[{id,title,projectId?,cwd?,updatedAt,status,model?}],nextCursor?}` |
| POST `/clients/{clientId}/sessions` | `{title?,projectId?,cwd?,model?}` → `{session}` |
| GET `/clients/{clientId}/sessions/{sessionId}/history?cursor=&limit=` | `{items:[],nextCursor?,turnId?,status,approvals:[],cursor}`；items 按时间正序 |
| POST `/clients/{clientId}/sessions/{sessionId}/turns` | `{text,attachmentIds:[],model?,effort?,requestId}` → `{turnId,status}` |
| POST `/clients/{clientId}/sessions/{sessionId}/stop` | `{turnId?}` → `{status}`；调用原生停止，不能以断订阅冒充 |
| POST `/clients/{clientId}/sessions/{sessionId}/approvals/{approvalId}` | `{decision:accept\|decline\|cancel\|option:N,answers?:{}}` → `{status}`；优先使用返回 choices 的 value |
| GET `/clients/{clientId}/sessions/{sessionId}/events?after=` | SSE `id` 为递增数字；每条 `data` 为下述事件 JSON |
| POST `/clients/{clientId}/sessions/{sessionId}/files` | 原始文件字节；`X-File-Name` 为 URL 编码文件名，Content-Type 原 MIME → `{file}` |
| GET `/clients/{clientId}/sessions/{sessionId}/files/{fileId}?download=1` | 已关联文件的原始内容；不接收任意主机路径 |
| DELETE `/clients/{clientId}/sessions/{sessionId}/files/{fileId}` | 仅移除本 Hub 上传且尚未发送的附件；已发送附件及原生产物保留 |

`RemoteItem`: `{id,role:user|assistant|tool|system,type:message|tool|fileChange|reasoning,text,status?,files?:[],data?:{}}`。

`RemoteFile`: `{id,name,size,mimeType,url?,kind?:attachment|artifact}`；所有 url 为该会话下 API 相对路径。

`RemoteApproval`: `{id,type:approval|question,title,message?,choices?:[{value,label}],questions?:[{id,prompt,options?:[{value,label}],required?}]}`。

能力键：`history,create,send,stop,approvals,questions,attachments,artifacts,modelSelection,effortSelection`（布尔值）。模型 `{id,name,efforts?:string[]}`，只采用运行端真实目录。

`RemoteEvent`: `{id,type,clientId,sessionId,turnId?,itemId?,data}`；type：`message.delta`（data.text 增量、role/type 指明内容种类）、`item.updated`（data.item 完整项，data.completed 标明原生终项）、`turn.updated`（data.status/turnId）、`approval.requested`（data.approval）、`approval.resolved`（data.id）、`reset`（客户端重新获取快照）、`error`（data.code/message）。不自动重放发送请求。

历史响应的事件 cursor 在开始读取原生内容前取得，避免跳过读取期间到达的事件。原生增量没有统一字符偏移，因此网页将快照中已有的 item id 作为基线：对这些项不直接重加 delta，而是合并刷新原生绝对内容；新项继续实时追加，原生终项直接替换。后台刷新保留期间收到的完整项、实时游标和已分页历史，不重新建立 SSE。发送和审批在接纳后由目标 Hub 独立任务执行，浏览器连接中断不取消执行或丢失结果记账。

## 管理 API

网络模块在现有受信本地管理路由增加 `/api/remote-network`：GET `/status`；POST `/settings`（必填 enabled，及可选 deviceName、relayUrl）、`/prepare`（仅显式 confirmed:true）、`/uninstall`、`/create`、`/join`（code）、`/pair/approve`（requestId）、`/pair/reject`（requestId）、`/revoke`（deviceId）、`/open`（deviceId?，返回 url）。状态返回 enabled/runtimeReady/runtimeInstalled/runtimeVersion/networkReady/connected/networkName/deviceName/deviceId/relayUrl/devices/pendingPairings/pairing/browserUrl/lastError；create 另返回 code。

`runtimeReady` 是自管安装完整，`runtimeInstalled` 是可识别的 Hub 安装记录，`networkReady` 是组件进程就绪，`connected` 是实际节点连接。设备的 online 和 connectionType 来自运行组件的真实路由，不能从已安装或已配对推断。`browserUrl` 仅为本机基址，必须通过 `/open` 获取带入口票据的地址；票据在浏览器换成 HttpOnly Cookie 后从网址移除。

停用不卸载。真正退出关闭自管组件；远程附件不属于搭档临时目录。已加入网络不等于已获会话权限；目标端确认一次后持久授权，可撤销。
