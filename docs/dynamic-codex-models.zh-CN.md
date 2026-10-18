# 动态 Codex 可见模型

维护日期：2026-10-18。当前开发版本：`v0.4.30-5`、唯一 `main`。关联 TC-006、TC-007；本轮继续修复 ULTRA、配置区块缺失及增强启动未同步提示，源码、必要编译、Actions 与待用户验收见 [v5 交付](releases/v0.4.30-5.md#2026-10-18-配置区块与增强启动超时修复)。用户已确认前轮 Fast 标志出现，但 ULTRA 仍未通过验收；只读源码定位不等于测试机故障已实测解决。此前保存流程见 [前轮交付](releases/v0.4.30-5.md#2026-10-12-空模型自动获取与模型未配置提示)，版本沿革见 [二开变更记录](customizations/CHANGELOG.md)，上游来源见 [v0.4.30 整合](upstream-v0.4.30-integration.md)，入口隔离见 [接入点用途与隔离](customizations/client-channel-scope.md)。

Codex 可见模型现在支持内置目录之外的模型。模型来源包括已配置渠道保存的模型列表，以及 Codex 接入页中手动输入的模型 ID。

## 初始化开启 MAX、ULTRA 与 Fast 选项

在“ChatGPT(Codex) 接入”点击“初始化 Codex 配置”或“更新 Codex 配置”时，Hub 同时补齐模型控制选项。已有接入但缺少这些设置时，按钮显示可更新；只升级 Hub、启动或查看接入状态不会自动改写 Codex 配置。操作成功后完全退出 Codex App，再从 Hub“增强启动”打开。配置页、按钮说明、悬浮说明和成功弹窗使用通用接入提示，不额外列出 MAX/ULTRA/Fast。

写入目标沿用当前 Codex home（默认 `~/.codex/config.toml`，支持原有 home 覆盖），Windows/macOS 使用同一 TOML 读写流程：

```toml
[desktop]
enabled-reasoning-efforts = ["low", "medium", "high", "xhigh", "max", "ultra"]

[features]
fast_mode = true
```

已有 `enabled-reasoning-efforts` 数组时，保留原条目、顺序和数组注释，只补缺少的 `max`、`ultra`，不重复追加；只有原键不存在时使用示例六档，原空数组只补两档。其他 desktop/features 设置、当前 `model`、`model_reasoning_effort`、`service_tier` 及 `desktop.default-service-tier` 均保留，不自动切换当前使用的强度或速度。内联表按原有 helper 合并；desktop/features 类型错误、推理列表不是纯文字数组或 Fast 开关不是布尔值时，在写入 config.toml 前提示修正，不清空这些配置来强行初始化。本轮不新增配置字段。

Fast 开关依据 [OpenAI 官方速度配置说明](https://learn.chatgpt.com/docs/agent-configuration/speed)。它不足以独立保证自定义 Provider 的桌面入口显示。只读核对 Codex `26.1013.4894.0` 确认：默认 `requires_openai_auth=false + Actor Authorization` 时，界面没有 ChatGPT 登录态；Fast 的界面和请求辅助函数分别据此返回不可用，ULTRA 的模型列表还受 `1186680773` 控制。前轮 `a68b5a6` 只写配置，用户在同版本、增强启动、多个模型下反馈仅 MAX 显示。

现在增强启动通过原有本机 CDP 会话，定向适配内存中加载的模型控制脚本：只在当前 Provider 确认为本 Hub 受管 `ai-gateway`、且 `features.fast_mode=true` 时启用，具体例外还限定 `hostId=local`。Fast 两处保留认证读取、真实 requirements 查询、缓存、显式禁止项和失败处理，只增加本地 Hub 的可用性分支；不改全局 `authMethod` 或 Provider 身份。ULTRA 只放行本地模型查询中的显示过滤，保留推理列表勾选和模型自身能力；不全局覆盖该 Statsig gate，其他调用仍读官方值。已静态追踪“配置 → 模型功能”的查询入口：`wvo` 默认 `includeUltraReasoningEffort=true`，该页仅传 host；无需改 ChatGPT 账号设置。安装 schema 中的 `show-ultra-in-model-picker-slider` 在此版本不参与这条 Codex 查询，本轮不额外写入。

同一增强会话现在还适配“设置 → 配置”的“模型功能”和“工作空间依赖项”入口。安装源码中它们分别受 `3693343337`、`3026692602` 控制；仅在原本机受管范围内增加例外，保留 Work 页面排除、local host kind 与真实 `primaryRuntime` 检查，中间 `2106641128` 不改。没有依赖运行时的客户端不会凭空出现可安装内容；Hub 不安装、重置或伪造工作空间依赖。设置模块按访问加载，`settingsSections` 独立合并，既不纳入启动 ready，也不纳入首屏刷新条件，不必先打开设置才能启动成功。

该适配使用 Windows/macOS 共用协议，不改 Codex 安装文件、签名或用户套餐。只拦截已知 `app://.../assets/` 的模型与配置脚本响应，四类结构必须唯一匹配且局部变量一致才修改；未知布局或正文获取失败保留原脚本，最多等待 10 秒后放行。增强会话先启用 Network、禁用该会话资源缓存，再启用 Fetch；改写响应移除旧缓存/长度等元数据并加 `Cache-Control: no-store`。browser/page CDP 的帧和消息上限都设为有限 64 MiB：本机脚本原文 14,403,933 字节，base64 为 19,205,244 字节，可能超过库默认 16 MiB 单帧限制；此前中断可静默转成外层 45 秒超时。该缺陷已修，但未取得测试机私有日志，不能认定它是本次故障的唯一原因。

增强启动仍核对 Fast UI、Fast request、Ultra models 三个实际执行标记，source marker 更新为 v2、增强脚本为 v23，不通过放松校验报告成功。连接中断立即反馈本机连接问题；45 秒内未齐则列出实际缺失的模型列表、语言或桌面配置等步骤。GUI 对该启动接口直接显示中文原因，不再套原始 HTTP 500/JSON。首次启动若错过早期挂载，只在该新窗口 load 完成后按需刷新一次，已运行窗口不刷新。维护入口：[配置及恢复](../src/codex_app_config.rs)、[增强启动](../src/codex_app_enhanced.rs)、[模型与设置脚本适配](../src/codex_app_enhanced/model_controls.rs)、[启动错误显示](../src/gui/api.rs)。

MAX/ULTRA 的 desktop 键来自安装 schema 和只读配置白名单，官方通用配置页面未列出这些键；不读取或登记完整个人配置、密钥与私有日志。普通启动、CLI 和其他客户端不经过这条桌面适配。可选项仍须模型真实支持：例如当前目录的 `gpt-6-luna` 有 MAX、无 ULTRA；无速度档的模型不制造 Fast 按钮。启用入口不代表已选择 Fast，不强制请求服务档，也不承诺代理或上游实际提速。

### 备份与恢复

沿用 Hub 受管备份 manifest v2，可选 `model_controls` 记录原推理列表（缺失和空数组区分）、最后写入列表及原 fast_mode，不含账号或秘密，本轮不增加备份字段。旧 v1/v2 在首次显式初始化/更新时补记，不重置原 ChatGPT 登录备份。重复初始化且受管值未变时保留原恢复点；用户手工修改/删除对应项后再显式初始化，分别以本次修改前值为新恢复点。

“恢复配置”仅在当前推理列表仍等于 Hub 最后写入列表、或对应开关仍为 Hub 写入的 true 时，分别还原原值；原键缺失则删除，其他 desktop/features 字段保留。用户后来修改/删除的项不覆盖；没有受管记录时不猜测并删除模型选项。前端适配只在本次增强会话内存中，完全退出 Codex 后结束；不必修复安装包。若需回退旧程序，应先用新程序恢复；旧程序不识别新增备份字段，也不会负责撤回它们。原有逐次 config.toml 备份与原子文件写入继续保留，配置和 manifest 为分步写入，不宣称跨文件事务。

维护入口：[初始化、更新状态与恢复](../src/codex_app_config.rs)（`enable_model_controls`、`desktop_reasoning_efforts`、`managed_gateway_config_is_current`、`restore_model_controls`）、[按钮入口](../src/gui/codex_tab.rs)、[中英文帮助及成功提示](../src/gui/text.rs)。开发方只做必要编译与静态核对，不运行 Hub/Codex、测试夹具或真实模型请求，不改本机正在使用的配置；Windows/macOS 实机分别待用户验收，完整构建身份见本轮交付。

## 厂商保存时自动获取模型

“大模型接入”新增或编辑普通厂商时，模型表中没有任何条目，点击“保存”会先在后台获取远端模型；获取到非空模型列表后继续完成这一次保存。ChatGPT 账号类型也支持该流程，但须已完成账号授权；空列表获取成功后默认选中获取到的全部模型再保存。列表已有条目时仍按原有选择、模型映射和校验规则保存；ChatGPT 有模型但一个也未勾选时仍要求选择，不把未勾选当作空列表重新获取。

开始自动获取前会先校验名称、保留名称限制和正整数优先级；普通厂商还校验服务地址、可选模型列表地址为完整 HTTP(S) 地址。缺少这些信息时直接说明需要修正的字段，不先发送发现请求。成功获取后仍使用原有最终保存校验，不略过模型映射或配置保存保护。

获取失败或返回空列表时弹窗说明失败原因，厂商不会保存，编辑窗口保留，可以修正地址、密钥或账号后重试。普通厂商筛选后没有可用模型、ChatGPT 返回空列表或只有空白模型 ID，也按失败处理，不先清掉已有列表及勾选。自动获取期间暂时禁用保存、获取、厂商选择和相关输入，避免把旧输入的结果保存成新配置；“取消”仍可关闭窗口，迟到的获取结果不继续保存。ChatGPT 模型获取还核对本次授权账号，其他事件不会提前解除获取的忙碌状态。该操作只查询模型目录，不发起推理或计费验证调用。

单独点击“获取远端模型列表”仍只是更新编辑窗口中的列表，不自动保存。下面的“ChatGPT(Codex) 接入 → Codex 可见模型”则是另一处可见模型编辑入口，继续按其原有“保存模型列表”流程工作，不能把厂商编辑器的自动保存规则套用到这里。

维护入口：[普通厂商编辑器与远端获取](../src/gui.rs)、[ChatGPT 账号厂商编辑器](../src/gui/chatgpt.rs)。不增加配置字段、不改变渠道身份、已有模型别名或账号凭据保存方式；旧配置无需迁移。回退程序恢复旧保存流程及手动获取操作，已保存的模型列表仍按旧格式读取。

## 使用方式

1. 在“大模型接入”中配置并启用渠道，填写 API Key 和地址。
2. 在“ChatGPT(Codex) 接入 → Codex 可见模型”选择渠道，点击“获取远端模型列表”；也可点击“同步渠道模型”读取所有已启用渠道中已保存的模型和映射（不包括 WorkBuddy 与天工专用渠道）。
3. 在文本框中保留需要的模型，每行一个，可手动增删。远端获取和同步均保留已有手动输入。勾选需要展示的内置模型。
4. 点击“保存模型列表”。文本框中尚无可用路由的模型将以同名加入所选渠道；已有路由及别名映射保持不变。未选择渠道且存在无路由模型时，会提示选择渠道，不会保存半成品配置。
5. 使用增强模式重新启动 Codex，验证模型下拉框和对话。普通启动的前端模型过滤仍取决于 Codex App 版本。

内置模型继续使用原能力描述。未内置的模型按完整名称前缀匹配同系列内置模型，完整继承其上下文、推理等级、图片、工具、Responses Lite、速度档、压缩参数和基础指令等能力与高级参数；只替换模型 ID、显示名称、说明及展示排序。

- 当前 `gpt-6-luna`、`gpt-6-sol`、`gpt-6-astra`、`gpt-6.1-sol` 都已内置，直接使用各自目录能力。只有未知 ID 才按匹配规则继承 GPT-6 家族模板；不能把 Luna 当成继承 Astra 的例子。
- `0.4.30-1` 新增目录后，未知 `gpt-6-next`、`GPT-6-NEXT`、`vendor/gpt-6-next` 依既有规则默认继承 `gpt-6.1-sol`，此前为 `gpt-6-astra`。默认上下文 `272000`、最大上下文 `872000` 未变，其他能力与高级参数随模板继承；有明确 `capabilityProfile` 或手填字段时仍优先采用用户覆盖。这里是本地目录行为，不是对未知模型真实能力的承诺。
- `gpt-5-new` → GPT-5 系列中数字版本最高的内置模型，当前为 `gpt-5.6-sol`。
- `deepseek-v4-new` → `deepseek-v4-pro`；`deepseek-v4-flash-next` → 更精确匹配的 `deepseek-v4-flash`。
- 优先最长完整前缀，再比较数字版本（6.10 大于 6.9）；版本相同按内置目录顺序选择。大小写和 `vendor/` 命名空间不影响匹配；`gpt-6` 不会误匹配 `gpt-60`。
- 使用任意别名时，也会尝试按该渠道映射的上游模型名继承。
- 手动指定 `capabilityProfile` 优先于自动匹配，可填写具体内置模型 ID 或协议模板名。手动填写的上下文、图片和推理设置最后覆盖，未填写的字段继续继承。

仅当既没有指定模板、也找不到同系列内置模型时，才使用原来的保守配置（32K、文本输入、普通 Responses）。继承规则来自本地模型目录，不代表已向厂商核验新模型的真实规格。

高级能力目前通过 Hub 的 `config.toml` 配置。按上游实际支持情况添加条目后重启 Hub；能力配置变化也会改变模型目录 ETag。不要把示例数值当作某个新模型的官方规格。

```toml
[[aiGateway.codexModelProfiles]]
id = "my-new-model"
displayName = "My New Model"
capabilityProfile = "openai_responses"
contextWindow = 64000
maxContextWindow = 128000
supportsImages = true
supportsReasoning = true
```

`supportsReasoning = true` 保留继承模型的完整推理等级；模板没有推理等级时才补充 low/medium/high。`false` 会清空推理等级。高级配置不是路由绑定，模型别名仍使用渠道的模型映射。旧版 `codexVisibleModels` 配置继续兼容。在 Codex 可见模型编辑页，获取失败不会清空列表，获取操作不自动保存，删除可见模型也不会删除已有渠道模型。

## 客户端所选模型未配置

客户端选中模型不代表 Hub 已有可用路由；仍须在“大模型接入”的对应厂商模型列表中保存这个模型或适用映射。网关找不到符合当前入口的候选渠道时，共用错误说明改为：

> 所选模型「模型ID」不在“大模型接入”的相关模型列表中。请新增大模型厂商，或检查对应厂商的模型列表。

HTTP 状态仍为 `422`，`error.type=invalid_model_error`、`error.code=invalid_model` 不变。网关正文固定中文，不随 GUI 语言切换；Codex 接入帮助也同步中英文配置指引。客户端自己加上的 `unexpected status 422` 或 `Unprocessable Entity` 等外层文字可能继续显示；Hub 改的是返回正文。停用、协议不适用或专用条目模型不匹配也可能导致没有候选，应同时检查对应厂商的启用状态、协议和客户端条目；不跨接入点自动改模型或回退到其他厂商。完整入口隔离与维护位置见 [接入点用途与隔离](customizations/client-channel-scope.md#模型未配置提示)。

## 建议验收

此处为用户验收参考。`0.4.30-1` 已同步目录和未知模型继承的测试期望，Windows 测试代码编译通过，未执行测试或真实模型调用。`v0.4.30-2` 将 WorkBuddy 多条目命名空间一并排除，已通过 Windows/macOS Actions 原生构建及打包并预发布；功能测试、安装升级、实机和真实模型调用仍待用户验收。Windows 未签名、macOS universal/ad-hoc/未公证的安装包信息见 [版本交付](releases/v0.4.30-2.md)。

本轮 `v0.4.30-5` 新增行为另行记录于 [本轮交付](releases/v0.4.30-5.md#2026-10-12-空模型自动获取与模型未配置提示)，不复用上述历史构建作为通过证据。普通/ChatGPT 厂商空列表保存、成功自动保存、失败或空结果留窗、已有列表与未勾选边界、取消和迟到结果、中文错误在各客户端及 Windows/macOS 的显示均待用户验收；开发方不主动运行产品或真实模型调用。

- 新模型 ID 不在内置目录中，手动输入后保存，并验证 Codex 能选择和发送请求。
- 直接获取厂商列表，删掉不需要的条目后保存；检查手动输入仍保留。
- 已有模型别名继续转发为原上游名称；WorkBuddy 与天工专用渠道继续排除普通 Codex 路由。
- 关闭并重启 Hub，确认新增、删除和内置勾选结果保留。
- 使用错误地址或密钥获取列表，确认错误提示及原有列表保留。

## 与网页导入的关系

上述“获取失败保留列表”和“保存时补齐所选渠道路由”属于可见模型编辑页。网页导入采用独立预览：先查询模型，失败时允许空列表禁用保存；默认不追加可见模型，只有勾选后才追加，不替换已有列表。导入也不自动初始化 Codex。见 [网页导入](hub-external-import.md)。

代码定位：[网关配置](../src/ai_gateway/config.rs)、[模型目录](../src/ai_gateway/model.rs)、[Codex 页签](../src/gui/codex_tab.rs)、[GUI 保存与发现](../src/gui.rs)。旧版 `codexVisibleModels` 仍可读取；回退到不支持动态能力的版本前应备份配置，未知模型能力以实际上游为准。
