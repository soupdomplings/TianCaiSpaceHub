# WorkBuddy 接入

维护日期：2026-10-03。基础能力适用版本：`0.4.29-2`；文末天工衔接及非流式错误处理为 `0.4.29-5` 工作区新增变更。关联 TC-002、TC-003、TC-004、TC-011；本页维护当前行为，历史开发和验证结果见 [二开变更记录](customizations/CHANGELOG.md)。

TianCaiSpace Hub 提供独立的 WorkBuddy 接入页签。WorkBuddy 调用本机的 Chat Completions 兼容地址，Hub 再将请求转发到 AI Gateway 中选择的上游 provider，并按上游协议处理思考强度和缓存参数。

## 推荐配置

1. 先打开“大模型接入”页签，保存至少一个 provider，并在该 provider 中填写上游 Base URL、API Key 和模型列表。
2. 打开“WorkBuddy 接入”页签，点击“刷新提供商与 WorkBuddy 配置”。
3. 在“上游提供商（来自 AI Gateway）”中选择刚才保存的 provider。
4. 在模型下拉框中选择该 provider 已配置的模型或模型别名。URL、Key 和模型列表会随 provider 自动带出；协议默认按 provider 自动选择，特殊兼容服务仍可手动调整。
5. 使用自动生成的本机 WorkBuddy 地址和 Key：

   ```text
   http://127.0.0.1:3847/ai-gateway/v1
   ```

   WorkBuddy 在该地址后追加 `/chat/completions`。默认 Key 为 `workbuddy-local`，它只是 WorkBuddy 发给本机 Hub 的客户端字段，不是上游 API Key；当前本机端点不校验该字段。
6. 思考强度改为下拉选择，随 provider、模型（含别名实际指向）和协议自动刷新。Claude 统一提供 `low, medium, high, xhigh, max` 五档，不为旧版 Claude 单独分档；OpenAI 等其他模型默认提供 `low, medium, high, xhigh`。GLM 的 Anthropic 兼容配置沿用适配器的 `high, max` 两档。默认值初始为 `high`，切换时保留仍然适用的选择，不适用则回退为 `high`。请求未携带强度时，Hub 补充保存的默认值；Claude 原生请求转换为 `thinking.type=adaptive` 和 `output_config.effort`。
7. OpenAI 协议的固定缓存键由 provider 自动生成 `workbuddy:<provider>`，无需手填。Anthropic 原生协议显示“不需要（使用原生缓存）”，保存时省略 `cacheKey`，由 Hub 自动添加 `cache_control` 标记。Claude 经 OpenAI 兼容协议接入时，缓存仍按所选协议处理。
8. 点击“保存 WorkBuddy 配置”。保存会先把现有 `models.json` 复制到 `models.json.bak`，再写入新配置，并更新专用的 `workbuddy` provider；原有 Codex provider 会保留不变。
9. 需要撤销最近一次保存时，点击“还原备份”。还原前当前文件会额外保存为 `models.json.before-restore.bak`，然后恢复 `models.json.bak`；恢复后 Hub 的专用 WorkBuddy provider 也会同步更新。即使当前 `models.json` 损坏，只要备份文件有效，仍可使用还原按钮自救。

## 协议规则

- AI Gateway 的 `OpenAiResponses` provider 自动映射为 `openai-responses`。
- ChatGPT 账号登录渠道同样使用 `openai-responses`。先在“大模型接入”完成登录并配置模型，WorkBuddy 会复用账号凭证引用和自动刷新能力，无需填写账号令牌；上游固定使用官方账号接口。
- `AnthropicMessages` provider 自动映射为 `anthropic-messages`。
- DeepSeek、Grok、Chat Completions 和其他 provider 类型自动按 `openai-chat` 处理。
- 协议仍可在 WorkBuddy 页签中手动调整，便于第三方兼容服务。

## 502 / 503 自动重试

WorkBuddy 专用渠道收到上游 HTTP `502` 或 `503` 时，Hub 会在返回错误前自动重试，最多额外重试 2 次，分别等待 1 秒、2 秒。重试使用同一上游地址、模型、请求体和缓存键，不会切换提供商；Responses、Chat Completions 和 Anthropic Messages 协议均适用。HTTP 错误和已有的传输错误重试共用次数上限，避免叠加重试。

