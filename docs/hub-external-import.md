# Hub 网页导入（Windows）

维护日期：2026-09-30。关联 TC-009、TC-010。本文维护当前实现；需求字段见 [接口契约](HUB_EXTERNAL_IMPORT_CONTRACT_V1.md)，开发沿革见 [二开变更记录](customizations/CHANGELOG.md)。

版本：0.4.29-2。实现 Sub2API 外部导入约定 v1，默认禁用保存。macOS 后续开发，Linux 不在本次范围；本次不宣称这两个平台支持网页唤起。

## 用户流程

1. Windows MSI 安装版自动注册 `tiancaispacehub` 协议。便携版先在“文件 → 注册网页导入…”确认关联到当前程序位置。
2. 在兼容站点的 API Key 页面选择导入 TianCaiSpace Hub。浏览器可能要求确认打开外部应用。
3. Hub 未运行时启动；已运行时将请求交给现有窗口。旧版 Hub 无法接收时明确提示关闭旧版并重新发起，不静默丢弃。
4. 官方来源 `https://tiancai.yc99.space` 直接兑换；其他 HTTPS 来源首次在本次运行中需确认。模型列表地址与来源不同，在发送 Key 前单独确认目标。来源和模型服务可以是不同域名。
5. Hub 兑换短时码，然后查询远端模型；主站给出的模型范围非空时，取远端结果与该范围的交集。只查询模型目录，不发起推理或付费测试。
6. 预览中确认名称、协议、模型服务地址及模型列表；Key 始终遮罩。可删减或手填模型。默认“启用”和“追加 Codex 可见模型”不勾选。
7. 同来源和 Key 默认选择更新；同名不同 Key 默认添加编号新建，也可明确选择更新目标。更新保留原有权重、超时、缓存、推理设置及模型映射；需要替换映射时单独勾选。
8. 空模型或发现失败仍可禁用保存，之后在“大模型接入”编辑渠道并获取/填写模型，补全后启用。启用同名模型渠道会参与现有优先级及会话路由，不自动抢占优先级。
9. 追加可见模型不初始化或切换 Codex 全局配置。首次接入仍通过“Codex 接入”完成初始化；新增模型后按需重新打开 Codex。

## 状态与保存

- URL 仅携带来源和一次性导入码；严格检查字段、长度、版本和地址。不跟随兑换或模型发现重定向，不关闭 TLS 校验。
- 导入码及待确认 Key 只保存在内存；退出或取消即丢弃。诊断文本不引用远端错误正文、完整导入 URL 或 Key。
- Windows 使用当前用户 SID 限制的命名管道转交，并检查接收进程用户；不创建磁盘票据队列。不使用公开 HTTP 接口接收网页深链接。
- 重复链接去重；并发导入最多 8 个，管道接收有长度、容量及超时限制。已确认来源的后续票据会继续兑换，预览按顺序显示。
- 保存会重读最新配置并仅合并目标渠道；目标在预览期间已变化则拒绝覆盖。其他页面的过期整份配置保存也会被拒绝，提示刷新重试。
- 配置写入使用文件锁、内容版本和同目录临时文件原子替换；`_revision` 仅用于本地 API，不写入 TOML。`importSource` 随渠道保存，改名不会丢失来源身份。
- 导入渠道的空模型启用检查同时覆盖后续普通配置保存。旧版 Hub 不具备版本冲突检查，联调或回退时不要与新版同时编辑同一配置文件。

## 协议关联、升级与回退

- MSI 使用 `HKLM\Software\Classes\tiancaispacehub`，随 MSI 安装/升级/卸载管理。原有安装路径和产品 UpgradeCode 保持一致。
- 便携注册使用 `HKCU\Software\Classes\tiancaispacehub`，不需要管理员权限。Windows 的用户级关联可能优先于 MSI 的系统级关联；改用安装版时，在原便携版解除注册，或在新程序中重新注册到新位置。
- 移动便携文件后需重新注册。解除注册前同时核对 owner 和启动命令，只删除当前程序路径拥有的用户关联；不删除其他安装位置的关联。
- CLI 支持 `register-web-import`、`unregister-web-import` 和 `import-url "tiancaispacehub://import/v1?..."`；用户流程优先使用菜单和网页。
- 回退前关闭新版 Hub、备份配置，再安装旧包。需要撤销导入时，在“大模型接入”删除此次新建渠道，或从自己的配置备份恢复被更新渠道。一次性导入码无法重用。

## 本地联调

`python scripts/mock-hub-import.py --port 18765` 启动仅绑定回环地址的模拟站点。打开 `http://127.0.0.1:18765`，选择协议及场景。包含正常、空模型、模型发现失败、过期和同名不同 Key；只返回明显的测试凭据，不记录请求内容。

开发环境须显式设置 `TIANCAISPACE_IMPORT_ALLOW_LOCALHOST=1` 才接受 HTTP localhost、127.0.0.1 或 ::1。正式 HTTPS 站点不需要此开关。真实主站的接口和模型权限仍需双方联调，模拟结果不能替代真实站点验收。

## 构建与验证

```powershell
cargo fmt --check
cargo test --locked --features gui --bin codexhub
cargo build --locked --release --features gui --bin codexhub
./scripts/package-hub-import.ps1
```

打包输出包含 MSI、ZIP、程序及 SHA-256 清单；构建清单说明源提交和是否包含未提交变更。测试包未签名，不自动发布远程版本。

首批本地交付：`target/dist/hub-import-0.4.29-2/` 中的 Windows x64 MSI、便携 ZIP 和 `build-manifest.json`，构建基于 `9d4a7f4` 加当时开发变更，保留原样。GitHub 预发布按已提交的 `v0.4.29-2` 源码重新构建和打包，使用 `target/dist/release-v0.4.29-2/`；完整提交、源码树及 SHA-256 以该批清单为准，见 [版本交付说明](releases/v0.4.29-2.md)。

2026-09-30：Windows release 构建完成；已完成的自动测试为 857 项通过、0 项失败、3 项忽略。隔离的中文预览窗口已打开并检查显示，保存交互及后续测试按用户要求停止，交由用户验收。未验证真实主站兑换、完整浏览器唤起、MSI 安装升级卸载或 macOS；未进行真实模型调用。现有安装版及其配置未被替换。

交互预览测试使用专用测试入口 `gui::external_import::tests::preview_smoke`（默认忽略），只启动隔离的临时配置及进程内测试后端，不运行实际 daemon、不修改 Codex 环境、不注册系统协议。

主要改动：`src/external_import/` 负责校验、网络、Windows 通信和关联；`src/gui/external_import.rs` 负责预览；CLI/主程序接入启动分发；配置层及 Web API 负责并发合并；Windows WiX 文件加入协议组件。
