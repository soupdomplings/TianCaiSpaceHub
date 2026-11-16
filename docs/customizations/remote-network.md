# 远程连接与个人设备网络

维护日期：2026-11-15。源码基线：`7c6d3d5`，功能实现 `e44b497`；用户随后要求统一生成 V1.0.32 Windows 本地测试包，构建和包身份见[交付记录](../releases/v1.0.32.md)。网络模块已实现，真实组件准备、双机组网、桌面会话续聊及 macOS 实机均待用户验收。会话协议见 [协作契约](../development/remote-session-contract.md)，总体功能及最终联合验证由二开总表登记。

## 当前使用流程

“远程连接”默认关闭。两台电脑都运行 Hub，目标端的 Codex／天工 Work 客户端须已启动并接入 Hub。每台 Hub 同时保存一个个人网络，设备可以互相访问；浏览器只连接访问端 Hub 的本机入口。

1. 在两台 Hub 启用远程连接。缺少组件时，点击准备并确认后才下载 EasyTier；取消或尚未启用均不下载。启用开关本身不安装组件。
2. 在一台 Hub 创建网络并复制连接码，另一台粘贴加入。已在网络中再次创建仅生成新连接码，保留已有网络。
3. 目标 Hub 显示设备名称和配对请求，确认一次后双方持久保存设备授权。未配对的组网设备不能使用会话接口。连接码有效期十分钟，每次邀请只能建立一项新的配对请求。
4. 选择已配对设备打开网页。本地入口兑换为 HttpOnly、SameSite=Strict 的会话 Cookie，并移除 URL 中的临时入口凭据；目标设备令牌、网络口令、原客户端令牌及模型密钥不交给网页。
5. 撤销设备授权后，目标请求及仍在等待新内容的事件流一并断开。重新配对会更换设备令牌，并结束旧令牌对应的流。撤销远程访问不自动取消目标端已经开始的模型任务。

设备名称保存不重启网络。启用状态、节点地址或设备证书变化，以及组件已经退出时，才重新准备后台连接。加入时遇到虚拟 IPv4 冲突，自动更换本机虚拟地址和证书，再重新配对。持久凭据被外部删除时不静默下载或重新授权：显式启用／创建／加入时可以重建本机证书并清除旧配对；创建网络发现网络口令缺失时建立新的个人网络，其他设备需要重新加入。

最小化和关闭到托盘保持网络；真正退出 Hub 时，关闭本机网页网关、HTTPS 会话服务和自管 EasyTier 后台，不关闭用户的原客户端。退出不会删除运行包、网络设置、配对记录、会话附件或产物。异常退出后，仅回收带本模块所有权标记且所属进程已经退出的运行临时目录；未知或仍在使用的目录保留。

## 组件、节点与真实状态

固定准备官方 EasyTier **v2.6.4** 的未修改 `easytier-core`、`easytier-cli`，不要求用户安装 Node.js、TUN 驱动或以管理员身份启动 Hub。运行模式为 `no_tun + use_smoltcp`，只有本功能的会话访问通过该网络。

| Hub 平台 | 官方 Release 资产 |
| --- | --- |
| Windows x64 | `easytier-windows-x86_64-v2.6.4.zip` |
| Windows Arm64 | `easytier-windows-arm64-v2.6.4.zip` |
| macOS Intel | `easytier-macos-x86_64-v2.6.4.zip` |
| macOS Apple Silicon | `easytier-macos-aarch64-v2.6.4.zip` |

