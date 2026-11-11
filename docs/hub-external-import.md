# Hub 网页导入（Windows / macOS）

## V1.0.25 文案整理

导入窗口改为“保存到 Hub、要导入的模型、在哪些客户端使用”三个直接分区；说明明确不选客户端只保存到 Hub。主按钮随选择显示“保存到 Hub”或“导入并配置客户端”，不再用“接入”概括两个不同步骤。“高级选项”改为“更多设置”，启用选项直称“保存后启用渠道”，悬停说明启用后 Hub 可使用该渠道的模型。选择客户端时自动勾选并锁定此项；取消所有客户端后恢复之前的手动选择，默认仍为仅保存、不启用。模型映射选项改为“用本次的模型别名替换旧别名”，悬停说明更新时勾选会替换、未勾选保留，新建渠道使用本次别名。没有把版本号或路由字段展示给用户，没有增加确认步骤或改变保存默认值。

导入完成、部分失败、保存失败及提交前错误统一使用 [右上操作提示](development/operation-feedback.md)，完整说明可按需查看；底部状态只显示保存与逐项接入的真实进度，结束后隐藏。逐模型结果表和所选项详情保留。客户端列表读取失败时，页面只提示暂不可用且可先保存到 Hub，完整原因交给同一错误提示。原通用确认同步采用主题确认窗。

本次仅调整显示与共用反馈入口，导入、模型和客户端保存合同沿下文。已完成定向格式化与差异空白检查；本轮最终 GUI 编译由管理会话统一核对。下文 V24 模拟流程与界面结果保留原版本归属，不能当作本轮实机验收。

## 多模型与多客户端一次接入（2026-11-11）

V24 集成已通过 4 项模拟接口流程和 1 项隐藏原生布局用例；管理会话实看选择、空目录、部分失败及保存未确认的浅深/窄宽组件图。固定动作与正文滚动独立，结果三列和选中详情可读。未执行真实站点导入、用户客户端启动或 macOS；唯一包和检查记录见 [V24 交付](releases/v1.0.24.md)。

本轮重做原生导入窗口，使用小分队共用的主题、分区、滚动正文和固定操作区。来源密钥保持掩码；模型从本次实际取得的目录勾选，可搜索、选择当前结果或清空，筛选不会丢失已选模型。沿原预览默认选择目录中的全部模型，客户端默认均不选择。保存方式与渠道名直接显示，站点、模型服务地址、协议、仅保存时启用及更新替换映射放入“更多设置”。

客户端使用 `/api/desktop/library` 返回的真实目标，仅列 ChatGPT/Codex、WorkBuddy、天工 Work、DeepSeek Harness、ZCode 中目录存在且不继承模型的目标；前三项还须对应当前原生模型配置位置，避免将只管理 Skill/MCP 的自定义目录当成模型目标。窗口不展示 VS Code/WebStorm，也不编造目标 ID。目录提示只用于本机选择，不查询或调用推理模型。

一次提交先调用 `/api/external-import/commit` 保存来源渠道，始终 `visible_models=false`。没有客户端时默认禁用保存，也可在“更多设置”中启用；选了客户端则启用来源。随后按目标与模型顺序执行：ChatGPT/Codex、WorkBuddy、天工使用 `/api/desktop/models/copy`，Harness/ZCode 使用 `/api/desktop/models/use`；每一项重新读取最新配置版本、以刚保存的名称和来源身份找到真实 `sourceRoute`，提交原生 `targetId`、模型和启用状态。Codex 由原复制接口生成独立可见别名；不再以给来源模型打可见标记代替客户端接入。首次 Codex 全局接入仍需在接入设置完成初始化。

执行中在当前窗口以“客户端、模型、结果”三列表格逐项显示，选中某项才查看具体说明或错误；完成后保留结果页，V1.0.25 起以右上小提示汇总结果。来源保存未确认时不启动后续接入；某个客户端/模型失败后报告该项并继续其他项，已经成功的项目保留。批次没有自动重试、重新保存来源或重放不确定写入，结束后不能误点同一按钮重复本批；失败项由用户到对应客户端页核对后处理。导入码和未保存凭据只留内存，不生成额外协议日志或缓存。

