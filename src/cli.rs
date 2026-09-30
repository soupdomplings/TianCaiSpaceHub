use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Gui,
    Import(crate::external_import::ImportLink),
    RegisterImport,
    UnregisterImport,
    Daemon,
    On,
    Off,
    Status,
    ConfigureCodexApp {
        codex_home: Option<PathBuf>,
        provider_name: Option<String>,
        provider_base_url: Option<String>,
        provider_key: Option<String>,
        model: Option<String>,
    },
    UninstallCodexApp {
        codex_home: Option<PathBuf>,
    },
}

#[derive(Debug, Clone)]
pub struct Cli {
    pub config_path: Option<PathBuf>,
    pub command: Command,
}

impl Cli {
    pub fn parse() -> anyhow::Result<Self> {
        Self::parse_args(
            std::env::args()
                .skip(1)
                .filter(|arg| {
                    // Legacy Finder launches may include a process serial number.
                    !cfg!(target_os = "macos") || !arg.starts_with("-psn_")
                })
                .collect(),
        )
    }

    fn parse_args(args: Vec<String>) -> anyhow::Result<Self> {
        let mut config_path = None;
        let mut remaining = Vec::new();
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--config" | "-c" => {
                    let Some(path) = args.get(index + 1) else {
                        anyhow::bail!("{} requires a path", args[index]);
                    };
                    config_path = Some(PathBuf::from(path));
                    index += 2;
                }
                _ => {
                    remaining.extend_from_slice(&args[index..]);
                    break;
                }
            }
        }

        let command = match remaining.first().map(String::as_str) {
            None => default_command(),
            Some("gui") => Command::Gui,
            Some("import-url") => {
                if remaining.len() != 2 {
                    anyhow::bail!("import-url requires one import link");
                }
                Command::Import(
                    crate::external_import::ImportLink::parse(&remaining[1])
                        .map_err(anyhow::Error::msg)?,
                )
            }
            Some("register-web-import") => Command::RegisterImport,
            Some("unregister-web-import") => Command::UnregisterImport,
            Some(link) if link.contains("://") => {
                if remaining.len() != 1 {
                    anyhow::bail!("invalid import arguments");
                }
                Command::Import(
                    crate::external_import::ImportLink::parse(link).map_err(anyhow::Error::msg)?,
                )
            }
            Some("daemon") | Some("run") => Command::Daemon,
            Some("on") => Command::On,
            Some("off") => Command::Off,
            Some("status") => Command::Status,
            Some("configure-codex-app") => parse_configure_codex_app(&remaining[1..])?,
            Some("uninstall-codex-app") => parse_uninstall_codex_app(&remaining[1..])?,
            Some("install-shim") | Some("uninstall-shim") | Some("shim") => anyhow::bail!(
                "CLI shim support has been removed. Use `codexhub configure-codex-app` and Codex App remote-control instead."
            ),
            Some("-h") | Some("--help") | Some("help") => {
                print_help();
                std::process::exit(0);
            }
            Some(_) => anyhow::bail!("unknown command. Run `codexhub help`."),
        };

        Ok(Self {
            config_path,
            command,
        })
    }
}

fn default_command() -> Command {
    if cfg!(feature = "gui") {
        Command::Gui
    } else {
        Command::Daemon
    }
}

fn parse_configure_codex_app(args: &[String]) -> anyhow::Result<Command> {
    let mut codex_home = None;
    let mut provider_name = None;
    let mut provider_base_url = None;
    let mut provider_key = None;
    let mut model = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--codex-home" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--codex-home requires a path");
                };
                codex_home = Some(PathBuf::from(value));
            }
            "--provider-name" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--provider-name requires a name");
                };
                provider_name = Some(value.to_string());
            }
            "--provider-base-url" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--provider-base-url requires a URL");
                };
                provider_base_url = Some(value.to_string());
            }
            "--provider-key" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--provider-key requires a token");
                };
                provider_key = Some(value.to_string());
            }
            "--model" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--model requires a model name");
                };
                model = Some(value.to_string());
            }
            other => anyhow::bail!("unknown configure-codex-app argument `{other}`"),
        }
    }
    Ok(Command::ConfigureCodexApp {
        codex_home,
        provider_name,
        provider_base_url,
        provider_key,
        model,
    })
}

fn parse_uninstall_codex_app(args: &[String]) -> anyhow::Result<Command> {
    let mut codex_home = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--codex-home" => {
                let Some(value) = iter.next() else {
                    anyhow::bail!("--codex-home requires a path");
                };
                codex_home = Some(PathBuf::from(value));
            }
            other => anyhow::bail!("unknown uninstall-codex-app argument `{other}`"),
        }
    }
    Ok(Command::UninstallCodexApp { codex_home })
}

pub fn print_help() {
    println!(
        r#"codexhub

Usage:
  codexhub [--config PATH] gui
  codexhub import-url "tiancaispacehub://import/v1?..."
  codexhub register-web-import
  codexhub unregister-web-import
  codexhub [--config PATH] daemon
  codexhub [--config PATH] on
  codexhub [--config PATH] off
  codexhub [--config PATH] status
  codexhub [--config PATH] configure-codex-app [--codex-home PATH] [--provider-name NAME] [--provider-base-url URL] [--provider-key TOKEN] [--model MODEL]
  codexhub [--config PATH] uninstall-codex-app [--codex-home PATH]

Default command is gui when built with the gui feature, otherwise daemon.
"#
    );
}

#[cfg(test)]
mod import_tests {
    use super::*;
    #[test]
    fn external_import_entrypoints_preserve_link_and_redact_diagnostics() {
        let link = format!(
            "tiancaispacehub://import/v1?origin=https%3A%2F%2Ftiancai.yc99.space&ticket={}",
            "x".repeat(43)
        );
        for args in [vec![link.clone()], vec!["import-url".into(), link.clone()]] {
            let cli = Cli::parse_args(args).unwrap();
            assert!(matches!(&cli.command, Command::Import(_)));
            assert!(!format!("{cli:?}").contains(&"x".repeat(43)));
        }
        for args in [
            vec!["import-url".into()],
            vec!["import-url".into(), link.clone(), "extra".into()],
            vec![format!("{link}#fragment")],
        ] {
            let error = Cli::parse_args(args).unwrap_err().to_string();
            assert!(!error.contains(&"x".repeat(43)));
        }
    }
}
