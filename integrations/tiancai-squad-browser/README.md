# 天才小分队浏览器接续登录配套补丁

适用 Hub V1.0.6，协议版本 1。此目录是可审查的站点源代码补丁，**尚未部署到线上，也不等于站点完整构建或账号验收通过**。不会部署第二套 Hub 网站。现有站点需要一次前后端更新，才能在默认浏览器与原生 Hub 之间接续登录；未更新时，Hub 提供内置登录，“回到主站”仍打开默认浏览器，使用浏览器自身登录状态。

## 流程

- Hub 登录：Hub 生成随机 verifier，只向站点发送 S256 challenge。站点生成三分钟请求。浏览器打开 `/hub/connect#flow=login&request=…`，沿原登录/OAuth/TOTP 流程登录，核对六位码并确认。Hub 每次轮询带 verifier，服务器原子消费已批准请求，签发独立会话。
- 回到主站：Hub 已认证接口签发一分钟单次票据，默认浏览器通过 fragment 接收票据与证明。站点页面立即清除地址中的证明，交换出浏览器独立会话后进入 `/dashboard`，重新初始化账号权限缓存。
- 不读取 Cookie 数据库，不解密浏览器配置，不共享 refresh token。长期 token 不放 URL、普通日志或 Redis 票据；Redis 只存挑战、用途、账号、有效期和密码/撤销版本摘要。站点账号被禁用、改密或撤销版本变化后拒绝兑换。
- 授权需用户点击确认；不同账号不能覆盖已批准请求；错误证明不消费票据。超时、重复兑换、Redis 故障均不降级放行。接口不加入请求正文审计，反代也应维持既有不记录认证正文的配置。

## 文件与接入

`overlay/` 含站点 Go Handler、Redis 脚本回归及 Vue 授权页。`apply.py` 针对用户提供的源码结构检查两个路由文件，默认只检查；`--apply` 才添加文件、备份并修改路由。拒绝标记重复、已有同名文件或不匹配的路由结构，不读取站点环境文件或用户数据。

在站点源码副本执行（将路径替换为实际源码目录）：

```powershell
python integrations/tiancai-squad-browser/apply.py D:/path/to/site-source
python integrations/tiancai-squad-browser/apply.py D:/path/to/site-source --apply
```

之后按原站点流程构建、测试与部署；本轮未运行这些部署操作。站点 `go.mod` 要求 Go 1.27.2，当前 Hub 构建环境未提供该工具链，因此 Go 完整编译及新增 Handler 的 Go 测试仍需站点构建环境完成：

```text
go test ./internal/handler -run TestHubProofConsumeAndApproval
pnpm typecheck
pnpm build
```

Hub 侧合成回环服务验证授权、取消/代次保护和链接内容；`tests/browser-contract.cjs` 可在隔离的 TypeScript 与 Vue SFC 编译器下验证真实授权页脚本。它不代表真实浏览器/OAuth或线上站点已经验收。

回滚时先停止站点发布，将两个 `.before-hub-browser` 路由备份恢复，再移除本补丁新增的三个站点文件。既有登录、业务 API、数据库结构、余额与组织权限不迁移。Redis 短期键自然到期；Hub 仍兼容原会话和内置登录，不影响已导入的本地模型渠道。