关键代码为 `src/gui/external_import.rs`（窗口、选择与反馈）、`src/gui/external_import/batch.rs`（真实目标过滤、一次来源保存和逐项客户端写入），复用既有本地接口，不改变站点契约或持久配置格式。回退旧界面不会删除已经保存的渠道及客户端配置。已完成定向格式化与差异空白检查；新增四项合成 API 检查覆盖五端路径、版本连续更新、部分失败不重放、来源保存失败及目标过滤，已由管理会话统一执行通过。另有不连接服务、不显示模态窗的隐藏原生夹具，覆盖浅深主题、短窗口、长名称、搜索选择、空目录、固定操作区及部分失败/保存未确认结果；可沿 `TIANCAI_NATIVE_REVIEW_DIR` 捕获本次合成页面，已由管理会话执行并人工查看浅深/窄宽组件图。真实站点、实际客户端加载、缩放和 macOS 仍待用户验收，构建及包身份以本批交付记录为准。

## 既有接入与协议范围

V1.0.8 原生账户补充：天才小分队API密钥“导入Hub”创建票据时发送固定官网同源 `Origin`，满足站点现有Origin/Host检查；复用连接完成创建、单次兑换和模型目录读取，核对登录代次、来源及所选密钥ID。修复遗漏来源导致失败又被本机包装为HTTP409的问题，账户页显示可读原因；通用网页深链接的来源规则不改。该版进入原预览、默认禁用及受保护确认保存，不自动重放票据或发模型推理；当前勾选客户端后的启用与接入见上文。完整行为见 [账户专题](customizations/tiancai-squad-account.md)，真实站点导入待用户验收，验证及包身份见 [V1.0.8](releases/v1.0.8.md)。

`v0.4.30-13` 地址处理补充：保留主站下发的合法 Base URL 路径，不在导入时凭空补 `/v1`；模型目录仍只请求主站明确的地址（缺省为 Base URL 后的 `/models`），兑换与导入目录均不增加候选重试或重定向。保存后真实模型请求的缺版本恢复属于独立的 [TC-015](customizations/provider-url-recovery.md)，不改变本导入的凭据发送边界。

维护日期：2026-11-11。关联 TC-009、TC-010。本文维护当前实现；需求字段见 [接口契约](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md)，开发沿革见 [二开变更记录](customizations/CHANGELOG.md)。

历史发布基线：`v0.4.30-2`，已发布 GitHub 预发布版。实现 Sub2API 外部导入约定 v1，默认禁用保存；该版Windows/macOS Actions原生构建和打包均通过，macOS系统接入代码随该版交付。该结果不代替V1本地测试和实机验收；Linux不在本次范围。历史安装包和验证边界见 [版本交付](releases/v0.4.30-2.md)。

本地未发布命名调整：当前源码的产品及程序统一为 `TianCaiSpaceHub`，Windows 为 `TianCaiSpaceHub.exe`，macOS 为 `TianCaiSpaceHub.app`；已发布包仍保留旧名称。导入协议、数据及安装升级身份不变，程序路径变化后需从所需版本重新注册，macOS 注意移除多余的旧名称 App 副本，详见 [品牌与交付](customizations/desktop-and-packaging.md)。

2026-10-03 接入点隔离：网页导入禁止创建或覆盖 `workbuddy`、`workbuddy:`、`gmclaw`、`gmclaw:` 整个保留命名空间，更新目标列表也排除这些渠道。导入普通渠道并启用后，可在对应客户端页签主动选择并保存；WorkBuddy 与天工可创建多个独立条目，见 [WorkBuddy](workbuddy.md)、[天工 Claw](customizations/gmclaw.md)及[用途标识](customizations/client-channel-scope.md)。

## 用户流程

1. Windows MSI 安装版自动注册 `tiancaispacehub` 协议，便携版先在“文件 → 注册网页导入…”关联到当前程序位置。macOS 使用完整的 `TianCaiSpaceHub.app`（已发布旧包为 `TianCaiSpace Hub.app`），从 DMG/App ZIP 复制到“应用程序”并打开；必要时通过同一菜单重新注册当前 App，裸命令行二进制不能注册为 App。
2. 在兼容站点的 API Key 页面选择导入 TianCaiSpaceHub。浏览器可能要求确认打开外部应用。
3. Hub 未运行时启动；已运行时将请求交给现有窗口。旧版 Hub 无法接收时明确提示关闭旧版并重新发起，不静默丢弃。
4. 支持任意符合契约的 HTTP/HTTPS 站点，不设官方域名白名单，本机、局域网和自定义域名均可。自 `0.4.29-4` 起，所有来源直接兑换导入码，不再弹出来源确认、跨站模型查询确认或 HTTP 提示。来源和模型服务可以是不同域名。
5. Hub 兑换短时码后，直接携带本次 Key 查询返回的模型列表地址，然后展示预览；主站给出的模型范围非空时，取远端结果与该范围的交集。只查询模型目录，不发起推理或付费测试。
6. 导入窗口确认保存名称，从本次目录勾选模型与客户端；支持搜索及多选，Key 始终遮罩。来源详情和少用的更新选项放入“更多设置”，不再手填模型或单独追加 Codex 可见模型。
7. 同来源和 Key 默认选择更新；同名不同 Key 默认添加编号新建，也可明确选择更新目标。更新保留原有权重、超时、缓存、推理设置及模型映射；需要替换映射时单独勾选。
8. 不选客户端时默认禁用保存；空模型或发现失败也可沿此路径先保存，之后到渠道页补全。选客户端必须有模型并启用来源；启用同名模型渠道仍参与原优先级路由，不自动抢占优先级。
9. 保存来源后逐项调用原生客户端配置接口，窗口展示成功及失败项。首次 ChatGPT/Codex 接入仍通过接入设置完成全局初始化，已有客户端会话按原行为新建会话或重开后读取新模型。

