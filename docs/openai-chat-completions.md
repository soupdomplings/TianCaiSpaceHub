# OpenAI Chat Completions 接入

维护日期：2026-09-30。适用版本：`0.4.29-2`。关联 TC-005，最初本地定制快照为 `18621b0`。

在“大模型接入”中新增大模型厂商，选择 **OpenAI Chat Completions**，填写上游 Base URL（例如 `https://tiancai.yc99.space/v1`）和自己的 API Key，获取远端模型列表后保存并启用。

Base URL 填到 `/v1` 即可，Hub 自动追加 `/chat/completions`。请选用上游明确支持该接口的模型；模型列表中可见不代表支持所有协议。

Codex 接入步骤不变：勾选可见模型、保存列表、初始化配置，再增强模式启动。Hub 将客户端的 Responses 请求转换为 Chat Completions，并将 JSON 或 SSE 返回转换为客户端需要的 Responses 格式，支持文本和函数工具调用。Responses 专属的托管工具能力取决于转换器及上游支持情况，不能保证等价。

新渠道使用 `providerType = "chat_completions"` 与 `compatibility = "openai_chat"`，发送 `reasoning_effort` 和 `max_completion_tokens`，不添加 DeepSeek 的 `thinking` 参数。原有未设置兼容标记的 Chat 渠道保留旧 DeepSeek 行为。

现有渠道编辑时锁定协议。如需从 Responses 改为 Chat Completions，请新增渠道，确认调用成功后停用原渠道；同一模型有多个启用渠道时仍按 Hub 的路由规则选择。

WorkBuddy 可在其接入页选择此渠道并保存，协议使用 `openai-chat`。

## 关闭推理（按渠道设置）

若上游提示函数工具不能与推理同时使用，可编辑对应 Chat Completions 渠道，勾选“Chat Completions：关闭推理”并保存。Hub 会覆盖客户端传入的推理强度，发送 `reasoning_effort: "none"`，保留工具调用，并移除冲突的 `thinking` / `reasoning` 字段。

此开关默认关闭，配置字段为 `chatDisableReasoning`。只作用于勾选的 Chat Completions 渠道，不改变其他渠道或全局推理设置。取消勾选即可恢复原有行为。WorkBuddy 若使用该渠道的配置副本，需要在 WorkBuddy 接入页重新选择渠道并保存，以同步此开关。

## 网页导入与维护

主站指定 `chat_completions` 时，网页导入创建相同的通用 `openai_chat` 兼容渠道；默认禁用保存，不默认关闭推理。普通渠道编辑锁定协议的规则不等于导入更新不能改变协议：导入预览展示主站协议，用户明确选择更新后才替换目标的协议字段。详见 [导入说明](hub-external-import.md)。

代码入口：[渠道配置](../src/ai_gateway/config.rs)、[协议转换](../src/ai_gateway/providers/deepseek_chat.rs)、[GUI](../src/gui.rs)。迁移旧 Chat 渠道时不可批量补 `openai_chat`，否则会改变原 DeepSeek 行为；保存前备份，回滚时恢复对应渠道配置。当前测试由用户安排，历史验证记录见 [二开变更记录](customizations/CHANGELOG.md)。