资产映射已对照 [官方 Release](https://github.com/EasyTier/EasyTier/releases/tag/v2.6.4) 及其公开 API 核对。来源和许可随 Hub 自管运行包保存：`SOURCE.txt`、`LICENSE-EasyTier`。本版本项目 [LICENSE](https://github.com/EasyTier/EasyTier/blob/v2.6.4/LICENSE) 为 **LGPL-3.0**；网页页脚的站点许可不作为程序许可。

默认节点为 `tcp://us01.225284.xyz:11010`，来自运营者在 [EasyTier 官方组织讨论 #1848](https://github.com/orgs/EasyTier/discussions/1848) 公布的社区节点。用户可修改节点；清空设置恢复这个默认地址。旧 `public.easytier.cn` 在本轮开发环境无法解析，不再作为默认值。社区节点由第三方运营，持续可用性需要实际网络验证，Hub 未部署自己的公共中继。

EasyTier 优先尝试设备直连，失败时使用可达节点中继。Hub 每五秒读取 CLI 真实节点状态：`p2p` 对应直连，`relay(n)` 对应中继；不把创建网络、进程启动或尚未连接任何节点显示为已连接。

状态区分三层：`runtimeReady` 表示自管组件文件可用，`networkReady` 表示后台进程和服务已启动，`connected` 表示 CLI 已返回实际连通节点。设备 `online` 依据已发现虚拟节点，`connectionType` 为 `local`／`direct`／`relay`／`pairing`。未连通或组件退出时保留可读原因，用户可以检查网络、更换节点或停用后重启。

## 隔离链路与本机数据

固定访问链路：访问端浏览器 → 访问端本地 HTTP 网关 → EasyTier 用户态连接 → 目标端 HTTPS 3861 → 独立会话接口。

- 目标 HTTPS 服务仅承载设备公开身份、配对接口和 `/api/remote/v1`。现有 Hub 管理 Router、组件管理接口及通用系统接口不合入该服务。
- EasyTier 入站和转发两条 ACL 链均默认拒绝，只允许目的为本机虚拟 IPv4 的 TCP **3861**。两条链同时限制，是因为 `no_tun` 可将虚拟地址转换成本机回环，不能仅依靠服务绑定地址隔离其他端口。
- EasyTier RPC、SOCKS 只监听随机回环端口，不通过远程 ACL 放行；禁用陌生网络中继。自管组件不继承现有 `ET_*` 环境选项。
- 配对时根据连接码中的证书固定校验目标 HTTPS；不关闭证书验证。会话请求还校验每对设备独立的访问令牌。
- 本地网关检查 Host、Origin 和跨站来源。网页用 `X-Hub-Target` 选择设备，网关持有并补入目标令牌。SSE、上传和下载流式转发，保留会话文件的 sandbox CSP。
- 附件／产物下载和预览也必须通过带 `X-Hub-Target` 的 fetch 后使用 Blob，不能直接把相对 URL 作为 `img.src`／下载链接而漏掉目标设备。

所有数据放在 `desktop::state_root(config_path)/remote-network/`，即配置路径扩展为 `.desktop` 后的独立子目录：

| 位置 | 内容与保留策略 |
| --- | --- |
| `network.json` | 设备公开身份、虚拟 IP、网络名称、节点地址、启用状态及已配对设备证书；长期保留 |
| `protected/` | 平台凭据存储：网络口令、本机 TLS 私钥、每设备令牌；不进入状态响应或普通日志 |
| `runtime/easytier-<platform>-v2.6.4/` | 两个官方程序、来源、许可及 `hub-owner.json`；停用和退出保留 |
| `active/hub-*/` | 当前自管实例的配置和所有权／PID 标记；正常退出移除，异常残留按进程状态回收 |

远程会话附件由独立会话服务管理，不在搭档临时内容目录。网页不复制保存一套聊天历史。

## 管理接口与维护定位

本机既有受信管理入口增加 `/api/remote-network`，远程侧不开放此入口：

| 方法与路径 | 作用 |
| --- | --- |
| `GET /status` | 启用、组件、真实连接状态、设备、待确认配对和可读错误；无私有凭据 |
| `POST /settings` | `{enabled,deviceName?,relayUrl?}`；仅保存和启动已有组件 |
| `POST /prepare` | `{confirmed:true}`；未启用或未确认时拒绝下载 |
| `POST /uninstall` | 必须先停用；仅删除版本／平台／归属标记匹配的 Hub 自装组件 |
| `POST /create` | 创建个人网络或为当前网络生成新的连接码，响应包含 `code` |
| `POST /join` | `{code}`；加入后由后台重试网络配对握手，不重发会话消息 |
| `POST /pair/approve`、`/pair/reject` | `{requestId}`；每项请求仅处理一次 |
| `POST /revoke` | `{deviceId}`；持久撤销并通知正在等待内容的流 |
| `POST /open` | `{deviceId?}`；返回带临时入口凭据的本机网页 URL |

主要代码：`src/remote_network/mod.rs` 管理设置／生命周期／配对；`runtime.rs` 准备官方包及 ACL；`pairing.rs` 连接码；`transport.rs` 独立 HTTPS、浏览器鉴权和流代理。`startup`／`shutdown` 接入 Hub 生命周期，网页资源由 `remote_frontend` 提供，会话 Router 由 `remote_sessions` 提供。

## 停用、移除与验证边界

停用立即结束本模块后台并保留配置和运行包；只有停用后才能移除 Hub 安装的组件。无正确所有权标记的外部目录不自动删除。后台退出未确认时保留本次运行记录并报告原因，不将该状态当成已完成移除。再次启用若运行包缺失，仍须用户明确确认准备，不自动补装。

2026-11-15 已完成源码格式化、定向差异空白和官方资产映射核对；本模块 16 项 Windows 离线／合成测试全部通过，最终 Windows GUI 联合 `cargo check` 通过。整批定向测试命令和范围见[集成验证](remote-connection.md#验证记录)。

合成测试 filter 为 `remote_network::`，覆盖默认关闭不落运行文件、取消准备不下载、状态不泄漏凭据、证书丢失和地址冲突重新配对、一次确认／持久保存／撤销通知、ACL 双链、真实 CLI 状态样例、拒绝移除外部目录、异常残留回收、独立 Router 无管理接口、浏览器 Cookie、跨站拒绝、产物 CSP 及无需新事件即可撤销等待流。Windows 的持久凭据用例标注 `cfg(windows)`；macOS 钥匙串和运行端生命周期尚未实测。

本轮未启动真实 EasyTier、未下载运行组件、未加入个人网络、未运行真实 Hub／客户端／模型。普通 TCP 节点可达性检查不等于组网或中继验证通过。Windows↔Windows、Windows↔macOS、macOS↔macOS、节点故障恢复、端口隔离实机验证，以及桌面—网页—桌面原会话续聊，均待用户按统一测试版本验收。
