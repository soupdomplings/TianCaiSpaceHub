# 天工 AI Gateway 本地账号与历史会话

维护日期：2026-11-12；适用 V1.0.31，关联 TC-024、TC-011/012/016。状态：实现已集成，Windows GUI／release、必要合成检查及唯一 MSI 静态核验通过，实机待用户验收。用户已明确允许按当前天工先实施，不等尚未发布的账号隔离版；此前 V30 评估保留原阶段归属。

## 入口与日常使用

天工接入设置增加“会话历史管理”，查看全部本地会话、AI Gateway 或其他归属，可按 Ctrl／Shift 多选后“归入 AI Gateway”或“恢复原归属”。查看只读数据，不启动天工；归属调整要求完全退出天工（包括托盘），并在 IM 先退出选中的当前会话。选择和按钮构成明确操作，旧历史不自动批量迁移。

增强工作区使用本地 `user_id="ai-gateway"`，前台未登录身份显示 AI Gateway，不伪造官方登录或令牌。新会话、运行、列表过滤、最近任务恢复和记忆统一使用这一作用域；普通启动保持天工原行为，官方登录仍代表原账号。原 `agent_id` 是执行智能体，不能当作用户字段覆盖。已打开的旧 V1 增强窗口不能直接热切归属，需要正常退出后重新增强启动。

当前 1.0.4 的 tasks／scenarios 无账号列，增强列表需要按关联 session 的真实归属过滤；未登记归属的旧条目在管理中单独说明，不猜测账号。未知新版的自建智能体定义／云端权限仍须升级后适配，本版只保留已知执行智能体身份与本地会话，不声称未来版本已兼容。

项目壳继续共享，场景内任务、搜索和恢复按 session owner 过滤并支持分页。原生整组删除项目会递归删除整个 `.tiangong`，因此增强模式提示分别删除可见任务，避免删掉其他归属的上下文；普通启动保持天工原操作。

## 数据与迁移

使用 `GMCLAW_CONFIG_PATH` 或原生 `electron-data.db`，不创建天工数据库、不重建表。任务通过原 session／scenario 关联取得真实工作目录，默认工作目录沿原生库目录的 `workspace`；归属未知、目录冲突、运行／排队／审批／状态未知的记录不能冒充可迁移。

迁移保留原 session／task／agent／project／model ID、消息及压缩记录和附件引用，只修改选中会话的用户归属及对应会话作用域索引。必要运行上下文在 `<工作目录>/.tiangong/memory/<user>/memory/<project>/session-<id>.jsonl`，已有 `session-` 前缀只使用一次，兼容原 `runtime/session-<id>.json`。非空会话缺少主 JSONL 时整批拒绝，数据库消息索引不能重建正文。公共图片保留于 `<工作目录>/.tiangong/memory/images/`，原绝对路径引用不变，不整体搬走其他会话的共享 MEMORY 或用户文件。

Hub 的 `<config stem>.gmclaw-sessions.json` 中选中会话 `context.user_id` 同步更新，其他关联和回复来源 ID 保留。IM 编辑门与现有 LINK_IO 保护写入期间的创建、恢复、执行及关联保存；成功后清除选中会话旧历史缓存，后续恢复读取真实元数据。进程发现失败不当作天工已退出，Hub 启动锁持续到实际写入完成；关闭请求或窗口不会在后台写入尚未结束时提前允许启动。

撤销记录保存于 Hub 自有状态目录的 `tiangong-history/ownership-moves.json`，只保存原归属、项目身份和本次必要文件摘要，不复制包含模型密钥的整个数据库。源上下文保留，在 AI Gateway 目录创建必要副本。恢复原归属带回迁移后新增消息和运行上下文；遇两处同名上下文各自续写、无法合并时整批拒绝。未改的自有会话及旧运行副本可清理；有更新的 AI Gateway 副本和共享记忆保留并提示。只在选中变更时检查必要文件，不在列表中读取正文或遍历哈希。

## 接口与维护

可信本机 POST：`/api/gmclaw/history/read`、`/move`、`/restore`。读取返回 `entries`；变更请求为 `{sessions:[{sessionId,userId}]}`，其中 userId 是列表读取到的当前归属，写入前复核。结果返回 `changed`、`backupPath` 和 `warnings`，不返回凭据。归属检查按选中记录完成，不增加全树哈希或全库备份。

维护入口：`src/tiangong_session_history.rs`、`src/gmclaw_im/history.rs`、`src/gmclaw_runtime/{workspace.rs,workspace-adapter.js}`、`src/{web,gui}/tiangong_history.rs`。回退前先将需在旧固定 local 身份继续的会话恢复原归属，再退出天工改用普通启动；仅回退 Hub 不自动改写已迁移的本地会话。

## 验证

Windows GUI 测试目标编译及 61 项不同 Rust／29 项 Node 检查通过，包含 17 项历史后端、32 项 IM、10 项工作区以及管理窗口／可信路由各 1 项 Rust。仅使用合成 SQLite、临时目录、虚构会话／任务和安装程序静态资源；未读取用户会话库、账号、日志或执行真实请求。最终构建和唯一包身份见 [V31 交付](../releases/v1.0.31.md)；Windows 实际显示、迁移后续聊、IM 和 macOS 待验。
