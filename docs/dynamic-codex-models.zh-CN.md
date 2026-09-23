# 动态 Codex 可见模型

Codex 可见模型现在支持内置目录之外的模型。模型来源包括已配置渠道保存的模型列表，以及 Codex 接入页中手动输入的模型 ID。

## 使用方式

1. 在“大模型接入”中配置并启用渠道，填写 API Key 和地址。
2. 在“Codex 接入 → Codex 可见模型”选择渠道，点击“获取远端模型列表”；也可点击“同步渠道模型”读取所有已启用渠道中已保存的模型和映射（不包括 WorkBuddy 专用渠道）。
3. 在文本框中保留需要的模型，每行一个，可手动增删。远端获取和同步均保留已有手动输入。勾选需要展示的内置模型。
4. 点击“保存模型列表”。文本框中尚无可用路由的模型将以同名加入所选渠道；已有路由及别名映射保持不变。未选择渠道且存在无路由模型时，会提示选择渠道，不会保存半成品配置。
5. 使用增强模式重新启动 Codex，验证模型下拉框和对话。普通启动的前端模型过滤仍取决于 Codex App 版本。

内置模型继续使用原能力描述。动态模型复用协议模板的基本工具和指令结构，但不会自动声明原模型的全部能力。默认使用普通 Responses、32K 上下文、文本输入，不启用推理等级、图片、搜索专用工具、WebSocket、Responses Lite 和付费速度档。上游只返回 ID 时，这些是兼容默认值，不是模型真实性能规格。

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

`supportsReasoning = true` 提供 low/medium/high 三档，默认 medium；高级配置不是路由绑定，模型别名仍使用渠道的模型映射。旧版 `codexVisibleModels` 配置继续兼容。获取失败不会清空列表，获取操作不自动保存，删除可见模型也不会删除已有渠道模型。

## 建议验收

- 新模型 ID 不在内置目录中，手动输入后保存，并验证 Codex 能选择和发送请求。
- 直接获取厂商列表，删掉不需要的条目后保存；检查手动输入仍保留。
- 已有模型别名继续转发为原上游名称；WorkBuddy 配置不变。
- 关闭并重启 Hub，确认新增、删除和内置勾选结果保留。
- 使用错误地址或密钥获取列表，确认错误提示及原有列表保留。
