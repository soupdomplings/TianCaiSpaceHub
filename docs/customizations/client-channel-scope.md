# 接入点用途标识与渠道隔离

维护日期：2026-10-12。当前开发版本：`v0.4.30-5`、唯一 `main`。关联 TC-013、TC-002、TC-011、TC-006；本轮模型未配置中文说明见 [v5 交付](../releases/v0.4.30-5.md#2026-10-12-空模型自动获取与模型未配置提示)。原用途隔离已随 `v0.4.30-2` 预发布，其构建与验收边界保留在 [历史交付](../releases/v0.4.30-2.md)，不作为本轮错误说明验证证据。

## 用户入口

“大模型接入”的渠道列表显示“适用接入点”。WorkBuddy 和天工保存模型时生成专用渠道，分别标为 WorkBuddy 专用、天工 Claw 专用；普通渠道用于 Codex，也可在另两个接入页中被选为上游来源。编辑渠道时显示用途说明，既有专用渠道的内部名称不可在通用编辑器改名，新建普通渠道也不能占用保留名称。

“可选为来源渠道”表示用户在对应接入页保存时复制来源配置，创建专用渠道。来源渠道之后更换 Key、参数或协议，不自动修改已有副本，需重新保存相应模型条目同步。用途标签不改变用户配置的上游协议。

## 实际请求规则

| 渠道身份 | 允许的入口 | 其他入口的处理 |
| --- | --- | --- |
| 普通名称 | Codex Responses、WebSocket、搜索、生图等普通网关入口 | WorkBuddy/天工专用请求不会把它当成缺省回退 |
| `workbuddy` | 旧 WorkBuddy `/ai-gateway/v1/chat/completions`，兼容 `legacy` 条目 | Codex、天工及其他 WorkBuddy 条目排除 |
| `workbuddy:<entryId>` | 对应 `/ai-gateway/workbuddy/<entryId>/v1/chat/completions` | Codex、天工、旧 WorkBuddy 入口及其他条目排除 |
| `gmclaw` | 旧天工 `/ai-gateway/gmclaw/v1/chat/completions`，兼容 `legacy` 条目 | Codex、WorkBuddy 及其他天工条目排除 |
| `gmclaw:<entryId>` | 对应 `/ai-gateway/gmclaw/<entryId>/v1/chat/completions` | Codex、WorkBuddy、旧天工入口及其他条目排除 |

过滤发生在候选渠道选择时，先于权重、健康状态及会话粘性。同模型名也不跨接入点或条目选择；条目被删、被停用、模型不匹配时返回错误，不绕到另一个渠道。旧无条目地址只选择旧专用渠道，不能随多模型增加而自动接管新条目。

保留名称不区分大小写，完整 `workbuddy:`、`gmclaw:` 命名空间都被普通路由排除，包括后缀不合法的名称。条目地址会验证 ID；错误 ID 不退回普通请求。Codex WebSocket 握手能力预筛选与续轮渠道复验同样排除这些渠道，只有专用 OpenAI 渠道时不误开放 Codex WebSocket 握手。

动态模型同步、补路由、WorkBuddy/天工来源下拉框及网页导入也使用同一保留判断。网页导入不能创建或覆盖专用渠道。这里是网关入口的路由归属，不是按发起请求的进程名鉴权；本机客户端仍须使用对应接入页给出的地址。

## 模型未配置提示

候选渠道缺失仍由共用 `GatewayError::invalid_model` 返回 `HTTP 422`、`error.type=invalid_model_error`、`error.code=invalid_model`；本轮将正文改为以下说明，其中保留客户端请求的模型 ID。网关正文固定中文，不随 GUI 语言切换；客户端自行显示的 HTTP 状态英文外层不由 Hub 控制。

> 所选模型「模型ID」不在“大模型接入”的相关模型列表中。请新增大模型厂商，或检查对应厂商的模型列表。

该错误既可能表示模型未加入适用厂商，也可能来自停用、协议筛选或 WorkBuddy/天工专用条目的模型不符。应检查“大模型接入”中对应厂商的模型列表、启用状态和协议，并核对客户端条目。错误文字不改变候选选择、权重、健康或粘性规则，不会自动跨接入点、换模型或新增路由；无效 entry ID、`previous_response_not_found` 和上游厂商错误继续使用各自原有处理。

正文定义在 [网关共用错误](../../src/ai_gateway/error.rs)，由 [Responses 等普通路由](../../src/ai_gateway/router.rs)、[Chat 分派](../../src/ai_gateway/handler.rs) 和 [WebSocket](../../src/ai_gateway/websocket/mod.rs) 的原有错误路径使用；[Codex 接入帮助](../../src/gui/text.rs) 同步中英文配置指引，不再引用旧英文错误正文。HTTP 状态与机器可读字段保持兼容，不需要迁移配置；回退恢复英文正文，但不会恢复缺失模型或更改已有隔离。厂商空列表保存时自动获取见 [动态模型](../dynamic-codex-models.zh-CN.md#厂商保存时自动获取模型)。

## 配置兼容与维护

用途由内部渠道名称派生，不增加用户必须迁移的 `scope` 配置字段，也不把展示文字写入渠道 ID。旧 `workbuddy`、`gmclaw` 自动获得正确标识，已有天工多条目保持不变。

- [渠道身份判断](../../src/ai_gateway/config.rs)、[精确入口路由](../../src/ai_gateway/router.rs)、[Chat 请求分派](../../src/ai_gateway/handler.rs)、[WebSocket](../../src/ai_gateway/websocket/mod.rs)。
- [列表及用途说明](../../src/gui/ai_gateway.rs)、[通用界面](../../src/gui.rs)、[账号渠道界面](../../src/gui/chatgpt.rs)、[网页导入](../../src/external_import.rs)。
- 模型新增、删除及撤销分别见 [WorkBuddy](../workbuddy.md) 和 [天工模型](gmclaw.md)。

回退到 `0.4.30-1` 会失去新用途标识和 WorkBuddy 多条目路由，且旧版不认识 `workbuddy:` 命名空间，可能把新条目渠道当作普通渠道。降级前先用本版本删除或撤销新增 WorkBuddy 条目、确认这些专用渠道已移除，并备份两端配置；不要让新旧 Hub 同时保存。天工既有多条目本轮无格式迁移。

已补充同模型、不同权重、错误粘性、缺失/停用条目及 WebSocket 握手的隔离夹具。Windows/macOS Actions 原生构建和打包均通过，已随 `v0.4.30-2` 预发布交付。按用户分工未执行功能测试、安装升级、应用实机交互或真实调用；这些行为仍待用户验收，不复用历史版本测试数量。Windows 未签名及 macOS universal/ad-hoc/未公证的产物状态见 [版本交付](../releases/v0.4.30-2.md)。
