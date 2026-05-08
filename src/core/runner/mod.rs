//! Pluggable agent runners. Each `Runner` impl owns the per-tool concerns
//! (launch command, status detection, session-id extraction, restart command).
//! See `docs/superpowers/specs/2026-05-08-pluggable-runner-trait-design.md`.

#![allow(dead_code)]

pub mod claude;
pub mod fallback;

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
