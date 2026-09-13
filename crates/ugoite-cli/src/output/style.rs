//! Stream-aware ANSI styling for human-facing CLI output.

use std::fmt::Display;
use std::io::IsTerminal;

/// The small set of semantic roles used by the CLI's human presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Primary,
    Heading,
    Muted,
    Success,
    Warning,
    Error,
}

/// Presentation policy for one output stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StylePolicy {
    enabled: bool,
}

/// Determine whether ANSI styling is allowed for a stream.
fn should_style(is_terminal: bool, no_color: bool, term: Option<&str>) -> bool {
    is_terminal && !no_color && term != Some("dumb")
}

/// Build the policy for human-facing stdout.
pub fn stdout_style() -> StylePolicy {
    policy_for(std::io::stdout().is_terminal())
}

/// Build the policy for human-facing stderr.
pub fn stderr_style() -> StylePolicy {
    policy_for(std::io::stderr().is_terminal())
}

impl StylePolicy {
    /// Apply a semantic role, or return the display value unchanged when ANSI
    /// styling is disabled for this stream.
    pub fn paint(&self, role: Role, text: impl Display) -> String {
        let text = text.to_string();
        if !self.enabled {
            return text;
        }

        let style = match role {
            Role::Primary => anstyle::AnsiColor::Cyan.on_default(),
            Role::Heading => anstyle::Style::new().bold(),
            Role::Muted => anstyle::Style::new().dimmed(),
            Role::Success => anstyle::AnsiColor::Green.on_default(),
            Role::Warning => anstyle::AnsiColor::Yellow.on_default(),
            Role::Error => anstyle::AnsiColor::Red.on_default().bold(),
        };
        format!("{}{text}{}", style.render(), style.render_reset())
    }
}

fn policy_for(is_terminal: bool) -> StylePolicy {
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let term = std::env::var("TERM").ok();
    StylePolicy {
        enabled: should_style(is_terminal, no_color, term.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_activation_requires_a_human_terminal() {
        assert!(should_style(true, false, Some("xterm-256color")));
        assert!(should_style(true, false, None));
        assert!(!should_style(false, false, Some("xterm")));
        assert!(!should_style(true, true, Some("xterm")));
        assert!(!should_style(true, false, Some("dumb")));
    }

    #[test]
    fn disabled_policy_preserves_text() {
        let policy = StylePolicy { enabled: false };
        assert_eq!(policy.paint(Role::Primary, "note-1"), "note-1");
    }

    #[test]
    fn roles_use_only_foreground_or_text_effects() {
        let policy = StylePolicy { enabled: true };
        assert_eq!(
            policy.paint(Role::Primary, "note-1"),
            "\u{1b}[36mnote-1\u{1b}[0m"
        );
        assert_eq!(
            policy.paint(Role::Heading, "Usage"),
            "\u{1b}[1mUsage\u{1b}[0m"
        );
        assert_eq!(policy.paint(Role::Muted, "ID"), "\u{1b}[2mID\u{1b}[0m");
        assert_eq!(policy.paint(Role::Success, "ok"), "\u{1b}[32mok\u{1b}[0m");
        assert_eq!(
            policy.paint(Role::Warning, "Warning"),
            "\u{1b}[33mWarning\u{1b}[0m"
        );
        assert_eq!(
            policy.paint(Role::Error, "Error:"),
            "\u{1b}[1m\u{1b}[31mError:\u{1b}[0m"
        );
    }
}
