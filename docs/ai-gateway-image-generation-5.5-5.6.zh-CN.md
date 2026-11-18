# ChatGPT / Codex 默认生图模型与生图协议

## 在 Hub 里指定默认生图模型（V1.0.36 本地测试）

Windows 安装与生图、改图的实机验收步骤见 [V36 交付](releases/v1.0.36.md)。

在 **ChatGPT / Codex → 接入设置 → 默认生图模型** 选择图片模型。保存后，下一个经 Hub 受管接入发起的生图或改图请求即读取新设置，无需重启客户端。不会修改本次对话的 GPT 模型、聊天模型列表或已有渠道。旧配置默认「沿用客户端默认」；随时选回此项即可恢复原行为。

可选 `gpt-image-2.6-sunburst`、`gpt-image-2.6-flare`，或已启用渠道提供的模型 ID／公开别名。预设表示可选型号，不代表你的渠道已开通。[Codex 内置图片工具说明](https://learn.chatgpt.com/docs/image-generation)将其模型标为 `gpt-image-2`，而 [Responses 的图片工具参数](https://developers.openai.com/api/reference/resources/responses/methods/create)列有以上两个 2.6 型号。前者解释了为什么客户端仍可能发来旧型号，即使上游已支持新型号。

| 请求类型 | Hub 怎样应用选择 | 在哪里核对 |
| --- | --- | --- |
| 独立生成／改图（Images JSON） | 用所选公开 ID 匹配已启用渠道，再按该渠道的别名映射向其 Images 接口发送。没有路由则报错，不退回旧型号。 | 请求日志列表为选路用的 ID。开启详情后，比较同一条 `image_generation` 或 `image_edit` 的「Codex 请求」与「上游请求」中的 `model`；后者是 Hub 发出的型号，图片数据沿用摘要。 |
| 随对话提交的托管图片工具（Responses HTTP / WebSocket） | 仅更新已声明的 `image_generation` 工具的 `model`；仍由当前聊天渠道执行，该渠道必须支持指定的图片模型。顶层聊天模型不变，已过滤的工具不会重新开启。 | 查看对话请求详情中上游的图片工具声明；列表仍是聊天模型。 |

Hub 仅在受管 ChatGPT / Codex 请求未指定图片模型，或使用旧默认名（精确的 `gpt-image-2` 或 `image2`）时替换。显式指定其他图片型号、其他客户端及不经 Hub 的请求均保留原行为。若选了渠道别名，例如 `studio → gpt-image-2.6-sunburst`，独立请求的日志列表会显示 `studio`，上游请求显示 `gpt-image-2.6-sunburst`。这能核对 Hub 发出的请求；渠道内部如何执行由提供方决定。

维护入口：`src/gui/codex_tab/image_model.rs`（候选与保存）、`aiGateway.codexImageModel`（可选字段）、`src/ai_gateway/providers/openai_images.rs`（两类请求的默认值与独立图片转发）、`src/ai_gateway/handler.rs` 和 `src/ai_gateway/websocket/native.rs`（HTTP、WebSocket 接入）。本轮验证记录见[二开变更记录](customizations/CHANGELOG.md)。

## 历史：5.5 与 5.6 Responses Lite 生图协议变化

2026-09-22 复核：独立 Images API 链路仍在，当前工具请求模型为 `gpt-image-2`。工具可见性不能只依据 `requires_openai_auth`：Core 还检查有效生图 feature、非 Free 套餐、模型图像输入能力、provider capabilities，以及 Actor 或 Codex backend 认证。最新配置条件见 [Provider 取舍文档](codex-app-web-run-model-visibility-tradeoff.zh-CN.md)。下文模型版本对比保留历史迁移背景，不表示只要切换到 5.6 就自动获得生图能力。

## 1. 文档目的

本文记录 Codex App 生图能力从旧版 Responses 托管工具迁移到新版独立 `image_gen` 工具后的协议变化，以及 CodexHub AI Gateway 必须提供的兼容接口。

本文记录迁移时的 Codex 源码路径；配置条件以文首所链接的最新复核为准：

- `references/codex-main/codex-rs/models-manager/models.json`
- `references/codex-main/codex-rs/ext/image-generation/src/tool.rs`
- `references/codex-main/codex-rs/ext/image-generation/src/backend.rs`
- `references/codex-main/codex-rs/codex-api/src/endpoint/images.rs`
- `references/codex-main/codex-rs/app-server/tests/suite/v2/imagegen_extension.rs`

## 2. 先说结论

Codex 生图存在两套不同架构：

1. 旧版 hosted `image_generation`：图片由 Responses 上游执行，base64 直接随 `/responses` 返回。
2. 新版 standalone `image_gen`：Codex App 本地执行工具，再单独请求 `/images/generations` 或 `/images/edits`，最后把图片作为工具结果提交给下一次 `/responses`。

`use_responses_lite = true` 并不直接规定必须使用 `/images/generations`。真正决定新链路的是 Codex App 新增的 standalone image-generation extension。GPT-5.6 同时启用了 Responses Lite 和 code mode，所以新链路在 5.6 上表现得最明显。

## 3. 模型配置差异

当前 Codex 模型目录中的关键配置如下：

| 模型 | `use_responses_lite` | `tool_mode` | 工具呈现方式 |
| --- | --- | --- | --- |
| `gpt-5.5` | `false` | 未指定 | 工具可以直接作为顶层 Responses tool/namespace 出现 |
| `gpt-5.6-sol` | `true` | `code_mode_only` | 工具通过 `exec` 中的嵌套工具表调用 |
| `gpt-5.6-terra` | `true` | `code_mode_only` | 工具通过 `exec` 中的嵌套工具表调用 |
| `gpt-5.6-luna` | `true` | `code_mode_only` | 工具通过 `exec` 中的嵌套工具表调用 |

因此，5.5 和 5.6 的主要表面差异是：

- 5.5 普通 Responses 可以看到顶层 `image_gen` namespace。
- 5.6 Responses Lite 通常只看到顶层 `exec`，`image_gen__imagegen` 被注册在 `exec` 的嵌套工具表和描述中。

但只要调用的是新版 standalone `image_gen`，最终都会由 Codex App 再请求独立 Images API。

## 4. 旧版 hosted image_generation

旧链路由 Responses 服务端托管生图工具。

### 4.1 请求

Codex 在 `/v1/responses` 请求的顶层 `tools` 中声明：

```json
{
  "model": "gpt-5.5",
  "tools": [
    {
      "type": "image_generation"
    }
  ],
  "input": []
}
```

### 4.2 返回

上游在同一个 Responses 响应或 SSE 流中返回：

```json
{
  "type": "image_generation_call",
  "id": "ig_123",
  "status": "completed",
  "revised_prompt": "...",
  "result": "<image-base64>"
}
```

### 4.3 调用链

```text
Codex App
  -> POST /v1/responses
  -> Responses 上游执行 image_generation
  <- image_generation_call.result = base64
```

从 Codex 客户端视角看，这条链路只访问 `/responses`。图片服务在上游内部如何实现，对 Codex App 不可见。

## 5. 新版 standalone image_gen

新版 Codex App 安装了独立 image-generation extension。模型调用的是 Codex 本地工具，而不是 Responses 服务端托管的 `type=image_generation`。

### 5.1 GPT-5.5 普通 Responses

在支持 namespace tool 的情况下，5.5 可以直接收到类似工具声明：

```json
{
  "type": "namespace",
  "name": "image_gen",
  "tools": [
    {
      "type": "function",
      "name": "imagegen"
    }
  ]
}
```

模型直接调用 `image_gen.imagegen`，Codex App 本地执行该工具。

### 5.2 GPT-5.6 Responses Lite

5.6 使用 `tool_mode = code_mode_only`。模型通常通过 `exec` 调用：

```javascript
const result = await tools.image_gen__imagegen({
  prompt: "生成一张图片"
});
generatedImage(result);
```

`image_gen__imagegen` 不一定作为普通顶层 tool 出现，而是包含在 `input[].type = "additional_tools"` 的 `exec` 工具描述和嵌套工具注册表中。

### 5.3 独立图片请求

Codex App 本地工具收到调用后，会使用当前 model provider 的 base URL 请求：

```http
POST /images/generations
Content-Type: application/json
```

当前 Codex 源码固定使用的图片模型是 `gpt-image-2`：

```json
{
  "model": "gpt-image-2",
  "prompt": "生成一张图片",
  "background": "auto",
  "quality": "auto",
  "size": "auto"
}
```

编辑图片时请求：

```http
POST /images/edits
Content-Type: application/json
```

请求中的图片使用 data URL：

```json
{
  "model": "gpt-image-2",
  "prompt": "给图片加一顶红帽子",
  "images": [
    {
      "image_url": "data:image/png;base64,..."
    }
  ]
}
```

注意：这是当前 Codex backend 使用的 JSON 形态，不是传统 OpenAI Images Edit API 常见的 multipart 上传形态。上游必须支持该 JSON 结构，或者由网关做额外协议转换。

### 5.4 完整调用链

```text
第一次 POST /v1/responses
  -> GPT-5.6 返回 exec custom tool call
  -> exec 调用 tools.image_gen__imagegen
  -> Codex App 本地 image-generation extension
  -> POST /images/generations 或 /images/edits
  <- data[0].b64_json
  -> Codex 将图片包装成 custom tool output
第二次 POST /v1/responses
  -> 模型继续处理并生成最终回复
```

Codex 源码测试明确断言了 standalone image generation 会产生两次 Responses 请求，并在两次请求之间调用独立 Images API。

## 6. 为什么 CodexHub 当前返回 404

CodexHub 写入 Codex 配置的 model provider base URL 是：

```text
http://127.0.0.1:3847/ai-gateway/v1
```

Codex Images client 在该 base URL 后追加 `images/generations`，所以实际请求为：

```text
POST http://127.0.0.1:3847/ai-gateway/v1/images/generations
```

图片编辑请求为：

```text
POST http://127.0.0.1:3847/ai-gateway/v1/images/edits
```

此前 CodexHub 只注册了 `/ai-gateway/v1/responses` 和 `/ai-gateway/v1/models`，没有注册 Images API，因此请求尚未进入任何上游渠道就在本地返回 404。

## 7. CodexHub 需要实现的能力

### 7.1 路由

AI Gateway 需要提供：

- `POST /ai-gateway/v1/images/generations`
- `POST /ai-gateway/v1/images/edits`

### 7.2 Provider 选择

图片请求体携带 `model = "gpt-image-2"`，但不可靠地携带当前对话模型。因此不能根据当前对话使用的是 `gpt-5.6-sol`、`gpt-5.5` 或其他模型来推断图片渠道。

CodexHub 使用现有模型路由规则：

1. 查找已启用且声明支持 `gpt-image-2` 的 Provider。
2. 支持通过 `modelAliases` 把 `gpt-image-2` 映射成上游实际图片模型。
3. 多个 Provider 同时支持时，继续使用现有权重和稳定路由规则。
4. 没有 Provider 声明该模型时，返回明确的 `invalid_model`，不静默选择任意对话渠道。

示例：

```toml
[[aiGateway.providers]]
name = "image-provider"
enabled = true
providerType = "open_ai_responses"
baseUrl = "https://example.com/v1"
apiKey = "..."
models = ["gpt-image-2"]
```

如果上游模型名不同：

```toml
[aiGateway.providers.modelAliases]
"gpt-image-2" = "upstream-image-model"
```

`gpt-image-2` 只用于 AI Gateway 路由，不需要加入 `codexVisibleModels`，否则会错误地出现在 Codex App 的对话模型选择器里。

### 7.3 透明转发

网关应当：

- 保留 Codex 请求的 JSON 结构。
- 只按照 `modelAliases` 替换上游模型名。
- 使用 Provider 的 API Key 和 base URL。
- 复用 Provider 超时、传输重试和错误映射逻辑。
- 原样返回成功响应中的 `data[].b64_json`。
- 在请求日志列表中记录 `image_generation` 或 `image_edit`、模型、渠道、状态、耗时和 usage。
- 日志详情只记录图片 MIME、base64 字符数和估算解码字节数，不持久化完整图片 base64，避免日志数据库、内存和 UI 被大字段拖慢。
- 图片请求日志中的 Authorization、API Key 和 Cookie 等敏感 header 必须显示为 `<redacted>`。

## 8. 与图片工具过滤开关的关系

`filterImageGenerationTool` 控制的是发送给对话模型的工具声明：

- 旧版顶层 `type = "image_generation"`。
- 新版顶层 `image_gen` namespace。
- Responses Lite `additional_tools` 或 `exec.description` 中的 `image_gen__imagegen`。

Images API 路由本身是独立基础设施。即使路由存在，只要模型看不到生图工具，正常情况下就不会主动调用它。

反过来，如果工具声明仍然存在但 Images API 路由缺失，模型会成功发起本地工具调用，随后在 `/images/generations` 收到 404。这正是本次问题的表现。

## 9. 更新 Codex 版本时的检查清单

每次同步 Codex 新版本后，至少检查：

1. `models-manager/models.json` 中目标模型的 `use_responses_lite` 和 `tool_mode`。
2. `ext/image-generation/src/tool.rs` 中的图片模型常量。
3. `codex-api/src/endpoint/images.rs` 中的请求路径和 body 结构。
4. image-generation extension 是否仍使用当前 model provider。
5. `imagegen_extension.rs` 测试中的工具调用和回填流程。
6. 是否重新出现 hosted `type = "image_generation"` 工具注册。
7. `/images/edits` 是否从 JSON data URL 改为 multipart。
8. 图片响应是否仍要求 `data[].b64_json`。

这些检查可以区分：模型目录变化、Responses Lite wire shape 变化，以及独立 Images API 变化，避免把三者混为同一个问题。
