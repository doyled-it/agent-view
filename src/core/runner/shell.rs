//! Plain shell runner. Drops the user into their tmux default-shell (which
//! itself defaults to `$SHELL`), with no agent-specific status detection,
//! no session-id extraction, and no hooks. The simplest possible Runner
//! impl — used as the inaugural non-Claude runner per issue #46.
//!
//! `launch_command` returns `None` so no command is sent into the pane;
//! tmux's own default-shell takes over. Sending `bash` here would nest a
//! new shell on top of the user's existing one (zsh on macOS), which is
//! both wasteful and visually noisy.

use super::{Runner, ToolStatus};

pub struct ShellRunner;

impl Runner for ShellRunner {
    fn name(&self) -> &'static str {
        "shell"
    }
    fn launch_command(&self) -> Option<&'static str> {
        None
    }
    fn parse_status(&self, _pane_content: &str) -> ToolStatus {
        ToolStatus::default()
    }
    fn extract_session_id(&self, _pane_content: &str) -> Option<String> {
        None
    }
    fn restart_command(&self, original_command: &str, _tool_data: &str) -> String {
        original_command.to_string()
    }
    // is_implemented defaults to true; install_hooks defaults to no-op.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_and_launch_command() {
        let r = ShellRunner;
        assert_eq!(r.name(), "shell");
        // None means "let tmux's default-shell run" — no send-keys.
        assert_eq!(r.launch_command(), None);
    }

    #[test]
    fn test_parse_status_returns_default() {
        let s = ShellRunner.parse_status("ctrl+c to interrupt\n\u{276f} \n");
        assert!(!s.is_busy);
        assert!(!s.is_waiting);
        assert!(!s.is_compacting);
        assert!(!s.has_error);
        assert!(!s.has_exited);
        assert!(!s.has_idle_prompt);
        assert!(!s.has_question);
        assert!(!s.has_draft);
        assert!(!s.is_monitoring);
    }

    #[test]
    fn test_extract_session_id_returns_none() {
        assert_eq!(
            ShellRunner.extract_session_id("anything claude --resume xyz"),
            None
        );
    }

    #[test]
    fn test_restart_command_passes_through() {
        assert_eq!(ShellRunner.restart_command("foo --bar", "{}"), "foo --bar");
    }

    #[test]
    fn test_is_implemented_returns_true() {
        assert!(ShellRunner.is_implemented());
    }
}
