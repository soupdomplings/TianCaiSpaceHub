# TianCaiSpace Hub v0.4.28-1 本地整合

历史整合记录，2026-09-30 校正定位。当前已进入上游 v0.4.29 基线及后续二开，见 [当前总表](customizations/README.md)。下列分支已不作为当前开发入口，恢复使用提交号。

## 来源与恢复点

- 官方上游：`happy-loki/codexhub`，`v0.4.28` / `1e1fa64`。
- 整合前二开基线：`755a795`（v0.4.27-2）。
- 合并前工作区代码快照：`18621b0`，包含 OpenAI Chat Completions 和关闭推理选项。
- 整合结果：`13d1eb0`；原临时整合分支不再作为恢复依赖。
- 原有未跟踪文件 `TianCaiSpaceHub-0.4.25-1.zip` 保留，不纳入代码提交。

## 整合内容

- 引入官方 Kimi K3 Responses 渠道、图标、372K 模型目录、模型映射、原生工具和搜索兼容。
- 保留天才空间名称、图标、更新地址与打包配置。
- 保留 WorkBuddy 独立路由、协议转换、缓存兼容和 HTTP 502 / 503 重试。
- 保留本地 OpenAI Chat Completions、按渠道关闭推理及旧 DeepSeek 渠道兼容。
- WorkBuddy 协议分发补充 `KimiResponses`，不自动注入 OpenAI 缓存参数。
- 切换到 Kimi 时禁用 Chat Completions 专属的关闭推理选项。
- 两侧 Responses 回归测试均保留；macOS 发布脚本检查采用上游精确匹配，并保留 CRLF 归一化。

## 验证范围

- 最终结果：`cargo test --locked --features gui --bin codexhub`，779 passed，0 failed，2 ignored。
- `cargo build --locked --release --features gui --bin codexhub`、`cargo fmt --check` 和 `git diff --check` 均通过；编译仍有未使用代码和链接器信息类警告。
- 本地可执行文件：`target/dist/TianCaiSpaceHub-v0.4.28-1-windows-x64/TianCaiSpace Hub.exe`。
- GUI 完整测试包含官方 Kimi JSON/SSE、工具与搜索兼容，以及现有 WorkBuddy/Chat Completions 回归测试。
- 新增本地模拟 HTTP 上游测试，覆盖 WorkBuddy → Kimi 的模型映射、工具声明、推理参数、503 重试、JSON/SSE 返回转换及不注入缓存参数。
- 使用锁定依赖编译 Windows release 桌面版；格式及差异检查。
- 未使用真实 Kimi/WorkBuddy 服务密钥进行联网验收，未发布远程版本。
