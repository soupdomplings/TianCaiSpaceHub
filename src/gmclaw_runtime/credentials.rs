//! Discover the official sidecar's temporary authorization for the current OS
//! user. The authorization remains in memory and is never logged or persisted.
//! Only the packaged desktop and its direct, bundled Python child are eligible.

use std::{path::Path, time::Duration};

use anyhow::{Result, ensure};
use reqwest::header::HeaderValue;
use tokio::io::AsyncReadExt;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_DISCOVERY_BYTES: usize = 128 * 1024;

/// This fallback is deliberately restricted to the official desktop endpoint.
/// Custom services must continue to use their explicitly configured credential.
pub(super) async fn running_authorization(endpoint: &str) -> Result<Option<HeaderValue>> {
    let Ok(url) = crate::gmclaw_executor::validate_endpoint(endpoint) else {
        return Ok(None);
    };
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port_or_known_default() != Some(7861)
    {
        return Ok(None);
    }
    tokio::time::timeout(DISCOVERY_TIMEOUT, platform_authorization())
        .await
        .map_err(|_| anyhow::anyhow!("天工运行授权检查超时，请稍后重试 /tg"))?
}

/// Never attach OS output or deserialization errors to an error: they can
/// contain the credential. Dropping this future also terminates the helper.
async fn bounded_output(mut command: tokio::process::Command, limit: usize) -> Result<Vec<u8>> {
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| anyhow::anyhow!("无法检查天工运行授权"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("无法读取天工运行授权检查结果"))?;
    let mut bytes = Vec::new();
    stdout
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| anyhow::anyhow!("天工运行授权检查未完成"))?;
    ensure!(bytes.len() <= limit, "天工运行授权检查结果超出限制");
    let status = child
        .wait()
        .await
        .map_err(|_| anyhow::anyhow!("天工运行授权检查未完成"))?;
    ensure!(status.success(), "无法核验当前用户的天工运行授权");
    Ok(bytes)
}

fn same_installed_file(actual: &Path, expected: &Path) -> bool {
    actual.is_absolute()
        && expected.is_absolute()
        && actual.is_file()
        && expected.is_file()
        && match (actual.canonicalize(), expected.canonicalize()) {
            (Ok(actual), Ok(expected)) => {
                #[cfg(windows)]
                {
                    actual
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&expected.to_string_lossy())
                }
                #[cfg(not(windows))]
                {
                    actual == expected
                }
            }
            _ => false,
        }
}

fn temporary_authorization(token: &str) -> Result<HeaderValue> {
    ensure!(
        !token.is_empty()
            && token.len() <= 8192
            && token.bytes().all(|byte| byte.is_ascii_graphic()),
        "天工运行授权格式无法核验"
    );
    super::authorization(token)
}

#[cfg(windows)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowsCandidate {
    parent_id: u32,
    process_id: u32,
    parent_path: String,
    python_path: String,
    script_path: String,
    token: String,
}

#[cfg(windows)]
impl Drop for WindowsCandidate {
    fn drop(&mut self) {
        // Replace with equally many NUL bytes before discarding the DTO.
        self.token.replace_range(.., &"\0".repeat(self.token.len()));
    }
}

#[cfg(windows)]
async fn platform_authorization() -> Result<Option<HeaderValue>> {
    // Select the Windows system PowerShell, rather than an executable from PATH.
    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| anyhow::anyhow!("无法定位系统授权检查工具"))?;
    let executable = std::path::PathBuf::from(system_root)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    ensure!(executable.is_absolute(), "系统授权检查工具路径无效");
    let mut command = tokio::process::Command::new(executable);
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_DISCOVERY_SCRIPT)
        .creation_flags(0x08000000); // CREATE_NO_WINDOW
    let mut bytes = bounded_output(command, MAX_DISCOVERY_BYTES).await?;
    let parsed = serde_json::from_slice::<Vec<WindowsCandidate>>(&bytes);
    bytes.fill(0);
    let mut candidates = parsed.map_err(|_| anyhow::anyhow!("天工运行授权检查结果无法核验"))?;
    ensure!(
        candidates.len() <= 1,
        "检测到多个天工运行授权，无法确定连接目标"
    );
    let Some(candidate) = candidates.pop() else {
        return Ok(None);
    };
    ensure!(
        candidate.parent_id > 0
            && candidate.process_id > 0
            && candidate.parent_id != candidate.process_id,
        "天工运行进程身份无法核验"
    );
    let parent = Path::new(&candidate.parent_path);
    ensure!(super::valid_executable(parent), "天工运行安装无法核验");
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("天工运行安装无法核验"))?;
    ensure!(
        super::valid_executable(&canonical_parent),
        "天工运行安装无法核验"
    );
    let resources = canonical_parent
        .parent()
        .ok_or_else(|| anyhow::anyhow!("天工运行安装无法核验"))?
        .join("resources");
    ensure!(
        same_installed_file(
            Path::new(&candidate.python_path),
            &resources.join("python/python/python.exe")
        ) && same_installed_file(
            Path::new(&candidate.script_path),
            &resources.join("harness-sidecar/packaging/harness_sidecar.py")
        ),
        "天工内置运行进程无法核验"
    );
    temporary_authorization(&candidate.token).map(Some)
}

