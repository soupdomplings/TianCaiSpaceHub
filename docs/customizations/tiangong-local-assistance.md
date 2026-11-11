# 天工 Work 本机协助

维护：2026-11-11；TC-022。当前正文说明 V1.0.17 后的“共享天才搭档 Server”开发批次：与天才搭档共用一份持久运行包，真人确认准备、品牌程序和退出清理已实现，现已纳入 [V1.0.18 唯一测试包](../releases/v1.0.18.md)，未正式发布，真实启动待用户验收。本批必要编译和离线结果由管理会话汇总。用户确认 V1.0.16 的天工已能通过协助 MCP 实际工作；V1.0.17 的 104 项定向离线回归、Windows release 和 MSI 核验仅归属 [V17 交付](../releases/v1.0.17.md)，此前 1354 个不同 Rust 用例和 14 项 Node 检查仅归属 [V16 统一交付](../releases/v1.0.16.md)，不作为本批新代码证据。

## 入口、默认值与使用

已移除旧 WindowsApps 缓存路径中按每文件多次读哈希的实现；已有程序只在真人确认准备时复制一次。下载提供摘要时沿下载流核对一次；已安装运行包只检查记录、清单和固定配套组件存在性，不全树哈希、不启动版本探针，状态轮询不重复枚举 PATH/WindowsApps。配置保存、后改保护及凭据边界保持。

Hub 内置 MCP、自动协作规则、独立 Codex 后台和天工反馈提示层。用户仍在天工完成主任务，不打开 Codex 窗口或发送到用户正在使用的 Codex 对话。

入口：**天工 Work → 接入设置 → 本机协助**。默认关闭；允许修改项目、完全访问默认关闭。旧 `autoClear` 字段兼容保留但不再控制完成后删除，界面已移除该开关；临时内容保留到主动清理或 Hub 真正退出。

1. 在 Hub 的 ChatGPT/Codex 可见模型中接入普通来源模型；获准分享的天工模型也可先加入此清单。模型接口、Codex 执行程序与天工主模型是不同角色。
2. 从 Hub 启动天工，沿用现有安装和本机通道核验。原实例未启通道时，核对任务后正常退出天工，再从 Hub 启动一次。
3. 启用并选择模型、思考强度后保存。选项来自该普通来源的模型能力目录，支持时默认 `xhigh`；否则用目录默认，没有已知推理能力则不发送强度。执行端是共享 **天才搭档 Server** 的 app-server。启用保存只检查；缺失或不兼容时界面要求本人确认，确认后优先从已有兼容 CLI 复制 Hub 自管品牌包，没有可用来源才从官方下载安装。不准备和启动未启用的功能，不因保存或后台任务静默安装。可选择已有程序作为准备来源，或恢复自动查找；不会直接执行用户选中的外部原程序。共享包就绪后跨功能、跨任务长期复用，不要求安装 ChatGPT 桌面端或 Node/npm。
4. 保存通过官方本机 API 登记 MCP，Hub 严格完成本机握手、五工具目录验证、写回读回及重载。新任务核对原生任务范围内的工具目录，再提供完整参数、真实上下文及 `mcp_call` 示例。新建天工任务生效，不强行插入已提交任务，不要求手选 Skill。
5. 天工按规则判断同方法两次失败、缺少关键能力或关键验证失败，交接目标、必要证据、已试方法和用户限制；自动等待、有限追问，采纳后继续原任务。

自动求助依赖主模型理解规则并调用工具，未证明每个模型必然识别所有困难。规则明确运行中及等待人类时都继续查询同一任务，用户回答后继续等待结果；若主模型仍提前结束，提示层保留反馈，但不强行重启已结束的主轮次。配置成功不能当作真实调用验收。

### 未触发或缺少 context_ref

