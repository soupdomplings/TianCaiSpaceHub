//! Windows desktop discovery; reads installation metadata, never executes it.
//! Harness registration rules follow Magpie eb56de0b (yetone, MIT; notices in
//! THIRD_PARTY_NOTICES.md), including custom folders and both registry views.
use crate::desktop::clients::ClientId;
use std::path::{Path, PathBuf};
use winreg::{RegKey, enums::*};

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const HARNESS_ID: &str = "1bf39983-50d0-5fe0-9ef4-cece76f67c5e";

pub(super) fn program_names(client: ClientId) -> &'static [&'static str] {
    match client {
        ClientId::VsCode => &["Code.exe"],
        ClientId::DeepSeekHarness => &["DeepSeek Harness.exe", "deepseek-harness.exe", "dsh.exe"],
        ClientId::ZCode => &["ZCode.exe"],
        ClientId::WebStorm => &["webstorm64.exe", "webstorm.exe"],
        _ => &[],
    }
}

pub(super) fn resolve(client: ClientId, path: &Path) -> Option<PathBuf> {
    if client == ClientId::VsCode {
        // Reuse configuration discovery's product, version and Scoop rules;
        // open the native app without executing a batch file or a shell.
        return crate::desktop::targets::vscode_program(path);
    }
    (path.is_file()
        && path
            .extension()
            .is_some_and(|n| n.eq_ignore_ascii_case("exe")))
    .then(|| path.to_path_buf())
}

fn registry_exe(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let value = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else {
        let end = value.to_ascii_lowercase().rfind(".exe")? + 4;
        &value[..end]
    };
    let path = PathBuf::from(value);
    (path.is_absolute()
        && path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe")))
    .then_some(path)
}

fn in_directory(client: ClientId, directory: &Path) -> Option<PathBuf> {
    program_names(client)
        .iter()
        .flat_map(|name| [directory.join(name), directory.join("bin").join(name)])
        .find_map(|path| resolve(client, &path))
}

fn registered_program(
    client: ClientId,
    location: &str,
    icon: &str,
    uninstall: &str,
) -> Option<PathBuf> {
    let location = PathBuf::from(location.trim().trim_matches('"'));
    if location.is_absolute() {
        if let Some(path) = in_directory(client, &location) {
            return Some(path);
        }
    }
    for value in [icon, uninstall] {
        if let Some(path) = registry_exe(value) {
            // An uninstaller is only a source of its directory. It is never
            // a launch candidate, even when the registry record is valid.
            if let Some(path) = path.parent().and_then(|p| in_directory(client, p)) {
                return Some(path);
            }
        }
    }
    None
}

fn product_matches(client: ClientId, display_name: &str) -> bool {
    let name = display_name.trim().to_ascii_lowercase();
    match client {
        ClientId::DeepSeekHarness => name == "deepseek harness",
        ClientId::VsCode => matches!(
            name.as_str(),
            "microsoft visual studio code"
                | "microsoft visual studio code (user)"
                | "microsoft visual studio code (system)"
        ),
        ClientId::ZCode => {
            name == "zcode"
                || name
                    .strip_prefix("zcode ")
                    .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()))
        }
        ClientId::WebStorm => name
            .strip_prefix("jetbrains ")
            .unwrap_or(&name)
            .strip_prefix("webstorm")
            .is_some_and(|s| {
                s.trim().is_empty() || s.trim_start().starts_with(|c: char| c.is_ascii_digit())
            }),
        _ => false,
    }
}

fn value(key: &RegKey, name: &str) -> String {
    key.get_value(name).unwrap_or_default()
}