#[cfg(windows)]
const WINDOWS_DISCOVERY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$WarningPreference = 'SilentlyContinue'
$VerbosePreference = 'SilentlyContinue'
$DebugPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
try {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HubHarnessArgv {
    [DllImport("shell32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    private static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr allocation);
    public static string[] Parse(string command) {
        int count;
        IntPtr argv = CommandLineToArgvW(command, out count);
        if (argv == IntPtr.Zero) throw new InvalidOperationException();
        try {
            if (count < 1 || count > 128) throw new InvalidOperationException();
            string[] values = new string[count];
            for (int i = 0; i < count; i++) {
                values[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(argv, i * IntPtr.Size));
            }
            return values;
        } finally { LocalFree(argv); }
    }
}
'@
    $currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    function Same-User($process) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid
        return $owner.ReturnValue -eq 0 -and $owner.Sid -eq $currentSid
    }
    function Same-Path([string]$actual, [string]$expected) {
        if ([string]::IsNullOrWhiteSpace($actual) -or -not [IO.Path]::IsPathRooted($actual)) { return $false }
        return [IO.Path]::GetFullPath($actual).Equals([IO.Path]::GetFullPath($expected), [StringComparison]::OrdinalIgnoreCase)
    }
    $found = New-Object System.Collections.Generic.List[object]
    # No command lines are requested during desktop enumeration.
    $desktops = @(Get-CimInstance Win32_Process -Filter "Name = 'tiangong-desktop.exe'" -Property ProcessId, ExecutablePath, CreationDate)
    if ($desktops.Count -gt 64) { throw 'Limit' }
    foreach ($desktop in $desktops) {
        if (-not (Same-User $desktop)) { continue }
        $desktopPath = [string]$desktop.ExecutablePath
        if ([string]::IsNullOrWhiteSpace($desktopPath) -or -not [IO.Path]::IsPathRooted($desktopPath)) { continue }
        if (-not [IO.File]::Exists($desktopPath)) { continue }
        $resources = [IO.Path]::Combine([IO.Path]::GetDirectoryName($desktopPath), 'resources')
        $scriptPath = [IO.Path]::Combine($resources, 'harness-sidecar\packaging\harness_sidecar.py')
        $pythonPath = [IO.Path]::Combine($resources, 'python\python\python.exe')
        if (-not [IO.File]::Exists([IO.Path]::Combine($resources, 'app.asar')) -or -not [IO.File]::Exists($scriptPath) -or -not [IO.File]::Exists($pythonPath)) { continue }
        $desktopPid = [uint32]$desktop.ProcessId
        # Read parameters only after parent, user and bundled executable match.
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $desktopPid AND Name = 'python.exe'" -Property ProcessId, ParentProcessId, ExecutablePath, CreationDate)
        if ($children.Count -gt 64) { throw 'Limit' }
        foreach ($child in $children) {
            if (-not (Same-Path ([string]$child.ExecutablePath) $pythonPath) -or -not (Same-User $child)) { continue }
            $childPid = [uint32]$child.ProcessId
            $verified = Get-CimInstance Win32_Process -Filter "ProcessId = $childPid" -Property ProcessId, ParentProcessId, ExecutablePath, CreationDate, CommandLine
            if ($null -eq $verified -or $verified.CreationDate -ne $child.CreationDate -or [uint32]$verified.ParentProcessId -ne $desktopPid -or -not (Same-Path ([string]$verified.ExecutablePath) $pythonPath) -or -not (Same-User $verified)) { continue }
            if ([string]::IsNullOrEmpty($verified.CommandLine) -or $verified.CommandLine.Length -gt 32768) { continue }
            $argv = [HubHarnessArgv]::Parse([string]$verified.CommandLine)
            if ($argv.Count -lt 8 -or ($argv.Count % 2) -ne 0 -or -not (Same-Path $argv[0] $pythonPath) -or -not (Same-Path $argv[1] $scriptPath)) { continue }
            $flags = @{}
            $valid = $true
            for ($i = 2; $i -lt $argv.Count; $i += 2) {
                $name = $argv[$i]
                if ($name -notin @('--port', '--parent-pid', '--auth-token', '--data-dir', '--node-path', '--electron-data-url', '--init-agent-dir', '--system-skill-dir', '--workspace-skill-dir') -or $flags.ContainsKey($name)) { $valid = $false; break }
                $flags[$name] = $argv[$i + 1]
            }
            if (-not $valid -or $flags['--port'] -cne '7861' -or $flags['--parent-pid'] -cne [string]$desktopPid -or $flags['--electron-data-url'] -cne 'http://127.0.0.1:18768' -or -not (Same-Path ([string]$flags['--node-path']) $desktopPath)) { continue }
            $token = [string]$flags['--auth-token']
            if ([string]::IsNullOrEmpty($token) -or $token.Length -gt 8192 -or $token -cmatch '[^\x21-\x7e]') { continue }
            $verifiedParent = Get-CimInstance Win32_Process -Filter "ProcessId = $desktopPid" -Property ProcessId, ExecutablePath, CreationDate
            if ($null -eq $verifiedParent -or $verifiedParent.CreationDate -ne $desktop.CreationDate -or -not (Same-Path ([string]$verifiedParent.ExecutablePath) $desktopPath) -or -not (Same-User $verifiedParent)) { continue }
            $found.Add([pscustomobject]@{ parentId = $desktopPid; processId = $childPid; parentPath = $desktopPath; pythonPath = $pythonPath; scriptPath = $scriptPath; token = $token })
            if ($found.Count -gt 1) { throw 'Ambiguous' }
        }
    }
    [Console]::Out.Write((ConvertTo-Json -InputObject @($found.ToArray()) -Compress -Depth 3))
} catch {
    # Do not emit exception text, command lines, paths or tokens.
    [Console]::Error.Write('Harness authorization discovery failed')
    exit 2
}
"#;

#[cfg(target_os = "macos")]
struct ProcessMetadata {
    pid: u32,
    parent_pid: u32,
    uid: u32,
    executable_hint: String,
}

#[cfg(target_os = "macos")]
async fn platform_authorization() -> Result<Option<HeaderValue>> {
    let mut command = tokio::process::Command::new("/bin/ps");
    // Never request `command`/`args` from the process listing.
    command.args(["-axo", "pid=,ppid=,uid=,comm="]);
    let bytes = bounded_output(command, 2 * 1024 * 1024).await?;
    tokio::task::spawn_blocking(move || discover_macos_authorization(&bytes))
        .await
        .map_err(|_| anyhow::anyhow!("天工运行授权检查未完成"))?
}

#[cfg(target_os = "macos")]
fn discover_macos_authorization(bytes: &[u8]) -> Result<Option<HeaderValue>> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| anyhow::anyhow!("天工进程身份检查结果无法核验"))?;
    let processes: Vec<_> = text.lines().filter_map(process_metadata).collect();
    let current_uid = unsafe { libc::geteuid() };
    let mut found = None;
    let mut examined = 0;
    for parent in &processes {
        if parent.uid != current_uid {
            continue;
        }
        let lower = parent.executable_hint.to_ascii_lowercase();
        if !(lower.contains(".app/contents/macos/")
            && (lower.contains("tiangong") || lower.contains("gmclaw") || lower.contains("天工")))
        {
            continue;
        }
        let Some(parent_path) = macos_executable_path(parent.pid) else {
            continue;
        };
        let Some(parent_identity) = macos_process_identity(parent.pid) else {
            continue;
        };
        if parent_identity.1 != current_uid || parent_identity.2 != current_uid {
            continue;
        }
        if !super::valid_executable(&parent_path) {
            continue;
        }
        let canonical_parent = match parent_path.canonicalize() {
            Ok(path) if super::valid_executable(&path) => path,
            _ => continue,
        };
        let resources = canonical_parent
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| anyhow::anyhow!("天工运行安装无法核验"))?
            .join("Resources");
        let python = resources.join("python/python/bin/python3");
        let script = resources.join("harness-sidecar/packaging/harness_sidecar.py");
        for child in &processes {
            if child.parent_pid != parent.pid || child.uid != current_uid {
                continue;
            }
            examined += 1;
            ensure!(examined <= 64, "天工运行授权候选超出限制");
            let Some(child_path) = macos_executable_path(child.pid) else {
                continue;
            };
            if !same_installed_file(&child_path, &python) {
                continue;
            }
            let Some(child_identity) = macos_process_identity(child.pid) else {
                continue;
            };
            if child_identity.0 != parent.pid
                || child_identity.1 != current_uid
                || child_identity.2 != current_uid
            {
                continue;
            }
            let Some(args) = macos_process_arguments(child.pid) else {
                continue;
            };
            if args.len() < 8
                || !same_installed_file(Path::new(&args[0]), &python)
                || !same_installed_file(Path::new(&args[1]), &script)
            {
                continue;
            }
            let Some((token, node_path)) = official_harness_parameters(&args, parent.pid) else {
                continue;
            };
            if !same_installed_file(Path::new(node_path), &canonical_parent) {
                continue;
            };
            // Recheck identity after reading arguments; a PID may have exited.
            if macos_executable_path(parent.pid)
                .is_none_or(|path| !same_installed_file(&path, &canonical_parent))
                || macos_executable_path(child.pid)
                    .is_none_or(|path| !same_installed_file(&path, &python))
                || macos_process_identity(parent.pid) != Some(parent_identity)
                || macos_process_identity(child.pid) != Some(child_identity)
            {
                continue;
            }
            ensure!(found.is_none(), "检测到多个天工运行授权，无法确定连接目标");
            found = Some(temporary_authorization(token)?);
        }
    }
    Ok(found)
}

