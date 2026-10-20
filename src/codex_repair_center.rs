//! User-invoked, scoped desktop repairs. Opening/checking is read-only; writes
//! require the same cross-process guard as initialization and enhanced launch.
mod assets;
mod launch_path;
mod package;
mod proxy;

use crate::codex_startup_repair as startup;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Startup,
    Models,
    Plugins,
    Browser,
    Headers,
    Ax,
    Surface,
    Context,
    Screenshot,
    Mcp,
    Proxy,
    Discovery,
    Desktop,
}
impl Kind {
    pub const ALL: [Self; 13] = [
        Self::Startup,
        Self::Models,
        Self::Plugins,
        Self::Browser,
        Self::Headers,
        Self::Ax,
        Self::Surface,
        Self::Context,
        Self::Screenshot,
        Self::Mcp,
        Self::Proxy,
        Self::Discovery,
        Self::Desktop,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Models => "models",
            Self::Plugins => "plugins",
            Self::Browser => "browser",
            Self::Headers => "headers",
            Self::Ax => "ax",
            Self::Surface => "surface",
            Self::Context => "context",
            Self::Screenshot => "screenshot",
            Self::Mcp => "mcp",
            Self::Proxy => "proxy",
            Self::Discovery => "discovery",
            Self::Desktop => "desktop",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Startup => "无法加载组织设置 / 启动失败",
            Self::Models => "模型与速度选项缺失（Fast / Power / Ultra）",
            Self::Plugins => "插件市场或缓存不完整",
            Self::Browser => "内置浏览器 / Chrome 本机连接失效",
            Self::Headers => "浏览器提示缺少 Codex 登录 token",
            Self::Ax => "浏览器辅助功能资源解压失败",
            Self::Surface => "Windows 电脑操控 / Any App 不可用",
            Self::Context => "电脑操控跨次调用失败",
            Self::Screenshot => "Windows 10 截图失败或超时",
            Self::Mcp => "更新后已配置的 MCP 无法启动",
            Self::Proxy => "浏览器 / 电脑操控子进程未继承代理",
            Self::Discovery => "Image-guided Assets 插件不可见",
            Self::Desktop => "界面与插件入口深度修复（含 Goal / 语言）",
        }
    }
    pub fn help(self) -> &'static str {
        match self {
            Self::Startup => {
                "分别检查内置程序路径和状态库。可将 CODEX_CLI_PATH 重新绑定到当前安装包；没有迁移冲突也能修复路径。数据库仅处理已确认冲突，先备份并验证副本，保留会话历史。"
            }
            Self::Models => {
                "补齐基础配置，并在增强启动时按实际组件适配模型、速度、Power 和 Ultra 偏好。不会为模型伪造能力或强制所有请求 Fast。"
            }
            Self::Plugins => {
                "从当前安装包恢复插件市场和已有插件的缓存；其他可选插件保持原安装、启用状态。"
            }
            Self::Browser => {
                "核对并修复当前浏览器插件、运行时、本机连接与两份连接记录。浏览器扩展需正常安装；macOS 系统权限仍在系统设置中授权。"
            }
            Self::Headers => {
                "仅处理第三方厂商下精确的“Codex auth token is unavailable”错误，按完整文件哈希匹配；不忽略其他认证或策略错误。"
            }
            Self::Ax => {
                "检查 Brotli / WASM 资源，仅从同版本、同服务的健康来源恢复，完成后需新建工具会话。"
            }
            Self::Surface => {
                "Windows 专项。分别识别旧版插件脚本和新版客户端结构；保留工具审批，不以修改临时生成配置冒充持久修复。"
            }
            Self::Context => {
                "Windows 专项。仅修复完整哈希匹配的已知跨调用上下文问题；完成后需新建工具会话。"
            }
            Self::Screenshot => {
                "Windows 10 专项。请在错误提示框粘贴 SetIsBorderRequired、0x80004002 或 FrameArrived timed out；仅处理已知 helper 哈希。"
            }
            Self::Mcp => {
                "仅更新已配置 MCP 中指向已删除 Codex Node 运行时的命令。保留原参数和独立环境，不安装其他 MCP。"
            }
            Self::Proxy => {
                "先在客户端仍运行时检查真实进程的代理变量名；只有根进程已有代理、子进程丢失的证据才修复生成器。只记录变量是否存在，不显示代理地址。检查后退出客户端再修复。"
            }
            Self::Discovery => {
                "仅在客户端原已包含插件及对应描述符时恢复可发现性，保留 installWhenMissing=false，不自动安装或启用。"
            }
            Self::Desktop => {
                "通常先试增强启动。深度修复只处理已匹配的界面与主程序结构。Windows 会准备签名更新包和恢复包，可能联网获取打包组件并弹出系统权限请求；macOS 备份应用后更新完整性并重新签名。Hub 保持运行。"
            }
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub message: String,
    pub backup: Option<PathBuf>,
    pub applied: bool,
    pub can_restore: bool,
    pub needs_repair: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub path: PathBuf,
    pub label: String,
    pub can_restore: bool,
}

