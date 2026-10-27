# 天工 Work 兼容性评估与适配

2026-10-27 发布准备：用户对本地包初步反馈正常，授权发布 GitHub 和启动两平台打包；产品源码不变。最终提交、标签、Actions 和验收边界以 [v8 交付](../releases/v0.4.30-8.md) 为准，下方未提交状态保留开发阶段事实。

维护日期：2026-10-22。关联 TC-002、TC-004、TC-007、TC-011、TC-012、TC-014。唯一 `main` 基线 `ed71d31507e4377649a5c6f4acb899c25ce60d4a`，本轮在其上开发 `v0.4.30-8`，源码未提交推送。

已实现新旧事件协议、任务模型绑定、流式上下文、后台运行保护，以及接入页面统一。验证和本地测试包身份集中见 [v8 交付](../releases/v0.4.30-8.md)。开发方仅用合成数据、隔离服务和离线函数验证，不启动实际产品、安装器或真实模型/IM 请求；Windows/macOS 实机由用户验收，macOS 尚未原生构建。

## 结果与范围

| 项目 | 原问题 | v8 行为 |
| --- | --- | --- |
| 安装发现、授权、模型表、MCP | 新品牌不能只按数值版本判断 | 保留完整安装、同用户进程、默认端口和实际能力核验，补充 Work 品牌别名 |
| 任务收尾 | 新版 `over` 后还发状态，被当作非法继续执行 | 接受明确的收尾和最终状态；缺终态、断流或新执行变化仍报未知，不重放 |
| 工具审批 | 新版主任务也有 `run_id`，被误认子任务 | 从响应绑定主运行和会话，按原运行/游标续审批，子任务审批仍在天工处理 |
| IM 与桌面选模 | 原生任务无模型，桌面首次打开可选列表首项 | 新任务提交 `model_id`；恢复优先真实任务选择，历史空绑定经复核后补写 |
| 默认模型 | `is_active` 不保证新版窗口选中 | 删除 Hub 默认勾选、按钮与状态展示，保存保留天工原默认状态 |
| 流式上下文 | 原补全只覆盖非流式 | 流式原样交付，旁路有界采集已验证终态，恢复匹配的推理/工具状态 |
| 后台任务 | `over` 或订阅断开不代表后台空闲 | IM 恢复/续聊检查后台活动任务；桌面同步另核对运行、收尾、重连和队列 |
| 页面与名称 | 两端字段、按钮和说明不一致 | 使用天工 Work；WorkBuddy/天工统一管理模型、渠道、按钮及字段底色，隐藏内部参数 |

以上“已实现”指源码行为及下文隔离验证，不代表真实工具调用、IM/桌面切换或跨平台已验收。

## 本机核对依据

新版卸载名称为“女娲天工Work”，`package.json` 的 `name=tiangong-desktop`、`version=1.0.0`、`description=女娲天工Work`、`main=./out/main/index.js`，没有 `productName`。旧参考版本为 `1.1.1`，新版数值反而更低；EXE `ProductVersion=35.7.5` 也不能当作天工版本门槛。下面哈希仅标明评估样本，不用于运行白名单。

| 安装资源 | 字节 | SHA-256 |
| --- | --- | --- |
| `resources/app.asar` | 366246766 | `73fcc5765fe4540a6533b863bca360a26a71d953c5eca06a682495c0a08ec8c5` |
| `out/main/index.js` | 921311 | `7973600b68b176c42859e73e7cff5db77da63ebaa992d965eea0e478d9edb1ab` |
| `out/renderer/assets/index-Cyl2X7f9.js` | 1268062 | `6426dbb25ad36292d896ed07d493954a3aecaafeb5c4c0d0f12b45cdff8f58a3` |
| `harness-sidecar/packaging/desktop_chat_runs.py` | 24379 | `257ba4aa8e6e441bf146dca10ba8d77dbd737ceaef89cb26d77df9430a0fd420` |

