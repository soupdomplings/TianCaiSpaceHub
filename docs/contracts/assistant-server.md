# 天才搭档配置接口契约

维护日期：2026-11-11。关联：TC-023。按用户最新决定，小分队只新增一个配置 GET，登录后返回普通模型 API 连接和模型参数。对话、工具、确认、会话及上下文摘要由 Hub / 天才搭档 Server 处理。本文不表示接口已部署；本机实现及验证见 [天才搭档](../customizations/genius-buddy.md)。

## 唯一新增接口

`GET https://tiancai.yc99.space/api/v1/hub/assistant/config`

使用现有小分队登录：`Authorization: Bearer <小分队访问令牌>`。返回现有站点 JSON envelope，示例均为占位值：

```json
{
  "code": 0,
  "data": {
    "enabled": true,
    "name": "天才搭档",
    "api": {
      "baseUrl": "https://models.example/v1",
      "apiKey": "<助手专用模型凭据>",
      "protocol": "responses",
      "stream": true
    },
    "model": {
      "id": "admin-assistant-model",
      "label": "助手模型",
      "contextWindow": 128000,
      "supportsReasoning": true,
      "reasoningEfforts": ["low", "medium", "high"],
      "defaultReasoningEffort": "medium"
    },
    "notice": null
  }
}
```

| 字段 | 约定 |
| --- | --- |
| `enabled` | 必填开关。停用可只返回 `{"enabled":false}`，Hub 结束旧助手会话。 |
| `name` | 启用时必填，当前为“天才搭档”。 |
| `api.baseUrl` | 普通模型 API 基址，可保留网关路径及 `/v1`；使用 HTTPS，不把凭据放进 URL。 |
| `api.apiKey` | 助手专用模型凭据。使用现有管理员付费服务的受限密钥，不能返回管理员登录令牌或管理密钥。 |
| `api.protocol` | `responses`（省略时默认）或 `chat_completions`。Hub 内置协议转换，不要求站点为助手另写协议适配。 |
| `api.stream` | 普通 API 是否使用流式，省略默认 `true`。设为 `false` 时 Hub 将完整结果适配给 Server，对话仍逐轮工作。 |
| `model.id` | 必填模型 ID；实际模型请求由 Hub 固定为此 ID。 |
| `model.label` | 可空，缺省展示 ID。 |
| `model.contextWindow` | 必填整数，至少 1024。Hub 把窗口写入 Server，80% 触发本机摘要。 |
| `model.supportsReasoning` | 必填布尔值。非推理模型不设置默认强度。 |
| `model.reasoningEfforts` | 必填数组；支持 `none/minimal/low/medium/high/xhigh/max/ultra`。非推理模型可为空。 |
| `model.defaultReasoningEffort` | 可空；非空须在可用数组中。 |
| `notice` | 可空提示。 |

不再要求 `capabilities.tools`、`capabilities.compact` 或 `billing` 对象；这些本机行为与固定费用说明内置于 Hub。管理员承担模型费用、用户零余额仍可用，依靠返回凭据对应的已有服务/额度规则实现，不新增个人或组织扣费流程。

## 复用普通模型 API

Hub 根据 `api.protocol` 直接请求已有基址下的标准 `responses` 或 `chat/completions` 路径，使用 `api.apiKey`；小分队登录 JWT 仅用于获取配置。普通接口需支持函数调用及函数结果多轮，这是本机配置工具工作的模型能力要求。`stream:true` 使用原始 SSE，`stream:false` 使用普通 JSON，均不加站点 `{code,data}` envelope。

不新增助手专用的 `POST /api/v1/hub/assistant/responses` 或 `POST /api/v1/hub/assistant/responses/compact`。Server 配置为本机摘要策略（`remote_compaction = "unsupported"`），需要压缩时通过同一普通模型 API 请求摘要，消耗对应模型额度，无需站点压缩接口。

模型只获得 Hub 的配置工具。Hub 负责工具执行、批量删除确认、凭据表单、停止取消、临时会话及退出清理，站点无需开发这些内容。运行端只接触随机本机桥接令牌；返回的模型凭据只保存在 Hub 本次运行内存，不进入对话、界面状态、日志或 Server 配置文件。退出/切换账号会结束旧对话和连接。

## 刷新与交接

启用且登录后读取配置，每 60 秒刷新一次；临时失败保留已知配置，明确停用或模型/连接变化时隔离旧会话。未启用不请求。配置 404 明确提示未部署；模型 API 的 401/403/429/5xx 原样失败，不刷新小分队登录后重放模型生成；用户可重新获取配置再试。停止/清空/切号时 Hub 关闭在途模型连接，已有 API 沿原取消规则处理。

服务端交接只需确认配置路径、一个脱敏配置样例及可登录测试环境。随后由用户验收普通 API 的文本、工具续轮、停止和零余额调用；不再要求专用压缩、SSE转发服务或额外响应样例开发。Hub 侧已纳入 [V1.0.18 本地测试包](../releases/v1.0.18.md)，接口部署及真实调用待用户验收。

参考：[官方 App Server](https://learn.chatgpt.com/docs/app-server)、[官方本机摘要配置](https://learn.chatgpt.com/docs/config-file/config-reference)。
