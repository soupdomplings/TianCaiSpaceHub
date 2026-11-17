# 固定 smoltcp 源码

本目录来自官方 [smoltcp-rs/smoltcp](https://github.com/smoltcp-rs/smoltcp) 提交 [`0a926767a68bc88d5512afefa7529c5ecdade4ea`](https://github.com/smoltcp-rs/smoltcp/tree/0a926767a68bc88d5512afefa7529c5ecdade4ea)，crate 版本 `0.12.0`。

来源归档：`https://codeload.github.com/smoltcp-rs/smoltcp/tar.gz/0a926767a68bc88d5512afefa7529c5ecdade4ea`。该提交沿 EasyTier v2.6.4 的原 `rev` 固定；不替换为 crates.io 同名版本，不在用户设备安装或运行时下载。

保留原 `Cargo.toml`、`build.rs`、`src`、示例／测试、README 和 [0BSD 许可全文](LICENSE-0BSD.txt)。仅省略与本地依赖构建无关的 `.github`、`.gitignore` 和 fuzz 工作区，新增本来源说明；原库和构建源码未修改。

EasyTier 通过 `default-features=false` 及原 `std,medium-ip,proto-ipv4,proto-ipv6,proto-ipv4-fragmentation,fragmentation-buffer-size-8192,assembler-max-segment-count-16,reassembly-buffer-size-8192,reassembly-buffer-count-16,socket-tcp,socket-udp,async` 使用用户态栈。主库版本要求 Rust 1.87，本轮最终工具链要求由 EasyTier 的 Rust 1.95 下限决定。构建结果和实机验收由 Hub 统一记录。