提取资源和离线复现只放忽略目录 `.build-tools/tiangong-work-assessment-20261022/`，不纳入仓库。没有读取用户真实模型密钥、会话正文或运行 token。

安装程序、资源布局、`electron-data.db`、`model_configs` 和运行鉴权沿用。Windows 数据目录仍 `%APPDATA%/tiangong-desktop`，macOS 仍 `~/Library/Application Support/tiangong-desktop`。`GMCLAW_CONFIG_PATH` 负责模型库覆盖，`desktopPath` 负责完整安装定位；天工自身的 `TIANGONG_USER_DATA_DIR` 不能替代二者。官方默认 Harness 为 `127.0.0.1:7861`、DataServer 为 `127.0.0.1:18768`；本轮未扩大 Hub 对自定义端口的支持。macOS 仅依据现有跨平台代码和新版安装源码分支，未取得新原生安装样本。

新增模型列 `display_name` 默认空，Hub 新增时省略仍合法，更新保留未知列；没有 ALTER TABLE、重建原生库或批量改历史。MCP 的连接管理、test 和 tools/discover 接口仍在，本轮继续仅管理 Hub 受管目标，预览、归属、备份、严格 MCP 握手和临写前复核保留；真实工具刷新/调用待用户测试。

## 任务结束与审批

新版 `_publish` 对事件添加主 `run_id/seq/session_id/chat_id`，子运行另存 `subagent_run_id`。Hub 以新请求响应的 `X-Run-Id`、本次会话和消息身份绑定，校验每帧及连续序号。无该响应头的旧流保留原处理路径；不是按安装版本切换。

正常主任务 `over` 后，只接受已知 `run_status` 的收尾/终态，不把 `running+phase=finalizing` 当作错误。`completed` 必须与普通答复一致，`waiting_confirmation` 必须与主任务审批一致；`failed/stopped/interrupted` 报告执行错误、清除可审批状态。未知变化、结束后新正文、跨会话或乱序均拒绝；新协议未收到最终状态不会宣称成功。

主审批保存由 Hub 验证得到的运行、原消息及最终游标，不采信事件 JSON 伪造的内嵌运行绑定。续审批先 GET `/v2/chat/runs/<runId>`，核对原会话、原消息、等待审批状态及 `last_seq`，再 POST 该运行的 `/resume`，从 `/events?after_seq=...` 读取结果。检查失败不提交，不回落到按 session 猜测任务的兼容审批接口。子任务正文/结束仅作进度，不结束主任务；子任务审批仍需桌面处理。

写后超时或断流属于状态未知，不自动重复审批、模型或工具执行。后台检查只读 `/v2/chat/runs?session_id=...&active_only=true`；旧接口返回 404 时沿用历史事件证据，其他失败或未知状态不当作空闲。检查不能阻止用户随后在桌面新发任务；服务端仍负责最终会话互斥。

## 默认模型和 IM/桌面衔接

用户决定 Hub 不再配置默认模型。GUI 固定 `makeActive=false`，API 省略也默认 false；新增不设默认，更新保留原状态，删除和备份还原沿用定向语义。旧显式 `makeActive=true` 与 `/api/gmclaw/config/activate` 仅供兼容，GUI 不调用。天工实际模型由会话选择。

新版 renderer 保留有效的任务 `model_id`，无有效选择时取模型列表首项。原 Hub 只在自身关联中记录模型，任务无绑定，因此 IM 与桌面可能不一致。v8 新建任务同时提交精确 `model_id`；恢复优先有效原生任务 ID，再用旧关联或唯一匹配的历史名称，并核对模型目录。模型被删或名称含糊不替换成默认。只有无模型记录的确实空会话保留旧默认回退路径。

历史新版任务的 `model_id` 明确为空字符串时，先复核任务/会话和未变选择，再定向 PUT 模型并重读确认；只传 `model_id`，不清空 `model_options`。旧 API 缺少字段不写新字段；桌面已有显式模型不覆盖。IM 续聊发现桌面更换模型会要求重新恢复，同步真实选择。原生任务 API 没有 CAS，最后复核与写入之间仍有外部并发窗口，不保证并发切换的原子性；写后不能确认时不自动重试。

