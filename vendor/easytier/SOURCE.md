# EasyTier 源码依赖

Hub 将本目录的 Rust 库编译到自身程序中，不在首次启用、安装或运行期间下载 EasyTier 可执行文件。当前保留 Windows／macOS 远程会话所需的无 TUN 用户态连接；原有客户端协议和 Hub HTTPS 设备配对由 Hub 自身模块管理。

## 来源与许可

- 官方仓库：[EasyTier/EasyTier](https://github.com/EasyTier/EasyTier)。固定标签 [v2.6.4](https://github.com/EasyTier/EasyTier/tree/v2.6.4)，标签对应提交 `8428a89d2dabc94c97d370ec607c6ca142473626`。
- 获取的官方源码归档：`https://codeload.github.com/EasyTier/EasyTier/tar.gz/refs/tags/v2.6.4`。原始归档与本轮调研脚本放在被忽略的 `target/remote-network-integrated-20261117/source-integration/`；不作为产品运行资源。
- 保留原 `easytier/src`、`locales` 与 `easytier-rpc-build/src`，两个 crate 的版权和 `LICENSE` 原文。上游 Tauri／Web 管理端、独立程序构建、工具链、工作流、原生驱动和展示图片不作为本库的构建输入。
- 上游许可为 LGPL-3.0，全文保留在 [COPYING.LESSER](COPYING.LESSER) 和两个 crate 的 `LICENSE`。配套 GPLv3 全文从 GNU 官方 `https://www.gnu.org/licenses/gpl-3.0.txt` 获取并保留在 [COPYING](COPYING)。发行声明、源代码获取和对应源码提供由 Hub 的第三方声明及发行流程统一维护。
- `vendor/smoltcp` 来自 [smoltcp-rs/smoltcp](https://github.com/smoltcp-rs/smoltcp)，使用 EasyTier 原指定提交 `0a926767a68bc88d5512afefa7529c5ecdade4ea`（crate 版本 `0.12.0`）。该源码及构建脚本不修改，保留其 `LICENSE-0BSD.txt`；具体来源见该目录的 `SOURCE.md`。不以 crates.io 同版本替换该固定提交。

## 本地构建入口

```toml
easytier = { path = "vendor/easytier/easytier", default-features = false, features = ["hub-embedded", "smoltcp", "socks5", "aes-gcm"] }
```

主 crate 的 `edition=2024`、`rust-version=1.95` 与 RPC 生成器元数据已展开为本地值，不继承上游工作区，不拉入 GUI、Web 或其他 contrib 成员。主库的 RPC 构建依赖为 `../easytier-rpc-build`，TCP/IP 栈为 `../../smoltcp`，三者均是相对路径源码依赖；本地 manifests 不含 Git 依赖。

保留 `no_tun`／`use_smoltcp` 配置语义、SOCKS5、AES-GCM、TCP／UDP 隧道与穿透、STUN、直连／转发路由和 ACL。`no_tun` 是运行配置，不是 Cargo feature；Hub 生成该配置并且不启用出口节点、通用转发或驱动。

`hub-embedded` 是本精简配置的必要 feature；默认 features 为 `hub-embedded,socks5,aes-gcm`，其中 `socks5` 自动启用 `smoltcp`。上游其他 feature 名称仅保留用于明确的编译错误及源码比较，不支持通过开启 `full`、`tun`、`faketcp`、`wireguard`、`kcp`、`quic`、`websocket`、`magic-dns`、替代加密／分配器等恢复独立程序功能。需要扩展时应先恢复所需依赖并核对平台行为。

构建机器需要 Rust 1.95 及以上、目标平台 Rust 链接工具和 `protoc`。构建脚本只使用 `PROTOC` 指定路径或 `PATH` 中的 `protoc`，不下载编译器、不安装驱动、不调用 Win7 thunk。保留原 `.proto`、描述符、Serde／RPC 生成及 locales 变更发现；`protoc` 在编译时生成的文件位于 Cargo `OUT_DIR`，用户运行 Hub 不需要 `protoc` 或 Node.js。

Windows 使用的 `protoc` 与 CI 准备由 Hub 根构建流程维护。macOS 使用原生 Apple 链接工具与构建期 protobuf；共用源码不表示已通过 macOS 构建或实机组网验收。

## 本地修改

| 文件 | 调整与原因 |
| --- | --- |
| `easytier/Cargo.toml` | 改为仅库构建并展开工作区元数据；两个内部依赖改本地路径；移除不使用的可选传输、驱动、独立 CLI、服务安装、分配器及相关 Git 依赖。Windows `pnet` 只保留 packet／serde，并单独启用 `pnet_packet/std` 以沿用标准库 IP 地址类型，不开启 `pnet/std` 或强制链接 WinPcap 的 `pnet_datalink`；其他平台保留上游标准模式。`nix` 限 Unix 目标。 |
| `easytier-rpc-build/Cargo.toml` | 展开 edition／Rust 版本，原生成器源码不变。 |
| `easytier/build.rs` | 去除 Windows 下载、原生驱动链接搜索及 Win7 thunk；只保留协议／描述符／Serde 和语言资源构建。 |
| `easytier/src/lib.rs` | `hub-embedded` 隔离独立 `core`、系统服务管理和 shell 补全入口；对不支持的 profile 给出明确编译错误。其余共用类型保留，Hub 不启动上游管理 RPC／Web 服务。 |
| `easytier/src/common/constants.rs` | 库版本固定为 `2.6.4-hub-8428a89d`，不将 Hub 仓库的 Git 状态误报为 EasyTier 版本。 |
| `easytier/src/common/network.rs` | Windows 沿已有 `network-interface` 获取接口，用本地元数据结构代替 WinPcap DTO；移除失败后调用 WinPcap 的回退。保留原地址／MAC 筛选，失败如实返回空接口列表；不安装网络驱动。 |
| `easytier/src/connector/{mod,dns_connector,http_connector,wrapped_tunnel}.rs` | 本 profile 禁用无关的 HTTP 节点跳转发现，不引入上游 `http_req` Git 分支；把 DNS 和 HTTP 共享的隧道元数据封装单独保留。普通 TCP／UDP 域名解析和 STUN 不受影响。 |
| `easytier/src/instance/mod.rs` | 禁用与本功能无关的 Windows 广播捕获，移除 WinDivert 依赖；原模块源码保留供比较。 |
| `easytier/src/instance/instance.rs` | 不记录含网络口令的完整配置；先加载同一 ACL，再开放监听／peer；嵌入配置跳过原始 ICMP 代理及其阻塞接收线程，TCP／UDP 保留。 |

上游 `launcher`、`instance_manager`、RPC trait／协议及 Web 客户端类型因为共用类型引用而保留，但本轮只使用 `instance::instance::Instance` 的内存 API。Hub 启用时创建实例，结束时清理实例及专用运行时；不生成上游系统服务、管理入口或 CLI 子进程。

实际移除的 Git 依赖为 `service-manager`、`http_req`、`thunk-rs`、`tun-easytier`、`kcp-sys`、`windivert`；`smoltcp` 改为上述固定提交的本地源码。未将这些不使用的程序或驱动改成预编译文件塞入 Hub。

## 验证与后续更新

本轮已做三个 manifests 的 TOML 解析、必要资源／路径检查和修改文件的定向 rustfmt。开发会话不执行 Cargo、不运行真实组件网络；根锁文件、统一编译、离线集成验证及包身份由管理会话记录。真实 Windows／macOS 双机穿透、中继、退出／停用和端口隔离待用户验收。

后续更新保留上游标签／提交、原版权及许可；优先比较本表列出的局部修改，再核对 Hub `src/remote_network/` 使用的配置、路由与生命周期 API。不能只覆盖本目录后将新版本视为已兼容。未发送的临时调研资料不随程序分发。