#[cfg(target_os = "macos")]
fn process_metadata(line: &str) -> Option<ProcessMetadata> {
    let mut rest = line.trim();
    let mut next = || {
        let end = rest.find(char::is_whitespace)?;
        let field = &rest[..end];
        rest = rest[end..].trim_start();
        field.parse::<u32>().ok()
    };
    let pid = next()?;
    let parent_pid = next()?;
    let uid = next()?;
    if pid == 0 || rest.is_empty() {
        return None;
    }
    Some(ProcessMetadata {
        pid,
        parent_pid,
        uid,
        executable_hint: rest.to_owned(),
    })
}

#[cfg(target_os = "macos")]
fn macos_executable_path(pid: u32) -> Option<std::path::PathBuf> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    let mut buffer = vec![0_u8; 4096];
    let length =
        unsafe { libc::proc_pidpath(pid as i32, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 || length as usize >= buffer.len() {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0)?;
    let value = std::str::from_utf8(&buffer[..end]).ok()?;
    let path = std::path::PathBuf::from(value);
    path.is_absolute().then_some(path)
}

#[cfg(target_os = "macos")]
fn macos_process_identity(pid: u32) -> Option<(u32, u32, u32, u64, u64)> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let copied = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as i32,
        )
    };
    if copied != size as i32 || info.pbi_pid != pid {
        return None;
    }
    Some((
        info.pbi_ppid,
        info.pbi_uid,
        info.pbi_ruid,
        info.pbi_start_tvsec,
        info.pbi_start_tvusec,
    ))
}

