# 天工 AI Gateway 本地账号与历史会话

维护日期：2026-11-16；关联 TC-024、TC-011/012/016。本轮适配 Windows 已安装 `tiangong-desktop 1.1.0` 的真实程序资源，源码已实现，Node 隔离检查通过；Rust 集成检查及新包由管理统一记录，实机待用户验收。下文 V1.0.31 的构建、检查和测试包只证明原 1.0.4 输入，不代替本轮结果。

## Work 1.1.0 适配

1.1.0 的 `tasks`、`scenarios` 新增 `owner_id`；DataServer 对会话接口核验当前账号及 `X-Tiangong-Account`。renderer 增加 `conversationAccount`／`conversationEpoch` 和 `workspaceOwner`，官方账号变化会清理实例及恢复缓存。仅去掉登录页面或改请求头不能提供本地工作区。

增强启动由 Hub 的主进程内存适配与 renderer 内存适配配合：本地会话身份固定 `ai-gateway`，官方登录、令牌及云端资源账号保持真实状态。renderer 的列表、工作区初始化、通知、恢复、草稿、权限键和 Work 更新使用本地身份；可选登录及登录失效不会切换已有本地会话。云端智能体目录同步单独按官方账号隔离。本轮识别原生启动壳新增的 `import(...).then(...)` 导出形式，启动壳原样放行，只适配它指向的唯一主脚本。新版 `chatRunOpen` 优先经 IPC 发起执行，增强加载的源码为 `runFetch` 提供协助传输钩子，不改写冻结的 `electronAPI`。

新建任务保留原生稳定 `task_id`／`session_id`，显式登记本地会话作用域；重试复用任务，登记失败不继续生成，也不删除可能已有内容的任务。续聊从原生 session 读取并保留 `agent_id`／`project_id`；请求的项目 ID 同步该原始身份（`default` 对应原生项目对象 ID `0`），目录和名称仍沿真实任务。模型动态刷新保留正在执行的选择；历史模型缺失时要求明确选模，不改成目录首项。

1.1.0 历史迁移同步选中 session 的 `user_id`、关联 task 的 `owner_id` 及已完整同步的终态 `work_task_runs.owner`。非 `default` 项目复制必要的项目条目并赋予 AI Gateway 归属，工作目录共用；选中任务显示在该副本，原项目、其他账号及未选任务不变。原 session／task／agent／model ID、原始记忆 `project_id`、消息、附件和运行请求／事件正文保留。恢复将任务带回记录中的原项目、原 owner；用户已修改的项目归属或任务关联不覆盖，空项目副本保留。

原生 `task_work_dir` 优先用于默认分组任务的实际工作目录，防止将独立任务上下文错当成工作区根目录。未同步的 Work 运行、未结束／排队／待审批／finalizing 状态及 `conversation_heads.pending` 编辑均拒绝迁移；只定向检查所选任务和已知关联，不重放运行。数据库缺少成对 owner 字段或未知 `user_id` 任务结构时报告不兼容，不自行升级原生库。

撤销记录新写 `version=2`，含所选任务的原 owner、原／目标项目条目 ID；当前版本兼容读取旧 `version=1`。旧 Hub 将明确拒绝新记录，避免只还原 session 而遗漏 task。升级后读到旧记录可继续恢复默认分组任务；旧记录缺少非默认项目来源时不猜测恢复位置。回退到 V1.0.31 或更早版本前，应先用当前 Hub 完成需要的恢复。

实物还包括按官方账号缓存云端智能体可用信息的 `resource_usage_agents(owner,ref,...)`，及账号目录中的 `agent-preferences.json`；`/data/web-resources/agents/{query,detail,enabled}` 仍使用真实云端授权。它们不是可转让的本地 agent 定义，历史迁移不改这些元数据或官方智能体权限，仅保留选中会话原 `agent_id`。本轮没有读取用户数据库、账号偏好或云端资料。

桌面 IM／远程消息同步从原 App 闭包读取 `workspaceOwner`，请求携带同一账号，读取期间切换账号则不写入界面；保留消息的工具执行、用量、错误及 response 元数据。旧增强工作区没有原生 owner Ref 时按 AI Gateway 的 session 关联过滤列表，避免刷新时显示其他归属。

## 入口与日常使用

天工接入设置增加“会话历史管理”，查看全部本地会话、AI Gateway 或其他归属，可按 Ctrl／Shift 多选后“归入 AI Gateway”或“恢复原归属”。查看只读数据，不启动天工；归属调整要求完全退出天工（包括托盘），并在 IM 先退出选中的当前会话。选择和按钮构成明确操作，旧历史不自动批量迁移。

增强工作区使用本地 `user_id="ai-gateway"`，前台未登录身份显示 AI Gateway，不伪造官方登录或令牌。新会话、运行、列表过滤、最近任务恢复和记忆统一使用这一作用域；普通启动保持天工原行为，官方登录仍代表原账号。原 `agent_id` 是执行智能体，不能当作用户字段覆盖。已打开的旧 V1 增强窗口不能直接热切归属，需要正常退出后重新增强启动。

