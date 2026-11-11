TianCaiSpaceHub V1.0.23：搭档配置流水线与界面精简（2026-11-11）

修正 Windows 嵌套业务页覆盖对话浮层的问题。对话继续属于 Hub 主窗口，展开、定位和切页时维护原生裁剪与子窗层级，消息、输入和发送区完整显示。

为搭档内置中文操作指引，按读取当前配置、执行、保存后回读、如实汇报的流程处理需求。补全渠道参数、客户端副本身份与 Codex 动态模型的有效能力；WorkBuddy 仅提供原生接口实际支持的默认强度，Codex 内置能力及页面导航按真实支持范围反馈。

按用户要求实际使用既有共享 Server 和小分队模型，完成 7 轮、18 次工具调用的隔离配置验收，覆盖本机凭据表单、渠道/别名、客户端参数、批量删除拒绝/批准、停止和续聊。模型、API 和鉴权继续由小分队配置 GET 提供，其他调用沿 Hub Codex 流程。

搭档支持读取当前小分队用户的 API 密钥列表，指定已有密钥，或按要求创建密钥后准备导入，再保存厂商渠道和指定客户端模型。凭据及票据保留在 Hub，模型只得到掩码与引用。客户端入口隐藏 WebStorm 和 VS Code；大模型厂商增加 MiniMax 模板，复用现有 Chat Completions 配置。

最终GUI测试目标编译及57项定向回归通过。密钥流水线另以真实Server/管理员模型完成3轮8次工具调用，实际保存隔离渠道与临时ZCode指定模型，回读原生文件正确；测试密钥已撤销，未调用该用户密钥的推理接口。

实现和回滚见 [天才搭档](docs/customizations/genius-buddy.md)、[账户](docs/customizations/tiancai-squad-account.md)、[厂商预设](docs/customizations/desktop-capability-management.md)；构建、真实验证及唯一 MSI 身份见 [V1.0.23 交付](docs/releases/v1.0.23.md)。真实安装、用户原生客户端、MiniMax 连通、计费和 macOS 仍由用户验收。本地测试包不代表正式发布。