pub(super) fn locate(client: ClientId) -> Option<PathBuf> {
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            for name in program_names(client) {
                if let Ok(key) = root.open_subkey_with_flags(
                    format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{name}"),
                    KEY_READ | view,
                ) {
                    if let Some(path) =
                        registry_exe(&value(&key, "")).and_then(|p| resolve(client, &p))
                    {
                        return Some(path);
                    }
                }
            }
            if client == ClientId::DeepSeekHarness {
                for path in [
                    format!(r"Software\{HARNESS_ID}"),
                    format!(r"{UNINSTALL}\{HARNESS_ID}"),
                ] {
                    if let Ok(key) = root.open_subkey_with_flags(path, KEY_READ | view) {
                        if let Some(path) = registered_program(
                            client,
                            &value(&key, "InstallLocation"),
                            &value(&key, "DisplayIcon"),
                            &value(&key, "UninstallString"),
                        ) {
                            return Some(path);
                        }
                    }
                }
            }
            if let Ok(entries) = root.open_subkey_with_flags(UNINSTALL, KEY_READ | view) {
                for name in entries.enum_keys().take(1024).flatten() {
                    let Ok(key) = entries.open_subkey_with_flags(&name, KEY_READ | view) else {
                        continue;
                    };
                    if !product_matches(client, &value(&key, "DisplayName"))
                        && !(client == ClientId::DeepSeekHarness && name == HARNESS_ID)
                    {
                        continue;
                    }
                    if let Some(path) = registered_program(
                        client,
                        &value(&key, "InstallLocation"),
                        &value(&key, "DisplayIcon"),
                        &value(&key, "UninstallString"),
                    ) {
                        return Some(path);
                    }
                }
            }
        }
    }
    let folders: &[&str] = match client {
        ClientId::VsCode => &["Microsoft VS Code"],
        ClientId::DeepSeekHarness => &["DeepSeek Harness", "deepseek"],
        ClientId::ZCode => &["ZCode"],
        ClientId::WebStorm => &["WebStorm", "JetBrains/WebStorm"],
        _ => &[],
    };
    let roots = std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Programs"))
        .into_iter()
        .chain(
            ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from),
        );
    for root in roots {
        for folder in folders {
            if let Some(path) = in_directory(client, &root.join(folder)) {
                return Some(path);
            }
        }
        if client == ClientId::WebStorm {
            if let Ok(entries) = std::fs::read_dir(root.join("JetBrains")) {
                let mut directories = entries
                    .take(32)
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|s| s.to_string_lossy().starts_with("WebStorm"))
                    })
                    .collect::<Vec<_>>();
                directories.sort();
                for dir in directories.into_iter().rev() {
                    if let Some(path) = in_directory(client, &dir) {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        // Discovery canonicalizes program paths; normalize the temp parent
        // too, since Windows TEMP can contain an ADMINI~1 short-name alias.
        let parent = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let text = parent.to_string_lossy();
        let parent = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{unc}"))
        } else {
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
        };
        tempfile::tempdir_in(parent).unwrap()
    }

    #[test]
    fn custom_install_location_icon_and_uninstall_metadata_find_the_desktop_app() {
        let temp = fixture();
        let exe = temp.path().join("DeepSeek Harness.exe");
        std::fs::write(&exe, b"synthetic; never executed").unwrap();
        for (location, icon, uninstall) in [
            (
                format!("\"{}\"", temp.path().display()),
                String::new(),
                String::new(),
            ),
            (String::new(), format!("{},0", exe.display()), String::new()),
            (
                String::new(),
                String::new(),
                format!(
                    "\"{}\" /currentuser",
                    temp.path().join("Uninstall DeepSeek Harness.exe").display()
                ),
            ),
        ] {
            assert_eq!(
                registered_program(ClientId::DeepSeekHarness, &location, &icon, &uninstall),
                Some(exe.clone())
            );
        }
        std::fs::remove_file(&exe).unwrap();
        std::fs::write(
            temp.path().join("Uninstall DeepSeek Harness.exe"),
            b"never execute",
        )
        .unwrap();
        assert_eq!(
            registered_program(
                ClientId::DeepSeekHarness,
                &temp.path().to_string_lossy(),
                "",
                ""
            ),
            None
        );
    }
    #[test]
    fn other_clients_use_their_own_executable_and_webstorm_bin_layout() {
        let temp = fixture();
        for (client, relative) in [
            (ClientId::VsCode, "Code.exe"),
            (ClientId::ZCode, "ZCode.exe"),
            (ClientId::WebStorm, "bin/webstorm64.exe"),
        ] {
            let exe = temp.path().join(relative);
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"synthetic; never executed").unwrap();
            assert_eq!(
                registered_program(client, &temp.path().to_string_lossy(), "", ""),
                Some(exe)
            );
        }
        assert!(!product_matches(
            ClientId::VsCode,
            "Microsoft Visual Studio Code Insiders"
        ));
        assert!(!product_matches(ClientId::WebStorm, "WebStorm Helper"));
        assert!(product_matches(
            ClientId::WebStorm,
            "JetBrains WebStorm 2026.2.3"
        ));
        assert!(!product_matches(ClientId::DeepSeekHarness, "DS Bot"));
    }
    #[test]
    fn vscode_path_batch_launcher_resolves_native_app_without_running_the_script() {
        let temp = fixture();
        let script = temp.path().join("bin/code.cmd");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, b"this must never be run").unwrap();
        assert_eq!(resolve(ClientId::VsCode, &script), None);
        let exe = temp.path().join("Code.exe");
        std::fs::write(&exe, b"synthetic").unwrap();
        assert_eq!(resolve(ClientId::VsCode, &script), Some(exe));
        assert_eq!(resolve(ClientId::DeepSeekHarness, &script), None);
    }
    #[test]
    fn vscode_product_version_and_scoop_rules_are_shared_with_configuration_discovery() {
        let temp = fixture();
        let install = temp.path().join("VS Code Portable");
        let script = install.join("bin/code.cmd");
        let app = install.join("1.106.0/resources/app");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(&script, b"must never execute").unwrap();
        let exe = install.join("1.106.0/Code.exe");
        std::fs::write(&exe, b"synthetic").unwrap();
        let product = app.join("product.json");
        std::fs::write(&product, br#"{"applicationName":"code"}"#).unwrap();
        assert_eq!(resolve(ClientId::VsCode, &script), Some(exe.clone()));
        let shim = temp.path().join("code.exe");
        std::fs::write(&shim, b"must never execute").unwrap();
        std::fs::write(
            shim.with_extension("shim"),
            format!("path = \"{}\"\n", script.display()),
        )
        .unwrap();
        assert_eq!(resolve(ClientId::VsCode, &shim), Some(exe));
        std::fs::write(product, br#"{"applicationName":"cursor"}"#).unwrap();
        assert_eq!(resolve(ClientId::VsCode, &script), None);
        assert_eq!(resolve(ClientId::VsCode, &shim), None);
        std::fs::remove_file(&script).unwrap();
        assert_eq!(resolve(ClientId::VsCode, &script), None);
    }
}