fn rt() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}
pub(super) fn digest(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}
pub(super) fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("文件没有父目录")?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub(super) fn system(
    program: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    rt()?.block_on(async {
        use tokio::io::AsyncWriteExt;
        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().context("无法启动内置修复工具")?;
        let stdin = child.stdin.take();
        let operation = async move {
            if let Some(mut stream) = stdin {
                if let Some(bytes) = input {
                    stream.write_all(&bytes).await?;
                }
                drop(stream);
            }
            child.wait_with_output().await.map_err(anyhow::Error::from)
        };
        let output = tokio::time::timeout(timeout, operation)
            .await
            .context("修复工具响应超时，已停止；请保留备份后重新检查。")??;
        ensure!(
            output.stdout.len() + output.stderr.len() <= 8 * 1024 * 1024,
            "修复返回内容过多，已停止"
        );
        // JSON errors are handled by the caller. Never echo arbitrary process
        // stderr: it can contain a configuration value or a proxy credential.
        ensure!(
            output.status.success() || !output.stdout.is_empty(),
            "修复工具未完成。请保留备份并导出连接诊断进一步检查。"
        );
        Ok(output.stdout)
    })
}
fn resources(cli: &Path) -> Result<PathBuf> {
    cli.ancestors()
        .find(|dir| dir.join("app.asar").is_file())
        .map(Path::to_path_buf)
        .context("未找到与内置程序对应的客户端资源目录")
}
pub(super) fn local_root() -> PathBuf {
    #[cfg(windows)]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join("AppData/Local"))
            .join("OpenAI/Codex");
    }
    #[cfg(target_os = "macos")]
    {
        return user_home().join("Library/Application Support/Codex");
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        user_home().join(".codex")
    }
}
fn user_home() -> PathBuf {
    dirs_from_environment().unwrap_or_else(|| {
        crate::codex_app_config::default_codex_home()
            .parent()
            .unwrap_or(Path::new("/nonexistent"))
            .to_path_buf()
    })
}
fn dirs_from_environment() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}
fn backup_root(home: &Path) -> PathBuf {
    home.join("hub-desktop-repair-backups")
}