## 状态与保存

- URL 仅携带来源和一次性导入码；严格检查字段、长度、版本和地址。不跟随兑换或模型发现重定向，不关闭 TLS 校验。
- `origin` 是 `scheme://host[:port]`，可带结尾 `/`，不能包含业务路径、用户名密码、查询参数或片段；例如 `http://127.0.0.1:8080`、`http://192.168.1.10:8080`、`https://sub2api.example.com`。兑换响应的 `source.origin` 必须与链接来源一致。`base_url` 和 `models_url` 也接受 HTTP/HTTPS，并保留合法业务路径。
- 链接解析、单实例 IPC、兑换响应、模型发现和提交保存使用同一套地址规则。`TIANCAISPACE_IMPORT_ALLOW_LOCALHOST` 已移除，不再需要环境变量或仅允许回环地址的特殊配置。
- 导入码及待确认 Key 只保存在内存；退出或取消即丢弃。诊断文本不引用远端错误正文、完整导入 URL 或 Key。
- Windows 使用当前用户 SID 限制的命名管道转交，并检查接收进程用户；不创建磁盘票据队列。不使用公开 HTTP 接口接收网页深链接。
- macOS 浏览器通过系统 URL 事件交给 Hub，事件经相同解析器校验后进入内存队列；wxDragon 在尚未安装 URL 回调时暂存已送达 `MacOpenURL` 的有界事件，注册回调后交付。更早的 wxWidgets 初始化阶段只暂存最后一个 URL，首次启动应等待主窗口出现后继续导入，不保证该极早阶段的多链接全部交付。GUI 已隐藏或最小化时恢复并激活窗口，不弹来源确认。
- macOS CLI `import-url` 或重复启动的 App 使用同用户 Unix socket 转交给现有 GUI；接收进程和发送进程都校验对方 UID。短路径 `/tmp/tiancaispacehub-import-<uid>/v1.sock` 位于权限 `0700` 的所属用户目录；`receiver.lock` 为 `0600` 的空锁文件，socket 为 `0600`，目录/锁拒绝符号链接，独占锁防止删除活跃接收端。目录、锁和 socket 不保存 ticket 或 Key；崩溃残留 socket 在持有独占锁后清理。
- 重复 macOS App 进程仅运行隐藏的事件转交循环，不启动第二个 Hub 后台。等待原生 URL 事件最多 12 秒；转交连接最多等待 12 秒，发送后不重试票据。转交失败给出脱敏提示，结束转交后退出。正常浏览器唤起由 macOS 直接投递给运行中的 App。
- 重复链接去重；并发导入最多 8 个，管道接收有长度、容量及超时限制。预览打开期间继续处理后续票据，已完成模型查询的预览依次显示。启动接收错误和无效原生链接不占用导入名额，也不扣减其他在途导入计数。
- 保存会重读最新配置并仅合并目标渠道；目标在预览期间已变化则拒绝覆盖。其他页面的过期整份配置保存也会被拒绝，提示刷新重试。
- 配置写入使用文件锁、内容版本和同目录临时文件原子替换；`_revision` 仅用于本地 API，不写入 TOML。`importSource` 随渠道保存，改名不会丢失来源身份。
- 导入渠道的空模型启用检查同时覆盖后续普通配置保存。旧版 Hub 不具备版本冲突检查，联调或回退时不要与新版同时编辑同一配置文件。

## 协议关联、升级与回退

