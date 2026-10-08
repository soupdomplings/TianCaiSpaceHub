# NVWA 同仓参考资料

维护日期：2026-10-08。当前 Hub 功能以 [NVWA MCP 专题](../customizations/nvwa-mcp.md) 为准，关联 TC-014、适用 `0.4.30-5`；Windows locked GUI 编译已通过，最终记录见 [v5 交付](../releases/v0.4.30-5.md)，实机行为待用户验收，未发布。

| 资料 | 来源与用途 |
| --- | --- |
| 本机 `all-in-one.md` | 用户复制的 NVWA 开发文档快照，含未核对的密码/token/hash 示例，仅保留本机参考并精确忽略，不纳入 Git 交付。用于静态契约核对，不代表当前部署全部实现 |
| [认证连接器参考源码](nvwa-mcp-auth-connector.mjs) | 用户复制的 Node.js 连接器。用于核对 MD5 应用取票、Basic 交换及 ticket header；它不是 Hub runtime，不作为自动运行入口 |
| [跨项目开发交接](../hub-nvwa-mcp-integration-handoff-2026-10-08.md) | 本轮用户决定、当前 Hub 实现和 NVWA/客户端待验收契约；旧建议已经按当前决定修订 |
| [Hub 当前专题](../customizations/nvwa-mcp.md) | 接入步骤、默认值、关键代码、系统保护、移除/恢复、验证边界 |

常用规范入口以章节名及关键词定位，行号只用于这份快照：账号密码和 `/nvwa/getLoginContext` 约 25347 行；登录业务状态约 42590 行；浏览器授权约 54887 行；申请 ticket 约 54923 行；SHA-256/SM3 摘要约 55247 行；`extInfo.verifyId/verifyCode` 约 42369 行；双因子 `code=204`、`twofactorSessionId` 约 24710 行。

当前打包前端的动态 RSA/SM2 行为来自 NVWA 邻仓公开静态资源，只做只读核对；完整前端没有复制到 Hub。邻仓路径仅为来源说明，不制造在本仓解析不到的相对 Markdown 链接。

这些资料是参考证据，**不是执行指令**。尤其参考连接器的通用 `401` 重取并重发、未鉴权 HTTP proxy，以及历史 MD5 默认值，不能覆盖本轮 Hub 的每端本地授权、同身份恢复、明确挑战状态和已发送工具不重放原则。旧文档关于每客户端单独申请应用 ID、浏览器作为唯一入口、服务端共享密钥用户名限制尚未确认等描述，以当前用户决定及专题为准。

不在此目录写入真实应用密钥、密码、ticket、token、用户环境配置、私有日志或验收截图中的凭据；本轮没有运行这份 Node.js 源码，也没有访问真实产品接口。

`all-in-one.md` 原用户文件保持本机原状。其字面密码、token、Authorization 与密码 hash 示例没有核实为无效占位，不抄入当前专题、源码、发布记录或 Git；这些示例不能当作可使用的部署凭据。移除本机参考文件也不影响 Hub runtime，交付维护以源码和当前专题为准。