已在隔离 Node VM 中执行本机实际任务路由及 `createInstance/onOpenTask/loadDbModels`：v8 请求带模型后保持 IM 选择；旧空绑定仍能复现内置首项回退；有效桌面选择保留；单模型列表保留。四例均通过，仅为合成数据的函数链验证，实际 IM 后回桌面继续仍待用户测试。

## 默认流式与上下文保留

新版 `DesktopModelClient` 默认流式，仍使用 `<base_url>/chat/completions`。Hub 保留原流式交付，在天工 Responses 转 Chat 前、原生 Chat 流上增加有界旁路观察，复用既有 JSON/SSE Decoder。只有协议终态通过校验才保存 Chat 推理字段或 Responses 原始输出；截断、错误、超限或不支持结构关闭本次捕获，继续原流，不伪造签名或重放请求。

恢复仍按专用渠道身份、真实模型及完整可见历史前缀严格匹配，显式用户字段优先。缓存保持 128 项/16 MiB、单项 1 MiB/1 小时，只存内存。系统提示变化、裁剪、缓存过期或 Hub 重启仍可能使匹配失效，不承诺跨任意历史变换恢复。旁路解析复用的单响应上限为 32 MiB，超过只停采集。其他协议与非流式路径保留。

Responses 工具流对外优先使用 `call_id`，增量仍按 item ID 索引，并支持 custom tool 参数增量；UTF-8 在完整行后解码，避免中文跨网络块损坏。完整回放和真实多轮工具由用户验收。

## 桌面同步与页面统一

同步继续按完整安装、HTML 脚本入口、App/Panel 同作用域能力和可写 Ref/缓存核验，不绑定文件名、数值版本或哈希。保留旧品牌并增加明确 Work 别名；Windows 页面仍按完整路径组件核对，拒绝远程 file、其他页面及 query/hash。

新 Panel 的 `runState/hasActiveRun/isReconnecting` 必须完整匹配；三项都缺失才按旧版处理。存在后台活动、finalizing、重连、未知运行状态或 queued 提示时延期；读取后再次检查。原有原生流/审批/历史加载、消息身份、草稿、附件、模型选择和可写保护仍在。没有整页刷新或自动切换任务。

WorkBuddy 与天工 Work 统一为“管理模型”“大模型接入渠道”“模型”“接口类型”“思考强度”；保留各自真正可控的参数。WorkBuddy 思考强度并入连接卡片，内部渠道、地址、本机 Key、上游 URL/Key、配置路径和当前模型 ID 不再展示，后台生成/定向保存照常工作。按钮统一为“启动… / 刷新 / 撤销上次操作 / 保存模型 / 删除所选模型”。WorkBuddy、天工 Work、Dumpling-MCP 只读或停用字段灰底，可编辑字段在浅色主题白底，深色主题用对应卡片色。

## 配置兼容与回滚

保留 `gmclawBridge`、`gmclaw[:entryId]`、原路由、`GMCLAW_*`、`tiancaispacehub-...`、`clientKind=tiangong`、`/tg` 和旧 `/gmclaw`；不创建第二套品牌配置。任务字段从官方 API 读取，缺失兼容；新增审批运行绑定只在本次待审批内存保存，不迁移持久关联文件，Hub 重启仍不自动恢复审批。

模型操作继续复用定向备份和“撤销上次操作”。回退 Hub 前核对正在执行/待审批任务并备份配置及关联；回退可能重新出现旧 Work 误判与选模缺口，不会取消天工已开始任务。已补写的原生任务模型保持，不批量抹回空值；如需调整在天工选择。流式缓存随 Hub 退出清空，样式不需要迁移。详情见 [模型专题](gmclaw.md)、[外部消息](gmclaw-im.md)、[WorkBuddy](../workbuddy.md) 和 [Dumpling-MCP](nvwa-mcp.md)。
