//! Shared, persistent Server package. Check never installs or executes anything.
use crate::{config::OutboundProxyConfig, desktop::transaction};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const OWNER: &str = "tiancai-companion-server-v1";
const RELEASE: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const MAX_ARCHIVE: u64 = 384 * 1024 * 1024;
const MAX_UNPACKED: u64 = 1536 * 1024 * 1024;
const LICENSE_REFERENCE: &str = "rust-v0.167.0";
static PREPARING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) fn root(config_path: &Path) -> PathBuf {
    crate::desktop::state_root(config_path).join("companion-server")
}
fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => anyhow::bail!("天才搭档 Server 仅支持 Windows / macOS 的 x64 和 Arm64"),
    }
}
fn executable() -> &'static str {
    if cfg!(windows) { "codex.exe" } else { "codex" }
}
fn native(name: &str) -> String {
    format!("{name}{}", if cfg!(windows) { ".exe" } else { "" })
}
fn safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':', '\0'])
        && !name.starts_with('/')
        && name.split('/').all(|s| s != "..")
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}
fn component(directory: &Path, relative: &str, is_dir: bool) -> Result<PathBuf> {
    ensure!(safe_relative(relative), "Server 运行包包含越界组件位置");
    let path = directory.join(relative);
    transaction::reject_links(&path)?;
    ensure!(
        if is_dir {
            path.is_dir()
        } else {
            path.is_file()
        },
        "Server 运行包缺少组件 {relative}，请在设置中重新确认准备"
    );
    Ok(path)
}
/// Inspect only known layout paths. No recursive hashes and no CLI probes.
fn validate_layout(directory: &Path, expected_target: &str) -> Result<Value> {
    let bytes = transaction::read(&directory.join("codex-package.json"))?
        .context("Server 运行包缺少完整清单")?;
    let manifest: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        manifest["layoutVersion"] == 1 && manifest["target"] == expected_target,
        "Server 运行包布局或架构不兼容，请重新确认准备"
    );
    let entrypoint = manifest["entrypoint"]
        .as_str()
        .context("运行包清单缺少主程序")?;
    ensure!(
        entrypoint == format!("bin/{}", executable()),
        "运行包主程序布局不兼容"
    );
    component(directory, entrypoint, false)?;
    for (key, expected) in [
        ("resourcesDir", "codex-resources"),
        ("pathDir", "codex-path"),
    ] {
        ensure!(manifest[key] == expected, "运行包配套资源布局不兼容");
        component(directory, expected, true)?;
    }
    component(
        directory,
        &format!("bin/{}", native("codex-code-mode-host")),
        false,
    )?;
    component(directory, &format!("codex-path/{}", native("rg")), false)?;
    // Older official manifests did not declare execServer.
    if let Some(path) = manifest.get("execServer") {
        component(
            directory,
            path.as_str().context("execServer 组件记录无效")?,
            false,
        )?;
    }
    #[cfg(windows)]
    for name in ["codex-command-runner", "codex-windows-sandbox-setup"] {
        component(
            directory,
            &format!("codex-resources/{}", native(name)),
            false,
        )?;
    }
    Ok(manifest)
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadyRuntime {
    pub(crate) program: PathBuf,
    pub(crate) package_root: PathBuf,
    pub(crate) version: String,
    pub(crate) source: String,
}
impl ReadyRuntime {
    pub(crate) fn configure_command(&self, command: &mut tokio::process::Command) -> Result<()> {
        let mut paths = vec![
            self.package_root.join("codex-path"),
            self.package_root.join("bin"),
        ];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        command.env("PATH", std::env::join_paths(paths)?);
        for key in [
            "CODEX_MANAGED_BY_NPM",
            "CODEX_MANAGED_BY_PNPM",
            "CODEX_MANAGED_BY_BUN",
            "CODEX_MANAGED_BY_VITE_PLUS",
        ] {
            command.env_remove(key);
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeCheck {
    pub(crate) ready: bool,
    pub(crate) installed: bool,
    pub(crate) can_uninstall: bool,
    pub(crate) needs_preparation: bool,
    pub(crate) detail: String,
    pub(crate) source: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) program: Option<PathBuf>,
    pub(crate) package_root: Option<PathBuf>,
}
pub(crate) fn resolve_ready(config_path: &Path) -> Result<ReadyRuntime> {
    resolve_at(&root(config_path), target()?)
}
fn owned(root: &Path) -> bool {
    transaction::read(&root.join("owner.json"))
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["owner"] == OWNER)
}
fn resolve_at(root: &Path, target: &str) -> Result<ReadyRuntime> {
    let bytes = transaction::read(&root.join("current.json"))?
        .context("尚未准备共享天才搭档 Server，请在设置中确认准备")?;
    let marker: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        marker["owner"] == OWNER && marker["schema"] == 1 && marker["target"] == target,
        "共享 Server 记录或架构不兼容，请重新确认准备"
    );
    let package = marker["packageRoot"]
        .as_str()
        .context("共享 Server 位置记录无效")?;
    ensure!(safe_relative(package), "共享 Server 位置记录无效");
    let package_root = root.join(package);
    validate_layout(&package_root, target)?;
    let program = component(
        &package_root,
        &format!("bin/{}", super::brand::executable()),
        false,
    )?;
    ensure!(
        marker["program"] == format!("{package}/bin/{}", super::brand::executable()),
        "共享 Server 主程序记录不符"
    );
    component(&package_root, "hub-provenance.json", false)?;
    Ok(ReadyRuntime {
        program,
        package_root,
        version: marker["version"]
            .as_str()
            .context("共享 Server 缺少版本记录")?
            .into(),
        source: marker["source"]
            .as_str()
            .context("共享 Server 缺少来源记录")?
            .into(),
    })
}
pub(crate) fn check(config_path: &Path, selected: Option<&Path>) -> RuntimeCheck {
    let installed = owned(&root(config_path));
    match resolve_ready(config_path) {
        Ok(ready) => RuntimeCheck {
            ready: true,
            installed,
            can_uninstall: installed,
            needs_preparation: false,
            detail: format!(
                "共享天才搭档 Server 已准备（{}），由启用的功能按需启动。",
                ready.version
            ),
            source: Some(ready.source),
            version: Some(ready.version),
            program: Some(ready.program),
            package_root: Some(ready.package_root),
        },
        Err(error) => {
            // Polling never enumerates PATH/WindowsApps. Automatic source
            // discovery belongs to the person's confirmed preparation action.
            let source = selected.and_then(|path| {
                target()
                    .ok()
                    .and_then(|target| inspect_source(path.to_owned(), target).ok())
            });
            RuntimeCheck {
                ready: false,
                installed,
                can_uninstall: installed,
                needs_preparation: true,
                detail: if source.is_some() {
                    format!("{error}。发现已有兼容程序，确认后将复制为 Hub 自管共享 Server。")
                } else {
                    format!(
                        "{error}。确认后优先复用已有兼容程序，无可用来源时从官方准备完整运行包。"
                    )
                },
                source: Some(
                    if source.is_some() {
                        "existing-program"
                    } else {
                        "auto"
                    }
                    .into(),
                ),
                version: source.as_ref().map(|s| s.version.clone()),
                program: None,
                package_root: None,
            }
        }
    }
}
#[derive(Debug)]
struct Source {
    program: PathBuf,
    package: Option<PathBuf>,
    version: String,
}
fn inspect_source(program: PathBuf, expected_target: &str) -> Result<Source> {
    transaction::reject_links(&program)?;
    validate_program_architecture(&program)?;
    let parent = program.parent().context("已有程序位置无效")?;
    let directory = parent
        .parent()
        .filter(|_| parent.file_name().is_some_and(|n| n == "bin"));
    if let Some(directory) = directory.filter(|p| p.join("codex-package.json").is_file()) {
        let manifest = validate_layout(directory, expected_target)?;
        let version = manifest["version"]
            .as_str()
            .or_else(|| manifest["codexVersion"].as_str())
            .map(str::to_string)
            .or_else(|| super::brand::version(&program))
            .unwrap_or_else(|| "existing-cli".into());
        let package = Some(directory.to_owned());
        return Ok(Source {
            program,
            package,
            version,
        });
    }
    ensure!(
        parent.join(native("codex-code-mode-host")).is_file(),
        "已有运行程序缺少 Code Mode 组件，无法准备天才搭档 Server"
    );
    ensure!(
        flat_rg(parent).is_some(),
        "已有运行程序缺少配套搜索组件，无法准备天才搭档 Server"
    );
    #[cfg(windows)]
    for name in ["codex-command-runner", "codex-windows-sandbox-setup"] {
        ensure!(
            parent.join(native(name)).is_file(),
            "已有运行程序缺少 {name} 组件，无法准备天才搭档 Server"
        );
    }
    let version = super::brand::version(&program).unwrap_or_else(|| "existing-cli".into());
    Ok(Source {
        program,
        package: None,
        version,
    })
}
fn validate_program_architecture(program: &Path) -> Result<()> {
    let mut input = std::fs::File::open(program)?;
    let mut header = [0u8; 64];
    input
        .read_exact(&mut header)
        .context("已有程序格式不完整")?;
    #[cfg(windows)]
    {
        ensure!(&header[..2] == b"MZ", "已有程序不是原生 Windows 运行程序");
        let offset = u32::from_le_bytes(header[60..64].try_into()?);
        input.seek(SeekFrom::Start(u64::from(offset)))?;
        let mut coff = [0u8; 6];
        input.read_exact(&mut coff)?;
        ensure!(&coff[..4] == b"PE\0\0", "已有程序 PE 格式不兼容");
        let machine = u16::from_le_bytes(coff[4..6].try_into()?);
        ensure!(
            machine
                == if cfg!(target_arch = "aarch64") {
                    0xaa64
                } else {
                    0x8664
                },
            "已有运行程序架构与本机不兼容"
        );
    }
    #[cfg(target_os = "macos")]
    {
        let magic = u32::from_be_bytes(header[..4].try_into()?);
        let expected = if cfg!(target_arch = "aarch64") {
            0x0100000c
        } else {
            0x01000007
        };
        if magic == 0xcafebabe || magic == 0xcafebabf {
            let count = u32::from_be_bytes(header[4..8].try_into()?);
            ensure!(count <= 16, "已有通用程序架构表无效");
            input.seek(SeekFrom::Start(8))?;
            let stride = if magic == 0xcafebabf { 32 } else { 20 };
            let mut found = false;
            for _ in 0..count {
                let mut row = vec![0u8; stride];
                input.read_exact(&mut row)?;
                found |= u32::from_be_bytes(row[..4].try_into()?) == expected;
            }
            ensure!(found, "已有运行程序架构与本机不兼容");
        } else {
            ensure!(
                matches!(magic, 0xcffaedfe | 0xfeedfacf),
                "已有程序不是原生 macOS CLI"
            );
            let machine = if magic == 0xcffaedfe {
                u32::from_le_bytes(header[4..8].try_into()?)
            } else {
                u32::from_be_bytes(header[4..8].try_into()?)
            };
            ensure!(machine == expected, "已有运行程序架构与本机不兼容");
        }
    }
    Ok(())
}
fn flat_rg(parent: &Path) -> Option<PathBuf> {
    [
        Some(parent.join(native("rg"))),
        parent.parent().map(|p| p.join("path").join(native("rg"))),
        Some(parent.join("codex-path").join(native("rg"))),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}
fn select_source(
    selected: Option<&Path>,
    target: &str,
    discover: impl FnOnce() -> Result<Vec<PathBuf>>,
) -> Result<Option<Source>> {
    if let Some(selected) = selected {
        ensure!(
            selected.is_absolute(),
            "天才搭档 Server 来源必须使用完整路径"
        );
        // A saved Store-version path can disappear after an upgrade. It is a
        // source preference, not an execution path; rediscover only on confirmed preparation.
        if selected.try_exists()? {
            return inspect_source(super::binary::resolve(Some(selected))?, target).map(Some);
        }
    }
    Ok(discover()?
        .into_iter()
        .find_map(|p| inspect_source(p, target).ok()))
}
fn find_source(config_path: &Path, selected: Option<&Path>) -> Result<Option<Source>> {
    select_source(selected, target()?, || {
        let mut candidates = Vec::new();
        if let Some(bytes) =
            transaction::read(&super::settings::root(config_path).join("runtime/current.json"))?
            && let Ok(marker) = serde_json::from_slice::<Value>(&bytes)
            && let Some(relative) = marker["program"].as_str().filter(|s| safe_relative(s))
        {
            candidates.push(
                super::settings::root(config_path)
                    .join("runtime")
                    .join(relative),
            );
        }
        candidates.extend(super::binary::available());
        Ok(candidates)
    })
}
#[derive(Debug)]
struct Package {
    tag: String,
    url: String,
    digest: Option<String>,
}
fn select_release(value: &Value, target: &str) -> Result<Package> {
    let tag = value["tag_name"]
        .as_str()
        .context("官方未返回天才搭档 Server 运行程序版本")?;
    ensure!(
        tag.starts_with("rust-v")
            && tag
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c)),
        "官方版本标识无效"
    );
    let name = format!("codex-package-{target}.tar.gz");
    let asset = value["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"] == name))
        .context("官方暂未提供本机架构的完整运行包，请选择已有兼容 CLI")?;
    ensure!(
        asset["size"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= MAX_ARCHIVE),
        "官方运行包大小异常"
    );
    let url = format!("https://github.com/openai/codex/releases/download/{tag}/{name}");
    ensure!(
        asset["browser_download_url"] == url,
        "官方运行包地址与所选版本不符"
    );
    let digest = asset["digest"]
        .as_str()
        .map(|d| -> Result<String> {
            let d = d
                .strip_prefix("sha256:")
                .context("官方运行包摘要格式不支持")?;
            ensure!(
                d.len() == 64 && d.bytes().all(|c| c.is_ascii_hexdigit()),
                "官方运行包摘要无效"
            );
            Ok(d.to_ascii_lowercase())
        })
        .transpose()?;
    Ok(Package {
        tag: tag.into(),
        url,
        digest,
    })
}
async fn fetch(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let mut response = client
        .get(url)
        .send()
        .await
        .context("无法连接官方 Server 下载服务，请检查网络后重试")?;
    for _ in 0..3 {
        if !response.status().is_redirection() {
            break;
        }
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .context("官方运行包跳转缺少地址")?;
        let location = url::Url::parse(location)?;
        ensure!(
            location.scheme() == "https"
                && matches!(
                    location.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                            | "raw.githubusercontent.com"
                    )
                ),
            "运行包下载跳转地址无效"
        );
        response = client
            .get(location)
            .send()
            .await
            .context("官方运行包下载中断")?;
    }
    ensure!(
        response.status().is_success(),
        "官方 Server 下载失败（HTTP {}）",
        response.status()
    );
    Ok(response)
}
pub(crate) async fn prepare(
    config_path: &Path,
    selected: Option<&Path>,
    proxy: &OutboundProxyConfig,
    confirmed: bool,
) -> Result<ReadyRuntime> {
    let _gate = PREPARING.lock().await;
    if let Ok(ready) = resolve_ready(config_path) {
        return Ok(ready);
    }
    ensure!(
        confirmed,
        "需要本人在设置中确认准备共享天才搭档 Server；尚未下载、复制或启动程序"
    );
    let runtime_root = root(config_path);
    let target = target()?;
    if let Some(source) = find_source(config_path, selected)? {
        return tokio::task::spawn_blocking(move || install_source(&runtime_root, target, &source))
            .await?;
    }
    let client = crate::outbound_http::apply_async_proxy(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("TianCaiSpaceHub-companion-server")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(540)),
        proxy,
        None,
    )?
    .build()?;
    let metadata = client
        .get(RELEASE)
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .context("无法读取官方 Server 版本")?;
    ensure!(
        metadata.status().is_success(),
        "读取官方 Server 版本失败（HTTP {}）",
        metadata.status()
    );
    let package = select_release(&metadata.json::<Value>().await?, target)?;
    let notices = Notices::download(&client, &package.tag).await?;
    transaction::reject_links(&runtime_root)?;
    std::fs::create_dir_all(&runtime_root)?;
    let archive = tempfile::NamedTempFile::new_in(&runtime_root)?;
    download_archive(&client, &package.url, package.digest.as_deref(), &archive).await?;
    tokio::task::spawn_blocking(move || {
        install_archive(&runtime_root, target, &package, archive.as_file(), &notices)
    })
    .await?
}
async fn download_archive(
    client: &reqwest::Client,
    url: &str,
    expected_digest: Option<&str>,
    archive: &tempfile::NamedTempFile,
) -> Result<()> {
    let mut output = tokio::fs::File::from_std(archive.reopen()?);
    let mut response = fetch(client, url).await?;
    ensure!(
        !response.content_length().is_some_and(|n| n > MAX_ARCHIVE),
        "Server 下载包超过容量限制"
    );
    let mut total = 0u64;
    let mut hash = Sha256::new();
    while let Some(chunk) = response.chunk().await.context("Server 下载中断，请重试")? {
        total += chunk.len() as u64;
        ensure!(total <= MAX_ARCHIVE, "Server 下载包超过容量限制");
        if expected_digest.is_some() {
            hash.update(&chunk);
        }
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    drop(output);
    ensure!(total > 0, "官方 Server 下载包为空");
    if let Some(digest) = expected_digest {
        ensure!(
            hex::encode(hash.finalize()) == digest,
            "官方运行包下载不完整，请重新准备"
        );
    }
    Ok(())
}
struct Notices {
    license: String,
    notice: String,
    reference: String,
}
impl Notices {
    fn bundled() -> Self {
        Self {
            license: include_str!("licenses/LICENSE.codex").into(),
            notice: include_str!("licenses/NOTICE.codex").into(),
            reference: LICENSE_REFERENCE.into(),
        }
    }
    async fn download(client: &reqwest::Client, tag: &str) -> Result<Self> {
        async fn text(client: &reqwest::Client, tag: &str, name: &str) -> Result<String> {
            let response = fetch(
                client,
                &format!("https://raw.githubusercontent.com/openai/codex/{tag}/{name}"),
            )
            .await?;
            ensure!(
                !response.content_length().is_some_and(|n| n > 256 * 1024),
                "官方许可文件过大"
            );
            let bytes = response.bytes().await?;
            ensure!(bytes.len() <= 256 * 1024, "官方许可文件过大");
            Ok(String::from_utf8(bytes.to_vec())?)
        }
        let license = text(client, tag, "LICENSE").await?;
        let notice = text(client, tag, "NOTICE").await?;
        ensure!(
            license.contains("Apache License") && license.contains("Version 2.0"),
            "官方运行包许可已变化，请更新 Hub 后再准备"
        );
        Ok(Self {
            license,
            notice,
            reference: tag.into(),
        })
    }
    fn write(&self, directory: &Path) -> Result<()> {
        let root = directory.join("hub-notices");
        std::fs::create_dir_all(&root)?;
        std::fs::write(root.join("LICENSE.codex"), &self.license)?;
        std::fs::write(root.join("NOTICE.codex"), &self.notice)?;
        std::fs::write(
            root.join("MODIFICATIONS.txt"),
            "Hub creates a local copy branded as 天才搭档 Server.\nThe original executable alias, companion layout and upstream version/copyright are retained.\nWindows FileDescription/ProductName/InternalName/OriginalFilename resources are changed only in the branded copy.\nThis local copy is not an official OpenAI product or a re-signed upstream executable.\n",
        )?;
        Ok(())
    }
}
fn unpack(input: impl Read, directory: &Path) -> Result<PathBuf> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(input));
    let mut paths = HashSet::new();
    let mut program = None;
    let mut total = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .to_str()
            .context("运行包包含无效文件名")?
            .trim_start_matches("./")
            .to_string();
        ensure!(safe_relative(&path), "运行包包含越界路径");
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_dir(),
            "运行包包含不支持的链接或文件类型"
        );
        let identity = if cfg!(windows) {
            path.to_lowercase()
        } else {
            path.clone()
        };
        ensure!(
            paths.insert(identity) && paths.len() <= 12000,
            "运行包包含重复路径或文件过多"
        );
        total = total.checked_add(entry.size()).context("运行包容量异常")?;
        ensure!(total <= MAX_UNPACKED, "运行包解压超过容量限制");
        let destination = directory.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&destination)?;
        std::io::copy(&mut entry, &mut output)?;
        output.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output.set_permissions(std::fs::Permissions::from_mode(
                entry.header().mode()? & 0o777,
            ))?;
        }
        if Path::new(&path)
            .file_name()
            .is_some_and(|n| n == executable())
        {
            ensure!(program.is_none(), "运行包包含多个 CLI 主程序");
            program = Some(PathBuf::from(path));
        }
    }
    program.context("完整运行包中未找到 CLI 主程序")
}
fn stage(root: &Path) -> Result<tempfile::TempDir> {
    let packages = root.join("packages");
    transaction::reject_links(&packages)?;
    std::fs::create_dir_all(&packages)?;
    tempfile::tempdir_in(packages).context("无法准备共享 Server 数据目录")
}
fn install_archive(
    root: &Path,
    target: &str,
    package: &Package,
    archive: &std::fs::File,
    notices: &Notices,
) -> Result<ReadyRuntime> {
    let stage = stage(root)?;
    let program = unpack(archive, stage.path()).context("无法解压官方完整运行包")?;
    ensure!(
        program == Path::new(&format!("bin/{}", executable())),
        "运行包主程序位置不兼容"
    );
    validate_layout(stage.path(), target)?;
    finalize(
        root,
        target,
        stage,
        &package.tag,
        "official-download",
        json!({"url":package.url,"archiveSha256":package.digest}),
        notices,
    )
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let mut queue = vec![(from.to_owned(), to.to_owned())];
    let mut count = 0;
    let mut total = 0u64;
    while let Some((from, to)) = queue.pop() {
        transaction::reject_links(&from)?;
        count += 1;
        ensure!(count <= 12000, "已有运行包文件过多");
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            for entry in std::fs::read_dir(&from)? {
                let entry = entry?;
                queue.push((entry.path(), to.join(entry.file_name())));
            }
        } else {
            total = total
                .checked_add(std::fs::metadata(&from)?.len())
                .context("已有运行包容量异常")?;
            ensure!(total <= MAX_UNPACKED, "已有运行包超过容量限制");
            std::fs::copy(from, to)?;
        }
    }
    Ok(())
}
fn install_source(root: &Path, target: &str, source: &Source) -> Result<ReadyRuntime> {
    let stage = stage(root)?;
    if let Some(package) = &source.package {
        copy_tree(package, stage.path())?;
    } else {
        let parent = source.program.parent().context("已有程序位置无效")?;
        for dir in ["bin", "codex-resources", "codex-path"] {
            std::fs::create_dir_all(stage.path().join(dir))?;
        }
        std::fs::copy(&source.program, stage.path().join("bin").join(executable()))?;
        for name in [
            "codex-code-mode-host",
            "codex-exec-server",
            "codex-command-runner",
            "codex-windows-sandbox-setup",
            "codex-windows-sandbox-service",
        ] {
            let name = native(name);
            let from = parent.join(&name);
            if from.is_file() {
                transaction::reject_links(&from)?;
                std::fs::copy(&from, stage.path().join("bin").join(&name))?;
                std::fs::copy(&from, stage.path().join("codex-resources").join(&name))?;
            }
        }
        let rg = flat_rg(parent).context("已有运行程序的搜索组件已移除")?;
        transaction::reject_links(&rg)?;
        std::fs::copy(rg, stage.path().join("codex-path").join(native("rg")))?;
        let mut manifest = json!({"layoutVersion":1,"target":target,"entrypoint":format!("bin/{}",executable()),
            "resourcesDir":"codex-resources","pathDir":"codex-path","hubImportedLegacyLayout":true});
        let exec = format!("bin/{}", native("codex-exec-server"));
        if stage.path().join(&exec).is_file() {
            manifest["execServer"] = exec.into();
        }
        std::fs::write(
            stage.path().join("codex-package.json"),
            serde_json::to_vec(&manifest)?,
        )?;
    }
    validate_layout(stage.path(), target)?;
    finalize(
        root,
        target,
        stage,
        &source.version,
        "existing-program",
        json!({"importedFrom":source.program,"originalPackage":source.package}),
        &Notices::bundled(),
    )
}
fn finalize(
    root: &Path,
    target: &str,
    stage: tempfile::TempDir,
    version: &str,
    source: &str,
    origin: Value,
    notices: &Notices,
) -> Result<ReadyRuntime> {
    let original = stage.path().join("bin").join(executable());
    let branded = stage.path().join("bin").join(super::brand::executable());
    std::fs::copy(&original, &branded)?;
    super::brand::apply(&branded).context("无法生成天才搭档 Server 品牌程序；原安装未修改")?;
    notices.write(stage.path())?;
    std::fs::write(
        stage.path().join("hub-provenance.json"),
        serde_json::to_vec_pretty(&json!({
        "owner":OWNER,"schema":1,"name":super::brand::NAME,"upstream":"https://github.com/openai/codex","version":version,"target":target,
        "source":source,"origin":origin,"license":"Apache-2.0","licenseReference":notices.reference,
        "licenseUrl":format!("https://github.com/openai/codex/blob/{}/LICENSE",notices.reference),
        "noticeUrl":format!("https://github.com/openai/codex/blob/{}/NOTICE",notices.reference),
        "originalEntrypoint":format!("bin/{}",executable()),"entrypoint":format!("bin/{}",super::brand::executable()),
        "modifiedWindowsVersionResources":cfg!(windows),"preparedAtMs":crate::types::now_ms(),"actualInitializationVerified":false}))?,
    )?;
    let identity = uuid::Uuid::new_v4().simple().to_string();
    let destination = root.join("packages").join(&identity);
    std::fs::rename(stage.path(), &destination)?;
    let package = format!("packages/{identity}");
    transaction::atomic_write(
        &root.join("owner.json"),
        &serde_json::to_vec(&json!({"owner":OWNER,"schema":1}))?,
    )?;
    transaction::atomic_write(
        &root.join("current.json"),
        &serde_json::to_vec(&json!({"owner":OWNER,"schema":1,"target":target,
        "version":version,"source":source,"packageRoot":package,"program":format!("{package}/bin/{}",super::brand::executable())}))?,
    )?;
    Ok(ReadyRuntime {
        program: destination.join("bin").join(super::brand::executable()),
        package_root: destination,
        version: version.into(),
        source: source.into(),
    })
}
/// The caller checks both feature switches and stops their workers first.
pub(crate) async fn uninstall(config_path: &Path) -> Result<Value> {
    let _gate = PREPARING.lock().await;
    let runtime = root(config_path);
    transaction::reject_links(&runtime)?;
    ensure!(
        owned(&runtime),
        "没有可移除的 Hub 自管 Server；未处理系统程序或旧运行包"
    );
    let canonical = runtime.canonicalize()?;
    ensure!(canonical.is_absolute(), "共享 Server 清理位置无效");
    if let Some(bytes) = transaction::read(&runtime.join("current.json"))? {
        ensure!(
            serde_json::from_slice::<Value>(&bytes)?["owner"] == OWNER,
            "当前 Server 记录归属不符，未移除"
        );
    }
    let packages = runtime.join("packages");
    let mut removed = 0;
    if packages.is_dir() {
        transaction::reject_links(&packages)?;
        for entry in std::fs::read_dir(&packages)? {
            let path = entry?.path();
            transaction::reject_links(&path)?;
            if !path.is_dir() {
                continue;
            }
            let Some(marker) = transaction::read(&path.join("hub-provenance.json"))? else {
                continue;
            };
            let marker: Value = serde_json::from_slice(&marker)?;
            if marker["owner"] != OWNER {
                continue;
            }
            ensure!(
                path.canonicalize()?.parent() == Some(canonical.join("packages").as_path()),
                "共享 Server 清理目录超出范围"
            );
            reject_tree_links(&path)?;
            std::fs::remove_dir_all(&path)
                .context("共享 Server 正在使用或无法移除，请停用后重试")?;
            removed += 1;
        }
    }
    if runtime.join("current.json").is_file() {
        std::fs::remove_file(runtime.join("current.json"))?;
    }
    std::fs::remove_file(runtime.join("owner.json"))?;
    let _ = std::fs::remove_dir(packages);
    let _ = std::fs::remove_dir(runtime);
    Ok(
        json!({"removed":removed,"detail":"已移除 Hub 自管的天才搭档 Server；系统 Codex、功能设置和旧运行包保留。"}),
    )
}
fn reject_tree_links(root: &Path) -> Result<()> {
    let mut queue = vec![root.to_owned()];
    let mut count = 0;
    while let Some(path) = queue.pop() {
        transaction::reject_links(&path)?;
        count += 1;
        ensure!(count <= 12000, "共享 Server 清理目录过大");
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                queue.push(entry?.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program_fixture() -> Vec<u8> {
        #[cfg(windows)]
        {
            std::fs::read(
                PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe"),
            )
            .unwrap()
        }
        #[cfg(target_os = "macos")]
        {
            std::fs::read("/usr/bin/true").unwrap()
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            b"synthetic CLI; never executed".to_vec()
        }
    }
    fn fixture(directory: &Path, target: &str, exec_server: bool) -> PathBuf {
        for name in ["bin", "codex-resources", "codex-path"] {
            std::fs::create_dir_all(directory.join(name)).unwrap();
        }
        let program = directory.join("bin").join(executable());
        std::fs::write(&program, program_fixture()).unwrap();
        for (dir, name) in [
            ("bin", "codex-code-mode-host"),
            ("codex-resources", "codex-command-runner"),
            ("codex-resources", "codex-windows-sandbox-setup"),
            ("codex-path", "rg"),
        ] {
            std::fs::write(directory.join(dir).join(native(name)), b"fixture companion").unwrap();
        }
        let mut manifest = json!({"layoutVersion":1,"target":target,"version":"fixture-version","entrypoint":format!("bin/{}",executable()),
            "resourcesDir":"codex-resources","pathDir":"codex-path"});
        if exec_server {
            let path = format!("bin/{}", native("codex-exec-server"));
            std::fs::write(directory.join(&path), b"fixture exec").unwrap();
            manifest["execServer"] = path.into();
        }
        std::fs::write(
            directory.join("codex-package.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        program
    }
    #[test]
    fn removed_saved_source_rediscovers_but_existing_invalid_source_remains_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let program = fixture(&temp.path().join("current-package"), "fixture", false);
        let removed = temp
            .path()
            .join("previous-store-package/resources")
            .join(executable());
        let source = select_source(Some(&removed), "fixture", || Ok(vec![program.clone()]))
            .unwrap()
            .unwrap();
        assert_eq!(source.program, program);
        assert!(!removed.exists());
        let invalid = temp.path().join(executable());
        std::fs::write(&invalid, b"incompatible existing program").unwrap();
        assert!(
            select_source(Some(&invalid), "fixture", || panic!(
                "must retain explicit source error"
            ))
            .is_err()
        );
        assert!(
            select_source(Some(Path::new("relative-codex")), "fixture", || panic!(
                "must reject relative path"
            ))
            .is_err()
        );
    }
    #[test]
    fn previous_npm_search_component_is_returned_without_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("vendor/codex");
        let search = temp.path().join("vendor/path").join(native("rg"));
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::create_dir_all(search.parent().unwrap()).unwrap();
        std::fs::write(&search, b"fixture search; never executed").unwrap();
        let found = flat_rg(&parent).unwrap();
        assert_eq!(found, search);
        assert!(
            !found
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        );
        transaction::reject_links(&found).unwrap();
    }
    #[test]
    fn legacy_manifest_and_branded_copy_keep_companion_discovery_and_original() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let original = fixture(&source, "fixture", false);
        let bytes = std::fs::read(&original).unwrap();
        let root = temp.path().join("shared");
        let imported = Source {
            program: original.clone(),
            package: Some(source.clone()),
            version: "fixture-version".into(),
        };
        let ready = install_source(&root, "fixture", &imported).unwrap();
        assert_eq!(
            ready.program.file_name().unwrap(),
            super::super::brand::executable()
        );
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        assert_eq!(
            std::fs::read(ready.package_root.join("bin").join(executable())).unwrap(),
            bytes
        );
        assert!(validate_layout(&ready.package_root, "fixture").is_ok());
        assert!(
            ready
                .package_root
                .join("hub-notices/LICENSE.codex")
                .is_file()
        );
        assert_eq!(resolve_at(&root, "fixture").unwrap().program, ready.program);
        let mut command = tokio::process::Command::new(&ready.program);
        ready.configure_command(&mut command).unwrap();
        let path = command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .unwrap();
        assert_eq!(
            std::env::split_paths(path).next().unwrap(),
            ready.package_root.join("codex-path")
        );
        std::fs::remove_file(ready.package_root.join("codex-path").join(native("rg"))).unwrap();
        assert!(resolve_at(&root, "fixture").is_err());
        assert!(source.join("codex-path").join(native("rg")).is_file());
    }
    #[tokio::test]
    async fn confirmation_precedes_preparation_and_concurrent_clients_share_one_copy() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.json");
        let source = temp.path().join("source");
        let program = fixture(&source, target().unwrap(), false);
        let proxy = OutboundProxyConfig::default();
        assert!(
            prepare(&config, Some(&program), &proxy, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("本人")
        );
        assert!(!root(&config).exists());
        let removed = temp.path().join("removed-store-package").join(executable());
        assert!(
            prepare(&config, Some(&removed), &proxy, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("本人")
        );
        assert!(!root(&config).exists());
        let (a, b) = tokio::join!(
            prepare(&config, Some(&program), &proxy, true),
            prepare(&config, Some(&program), &proxy, true)
        );
        let a = a.unwrap();
        assert_eq!(a.program, b.unwrap().program);
        assert_eq!(
            std::fs::read_dir(root(&config).join("packages"))
                .unwrap()
                .count(),
            1
        );
        assert!(check(&config, None).ready);
        std::fs::remove_file(&a.program).unwrap();
        assert!(check(&config, Some(&program)).needs_preparation);
        assert!(
            prepare(&config, Some(&program), &proxy, false)
                .await
                .is_err()
        );
        let unrelated = root(&config).join("packages/unowned");
        std::fs::create_dir(&unrelated).unwrap();
        std::fs::write(unrelated.join("keep.txt"), b"unowned").unwrap();
        uninstall(&config).await.unwrap();
        assert!(program.is_file() && unrelated.join("keep.txt").is_file());
        assert!(!root(&config).join("current.json").exists());
        assert!(uninstall(&config).await.is_err());
    }
    #[test]
    fn manifest_checks_optional_exec_server_and_required_companions() {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "fixture", true);
        assert!(validate_layout(temp.path(), "fixture").is_ok());
        std::fs::remove_file(temp.path().join("bin").join(native("codex-exec-server"))).unwrap();
        assert!(validate_layout(temp.path(), "fixture").is_err());
        for name in ["../outside", "C:/outside", "a\\outside", "/outside"] {
            assert!(!safe_relative(name));
        }
        let mut v = json!({"tag_name":"rust-v0.0.1","assets":[{"name":"codex-package-fixture.tar.gz","size":123,
            "browser_download_url":"https://github.com/openai/codex/releases/download/rust-v0.0.1/codex-package-fixture.tar.gz"}]});
        assert!(select_release(&v, "fixture").is_ok());
        v["assets"][0]["browser_download_url"] = "https://foreign.invalid/runtime".into();
        assert!(select_release(&v, "fixture").is_err());
    }
    fn bundle(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(gzip);
        for (path, bytes) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(bytes.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            tar.append_data(&mut h, path, *bytes).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }
    #[test]
    fn invalid_archive_never_replaces_shared_marker() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let program = fixture(&source, "fixture", false);
        let root = temp.path().join("shared");
        install_source(
            &root,
            "fixture",
            &Source {
                program,
                package: Some(source),
                version: "fixture".into(),
            },
        )
        .unwrap();
        let previous = std::fs::read(root.join("current.json")).unwrap();
        let mut archive = tempfile::NamedTempFile::new().unwrap();
        archive.write_all(b"invalid gzip").unwrap();
        let package = Package {
            tag: "rust-v0.0.1".into(),
            url: "fixture".into(),
            digest: None,
        };
        assert!(
            install_archive(
                &root,
                "fixture",
                &package,
                archive.as_file(),
                &Notices::bundled()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(root.join("current.json")).unwrap(), previous);
        assert!(
            unpack(
                bundle(&[(executable(), b"a"), (executable(), b"b")]).as_slice(),
                temp.path()
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn runtime_download_streams_one_digest_and_failure_keeps_marker_untouched() {
        let bytes = bundle(&[(executable(), b"synthetic download; never executed")]);
        let expected = hex::encode(Sha256::digest(&bytes));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route(
                "/runtime",
                axum::routing::get(move || {
                    let bytes = bytes.clone();
                    async move { bytes }
                }),
            )
            .route(
                "/failed",
                axum::routing::get(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
            );
        let job = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let archive = tempfile::NamedTempFile::new().unwrap();
        download_archive(
            &client,
            &format!("{base}/runtime"),
            Some(&expected),
            &archive,
        )
        .await
        .unwrap();
        assert!(
            download_archive(
                &client,
                &format!("{base}/runtime"),
                Some("invalid"),
                &archive
            )
            .await
            .is_err()
        );
        assert!(
            download_archive(&client, &format!("{base}/failed"), None, &archive)
                .await
                .is_err()
        );
        job.abort();
    }
}
