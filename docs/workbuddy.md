# WorkBuddy 接入

TianCaiSpace Hub 提供独立的 WorkBuddy 接入页签。WorkBuddy 调用本机的 Chat Completions 兼容地址，Hub 再将请求转发到 AI Gateway 中选择的上游 provider，并自动补充 `prompt_cache_key`。

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
6. 思考强度默认是 `high`，支持列表默认是 `low, medium, high, xhigh`，可按上游实际支持情况修改。WorkBuddy 选择的强度会原样转发；请求没有携带强度时，Hub 会补充这里设置的默认值。
7. 固定缓存键由 provider 自动生成 `workbuddy:<provider>`，例如 provider 名称为 `tiancai` 时是 `workbuddy:tiancai`。同一 provider 的不同模型共用这个缓存命名空间，切换 provider 后自动更新；该字段只读，无需手工填写。
8. 点击“保存 WorkBuddy 配置”。保存会先把现有 `models.json` 复制到 `models.json.bak`，再写入新配置，并更新专用的 `workbuddy` provider；原有 Codex provider 会保留不变。
9. 需要撤销最近一次保存时，点击“还原备份”。还原前当前文件会额外保存为 `models.json.before-restore.bak`，然后恢复 `models.json.bak`；恢复后 Hub 的专用 WorkBuddy provider 也会同步更新。即使当前 `models.json` 损坏，只要备份文件有效，仍可使用还原按钮自救。

## 协议规则

- AI Gateway 的 `OpenAiResponses` provider 自动映射为 `openai-responses`。
- `AnthropicMessages` provider 自动映射为 `anthropic-messages`。
- DeepSeek、Grok、Chat Completions 和其他 provider 类型自动按 `openai-chat` 处理。
- 协议仍可在 WorkBuddy 页签中手动调整，便于第三方兼容服务。

## 配置文件

默认路径：

```text
Windows: %USERPROFILE%\\.workbuddy\\models.json
macOS/Linux: $HOME/.workbuddy/models.json
```

也可以用 `WORKBUDDY_CONFIG_PATH` 指定路径。备份文件与配置文件位于同一目录，名称分别为 `models.json.bak` 和 `models.json.before-restore.bak`。配置文件最外层是模型数组；当前页签管理数组中的第一个模型，同时仍兼容读取旧版单对象文件。WorkBuddy 地址、WorkBuddy Key、上游 URL/Key、模型和缓存键由页签自动维护；用户只需在“大模型接入”中维护 provider，再在本页选择 provider、模型和思考强度。

## 缓存

每个 WorkBuddy 请求都会带有 `prompt_cache_key`，优先级如下：请求体中的显式值、`x-workbuddy-session-id`、已配置 provider 派生的 `workbuddy:<provider>`、旧版 `cacheKey`（仅在没有 provider 配置时兼容使用）、最后才回退到模型名。因此 WorkBuddy 没有主动发送缓存键时，Hub 仍会向支持该字段的上游发送稳定键，触发缓存创建。

Anthropic Messages provider 会按 Claude 的 `cache_control` 规则标记 system、tools 和消息尾部；OpenAI Responses provider 会发送 `prompt_cache_key`，并沿用 AI Gateway provider 的缓存保留时间设置。OpenAI 缓存通常没有单独的“创建成功”响应字段，应在重复发送相同且足够长的前缀后，通过响应 usage 中的 `cached_tokens` 是否大于 0 判断命中。
