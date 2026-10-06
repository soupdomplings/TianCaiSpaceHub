//! Pure command parsing shared by IM executor routing and session menus.
//!
//! Parsing never changes the selected executor. Callers must still check the
//! sender, active executor, menu ownership, and in-flight work before acting.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ExecutorTarget {
    Codex,
    GmClaw,
    WorkBuddy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetCommand<'a> {
    New,
    Status,
    Help,
    Off,
    Approve(&'a str),
    Reject(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuCommand {
    /// One-based index into the currently displayed menu, never a session ID.
    Select(usize),
    Next,
    Prev,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutorCommand<'a> {
    Switch(ExecutorTarget),
    Target {
        target: ExecutorTarget,
        command: TargetCommand<'a>,
    },
    Interrupt,
    Quit,
    Menu(MenuCommand),
    /// A known command with unsupported or extra arguments. Do not execute it
    /// as its argument-free form or forward it as a model prompt.
    InvalidArguments {
        command: &'a str,
        arguments: &'a str,
    },
    UnknownSlash {
        command: &'a str,
        arguments: &'a str,
    },
    Text(&'a str),
}

/// `menu_active` must reflect a menu belonging to this sender and executor.
/// Bare numeric replies are text unless such a menu is active. Slash numeric
/// replies stay explicit menu commands so callers can resolve approval/menu
/// precedence without accidentally forwarding them to an executor.
pub(crate) fn parse_command(text: &str, menu_active: bool) -> ExecutorCommand<'_> {
    let text = text.trim();
    let (command, arguments) = split_command(text);
    let invalid = || ExecutorCommand::InvalidArguments { command, arguments };

    if !command.starts_with('/') {
        return if menu_active && arguments.is_empty() && ascii_digits(command) {
            menu_index(command).map_or_else(invalid, ExecutorCommand::Menu)
        } else {
            ExecutorCommand::Text(text)
        };
    }

    if command.eq_ignore_ascii_case("/tg") || command.eq_ignore_ascii_case("/gmclaw") {
        return if arguments.is_empty() {
            ExecutorCommand::Switch(ExecutorTarget::GmClaw)
        } else {
            parse_target_command(arguments).map_or_else(invalid, |command| {
                ExecutorCommand::Target {
                    target: ExecutorTarget::GmClaw,
                    command,
                }
            })
        };
    }

    let parsed = if command.eq_ignore_ascii_case("/gpt") {
        ExecutorCommand::Switch(ExecutorTarget::Codex)
    } else if command.eq_ignore_ascii_case("/wb") {
        ExecutorCommand::Switch(ExecutorTarget::WorkBuddy)
    } else if command.eq_ignore_ascii_case("/s") {
        ExecutorCommand::Interrupt
    } else if command.eq_ignore_ascii_case("/q") {
        ExecutorCommand::Quit
    } else if command.eq_ignore_ascii_case("/next") {
        ExecutorCommand::Menu(MenuCommand::Next)
    } else if command.eq_ignore_ascii_case("/prev") {
        ExecutorCommand::Menu(MenuCommand::Prev)
    } else if command.eq_ignore_ascii_case("/back") {
        ExecutorCommand::Menu(MenuCommand::Back)
    } else if let Some(index) = command.strip_prefix('/')
        && ascii_digits(index)
    {
        match menu_index(index) {
            Some(index) => ExecutorCommand::Menu(index),
            None => return invalid(),
        }
    } else {
        return ExecutorCommand::UnknownSlash { command, arguments };
    };

    if arguments.is_empty() {
        parsed
    } else {
        invalid()
    }
}

fn split_command(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map(|(command, arguments)| (command, arguments.trim()))
        .unwrap_or((text, ""))
}

fn parse_target_command(arguments: &str) -> Option<TargetCommand<'_>> {
    let (command, arguments) = split_command(arguments);
    if command.eq_ignore_ascii_case("approve") || command.eq_ignore_ascii_case("reject") {
        if arguments.is_empty() || arguments.chars().any(char::is_whitespace) {
            return None;
        }
        return Some(if command.eq_ignore_ascii_case("approve") {
            TargetCommand::Approve(arguments)
        } else {
            TargetCommand::Reject(arguments)
        });
    }
    if !arguments.is_empty() {
        return None;
    }
    if command.eq_ignore_ascii_case("new") {
        Some(TargetCommand::New)
    } else if command.eq_ignore_ascii_case("status") {
        Some(TargetCommand::Status)
    } else if command.eq_ignore_ascii_case("help") {
        Some(TargetCommand::Help)
    } else if command.eq_ignore_ascii_case("off") {
        Some(TargetCommand::Off)
    } else {
        None
    }
}

fn ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn menu_index(text: &str) -> Option<MenuCommand> {
    text.parse::<usize>()
        .ok()
        .filter(|index| *index > 0)
        .map(MenuCommand::Select)
}

#[cfg(test)]
mod tests {
    use super::{ExecutorCommand, ExecutorTarget, MenuCommand, TargetCommand, parse_command};

    #[test]
    fn switch_requires_a_complete_command_without_unrecognized_arguments() {
        assert_eq!(
            parse_command("  /GPT\n", false),
            ExecutorCommand::Switch(ExecutorTarget::Codex)
        );
        assert_eq!(
            parse_command("/wb", false),
            ExecutorCommand::Switch(ExecutorTarget::WorkBuddy)
        );
        for input in ["/tgfoo", "/gpt-other", "/wb123", "/gmclawnew"] {
            assert!(matches!(
                parse_command(input, false),
                ExecutorCommand::UnknownSlash { .. }
            ));
        }
        for input in ["/gpt something", "/wb new", "/tg unknown", "/tg new extra"] {
            assert!(matches!(
                parse_command(input, false),
                ExecutorCommand::InvalidArguments { .. }
            ));
        }
    }

    #[test]
    fn legacy_and_short_tiangong_commands_share_explicit_subcommands() {
        for prefix in ["/tg", "/gmclaw"] {
            assert_eq!(
                parse_command(prefix, false),
                ExecutorCommand::Switch(ExecutorTarget::GmClaw)
            );
            assert_eq!(
                parse_command(&format!("{prefix}\tAPPROVE AbC-123"), false),
                ExecutorCommand::Target {
                    target: ExecutorTarget::GmClaw,
                    command: TargetCommand::Approve("AbC-123"),
                }
            );
            for suffix in ["approve", "reject one two", "off extra"] {
                assert!(matches!(
                    parse_command(&format!("{prefix} {suffix}"), false),
                    ExecutorCommand::InvalidArguments { .. }
                ));
            }
        }
    }

    #[test]
    fn bare_numbers_are_model_text_without_an_owned_menu() {
        assert_eq!(parse_command(" 2 ", false), ExecutorCommand::Text("2"));
        assert_eq!(
            parse_command(" 2 ", true),
            ExecutorCommand::Menu(MenuCommand::Select(2))
        );
        assert_eq!(
            parse_command("/2", false),
            ExecutorCommand::Menu(MenuCommand::Select(2))
        );
        assert_eq!(
            parse_command("2 items", true),
            ExecutorCommand::Text("2 items")
        );
        assert_eq!(parse_command("１２", true), ExecutorCommand::Text("１２"));
        for input in ["0", "9999999999999999999999999999999999999999999"] {
            assert!(matches!(
                parse_command(input, true),
                ExecutorCommand::InvalidArguments { .. }
            ));
        }
    }

    #[test]
    fn control_commands_reject_trailing_prompts_and_keep_unknown_slash_distinct() {
        assert_eq!(parse_command("/s", false), ExecutorCommand::Interrupt);
        assert_eq!(parse_command("/q", false), ExecutorCommand::Quit);
        for input in ["/s task", "/q please", "/1 extra", "/next now"] {
            assert!(matches!(
                parse_command(input, true),
                ExecutorCommand::InvalidArguments { .. }
            ));
        }
        assert_eq!(
            parse_command("/custom   keep This", false),
            ExecutorCommand::UnknownSlash {
                command: "/custom",
                arguments: "keep This",
            }
        );
        assert_eq!(
            parse_command("hello /tg", false),
            ExecutorCommand::Text("hello /tg")
        );
    }
}