fn document(home: &Path) -> Result<(String, toml_edit::DocumentMut)> {
    let text = fs::read_to_string(home.join("config.toml"))
        .context("请先初始化 ChatGPT(Codex) 接入配置，再修复本项")?;
    let doc = text
        .trim_start_matches('\u{feff}')
        .parse::<toml_edit::DocumentMut>()
        .context("Codex 配置格式有误，未覆盖")?;
    Ok((text, doc))
}
fn table<'a>(doc: &'a mut toml_edit::DocumentMut, key: &str) -> Result<&'a mut toml_edit::Table> {
    if !doc.contains_key(key) {
        doc[key] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc[key]
        .as_table_mut()
        .context("已有配置项类型不是配置表，未覆盖")
}
fn candidate(
    kind: Kind,
    home: &Path,
    res: &Path,
    proxy_for_mcp: bool,
) -> Result<(Option<String>, usize)> {
    if !matches!(
        kind,
        Kind::Models | Kind::Plugins | Kind::Browser | Kind::Mcp | Kind::Surface
    ) {
        return Ok((None, 0));
    }
    let (before, mut doc) = document(home)?;
    let mut count = 0;
    if kind == Kind::Models {
        let desktop = table(&mut doc, "desktop")?;
        let mut values = match desktop.get("enabled-reasoning-efforts") {
            Some(item) => item
                .as_array()
                .cloned()
                .context("思考强度配置不是列表，未覆盖")?,
            None => ["low", "medium", "high", "xhigh"].into_iter().collect(),
        };
        ensure!(
            values.iter().all(|v| v.as_str().is_some()),
            "思考强度列表包含非文本值，未覆盖"
        );
        for key in ["max", "ultra"] {
            if !values.iter().any(|v| v.as_str() == Some(key)) {
                values.push(key);
            }
        }
        desktop["enabled-reasoning-efforts"] = toml_edit::value(values);
        let features = table(&mut doc, "features")?;
        ensure!(
            features
                .get("fast_mode")
                .is_none_or(|v| v.as_bool().is_some()),
            "速度配置类型不正确，未覆盖"
        );
        features["fast_mode"] = toml_edit::value(true);
    }
    if matches!(kind, Kind::Plugins | Kind::Browser) || (kind == Kind::Surface && cfg!(windows)) {
        let marketplaces = table(&mut doc, "marketplaces")?;
        if marketplaces.get("openai-bundled").is_none() {
            marketplaces["openai-bundled"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let bundled = marketplaces["openai-bundled"]
            .as_table_like_mut()
            .context("已有插件市场配置类型不正确，未覆盖")?;
        bundled.insert("source_type", toml_edit::value("local"));
        bundled.insert(
            "source",
            toml_edit::value(
                home.join("marketplaces/openai-bundled-local")
                    .to_string_lossy()
                    .as_ref(),
            ),
        );
    }
    if kind == Kind::Browser || (kind == Kind::Surface && cfg!(windows)) {
        let names: &[&str] = if kind == Kind::Browser {
            &["browser", "chrome"]
        } else {
            &["computer-use", "unified-computer-use"]
        };
        let plugins = table(&mut doc, "plugins")?;
        for name in names {
            if !res
                .join("plugins/openai-bundled/plugins")
                .join(name)
                .join(".codex-plugin/plugin.json")
                .is_file()
            {
                continue;
            }
            let key = format!("{name}@openai-bundled");
            if plugins.get(&key).is_none() {
                plugins[&key] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let entry = plugins[&key]
                .as_table_like_mut()
                .context("已有插件配置类型未知，未覆盖")?;
            if entry.get("enabled").is_none() {
                entry.insert("enabled", toml_edit::value(true));
            } else {
                ensure!(
                    entry
                        .get("enabled")
                        .and_then(toml_edit::Item::as_bool)
                        .is_some(),
                    "已有插件启用项类型未知，未覆盖"
                );
            }
        }
    }
    if kind == Kind::Surface && cfg!(windows) {
        let features = table(&mut doc, "features")?;
        ensure!(
            features
                .get("computer_use")
                .is_none_or(|v| v.as_bool().is_some()),
            "电脑操控配置类型未知，未覆盖"
        );
        features["computer_use"] = toml_edit::value(true);
    }
    if kind == Kind::Mcp {
        let name = if cfg!(windows) { "node.exe" } else { "node" };
        let root = local_root().join("runtimes/cua_node");
        let reference = res.join("cua_node/bin").join(name);
        ensure!(reference.is_file(), "当前客户端没有可核对的 Node 运行时");
        let expected = digest(&reference)?;
        let candidates = fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|e| e.path().join("bin").join(name))
            .filter(|p| p.is_file() && digest(p).ok().as_deref() == Some(expected.as_str()))
            .collect::<Vec<_>>();
        let replacement=candidates.first().context("尚无与当前安装包匹配的用户运行时。请先修复浏览器本机连接，或正常启动客户端让它提取当前运行时，再检查本项。")?;
        if let Some(servers) = doc
            .get_mut("mcp_servers")
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            for (_, server) in servers.iter_mut() {
                let Some(server) = server.as_table_like_mut() else {
                    continue;
                };
                let Some(command) = server.get("command").and_then(toml_edit::Item::as_str) else {
                    continue;
                };
                let old = PathBuf::from(command);
                // Only an absolute, removed Codex CUA Node command. Other node
                // installations, arguments, env and remote MCP URLs are retained.
                if old.is_absolute() && !old.exists() && is_retired_node(&old, &root, name) {
                    server.insert(
                        "command",
                        toml_edit::value(replacement.to_string_lossy().as_ref()),
                    );
                    count += 1;
                }
                if proxy_for_mcp && server.get("command").is_some() {
                    let mut vars = match server.get("env_vars") {
                        Some(v) => v
                            .as_array()
                            .cloned()
                            .context("MCP env_vars 不是列表，未覆盖")?,
                        None => toml_edit::Array::new(),
                    };
                    ensure!(
                        vars.iter().all(|v| v.as_str().is_some()),
                        "MCP env_vars 含未知值，未覆盖"
                    );
                    for name in proxy::NAMES {
                        if std::env::var_os(name).is_some()
                            && !vars.iter().any(|v| v.as_str() == Some(name))
                        {
                            vars.push(name);
                        }
                    }
                    if !vars.is_empty() {
                        server.insert("env_vars", toml_edit::value(vars));
                    }
                }
            }
        }
    }
    let next = doc.to_string();
    let _ = next.parse::<toml_edit::DocumentMut>()?;
    Ok(((next != before).then_some(next), count))
}
fn is_retired_node(old: &Path, root: &Path, name: &str) -> bool {
    let clean = |p: &Path| {
        let value = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value
        }
    };
    let old = clean(old);
    let root = clean(root);
    let Some(tail) = old.strip_prefix(&(root + "/")) else {
        return false;
    };
    let parts: Vec<_> = tail.split('/').collect();
    parts.len() == 3
        && !parts[0].is_empty()
        && parts[0] != ".."
        && parts[1] == "bin"
        && parts[2] == name
}
fn node(toolkit: &Path, res: &Path) -> Result<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let source = res.join("cua_node/bin").join(name);
    ensure!(
        source.is_file(),
        "此客户端未附带修复工具所需运行时；没有改动文件，请先正常更新客户端"
    );
    // Do not execute a WindowsApps binary in place. Core Node is copied into
    // the private task directory; it is not an app-server or model invocation.
    let target = toolkit.join(name);
    fs::copy(source, &target)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
    }
    Ok(target)
}
fn node_run(toolkit: &Path, node: &Path, options: Value) -> Result<Value> {
    let input = serde_json::to_vec(&options)?;
    let tool = toolkit.join("hub-repair.cjs");
    let out = system(
        node,
        &[tool.to_str().context("修复工具路径不可用")?],
        Some(input),
        Duration::from_secs(240),
    )?;
    let result: Value =
        serde_json::from_slice(&out).context("修复工具返回格式未识别，未继续操作")?;
    if let Some(error) = result["error"].as_str() {
        bail!("{error}");
    }
    Ok(result)
}
fn native_roots() -> Vec<PathBuf> {
    let user = user_home();
    if cfg!(windows) {
        vec![user.join("AppData/Local/OpenAI/extension")]
    } else {
        [
            "Google",
            "Chromium",
            "Microsoft Edge",
            "BraveSoftware",
            "Vivaldi",
            "com.operasoftware.Opera",
        ]
        .iter()
        .map(|name| user.join("Library/Application Support").join(name))
        .collect()
    }
}
fn outcome(value: &Value) -> Outcome {
    Outcome {
        message: value["message"]
            .as_str()
            .unwrap_or("检查已结束")
            .to_string(),
        backup: value["backup"].as_str().map(PathBuf::from),
        applied: value["applied"].as_bool().unwrap_or(false),
        can_restore: value["canRestore"].as_bool().unwrap_or(false),
        needs_repair: value["needsRepair"].as_bool().unwrap_or(false),
    }
}

