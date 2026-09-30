# Sub2API → TianCaiSpace Hub 外部导入约定 v1

维护说明（2026-09-30）：本文件是两端接口契约。Hub 侧 Windows 实现已完成开发，实际交付与待验收范围见 [实现说明](hub-external-import.md)。下文关于主站计划的记录不表示主站已经上线，也不是要求 Hub 执行主站开发的指令；协议变更需同步两端。

日期：2026-09-29。供 Hub 开发者实施；当前 Sub2API 第一项工作仅整合官方 v0.2.9，下面的主站接口将在第二项实现，现阶段不得假定已可调用。Hub 可用模拟响应独立开发。协议字段如需调整，请在两端联调前同步。

本轮确认：优先交付 Windows 安装版及便携版，macOS 为后续阶段，Linux 不纳入；支持兼容 Sub2API 站点，先以官方站点联调。一次导入一个渠道，主站确定协议和模型范围。导入预览阶段获取远端模型列表，默认禁用保存，启用及追加 Codex 可见模型均由用户勾选。空模型或获取失败允许禁用保存，之后获取或手填模型再启用。

## 目标与范围

用户在 Sub2API 的 API 密钥页面选择“导入到外部 → TianCaiSpace Hub”，打开 Hub 的渠道导入预览，确认后新增或更新一个 AI Gateway 渠道。导入不自动修改 IM、WorkBuddy、其他渠道或 Codex 原有配置；可以明确提供“启用此渠道”和“将选定模型显示在 Codex 中”选项。

## Hub 需要开发

1. 专属 URL 协议 `tiancaispacehub`，启动入口识别 `tiancaispacehub://import/v1`。
2. 未运行时启动 GUI；已运行时把导入请求交给现有 GUI 并置前。现有单实例检查不能直接丢弃请求。后台未启动、启动较慢、连续两次导入需正确处理。
3. Windows MSI 注册协议；便携版提供用户可见的“注册网页导入”入口并使用当前用户注册项。更新路径和卸载只处理本应用自己的注册项。macOS 后续支持 URL 事件与协议关联；Linux 不纳入本需求，未经平台实测不得标记该平台已验证。
4. 解析并兑换短时导入码，展示预览，确认后仅合并目标渠道字段。请求／错误日志必须隐藏 ticket、API Key 和完整导入链接。
5. 同名或重复来源不能静默覆盖；提供“更新已有渠道／新建渠道／取消”。保存时重新读取最新配置，保留导入期间其他页面修改的设置。
6. 使用返回的协议提示与实际模型列表，不硬编码 gpt-5.5。Hub 目前按明确模型列表匹配渠道，且 Codex 可见模型是单独配置，两者都需正确处理。

实现定位：`src/cli.rs`、`src/main.rs`、`src/gui.rs`、`src/external_import/`、`src/gui/external_import.rs`、配置读写及本地 Web API，以及 Windows packaging。macOS 协议关联另列后续任务，Linux 不纳入。导入使用独立确认与目标合并流程，不复用普通渠道按名称覆盖的保存行为。

## 唤起格式

```text
tiancaispacehub://import/v1?origin=<URL编码的主站origin>&ticket=<随机导入码>
```

- `origin` 示例 `https://tiancai.yc99.space`；它是主站管理 API 来源，不等同于模型请求 Base URL。
- `ticket` 为至少 256 bit 随机的 base64url 字符串，主站保存 120 秒，只能兑换一次。链接不包含长期 API Key。
- 严格校验协议、host=`import`、path=`/v1`、字段重复、长度和编码；拒绝未知协议版本。
- 正式来源只接受 HTTPS，不允许 URL 用户名密码、fragment 或路径。HTTP 仅允许显式本地开发的 localhost / 127.0.0.1 / [::1]。不跟随兑换接口重定向，不绕过 TLS 验证。
- 不执行 URL 中的命令、脚本或任意文件路径。确认保存前不持久化渠道或 API Key；允许预览阶段携带该 Key 查询模型列表，不发起推理。非官方来源先确认来源；模型列表地址与来源站点不同则在发送 Key 前确认目标地址。

