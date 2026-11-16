# Hub 远程会话网页

React、TypeScript、Vite；仅供 Hub 的本地浏览器网关使用，正式页面没有演示会话或模拟运行端。

```powershell
npm ci
npm run test
npm run build
```

构建输出 `../assets/remote-web/`，由 Rust 嵌入并以 `/remote/` 提供。Node.js 22.12+ 或 24 用于开发构建，用户运行 Hub 不需要 Node.js。生产构建同时生成运行依赖的第三方声明。

会话、文件和事件接口见 [协作契约](../docs/development/remote-session-contract.md)。网页通过 `X-Hub-Target` 选择已配对设备，不保存设备令牌或上游密钥。`?device=`、`?client=`、`?session=` 只记录对象身份，方便从 Hub 打开原会话；URL 不包含认证信息。

默认页面会读取真实本地网关。自动检查使用 Vitest/jsdom 的合成接口，不启动真实 Hub、Codex、天工或模型。不要把网页测试通过当作 Windows/macOS 双机验收。
