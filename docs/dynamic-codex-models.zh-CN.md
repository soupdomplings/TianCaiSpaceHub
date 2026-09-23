# 动态 Codex 可见模型

Codex 可见模型现在支持内置目录之外的模型。模型来源包括已配置渠道保存的模型列表，以及 Codex 接入页中手动输入的模型 ID。

## 使用方式

1. 在“大模型接入”中打开一个渠道，填写 API Key 和地址，点击“获取远端模型列表”并保存渠道。
2. 打开“Codex 接入”页面，在“Codex 可见模型”区域点击“同步渠道模型”。
3. 在文本框中补充手动模型 ID，每行一个；勾选需要展示的内置模型。
4. 点击“保存模型列表”，然后重新获取 Codex 模型列表或使用增强模式启动 Codex。

新模型会根据渠道类型套用默认能力模板。内置目录中的模型继续使用完整能力描述；动态模型会生成 `codexhub-dynamic-v1` 能力指纹，并可通过 `codexModelProfiles` 配置显示名称、上下文长度、图片和推理能力。

```json
{
  "id": "gpt-6-luna",
  "displayName": "GPT-6 Luna",
  "provider": "openai",
  "upstreamModel": "gpt-6-luna",
  "capabilityProfile": "openai_responses",
  "contextWindow": 272000,
  "maxContextWindow": 872000
}
```

手动加入模型只会让它出现在 Codex 列表中；要真正发送请求，仍需在至少一个模型渠道中配置相同模型，或配置模型映射。旧版 `codexVisibleModels` 配置继续兼容。