该规则只针对尚未成功建立响应的上游 HTTP 请求。成功返回流式响应后，即使流中报错或中途断开，也不会重新发送已经开始的请求。持续失败时保留最后一次上游错误；`401`、`403`、`422` 等其他 HTTP 状态不触发这项重试。运行日志中的 `retrying upstream HTTP error` 会记录状态码和重试次数，不记录密钥或请求正文。

## 配置文件

默认路径：

```text
Windows: %USERPROFILE%\\.workbuddy\\models.json
macOS/Linux: $HOME/.workbuddy/models.json
```

也可以用 `WORKBUDDY_CONFIG_PATH` 指定路径。备份文件与配置文件位于同一目录，名称分别为 `models.json.bak` 和 `models.json.before-restore.bak`。配置文件最外层是模型数组；当前页签管理数组中的第一个模型，同时仍兼容读取旧版单对象文件。WorkBuddy 地址、WorkBuddy Key、上游 URL/Key、模型和缓存键由页签自动维护；用户只需在“大模型接入”中维护 provider，再在本页选择 provider、模型和思考强度。

## 缓存

OpenAI 协议的缓存键优先级为：请求体中的有效 `prompt_cache_key`、`x-workbuddy-session-id`、已配置 provider 派生的 `workbuddy:<provider>`、旧版 `cacheKey`（仅在没有 provider 配置时兼容使用）、最后回退到模型名。缺失、`null`、空字符串和纯空白均视为未提供缓存键。相同 provider 下切换模型保留稳定的键，但不意味着不同模型可以共享实际缓存。

Anthropic Messages provider 会按 Claude 的 `cache_control` 规则标记 system、tools 和消息尾部，使用原生默认缓存期限。WorkBuddy 请求有无缓存键都能使用这些标记；上游 Messages 请求不发送 OpenAI 的 `prompt_cache_key` 或 `prompt_cache_retention`。Hub 内部仍保留稳定的会话标识。

OpenAI Responses / Chat Completions provider 会发送 `prompt_cache_key`，并沿用 AI Gateway provider 的缓存保留时间设置。缓存键或缓存标记均不保证命中，实际取决于上游支持、前缀长度和前缀是否相同。可通过响应 usage 中的缓存读取 token 统计判断；OpenAI 通常对应 `cached_tokens`，Anthropic 原生对应 `cache_read_input_tokens`。

## 维护与关联功能

代码入口：[界面及选择联动](../src/gui/workbuddy.rs)、[配置与备份](../src/workbuddy_config.rs)、[请求转换](../src/ai_gateway/workbuddy.rs)、[上游重试](../src/ai_gateway/providers/mod.rs)、[请求分发](../src/ai_gateway/handler.rs)。普通 Codex 模型路由排除保留名称 `workbuddy`，避免两种客户端相互抢占路由。

网页导入创建的是普通渠道，不自动写 WorkBuddy 配置；导入并启用后，可在本页主动选择并保存。网页导入禁止创建或覆盖保留名称 `workbuddy`、`gmclaw` 及多模型条目 `gmclaw:<entryId>`。WorkBuddy 的来源列表同样排除这些专用渠道，天工 Claw 不会占用 WorkBuddy 的配置或请求入口。修改来源渠道的 Key、协议或关闭推理选项后，需要重新保存 WorkBuddy 配置以同步副本。

2026-10-03：共享 Chat Completions 转换在非流式 Responses JSON 返回 `status=failed/cancelled` 或非空 `error` 时返回网关错误，保留失败日志，不再将它转换成空的成功回复。该行为同时用于 WorkBuddy 和天工 Claw；原有 WorkBuddy 协议、思考与缓存配置继续保留。

同日用户反馈修复新增的 SSE 聚合仅用于天工专用渠道的非流式客户端请求，不改变 WorkBuddy 的流式转发方式；详见 [天工流式兼容](customizations/gmclaw.md)。

当前用户负责测试；本轮未重新验证 WorkBuddy 或真实上游调用。已有模拟回归结果按原版本登记，不代表所有兼容站点均已验收。