## 主站将提供的接口

### 创建导入码（只由主站网页调用）

`POST /api/v1/keys/:id/external-import`

使用原登录鉴权，校验当前用户对密钥的所有权和可导入状态。请求：

```json
{"target":"tiancaispace-hub","schema_version":1}
```

响应沿用主站统一 envelope：

```json
{"code":0,"message":"success","data":{"ticket":"<opaque>","expires_in":120,"deeplink":"tiancaispacehub://import/v1?origin=...&ticket=..."}}
```

### 兑换（由 Hub 调用）

`POST {origin}/api/v1/external-import/resolve`

`Content-Type: application/json`；无需网站登录 Cookie。请求体：

```json
{"target":"tiancaispace-hub","schema_version":1,"ticket":"<opaque>"}
```

成功响应示例，字段及大小写以此为准：

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "schema_version": 1,
    "target": "tiancaispace-hub",
    "source": {
      "origin": "https://tiancai.yc99.space",
      "site_name": "TianCaiSpace",
      "key_id": "123",
      "key_name": "我的开发密钥"
    },
    "provider": {
      "name": "TianCaiSpace · 我的开发密钥",
      "platform": "openai",
      "protocol": "openai_responses",
      "base_url": "https://tiancai.yc99.space/v1",
      "models_url": "https://tiancai.yc99.space/v1/models",
      "api_key": "<用户选择的API密钥>",
      "models": ["example-model"],
      "model_aliases": {}
    }
  }
}
```

- 来源身份使用 `source.origin + source.key_id`，不以可变的名称唯一识别；来源相同再次导入优先提示更新。
- `protocol` v1 枚举：`openai_responses`、`anthropic_messages`、`chat_completions`、`grok_responses`。这是接口兼容协议，不直接等于上游平台名称。Hub 将其映射到自己的 ProviderType。
- `base_url` 保留合法路径前缀（例如 `/antigravity/v1`），Hub 按现有 URL 规范化函数处理，不能重复追加 `/v1`。
- `models` 必须来自该 Key 可用范围。空列表时要求用户获取／填写模型，不能宣称已可调用；未知协议直接提示需要升级 Hub，不回退为 OpenAI。
- `model_aliases` 默认空；导入时不要凭空映射模型。更新已有渠道时保留用户自己的映射，除非用户明确选择替换。
- 收到凭据后仅保留在内存直到确认保存。来源、地址、名称和模型可以显示，API Key 默认遮罩。
- 不默认执行付费测试请求，也不自动写入／切换 Codex 全局配置。
- 主站以 Redis TTL 和原子消费实现；不为此新增 SQL 表。兑换时重新检查 Key 状态；不得返回后台管理员凭据或其他 Key。
- 成功响应 `Cache-Control: no-store`；禁止记录请求体和返回密钥。

错误使用主站错误 envelope，Hub 以 HTTP 状态处理并展示可读提示：400 格式／版本不支持；403 Key 已不可导入；404 导入码无效、过期或已消费；429 频率限制；503 暂不可用。超时或失败不改现有配置；网络结果不确定时引导用户从主站重新发起，不无限重试一次性码。

## Hub 交付内容

- 可安装的 Windows 测试包及便携版（注明是否都支持协议注册），版本号、构建提交、SHA-256。
- 协议注册／解除与版本更新说明；明确已验证的平台。
- 解析器和导入状态机测试、配置合并测试，以及端到端手工／自动化结果。
- 不含真实密钥的示例或 mock 服务，便于 Sub2API 一侧联调。
- 简短变更文档：改动文件、安装方式、导入位置、回滚方法、已知限制。

验收至少覆盖：首次启动、已运行实例、便携路径含空格、未注册／旧版、取消、过期、连续点击、重复 Key、同名不同 Key、配置并发修改、中文名称、带路径 Base URL、四种协议、模型列表为空／模型发现失败、Codex 模型可见、敏感信息不写日志。真实模型计费调用另行由用户决定。