用户实测前包未触发，随后报告天工已尝试调用但缺少必填 `context_ref`。本轮静态确认天工只暴露统一的 `mcp_call`，并且 `orch_extra_system_prompt` 非空时跳过原生 MCP 目录渲染。前包只追加协作规则及上下文文字，未补齐完整目录和实际调用示例；离线用例未覆盖这一真实组合，不能把此前构建通过当作互操作通过。

现在按原任务的 agent、user、project 读取官方本机 `/data/mcp/catalog/<agent>`，保留本轮选择范围内的已启用连接及工具参数，验证本机协助五工具 schema 完整，再合并原规则。模型收到 `mcp_id`、`tool_name`、`arguments` 的实际调用格式，每次的 `arguments.context_ref` 都使用当前任务随机值，不猜测、不取“最近任务”、不让用户手填。认证、服务器地址和原始连接配置不进入系统提示。

设置状态分别显示“等待新任务”“最近任务已附带规则”“已发起协助”及具体参数错误；它们是对应环节的证据，不等于上游或真实协助成功。工具目录读取失败、任务不兼容、明确选择其他 MCP 或明确排除 `mcp_call` 时保留原任务且提示原因；无协助任务时也能看到接入失败提示。旧任务需新建一轮，安装新包不会重放原请求或偷偷放宽连接范围。修复后的真实触发和参数传递仍待用户验收。

### CLI 启动与受管 MCP 维护

用户曾反馈“无法启动 Codex 协助后台”，旧提示隐藏了操作系统错误，不能据此认定是安全软件。确认准备时自动查找原生程序、npm 原生包、`%LOCALAPPDATA%/Programs/OpenAI/Codex/bin/codex.exe` 和桌面资源程序，拒绝同名桌面窗口 shell。系统程序仅是复制来源，不再在任务启动时走 WindowsApps 特殊缓存。原安装目录、注册表、权限及用户日常 Codex 配置保持；实际创建进程失败仍显示系统错误码及程序位置。

