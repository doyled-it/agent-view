//! Pluggable agent runners. Each `Runner` impl owns the per-tool concerns
//! (launch command, status detection, session-id extraction, restart command).
//! See `docs/superpowers/specs/2026-05-08-pluggable-runner-trait-design.md`.

pub mod claude;
pub mod fallback;
pub mod hook_handler;

use crate::types::Tool;

/// Result of parsing tmux pane output for tool status.
/// Runner-agnostic; `resolve_session_status` maps it onto `SessionStatus`.
#[derive(Debug, Clone, Default)]
pub struct ToolStatus {
    #[allow(dead_code)]
    pub is_active: bool,
    pub is_waiting: bool,
    pub is_compacting: bool,
    pub is_busy: bool,
    pub has_error: bool,
    pub has_exited: bool,
    pub has_idle_prompt: bool,
    pub has_question: bool,
    pub has_draft: bool,
    pub is_monitoring: bool,
}

pub trait Runner: Send + Sync {
    #[allow(dead_code)] // part of the public Runner API surface; used by tests and reserved for future runners
    fn name(&self) -> &'static str;
    fn launch_command(&self) -> &'static str;
    fn parse_status(&self, pane_content: &str) -> ToolStatus;
    fn extract_session_id(&self, pane_content: &str) -> Option<String>;
    fn restart_command(&self, original_command: &str, tool_data: &str) -> String;
}

pub fn runner_for(tool: Tool) -> &'static dyn Runner {
    match tool {
        Tool::Claude => &claude::ClaudeRunner,
        Tool::Codex => &fallback::CODEX,
        Tool::Opencode => &fallback::OPENCODE,
        Tool::Gemini => &fallback::GEMINI,
        Tool::Custom => &fallback::CUSTOM,
        Tool::Shell => &fallback::SHELL,
    }
}

/// Resolve a parsed `ToolStatus` plus the tmux pane's active flag into the
/// canonical `SessionStatus` shown in the UI. Moved verbatim from the old
/// `core::status` module.
pub fn resolve_session_status(parsed: &ToolStatus, is_active: bool) -> crate::types::SessionStatus {
    use crate::types::SessionStatus;
    if parsed.is_waiting {
        SessionStatus::Waiting
    } else if parsed.is_compacting {
        SessionStatus::Compacting
    } else if parsed.has_exited {
        SessionStatus::Idle
    } else if parsed.has_error {
        SessionStatus::Error
    } else if parsed.has_draft {
        SessionStatus::Draft
    } else if parsed.has_idle_prompt {
        if parsed.is_monitoring {
            SessionStatus::Monitoring
        } else if parsed.has_question {
            SessionStatus::Paused
        } else {
            SessionStatus::Idle
        }
    } else if parsed.is_busy || is_active {
        SessionStatus::Running
    } else if parsed.is_monitoring {
        SessionStatus::Monitoring
    } else {
        SessionStatus::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SessionStatus, Tool};

    #[test]
    fn test_runner_for_claude() {
        assert_eq!(runner_for(Tool::Claude).name(), "claude");
        assert_eq!(runner_for(Tool::Claude).launch_command(), "claude");
    }

    #[test]
    fn test_fallback_launch_commands_match_tool_command() {
        // Bit-for-bit parity check against the launch commands
        // `Tool::command()` returned on main before this refactor.
        assert_eq!(runner_for(Tool::Codex).launch_command(), "codex");
        assert_eq!(runner_for(Tool::Opencode).launch_command(), "opencode");
        assert_eq!(runner_for(Tool::Gemini).launch_command(), "gemini");
        assert_eq!(runner_for(Tool::Custom).launch_command(), "bash");
        assert_eq!(runner_for(Tool::Shell).launch_command(), "bash");
    }

    #[test]
    fn test_fallback_parse_status_returns_default() {
        for tool in [
            Tool::Codex,
            Tool::Opencode,
            Tool::Gemini,
            Tool::Custom,
            Tool::Shell,
        ] {
            let s = runner_for(tool).parse_status("ctrl+c to interrupt");
            assert!(
                !s.is_busy,
                "fallback runner should not detect Claude patterns ({:?})",
                tool
            );
            assert!(!s.has_idle_prompt);
        }
    }

    #[test]
    fn test_fallback_extract_session_id_returns_none() {
        for tool in [
            Tool::Codex,
            Tool::Opencode,
            Tool::Gemini,
            Tool::Custom,
            Tool::Shell,
        ] {
            assert_eq!(
                runner_for(tool).extract_session_id("claude --resume xyz"),
                None
            );
        }
    }

    #[test]
    fn test_fallback_restart_command_returns_original() {
        for tool in [
            Tool::Codex,
            Tool::Opencode,
            Tool::Gemini,
            Tool::Custom,
            Tool::Shell,
        ] {
            assert_eq!(
                runner_for(tool).restart_command("foo --bar", "{}"),
                "foo --bar"
            );
        }
    }

    #[test]
    fn test_resolve_monitoring_overrides_paused() {
        let parsed = ToolStatus {
            has_idle_prompt: true,
            has_question: true,
            is_monitoring: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_session_status(&parsed, false),
            SessionStatus::Monitoring
        );
    }

    #[test]
    fn test_resolve_paused_without_monitor() {
        let parsed = ToolStatus {
            has_idle_prompt: true,
            has_question: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_session_status(&parsed, false),
            SessionStatus::Paused
        );
    }

    #[test]
    fn test_resolve_draft_overrides_monitoring() {
        let parsed = ToolStatus {
            has_idle_prompt: true,
            has_draft: true,
            is_monitoring: true,
            ..Default::default()
        };
        assert_eq!(resolve_session_status(&parsed, false), SessionStatus::Draft);
    }

    #[test]
    fn test_resolve_running_overrides_monitoring() {
        let parsed = ToolStatus {
            is_busy: true,
            is_monitoring: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_session_status(&parsed, false),
            SessionStatus::Running
        );
    }
}