- MSI 使用 `HKLM\Software\Classes\tiancaispacehub`，随 MSI 安装/升级/卸载管理。产品 UpgradeCode 保持一致；本地未发布版本安装路径改为 `TianCaiSpaceHub\TianCaiSpaceHub.exe`，升级与回滚待新包实机验收。
- 便携注册使用 `HKCU\Software\Classes\tiancaispacehub`，不需要管理员权限。Windows 的用户级关联可能优先于 MSI 的系统级关联；改用安装版时，在原便携版解除注册，或在新程序中重新注册到新位置。
- 移动便携文件后需重新注册。解除注册前同时核对 owner 和启动命令，只删除当前程序路径拥有的用户关联；不删除其他安装位置的关联。
- macOS 在 App 的 `Info.plist` 声明 `CFBundleURLTypes` / `CFBundleURLSchemes=tiancaispacehub`，保持 Bundle ID `com.codexhub.app`。菜单/CLI 注册使用系统 `LSRegisterURL` 更新当前 App 路径，不改写已签名的 App 内容。保留多个版本时应先退出旧版并移除多余副本，再从保留版本注册，避免系统选到旧 App。
- macOS 协议关联随 App 由系统管理，不提供 Windows 式“解除注册”菜单；`unregister-web-import` 会说明系统管理方式。卸载时退出并移除 App，不用删除其他 App 的关联。移动 App 后从新位置重新注册。
- CLI 支持 `register-web-import`、`unregister-web-import` 和 `import-url "tiancaispacehub://import/v1?..."`；用户流程优先使用菜单和网页。
- 回退前关闭新版 Hub、备份配置，再安装旧包。需要撤销导入时，在“大模型接入”删除此次新建渠道，或从自己的配置备份恢复被更新渠道。一次性导入码无法重用。
- `0.4.29-3` 不改变配置格式，已有 HTTPS 渠道无需迁移。回退到 `0.4.29-2` 会恢复旧的 HTTP 导入限制；需要重新导入时升级修复版或使用 HTTPS。升级测试前退出仍在运行的旧版 Hub；便携版在新位置重新注册网页导入，让浏览器唤起新程序。
- `0.4.29-4` 只精简导入前确认流程，无配置格式迁移，也不新增开关。回退到 `0.4.29-3` 会恢复来源确认、跨站查询确认及 HTTP 提示；两个版本都保留最终预览保存和默认禁用。
- `0.4.29-5` 不更改导入协议字段、Sub2API 接口或渠道配置格式。macOS 回退旧版会失去网页导入接收能力；退出新版、备份配置并恢复旧 App 即可，旧 App 不支持新协议时需在网页取消打开。不要同时运行两个版本编辑相同配置。

## macOS 打包与维护

[macOS 工作流](../.github/workflows/release-macos.yml) 构建 Apple Silicon 与 Intel 的 universal App、DMG 和 App ZIP；协议声明及 `WEB-IMPORT.md` 纳入 App，在签名前检查 plist 和双架构程序，构建使用 Cargo 锁文件。`v0.4.30-2` 标签触发的 [macOS Actions 37092082348](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/37092082348) 已成功，安装包已预发布，为 ad-hoc 签名且未公证的 universal 包。该结果覆盖编译与打包，不替代浏览器唤起和系统关联实机验收。源码只维护 `main`；手动 `workflow_dispatch` 只上传 Actions artifact，完整版本标签触发 Release 发布。

关键入口：`packaging/macos/Info.plist`、`src/gui/external_import/macos.rs`（系统事件与重复进程转交）、`src/external_import/ipc/macos.rs`（Unix socket）、`src/external_import/registration/macos.rs`（App 注册）、`vendor/wxdragon/rust/wxdragon-sys/cpp/src/app.cpp`（回调前事件暂存）。GUI 公共预览、兑换、模型查询及保存使用原有实现。