pub(crate) fn run(
    kind: Kind,
    apply: bool,
    evidence: &str,
    proxy_for_mcp: bool,
    mut progress: impl FnMut(&str),
) -> Result<Outcome> {
    if cfg!(target_os = "macos") && matches!(kind, Kind::Surface | Kind::Context | Kind::Screenshot)
    {
        return Ok(Outcome {message:"本项为 Windows 专项，当前 macOS 不适用。电脑操控权限请到系统设置的辅助功能和屏幕录制中确认。".into(),backup:None,applied:false,can_restore:false,needs_repair:false});
    }
    if kind == Kind::Startup {
        return startup_issue(apply, &mut progress);
    }
    let _access = if apply {
        crate::codex_access::repair()?
    } else {
        crate::codex_access::operation()?
    };
    if apply {
        startup::require_stopped()?;
    }
    progress("正在定位当前安装包并核对所选问题；Hub 保持运行…");
    let (home, _) = startup::locations()?;
    let cli = startup::locate_cli()?;
    let res = resources(&cli)?;
    let toolkit = tempfile::tempdir()?;
    assets::extract(toolkit.path())?;
    let node = node(toolkit.path(), &res)?;
    let (candidate, changes) = candidate(kind, &home, &res, proxy_for_mcp)?;
    let enabled_plugins = document(&home)
        .ok()
        .and_then(|(_, d)| {
            d.get("plugins")
                .and_then(toml_edit::Item::as_table_like)
                .map(|p| {
                    p.iter()
                        .filter(|(_, v)| {
                            v.get("enabled").and_then(toml_edit::Item::as_bool) == Some(true)
                        })
                        .map(|(k, _)| k.to_string())
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_default();
    let probe = if kind == Kind::Proxy {
        Some(proxy::check(&home, &res, apply)?)
    } else {
        None
    };
    let user = user_home();
    let native_roots = native_roots();
    let options = json!({"action":if apply{"repair"}else{"check"},"kind":kind.key(),"home":home,"cli":cli,"resources":res,"localRoot":local_root(),"userHome":user,"nativeRoots":native_roots,"backupRoot":backup_root(&home),"enabledPlugins":enabled_plugins,"configCandidate":candidate,"configChanges":changes,"evidence":evidence,"windowsBuild":package::windows_build(),"proxyEvidence":probe.as_ref().is_some_and(|p|p.patchable)});
    let value = node_run(toolkit.path(), &node, options)?;
    let mut result = outcome(&value);
    if kind == Kind::Models && !apply {
        result.needs_repair = true;
    }
    if let Some(probe) = probe {
        result.message = format!("{}\n\n{}", probe.message, result.message);
    }
    if value["registry"].is_object() {
        if let Err(error) = package::registry(&home, &value["registry"], apply, &mut result) {
            bail!(
                "{}\n备份目录：{}",
                error,
                result
                    .backup
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| backup_root(&home).display().to_string())
            );
        }
    }
    if value["packagePlan"].is_object() {
        progress("已生成候选文件，正在备份并执行客户端安装文件修复…");
        startup::require_stopped()?;
        let p = package::apply(toolkit.path(), &value["packagePlan"], &mut progress)?;
        result.message.push_str(&format!("\n\n{}", p.message));
        result.applied = p.applied;
        result.can_restore = p.can_restore;
        result.backup = p.backup;
    }
    if kind == Kind::Surface {
        package::surface_environment(&home, apply, &mut result)?;
    }
    if apply {
        result.needs_repair = false;
    }
    Ok(result)
}

fn startup_issue(apply: bool, progress: &mut impl FnMut(&str)) -> Result<Outcome> {
    if apply {
        startup::require_stopped()?;
    }
    progress("正在核对内置程序路径；随后单独检查状态库…");
    let path = launch_path::run(apply);
    let database = startup::check()
        .and_then(|checked| {
            if apply && checked.needs_repair {
                startup::run(progress)
            } else {
                Ok(checked)
            }
        })
        .map(|r| Outcome {
            needs_repair: r.needs_repair,
            can_restore: r.applied,
            applied: r.applied,
            backup: r.backup,
            message: r.message,
        });
    Ok(startup_results(path, database))
}
fn startup_results(path: Result<Outcome>, database: Result<Outcome>) -> Outcome {
    let mut result = Outcome {
        message: String::new(),
        backup: None,
        applied: false,
        can_restore: false,
        needs_repair: false,
    };
    for (label, part) in [("程序路径", path), ("状态库", database)] {
        let message = match part {
            Ok(part) => {
                result.applied |= part.applied;
                result.can_restore |= part.can_restore;
                result.needs_repair |= part.needs_repair;
                if let Some(path) = part.backup {
                    result.backup = Some(path.clone());
                    format!("{}\n本项备份：{}", part.message, path.display())
                } else {
                    part.message
                }
            }
            Err(e) => format!("本项未完成：{e}。其他检查项仍单独处理。"),
        };
        result
            .message
            .push_str(&format!("{label}：\n{message}\n\n"));
    }
    result.message.push_str("“无法加载组织设置”可能有多种原因，上述结果分别针对程序路径和状态库。修复后需重新启动客户端验证；若仍报错，请导出连接诊断继续定位。");
    result
}

pub(crate) fn records() -> Result<Vec<Record>> {
    let home = startup::user_directory()?;
    let db = startup::locations()
        .map(|(_, db)| db)
        .unwrap_or_else(|_| home.join("state_5.sqlite"));
    let mut records = Vec::new();
    for root in [
        backup_root(&home),
        db.parent().unwrap().join("hub-startup-repair-backups"),
    ] {
        for entry in fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let m = path.join("manifest.json");
            let native = path.join("native-registry.json");
            let environment = path.join("native-environment.json");
            let package = path.join("package-plan.json");
            let launch_path = launch_path::is_record(&path);
            if m.is_file()
                || native.is_file()
                || environment.is_file()
                || package.is_file()
                || launch_path
            {
                let kind = fs::read(&m)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .and_then(|v| v["kind"].as_str().map(str::to_string))
                    .unwrap_or_else(|| {
                        if package.is_file() {
                            "package".into()
                        } else if native.is_file() {
                            "browser".into()
                        } else {
                            "startup".into()
                        }
                    });
                let label = format!(
                    "{} · {}",
                    entry.file_name().to_string_lossy(),
                    Kind::ALL
                        .into_iter()
                        .find(|k| k.key() == kind)
                        .map(Kind::label)
                        .unwrap_or("客户端文件修复")
                );
                let state = |file: &Path| {
                    fs::read(file)
                        .ok()
                        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                };
                let manifest = state(&m);
                let journal = state(&path.join(if cfg!(windows) {
                    "package-journal.json"
                } else {
                    "mac-package.json"
                }));
                let native_done = [native.clone(), environment.clone()]
                    .iter()
                    .filter(|f| f.is_file())
                    .all(|f| state(f).is_some_and(|v| v["restored"] == true));
                let files_done = manifest.as_ref().is_none_or(|v| v["stage"] == "restored");
                let related_done = associated_record(&backup_root(&home), &path)
                    .ok()
                    .flatten()
                    .is_none_or(|p| {
                        state(&p.join("manifest.json")).is_some_and(|v| v["stage"] == "restored")
                    });
                let package_done = !package.is_file()
                    || journal.as_ref().is_some_and(|v| v["stage"] == "restored");
                let restored = (if launch_path {
                    launch_path::restored(&path)
                } else {
                    files_done && native_done && related_done && package_done
                }) || state(&path.join("restore-complete.json"))
                    .is_some_and(|v| v["schemaVersion"] == 1 && v["restored"] == true);
                records.push(Record {
                    path,
                    label: if restored {
                        format!("{label} · 已恢复")
                    } else {
                        label
                    },
                    can_restore: !restored,
                });
            }
        }
    }
    records.sort_by(|a, b| b.label.cmp(&a.label));
    records.truncate(60);
    Ok(records)
}
fn associated_record(root: &Path, path: &Path) -> Result<Option<PathBuf>> {
    let file = path.join("package-plan.json");
    if !file.is_file() {
        return Ok(None);
    }
    let plan: Value = serde_json::from_slice(&fs::read(file)?)?;
    let Some(related) = plan["relatedBackup"].as_str() else {
        return Ok(None);
    };
    let related = PathBuf::from(related).canonicalize()?;
    ensure!(
        related.parent() == Some(root.canonicalize()?.as_path())
            && related != path
            && !related.join("package-plan.json").exists(),
        "关联备份不属于本次修复范围"
    );
    Ok(Some(related))
}
pub(crate) fn restore(path: &Path, mut progress: impl FnMut(&str)) -> Result<Outcome> {
    let home = startup::user_directory()?;
    let db = startup::locations()
        .map(|(_, db)| db)
        .unwrap_or_else(|_| home.join("state_5.sqlite"));
    let path = path.canonicalize()?;
    let startup_root = db.parent().unwrap().join("hub-startup-repair-backups");
    if startup_root.exists() && path.starts_with(startup_root.canonicalize()?) {
        startup::restore(&path)?;
        return Ok(Outcome {
            message: "已恢复修复前的状态库。原启动问题也可能恢复。".into(),
            backup: Some(path),
            applied: false,
            can_restore: false,
            needs_repair: false,
        });
    }
    let _access = crate::codex_access::repair()?;
    startup::require_stopped()?;
    ensure!(
        path.starts_with(backup_root(&home).canonicalize()?)
            && path.parent() == Some(backup_root(&home).canonicalize()?.as_path()),
        "所选备份不属于本用户的修复记录"
    );
    if launch_path::is_record(&path) {
        progress("正在核对并恢复程序路径设置；Hub 保持运行…");
        return launch_path::restore(&path);
    }
    progress("正在核对修复后文件，只有未被后续修改的内容才允许恢复…");
    let toolkit = tempfile::tempdir()?;
    assets::extract(toolkit.path())?;
    let related = associated_record(&backup_root(&home), &path)?;
    let mut parts = vec![path.clone()];
    if let Some(related) = related {
        parts.push(related);
    }
    let needs_node = parts.iter().any(|p| p.join("manifest.json").is_file());
    let node = if needs_node {
        let cli = startup::locate_cli()?;
        Some(node(toolkit.path(), &resources(&cli)?)?)
    } else {
        None
    };
    let file_restore = |part: &Path, verify: bool| -> Result<()> {
        if part.join("manifest.json").is_file() {
            node_run(
                toolkit.path(),
                node.as_ref().context("缺少文件恢复工具")?,
                json!({"action":"restore","verifyOnly":verify,"home":home,"localRoot":local_root(),"nativeRoots":native_roots(),"backupRoot":backup_root(&home),"backup":part}),
            )?;
        }
        Ok(())
    };
    // Every part is checked before any part is changed. A resumed restore can
    // finish its remaining configuration after the package was restored.
    for part in &parts {
        package::registry_restore(part, false)?;
        package::environment_restore(part, false)?;
        file_restore(part, true)?;
    }
    if path.join("package-plan.json").is_file() {
        package::verify_restore(&path)?;
    }
    if path.join("package-plan.json").is_file() {
        package::restore(toolkit.path(), &path, &mut progress)?;
    }
    for part in &parts {
        file_restore(part, false)?;
        package::registry_restore(part, true)?;
        package::environment_restore(part, true)?;
    }
    private_write(
        &path.join("restore-complete.json"),
        &serde_json::to_vec(&json!({"schemaVersion":1,"restored":true}))?,
    )?;
    Ok(Outcome {
        message: "已恢复所选修复及其关联配置。原问题也可能恢复，请按需重新检查。".into(),
        backup: Some(path),
        applied: false,
        can_restore: false,
        needs_repair: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_path_repair_remains_available_without_database_conflict() {
        let path = Ok(Outcome {
            message: "path binding available".into(),
            backup: None,
            applied: false,
            can_restore: false,
            needs_repair: true,
        });
        let database = Ok(Outcome {
            message: "no migration conflict".into(),
            backup: None,
            applied: false,
            can_restore: false,
            needs_repair: false,
        });
        let result = startup_results(path, database);
        assert!(result.needs_repair);
        assert!(result.message.contains("程序路径"));
        assert!(result.message.contains("状态库"));
        let path = Ok(Outcome {
            message: "path repaired".into(),
            backup: Some(PathBuf::from("fixture-backup")),
            applied: true,
            can_restore: true,
            needs_repair: false,
        });
        let result = startup_results(path, Err(anyhow::anyhow!("invalid sqlite_home")));
        assert!(result.applied && result.can_restore);
        assert!(result.backup.is_some());
        assert!(result.message.contains("invalid sqlite_home"));
    }
    #[test]
    fn retired_node_commands_are_narrowly_scoped() {
        let root = Path::new("D:/data/Codex/runtimes/cua_node");
        assert!(is_retired_node(
            Path::new("D:/data/Codex/runtimes/cua_node/old/bin/node.exe"),
            root,
            "node.exe"
        ));
        for s in [
            "D:/other/node.exe",
            "D:/data/Codex/runtimes/cua_node/../bin/node.exe",
            "D:/data/Codex/runtimes/cua_node/old/bin/node_repl.exe",
            "D:/data/Codex/runtimes/cua_node/old/more/bin/node.exe",
        ] {
            assert!(!is_retired_node(Path::new(s), root, "node.exe"));
        }
    }
    #[test]
    fn model_config_merge_preserves_choices_comments_and_other_settings() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"),"# personal choice\nmodel = 'kept'\n[desktop]\nenabled-reasoning-efforts = ['low','high']\n[features]\nfast_mode = false\nother = true\n").unwrap();
        let (next, _) = candidate(Kind::Models, root.path(), root.path(), false).unwrap();
        let text = next.unwrap();
        let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
        assert!(text.contains("# personal choice"));
        assert_eq!(doc["model"].as_str(), Some("kept"));
        assert_eq!(doc["features"]["other"].as_bool(), Some(true));
        let choices = doc["desktop"]["enabled-reasoning-efforts"]
            .as_array()
            .unwrap();
        assert_eq!(
            choices
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>(),
            ["low", "high", "max", "ultra"]
        );
    }
    #[test]
    fn unsupported_configuration_types_are_not_replaced() {
        let root = tempfile::tempdir().unwrap();
        for text in [
            "desktop='unknown'",
            "[desktop]\nenabled-reasoning-efforts=[3]",
            "[features]\nfast_mode='unknown'",
        ] {
            fs::write(root.path().join("config.toml"), text).unwrap();
            assert!(candidate(Kind::Models, root.path(), root.path(), false).is_err());
            assert_eq!(
                fs::read_to_string(root.path().join("config.toml")).unwrap(),
                text
            );
        }
    }
    #[test]
    fn browser_config_keeps_explicitly_disabled_and_optional_plugins() {
        let root = tempfile::tempdir().unwrap();
        for name in ["browser", "chrome"] {
            let p = root
                .path()
                .join("plugins/openai-bundled/plugins")
                .join(name)
                .join(".codex-plugin");
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join("plugin.json"), "{}").unwrap();
        }
        fs::write(root.path().join("config.toml"),"[plugins.'browser@openai-bundled']\nenabled=false\n[plugins.'other@elsewhere']\nenabled=true\n").unwrap();
        let (next, _) = candidate(Kind::Browser, root.path(), root.path(), false).unwrap();
        let doc = next.unwrap().parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(
            doc["plugins"]["browser@openai-bundled"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            doc["plugins"]["chrome@openai-bundled"]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(
            doc["plugins"]["other@elsewhere"]["enabled"].as_bool(),
            Some(true)
        );
    }
    #[test]
    fn associated_backups_cannot_escape_or_chain_packages() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("package");
        let b = root.path().join("files");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(
            a.join("package-plan.json"),
            serde_json::to_vec(&json!({"relatedBackup":b})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            associated_record(root.path(), &a).unwrap(),
            Some(b.canonicalize().unwrap())
        );
        fs::write(b.join("package-plan.json"), "{}").unwrap();
        assert!(associated_record(root.path(), &a).is_err());
        fs::write(
            a.join("package-plan.json"),
            serde_json::to_vec(&json!({"relatedBackup":root.path().parent()})).unwrap(),
        )
        .unwrap();
        assert!(associated_record(root.path(), &a).is_err());
    }
}