V1.0.21 修正共享准备的 HTTP 409“Codex CLI 程序不存在”：天工协助保存的旧来源已被 Store 更新删除时，不再阻断新版来源发现；已有共享包仍优先直接复用，来源重新发现只在本人确认准备后执行。仍存在但不兼容的明确选择继续返回组件或架构原因。Windows 读取当前用户登记的 Codex/ChatGPT 安装位置并按数字版本选候选，登记不可用再查已知 WindowsApps；补 npm `vendor/<target>/bin/codex.exe` 新布局与旧布局，配套搜索程序位置消除自己生成的 `..`。没有修改或执行官方原安装，不增加轮询扫描或反复哈希。天才搭档未就绪后的准备确认和取消约定见 [搭档修正](genius-buddy.md#v1021-运行端来源与准备提示修正)，最终编译、定向回归及唯一包见 [V21 交付](../releases/v1.0.21.md)。

### 共享持久 Server 与准备确认

天工本机协助与天才搭档使用独立功能开关，共用 `desktop::state_root(config_path)/companion-server/` 内的一份运行包和 `current.json`。两功能均关闭时不能准备；任一启用后，检查和设置保存不联网、不复制、不启动。界面真人确认后才提交 `prepare {confirmed:true}`；缺文件或不兼容需要重新确认。后台任务不能代替人确认，也不会自动安装。并发准备合并在共享锁内，后到者复用已准备结果。

没有兼容本机来源时，读取 `openai/codex` 官方最新稳定 Release，只选 Windows/macOS x64/Arm64 的 `codex-package-<target>.tar.gz`，同时保留该版本 LICENSE/NOTICE。下载、解包在后台进行，不执行远程脚本、不改系统 PATH/安装项或用户认证；有官方 SHA-256 时只在下载流中核对一次。失败不切换旧 `current.json`。

完整包复制或解包后保留 `bin/`、`codex-resources/`、`codex-path/`、`codex-package.json` 和其他资源；轻量核对目标架构、Code Mode 主机、rg、Windows runner/sandbox setup。真实旧清单不含 `execServer` 时兼容；若声明则必须存在。仅有单个 exe 不能视为兼容来源：旧平铺 CLI 须同目录有 Code Mode 和 Windows 必要 helper，并能定位配套 rg，才复制已知程序族并生成自管布局；否则自动来源继续找完整包或转官方准备，手选不兼容来源报明原因。已有源还读取 PE/Mach-O 架构，不执行 `--version`。

实际主程序为 `bin/天才搭档 Server.exe`（macOS 为 `bin/天才搭档 Server`）；Windows FileDescription/ProductName 为“天才搭档 Server”，仅改 Hub 副本元数据，保留原版本、版权、原 exe alias 及上游清单。upstream install-context 在 `bin/` 下按父包清单发现 resources/path，启动命令同时把包内 `codex-path`/`bin` 前置到子进程 PATH，保留 helper 发现关系。并非仅重命名后宣称运行通过：离线夹具覆盖品牌元数据读回、旧清单、伴随资源和命令环境，真实首次初始化及各 helper 行为仍待用户验收。

持久包在 `companion-server/packages/<目录标识>/`；`hub-provenance.json` 登记官方项目、版本/来源、原入口、品牌修改和许可引用，`hub-notices/` 保存许可、NOTICE、修改说明。旧天工 `local-assistance/runtime/` 只作为确认后的迁移来源，原目录保留。跨任务不反复检查最新版本。

**移除安装**是独立明确操作：先停用天工协助和天才搭档、确认无活跃后台，再由本人确认调用 `/assistance/uninstall`。启停、准备与移除串行协调；只删除 `companion-server/packages/` 内有 Hub 归属记录的包及 Hub `current/owner` 记录，未知目录、系统来源、旧未迁移包和功能设置保留。以后再次使用需要重新确认准备。

安装布局与环境依据 [官方 CLI 文档](https://learn.chatgpt.com/docs/codex/cli)、[环境变量](https://learn.chatgpt.com/docs/config-file/environment-variables)、[Windows 部署文档](https://learn.chatgpt.com/docs/enterprise/windows-deployment) 及核对时的官方 [`layout.py`](https://github.com/openai/codex/blob/rust-v0.167.0/scripts/codex_package/layout.py)、[`archive.py`](https://github.com/openai/codex/blob/rust-v0.167.0/scripts/codex_package/archive.py)。实际下载使用最新稳定 Release，并按清单核对兼容布局。

### 受管 MCP 目录维护

本机协助启用期间，除建立窗口连接前核对外，每15秒独立核对目录；不再依赖`nativeCatalogPending`或窗口断开才更新。受管静态字段及工具参数升级均需旧指纹吻合、备份、写入读回和重载；不变化时不重复写入或重载。后续本地写入沿用本次实际写入的revision，不重新读取并借用较新revision覆盖其他更改。静态字段已写入但重载失败时保留新的归属指纹及待重试标识，可以继续恢复，不要求手动删后重建。该维护只操作目录元数据，不调用模型或执行协助工具。

天工明确返回连接已删除（404）时，Hub同步停用、撤销旧令牌并停止旧协助，不自动重新创建；天工仅停用且其他受管字段未变时同样尊重该操作。接口不可达、格式或权限失败保留现状，不当作删除。Hub中主动停用则撤销本地访问，再移除仍匹配归属的天工连接并核对/重载；天工关闭时保留待移除标记并显示等待原因，恢复后继续，完成后明确显示已移除。不覆盖用户独立修改过的连接。

通用MCP管理不再把本机协助作为普通带凭据服务重新导入或跨端复制。旧版误收录、仅关联天工的同源内置条目只清理Hub的重复索引，实际天工连接及协助设置不随索引删除；其他端已有副本或未知归属保留供核对。普通MCP的端内“移除”、外部删除后使用记录清理见[管理专题](desktop-capability-management.md)。

V1.0.16 的维护定向记录保留在 `target/v1.0.16-assistance-maintenance-20261110/`，其最终汇总结果见 [V16 记录](../releases/v1.0.16.md)。用户随后已确认协助实际调通；天工删除/停用同步、异常恢复及 macOS 不因这项确认自动视为验收通过。本轮新增功能由唯一打包会话汇总交付。

## 用户反馈

| 情况 | 当前行为 |
| --- | --- |
| 开始/处理 | 右下简短进度，按需展开记录，无调用不常驻任务卡 |
| 询问 | 天工显示问题/选项/输入，仅对应活动会话自动弹出；其他任务可在提示层处理 |
| 审批 | 未开完全访问时展示实际命令/文件/权限预览及目录，允许这一次或拒绝；完全访问由 Hub 设置明确选择后生效 |
| 完成 | 返回结果、实际完成的文件变动、命令退出码及可取得的检查输出，天工复核并继续 |
| 失败 | 返回原因，不把开始应答、提议文件变更或未知验证当作完成 |
| 停止 | 天工停止协助，原任务停止/修改也撤销旧上下文；确认退出。文件修改不自动回滚 |
| 清理 | Hub 主动清理先停止全部协助后台，确认退出后删除归属临时目录；共享 Server 保留 |

MCP 只给待人类处理数量，不返回审批内容或凭据，没有 approve 工具。需要人类的问题仍在受核验主页面提示层处理；完全访问不会自动替用户回答 `requestUserInput` 或表单。未开启完全访问时的权限审批仍由人类单次确认，模型不能自批。详细结果可展开，内部推理不回传；没运行检查或没退出码时保留未知。

## 执行、模型与权限

使用独立 **`天才搭档 Server app-server --listen stdio://`**，协议来自 Codex app-server；全部询问/审批回天工，取代早期评估优先 App 远控的推荐，不是官方 `codex mcp-server`。

每任务独立 `CODEX_HOME`，不复制、读取或修改用户 Codex 的认证、配置和历史。只配置 Hub 模型入口，子进程移除继承的模型 key/代理覆盖；`thread/start.ephemeral=true`、history persistence=none，不额外落盘协议或第二份结果摘要。关闭多代理、应用、插件、远控及网页搜索，不承诺 App 浏览器/Office/Computer Use 插件全继承。

模型来自 ChatGPT/Codex 可见清单及普通来源，固定 route/来源指纹，不借 WorkBuddy/天工专用同名渠道。任务中来源/密钥/别名变化拒绝后续请求，同源已确认 `/v1` 修正保留身份；原网关在同一配置快照再核对绑定。受信协助模型入口经过上下文、任务令牌和模型校验后标记 `local-assistance-mcp`，概览可沿 MCP 关联到实际模型来源；请求日志客户端显示“协助处理 MCP”，不再被 Codex CLI 的 User-Agent 归成未标记或普通 CLI。等待/状态/取消等管理调用不创建新的模型执行证据，真实请求活动持续到响应体消费或取消。

思考强度保存在本机协助自己的 `reasoningEffort`，不继承用户日常 Codex 配置。缺省为自动：支持时选 `xhigh`，否则按该模型目录默认；可明确选择目录提供的其他强度。已保存强度后来不再可用时保留并提示重新选择，保存/新任务拒绝静默降级。每次协助固定实际强度，写独立 `config.toml` 的 `model_reasoning_effort`，首轮和追问均传 `turn/start.effort`，不支持时省略。目录选项不证明上游及任意旧版 Codex 都接受该强度，协议拒绝会如实返回。

旧配置默认保留原权限：只读，写项目须同时满足 Hub 允许修改、原生天工任务 `full_control`、本次 `analysis_only=false`；额外权限由人类单次确认。

用户可显式勾选“完全访问”：独立 `config.toml`、`thread/start` 和每次 `turn/start` 均使用 `never`，非只读协助对应 `danger-full-access` / `dangerFullAccess`，允许以当前系统用户权限读写本机文件、运行命令和联网，不再逐次请求执行授权。此选项不依赖天工主任务是否使用 `full_control`，也不改天工本身的 MCP 工具审批。若本次明确 `analysis_only=true`，仍使用只读沙箱且审批策略保持 `never`，越界动作不能通过弹窗重新放宽；需要修改时由天工另发获准修改的协助。示例明确提示模型按实际任务选择 `analysis_only`，工具省略此字段仍默认只读。

权限设置固定于每次发起的协助，后续追问继续使用同一快照；设置变更影响新任务。完全访问仍须遵守用户的任务限制，真实用户问答继续显示，不代填。支持普通模式的命令/文件/权限审批、用户问题和简单表单 elicitation；未知或 URL 交互仍反馈不支持。协议依据 [官方 app-server](https://learn.chatgpt.com/docs/app-server) 与 [Sandbox 文档](https://learn.chatgpt.com/docs/sandboxing)。

## 生命周期

- 核验安装、CDP listener 和主页面执行上下文，从真实新运行请求建立父上下文并注入随机 `context_ref`，不扫私人历史。目录必须存在、绝对且通过链接检查。
- 原生请求只提交一次，登记/解析/网络失败不重放。明确限定其他 MCP 时不强行加入。原生停止/设置/追加提示更改后保守撤销旧上下文，新任务重建，不承诺任意增量同步。
- MCP 跨连接查询同一任务，超时不重复创建写操作。上下文+问题键幂等，任务清理/逐出内存后仍防重放。
- 同项目不并发；最多三个仍存活后台、内存64项任务；追问最多两次、wait最多20秒，活动轮次30分钟未更新及人类问题10分钟未答停止，上下文两小时有效。已完成会话取消五分钟闲置自动终结，保留至明确结束/清理或真正退出；完成状态不会因旧总任务期限自行删除。
- 核对线程/轮次，连接内帧读取器处理分片及取消；天工刷新/关闭/失联停止后台但保留归属临时目录；Hub 真正退出统一停止并清理。Windows Job Object 管理进程树，macOS独立进程组尚未原生构建。
- 退出不确定保留记录；重启不重放，活PID/未知归属不清理。仅单机同用户受信范围。独立协助入口暂不支持 compact，应缩小子任务；原V16普通Codex有限unary压缩保持。

## 配置、接口与恢复

功能根仍为 `desktop::state_root(config_path)/local-assistance/`。`settings.json` 保存 enabled/model/route/reasoningEffort/binary/allowWorkspaceWrite/fullAccess/autoClear/token/原生指纹及同步状态，token 不返回 UI；`fullAccess` 旧值缺省 `false`，思考强度缺省自动；`binary` 仅为准备来源偏好。`autoClear` 兼容保留并在新保存写 false，不再使完成任务自动删除。`transactions/` 沿原 revision/锁/备份/原子替换，`native-undo/` 保留受保护原生备份。`runs/<UUID>/` 含 owner/PID 和临时 codex-home，不再写完整结果摘要。持久运行包独立于此根，位于共享 `companion-server/`。

回滚权限可关闭完全访问，已有任务须结束后重建。回滚到 V17 时其旧 `local-assistance/runtime/` 和外部程序仍在，共享包不会改原安装或历史包；旧版本不识别共享记录，不能把其行为当成本版生命周期。移除共享包使用界面明确操作，不手工清空用户 Codex 目录。

天工采纳后 `assist_finish` 或取消只收尾后台，临时目录保留；主动“清理协助临时内容”及 Hub 真正退出统一撤销并停止全部协助、确认进程树退出后清理归属 runs/logs。内部清理走直接收尾，不等待模型中断往返；超时或活 PID 保留，启动 `recover_runs` 回收已退出的归属残留，共享 runtime 不删。保留天工主对话、项目、成果和其他 Codex 历史。停用仍先本地撤销凭据/上下文并停止，下次官方接口可用时移除受管 MCP 并读回/重载。重新启用需打开天工保存；目录未就绪显示具体原因，不伪造就绪。

天工API无CAS：保存前检查指纹、保存后读回，局部失败只对仍等于本次结果的原生条目补偿。检查与PUT之间仍有外部并发窄窗口，不宣称等同文件原子事务；目录更新只写工具/连通字段，不覆盖认证/地址。

| 接口 | 职责 |
| --- | --- |
| `/api/desktop/assistance`、`/read`、`/control`、`/clear` | 设置/读取/停止清理；原桌面校验、无网页Origin |
| `/api/desktop/assistance/prepare` | 至少一个共享功能已启用，真人 `{confirmed:true,binary?:来源}` 后准备/复用品牌包；缺省拒绝准备，不启动产品 |
| `/api/desktop/assistance/uninstall` | 两功能均停用且无后台，真人 `{confirmed:true}` 后只移除 Hub 自管共享安装 |
| `/api/local-assistance/mcp` | Bearer本机MCP，严格initialize/tools/list，Streamable HTTP JSON |
| `assist_start / assist_wait / assist_followup / assist_cancel / assist_finish` | 发起/等待/有限追问/中断/采纳收尾，无approve |
| `/api/local-assistance/model/<task>/responses` | 父上下文、任务token、模型及来源指纹校验，原网关固定来源 |

## 维护与验证

源码为 `src/local_assistance/{mod,settings,web,worker,process,protocol,records,host,catalogue,binary,runtime,brand}.rs`、`tiangong-host.js`；`runtime`/`process` 在 crate 内供搭档复用，`licenses/` 保存固定参考 LICENSE/NOTICE。设置 `src/gui/local_assistance.rs`、后台请求反馈 `src/gui/dialog_job.rs`；主进程接统一恢复/退出清理。完整原生包使用 `flate2` / `tar`，Windows 品牌资源使用局部 Win32 Resource API，进程树沿现有 JobObjects，不新增 Linux 支持。

本批夹具新增真人未确认零准备、双功能停用时拒绝安装、并发客户端共用一个副本、缺文件再次确认、真实旧清单无 execServer、品牌元数据/版本版权保留、资源/命令环境发现、移除接口要求双开关关闭、卸载保留系统来源/未知目录、取消旧五分钟空闲退出、启动残留恢复及主动清理先停止后台。Windows 含 GUI 类型编译和相关定向离线检查已通过，准确数量及日志统一见 [本批集成记录](genius-buddy.md#本批集成记录2026-11-11)。未真实下载安装 CLI、启动 Hub/Codex 或调用模型，真实品牌进程和 helper 初始化、macOS 仍待用户验收。

既有合成回归覆盖身份/路由/幂等/并发、分片事件、询问/权限子集、人类单次确认、断线/退出/记录归属、跨连接任务、工具登记/读回/后改/重载失败。Node 合成 DOM 核对原请求仅提交一次和提示稳定性；V1.0.16 各阶段数量、截图与包身份集中保留在 [V16 交付](../releases/v1.0.16.md)，本轮只补受影响检查，不累计历史结果。隐藏原生设置图不等于真实天工提示层验收。

V1.0.17 的验证和原测试包见 [历史 V17 交付](../releases/v1.0.17.md)，本批没有新包。后续用户实机验收共享 Server 确认准备/本地复用、品牌进程与组件发现、首次 app-server 初始化、协助进度/询问/权限/结果、退出清理和移除安装；此前调通反馈保留 V16 范围，macOS 待验收。

协议依据：[官方app-server](https://learn.chatgpt.com/docs/app-server)。实验协议及天工结构变化会影响适配，不兼容明确提示。[历史评估](tiangong-local-assistance-assessment.md) 不替代本专题。
