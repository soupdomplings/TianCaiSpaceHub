# 仓库文档与分支清理记录

## 2026-10-03 · 二开分支更名

用户要求二开分支使用本项目和完整二开版本。当前唯一 worktree 的分支已由 `integrate/upstream-v0.4.30` 更名为 `tiancaispace/v0.4.30-2`，保留 `e3c6208` 及全部祖先提交，`main` 不变。原整合记录保留历史名称并补充当前名称，今后按 [维护规范](documentation.md) 命名。

源码 `e3c6208` 首先推送到用户仓库 `soupdomplings/TianCaiSpaceHub` 的旧分支；本次在核对远端旧分支仍为该提交、新名称未占用后，使用带预期提交保护的原子推送创建新分支并删除旧引用。服务器核验结果：`tiancaispace/v0.4.30-2` 为 `c775bcc5008841dc5e7aa80b7d90ff7465eaf38a`，包含旧提交及命名规则文档；旧分支不存在，`main` 仍为 `3b3f213`。随后只补记完成状态。

未删除上游仓库分支，未触发 Actions 或发布版本。本次只改文档与分支引用，不改代码、版本或现有构建产物，不执行产品测试。文档链接与差异空白检查通过。

## 2026-09-30

用户要求更新或删除过时文档，并检查残留历史分支和内容。本轮仅修改文档与协作约定，不修改产品代码、用户配置、已交付包或 Git 提交历史。

### 已删除并替代的文档

以下文件均已有 Git 历史，旧正文可按文件路径在历史提交中恢复；不额外复制一份“归档”制造重复现行说明。

| 删除文件 | 原因 | 当前替代 |
| --- | --- | --- |
| `docs/rename-codexhub.zh-CN.md` | 上游更名已经结束，仍含旧分支推送、远端仓库改名、目录迁移步骤，与当前品牌及阶段不符 | [品牌与打包身份](../customizations/desktop-and-packaging.md) |
| `docs/desktop-tray-auto-update-plan.zh-CN.md` | 把启动检查更新和半自动升级写成当前能力；二开早已取消这些入口 | [当前桌面与升级方式](../customizations/desktop-and-packaging.md) |
| `docs/step1-tree-ctrl-verification.md` | 空白验证表、未执行状态及 `tests/tree_ctrl_poc/` 不存在，“骨架已完成”的说明无效 | [Agent Manager 未排期设计](../agent-manager-mvp-plan.md) 中保留待验证需求 |
| `docs/ui-redesign-dark-mode.zh-CN.md` | 仍以“没有暗色模式”为起点安排已实现的主题开发，代码行号和现状过时 | [GUI 主题维护](../gui-theme.zh-CN.md) |
| `docs/wxdragon-sync-and-resource-optimization-plan.zh-CN.md` | 依赖版本过时，包含当年脏工作区和 stash/提交指令、未经当前采样的优化计划 | [GUI 与 wxDragon 维护](../gui-runtime-maintenance.zh-CN.md) |

### 已更新的过时内容

- README 中英文的检查更新入口，改为当前人工升级方式，并增加文档导航。
- 动态模型示例不再把已内置的 `gpt-6-luna` 当成未知模型，补充与网页导入不同的保存行为。
- v0.4.28 整合记录改用提交定位，移除“原 main 保持在旧提交”及当前依赖临时分支的表述。
- Agent Manager 删除不存在的 POC 骨架说明，将旧排期标记为未排期设想；保留设计与示意图供后续需求使用。
- 历史 Provider/Anthropic 路线、快速启动、插件与语言排障资料标明历史范围，保留仍有维护价值的依据。
- Grok Build 调研中的 6 处参考源码链接在当前仓库没有目标文件，改为明确标注原参考快照的历史路径，不伪造新的上游链接或已验证结论。
- 更新贡献约定、配置 API 说明、发布检查表和短发布摘要；移除发布检查表中的默认全量清空构建缓存指令。

### 分支核对

只读取本地引用和 worktree，并尝试核对服务器：

| 对象 | 检查结果 | 处理 |
| --- | --- | --- |
| `main` | 本地主分支，当前指向 `9d4a7f4` | 保留 |
| `codex/hub-external-import` | 当前唯一 worktree 使用，包含尚未提交的开发/文档变更 | 保留 |
| 其他本地分支 | 无 | 无需删除 |
| `tcs/codex/integrate-v0.4.29` | 本地跟踪记录指向 `9d4a7f4`，已被主线包含；服务器查询失败，无法确认远端是否又有新增提交 | 本轮未删除，待能访问服务器时核对 |
| `origin/codex/macos-intel-ci`、`origin/macos-posix-shim-rust`、`origin/refactor/split-remote-control-backend-20260609` | 本地跟踪记录已被当前历史包含，但属于上游仓库协作分支 | 不操作上游远端；跟踪引用不视为本地待开发分支 |

`git ls-remote --heads tcs` 在普通访问及获准的外部访问下均未成功，分别出现连接失败和连接重置；未执行远端删除，也未用删除本地跟踪引用冒充服务器清理。

### 保留内容

保留用户原有 `TianCaiSpaceHub-0.4.25-1.zip`、已交付的 `0.4.29-2` 测试包与哈希清单，以及未完成但仍有设计依据的研究资料。根目录发布说明仍由 CI 使用，不属于冗余文件。今后按 [维护规范](documentation.md) 更新清理记录。

### 文档核对结果

本轮静态核对覆盖 62 份 Markdown 文档和 322 处同仓库链接，未发现缺失目标；`docs/` 中全部 Markdown 已纳入总索引，Markdown 差异检查通过。本轮未运行产品测试、未启动应用，也未重新构建已交付包。