#[cfg(target_os = "macos")]
struct MacArguments(Vec<String>);

#[cfg(target_os = "macos")]
impl std::ops::Deref for MacArguments {
    type Target = [String];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacArguments {
    fn drop(&mut self) {
        for value in &mut self.0 {
            value.replace_range(.., &"\0".repeat(value.len()));
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_process_arguments(pid: u32) -> Option<MacArguments> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut bytes = vec![0_u8; MAX_DISCOVERY_BYTES];
    let mut length = bytes.len();
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    let parsed = if result == 0 && length <= bytes.len() {
        parse_macos_arguments(&bytes[..length])
    } else {
        None
    };
    // KERN_PROCARGS2 also includes an environment tail. Only argc arguments
    // are parsed; the unexamined tail and temporary raw buffer are discarded.
    bytes.fill(0);
    parsed.map(MacArguments)
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_arguments(bytes: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if !(1..=128).contains(&argc) {
        return None;
    }
    let mut rest = bytes.get(4..)?;
    let executable_end = rest.iter().position(|byte| *byte == 0)?;
    rest = rest.get(executable_end + 1..)?;
    // Kernel executable-path padding precedes argv[0].
    let argv_start = rest.iter().position(|byte| *byte != 0)?;
    rest = &rest[argv_start..];
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        let end = rest.iter().position(|byte| *byte == 0)?;
        args.push(std::str::from_utf8(&rest[..end]).ok()?.to_owned());
        rest = rest.get(end + 1..)?;
    }
    Some(args)
}

#[cfg(any(target_os = "macos", test))]
fn official_harness_parameters(args: &[String], expected_parent: u32) -> Option<(&str, &str)> {
    if args.len() < 8 || args.len() > 128 || args.len() % 2 != 0 {
        return None;
    }
    let mut flags = std::collections::HashMap::new();
    for pair in args[2..].chunks_exact(2) {
        if !matches!(
            pair[0].as_str(),
            "--port"
                | "--parent-pid"
                | "--auth-token"
                | "--data-dir"
                | "--node-path"
                | "--electron-data-url"
                | "--init-agent-dir"
                | "--system-skill-dir"
                | "--workspace-skill-dir"
        ) || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return None;
        }
    }
    if flags.get("--port") != Some(&"7861")
        || flags.get("--parent-pid")?.parse::<u32>().ok()? != expected_parent
        || flags.get("--electron-data-url") != Some(&"http://127.0.0.1:18768")
    {
        return None;
    }
    let token = *flags.get("--auth-token")?;
    let node_path = *flags.get("--node-path")?;
    (!token.is_empty() && token.len() <= 8192 && token.bytes().all(|byte| byte.is_ascii_graphic()))
        .then_some((token, node_path))
}

#[cfg(not(any(windows, target_os = "macos")))]
async fn platform_authorization() -> Result<Option<HeaderValue>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_args() -> Vec<String> {
        [
            "/fixture/python3",
            "/fixture/harness_sidecar.py",
            "--port",
            "7861",
            "--parent-pid",
            "321",
            "--auth-token",
            "synthetic-authorization-fixture",
            "--node-path",
            "/fixture/tiangong-desktop",
            "--electron-data-url",
            "http://127.0.0.1:18768",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn arguments_require_exact_port_parent_and_unique_known_flags() {
        let args = fixture_args();
        assert!(official_harness_parameters(&args, 321).is_some());
        assert!(official_harness_parameters(&args, 322).is_none());
        let mut duplicate = args.clone();
        duplicate.extend(["--auth-token".into(), "other-fixture".into()]);
        assert!(official_harness_parameters(&duplicate, 321).is_none());
        let mut different_port = args.clone();
        different_port[3] = "7862".into();
        assert!(official_harness_parameters(&different_port, 321).is_none());
        let mut different_data_url = args.clone();
        different_data_url[11] = "http://127.0.0.1:18769".into();
        assert!(official_harness_parameters(&different_data_url, 321).is_none());
        let mut unknown = args;
        unknown.extend(["--foreign-mode".into(), "enabled".into()]);
        assert!(official_harness_parameters(&unknown, 321).is_none());
    }

    #[test]
    fn macos_argument_parser_uses_nul_boundaries_and_ignores_environment_tail() {
        let mut args = fixture_args();
        args[0] = "/fixture path/python3".into();
        args[1] = "/fixture path/harness_sidecar.py".into();
        let mut bytes = (args.len() as i32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/fixture path/python3\0\0\0");
        for argument in &args {
            bytes.extend_from_slice(argument.as_bytes());
            bytes.push(0);
        }
        bytes.extend_from_slice(b"UNRELATED_ENV=must-not-be-an-argument\0");
        assert_eq!(parse_macos_arguments(&bytes), Some(args));
        assert!(parse_macos_arguments(&[0, 0, 0, 0]).is_none());
    }

    #[test]
    fn authorization_is_sensitive_and_rejects_control_whitespace() {
        let header = temporary_authorization("synthetic-authorization-fixture").unwrap();
        assert!(header.is_sensitive());
        assert!(!format!("{header:?}").contains("synthetic-authorization-fixture"));
        assert!(temporary_authorization("invalid\r\nfixture").is_err());
        assert!(temporary_authorization(" invalid-fixture").is_err());
    }
}