系统接入依据：[Apple URL 类型声明](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleurltypes)、[Launch Services 注册](https://developer.apple.com/documentation/coreservices/1446350-lsregisterurl)。这些说明支持实现方式，不替代本版本的实机验收。

## 本地联调

`python scripts/mock-hub-import.py --port 18765` 启动仅绑定回环地址的模拟站点。打开 `http://127.0.0.1:18765`，选择协议及场景。包含正常、空模型、模型发现失败、过期和同名不同 Key；只返回明显的测试凭据，不记录请求内容。

本机或局域网 HTTP 站点与 HTTPS 站点使用同一导入流程，无需设置开发环境开关。来源须填写运行 Hub 的电脑实际可访问的站点地址；`localhost` / `127.0.0.1` / `[::1]` 指向 Hub 所在电脑，另一台设备上的站点应填写其局域网 IP 或域名。真实站点的接口和模型权限仍需双方联调，模拟结果不能替代真实站点验收。

## 构建与验证

当前源码编译使用 `--bin TianCaiSpaceHub`，示例见 [发布检查表](release-checklist.md)。以下旧 binary 的命令保留历史打包记录，不用于当前源码或后续发布：

```powershell
cargo fmt --check
cargo build --locked --release --features gui --bin codexhub
./scripts/package-hub-import.ps1
```

当前功能测试由用户负责；上述本地打包命令仅为历史维护参考，不用于本版及后续发布安装包。发布包统一由 GitHub Actions 生成。后续按约定需要运行已有自动测试时，当前源码使用 `cargo test --locked --features gui --bin TianCaiSpaceHub`，不将用例更新或编译视为测试通过。

当前 `v0.4.30-2` 的 [Windows Actions 37092082347](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/37092082347) 与 [macOS Actions 37092082348](https://github.com/soupdomplings/TianCaiSpaceHub/actions/runs/37092082348) 均成功，标签源码为 `5114002fceee61c7db6cc7709cbd99b0fecf672c`。Windows MSI/便携 ZIP 未签名；macOS DMG/App ZIP 为 universal、ad-hoc 签名、未公证。发布产物、哈希和静态核对见 [版本交付](releases/v0.4.30-2.md)。本轮未进行功能、安装升级、实机交互或真实调用验收，未替换用户现有安装与配置。

`0.4.29-5` 历史状态：基于 `16eab07` 加当时未提交的 macOS 系统接入。Windows GUI 及测试代码编译核对通过，未执行测试；格式、plist、工作流脚本语法和文档链接核对通过。当时未生成或发布该版本 macOS 安装包，详见 [历史开发状态](releases/v0.4.29-5.md)。其接入代码已随当前 `v0.4.30-2` 成功构建打包；首次浏览器唤起、已运行/隐藏窗口、连续导入、CLI/重复进程转交、App 移动与多副本关联仍待用户在 Mac 验收，当前结果另见 [版本交付](releases/v0.4.30-2.md)。

`0.4.29-4`：导入前确认已移除，Windows release 构建完成，本次流程调整待用户验收。交付目录为 `target/dist/hub-import-0.4.29-4/`，打包时基于 `e25ecd9` 加当时未提交的 HTTP 修复和流程调整；源码随后随 GitHub 提交归档，原包和清单保留。详见 [本地交付说明](releases/v0.4.29-4.md)。

`0.4.29-3`：HTTP/HTTPS 兼容地址修复已实现，Windows release 构建完成；2026-09-30 用户反馈本地导入正常。该反馈不扩展为全部站点、安装升级或后续版本验收。交付目录为 `target/dist/hub-import-0.4.29-3/`，源码基于 `e25ecd9` 加当时未提交变更，保留原包。详见 [本地修复版交付说明](releases/v0.4.29-3.md)。

首批本地交付：`target/dist/hub-import-0.4.29-2/` 中的 Windows x64 MSI、便携 ZIP 和 `build-manifest.json`，构建基于 `9d4a7f4` 加当时开发变更，保留原样。GitHub 预发布按已提交的 `v0.4.29-2` 源码重新构建和打包，使用 `target/dist/release-v0.4.29-2/`；完整提交、源码树及 SHA-256 以该批清单为准，见 [版本交付说明](releases/v0.4.29-2.md)。

`0.4.29-2` 历史验证（2026-09-30）：Windows release 构建完成；已完成的自动测试为 857 项通过、0 项失败、3 项忽略。隔离的中文预览窗口已打开并检查显示，保存交互及后续测试按用户要求停止，交由用户验收。这些结果不覆盖 `0.4.29-3` 及后续变更。未验证真实主站兑换、完整浏览器唤起、MSI 安装升级卸载或 macOS；未进行真实模型调用。现有安装版及其配置未被替换。

交互预览测试使用专用测试入口 `gui::external_import::tests::preview_smoke`（默认忽略），只启动隔离的临时配置及进程内测试后端，不运行实际 daemon、不修改 Codex 环境、不注册系统协议。

主要改动：`src/external_import/` 负责校验、网络、Windows 通信和关联；`src/gui/external_import.rs` 负责预览；CLI/主程序接入启动分发；配置层及 Web API 负责并发合并；Windows WiX 文件加入协议组件。