兼容原 1.0.4 的 tasks／scenarios 无账号列结构，增强列表按关联 session 的真实归属过滤；1.1.0 按原生任务／项目 owner 列处理。未登记归属的旧条目在管理中单独说明，不猜测账号。其他未知新版的自建智能体定义和云端权限不自动认定兼容。

项目壳继续共享，场景内任务、搜索和恢复按 session owner 过滤并支持分页。原生整组删除项目会递归删除整个 `.tiangong`，因此增强模式提示分别删除可见任务，避免删掉其他归属的上下文；普通启动保持天工原操作。

## 数据与迁移

使用 `GMCLAW_CONFIG_PATH` 或原生 `electron-data.db`，不创建天工数据库、不重建表。任务通过原 session／scenario 关联取得真实工作目录，默认工作目录沿原生库目录的 `workspace`；归属未知、目录冲突、运行／排队／审批／状态未知的记录不能冒充可迁移。

迁移保留原 session／task／agent／记忆 project／model ID、消息及压缩记录和附件引用，只修改选中会话的用户归属及对应索引、已知任务与运行归属；1.1.0 的项目显示副本按上节处理。必要运行上下文在 `<工作目录>/.tiangong/memory/<user>/memory/<project>/session-<id>.jsonl`，已有 `session-` 前缀只使用一次，兼容原 `runtime/session-<id>.json`。非空会话缺少主 JSONL 时整批拒绝，数据库消息索引不能重建正文。公共图片保留于 `<工作目录>/.tiangong/memory/images/`，原绝对路径引用不变，不整体搬走其他会话的共享 MEMORY 或用户文件。

Hub 的 `<config stem>.gmclaw-sessions.json` 中选中会话 `context.user_id` 同步更新，其他关联和回复来源 ID 保留。IM 编辑门与现有 LINK_IO 保护写入期间的创建、恢复、执行及关联保存；成功后清除选中会话旧历史缓存，后续恢复读取真实元数据。进程发现失败不当作天工已退出，Hub 启动锁持续到实际写入完成；关闭请求或窗口不会在后台写入尚未结束时提前允许启动。

撤销记录保存于 Hub 自有状态目录的 `tiangong-history/ownership-moves.json`，只保存原归属、项目身份和本次必要文件摘要，不复制包含模型密钥的整个数据库。源上下文保留，在 AI Gateway 目录创建必要副本。恢复原归属带回迁移后新增消息和运行上下文；遇两处同名上下文各自续写、无法合并时整批拒绝。未改的自有会话及旧运行副本可清理；有更新的 AI Gateway 副本和共享记忆保留并提示。只在选中变更时检查必要文件，不在列表中读取正文或遍历哈希。

## 接口与维护

可信本机 POST：`/api/gmclaw/history/read`、`/move`、`/restore`。读取返回 `entries`；变更请求为 `{sessions:[{sessionId,userId}]}`，其中 userId 是列表读取到的当前归属，写入前复核。结果返回 `changed`、`backupPath` 和 `warnings`，不返回凭据。归属检查按选中记录完成，不增加全树哈希或全库备份。

维护入口：`src/tiangong_session_history.rs`、`src/gmclaw_im/history.rs`、`src/gmclaw_runtime/{workspace.rs,workspace-adapter.js}`、`src/{web,gui}/tiangong_history.rs`。回退前先将需在旧固定 local 身份继续的会话恢复原归属，再退出天工改用普通启动；仅回退 Hub 不自动改写已迁移的本地会话。

## 验证

2026-11-16 本轮通过 `node scripts/test-tiangong-workspace.cjs --renderer <只读提取的原生启动脚本>` 的 35 项检查：包含旧结构 28 项、1.1.0 原启动壳／约 3.5 MB 主脚本适配后解析，以及真实脚本中抽出的任务登记、归属、模型、请求头、登录生命周期和 IPC 协助包装 6 项隔离检查。`node scripts/test-tiangong-display.cjs` 的 27 项通过，覆盖官方／本地账号请求头、切换不回写、旧列表隔离和工具／用量元数据保留。管理统一执行 `tiangong_session_history::` 24/24 通过，包含新增 7 项合成 SQLite／临时目录用例，Windows GUI 测试目标编译通过。没有启动 Hub／天工、安装器或真实模型调用，Windows 实际登录切换、历史续聊、附件与双端同步仍待用户验收；macOS 无新版实物样本，未原生验收。

V1.0.31 历史结果：Windows GUI 测试目标编译及 61 项不同 Rust／29 项 Node 检查通过，包含 17 项历史后端、32 项 IM、10 项工作区以及管理窗口／可信路由各 1 项 Rust。仅使用合成 SQLite、临时目录、虚构会话／任务和安装程序静态资源；未读取用户会话库、账号、日志或执行真实请求。该版本最终构建和唯一包身份见 [V31 交付](../releases/v1.0.31.md)。
