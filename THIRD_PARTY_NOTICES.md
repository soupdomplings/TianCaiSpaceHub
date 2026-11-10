# 第三方参考及许可

V1.0.0 的客户端配置适配参考 Magpie 的原生配置格式和客户端发现路径，并由 TianCaiSpaceHub 的 Rust 事务与路由实现承接。没有嵌入 Magpie 可执行程序。固定参考提交：

- 项目：https://github.com/yetone/magpie
- 提交：`e6cdeac9c681ad0377b0f863af556d4c1d7be2a8`
- 许可：https://github.com/yetone/magpie/blob/e6cdeac9c681ad0377b0f863af556d4c1d7be2a8/LICENSE

以下保留 Magpie 的 MIT 许可全文：

V1.0.12 将固定提交中的 VS Code 便携目录/Profile 和 ZCode `HOME`/数据迁移规则移植到 Rust（`src/desktop/targets/paths.rs`），并移植部分对应临时目录测试场景；WorkBuddy 配置目录、对象式模型列表与两端 MCP 传输名称据相同提交核对。原版权及 MIT 许可适用于这些派生部分。Hub 的独立条目、凭据、网关和事务由本项目承接；未嵌入或运行 Magpie 程序，也未执行其 Go 测试。逐端差异与范围见 `docs/customizations/client-configuration-audit.md`。

```text
MIT License

Copyright (c) 2026 yetone

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

WebStorm Air/ACP 使用用户在设置中显式安装的 OpenCode `1.2.21` 官方组件，不随本包预装或自动运行。下载前后核对固定归档 SHA-256；组件许可及原始发行内容见官方仓库。配置格式核对该版本的 `packages/opencode/src/config/config.ts`、`provider/models.ts` 和 `provider/provider.ts`：https://github.com/anomalyco/opencode/tree/v1.2.21 。这不代表两个产品属于同一项目。

Rust 依赖及 vendor wxDragon/wxWidgets 继续遵循各自随源码提供的许可。上游来源、参考研究和本地二开不混记为原创。

V1.0.3 客户端图标用于标识对应产品，均去掉底板并适配当前主题颜色：

- ChatGPT/Codex、WorkBuddy、天工 Work 沿用本仓库原品牌线图；DeepSeek 沿用 `packaging/brand/providers/deepseek.svg` 的轮廓，运行时转为空心描边，其 Lobe Icons 许可在同目录 `LICENSE.lobehub-icons`。
- VS Code：Microsoft [vscode-codicons](https://github.com/microsoft/vscode-codicons/blob/6200d186bea39354a18737cfff73fe19dbc5fcff/src/icons/vscode.svg)，固定提交 `6200d186bea39354a18737cfff73fe19dbc5fcff`，本仓库 `vscode-line.svg` 改为透明描边。采用 [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)，原许可保留在 `packaging/brand/LICENSE.vscode-codicons`。
- ZCode：依据 Z.ai [官方应用图标](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/public/logo/icons/128x128.png) 重绘透明轮廓，移除渐变底板；来源仓库为 Apache-2.0。这里是产品标识参考，不包含其运行代码。
- WebStorm：依据 JetBrains [官方品牌资源](https://github.com/JetBrains/logos/blob/3a800f11772b4d3d883d6f61f9cc8120e13a9ce0/web/webstorm/webstorm.svg) 保留 WS 字形及下划线，外框改为透明线条、去掉彩色底板。产品名及商标仍归各自权利人。
