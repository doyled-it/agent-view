//! Pluggable agent runners. Each `Runner` impl owns the per-tool concerns
//! (launch command, status detection, session-id extraction, restart command).
//! See `docs/superpowers/specs/2026-05-08-pluggable-runner-trait-design.md`.

pub mod claude;
pub mod event_watcher;
pub mod fallback;
pub mod hook_io;
pub mod osc_title;
pub mod shell;

use crate::core::runner::event_watcher::HookStatus;
use crate::types::Tool;
use std::time::{Duration, SystemTime};

const HOOK_FRESHNESS: Duration = Duration::from_millis(1100);

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
    /// Command to run inside the freshly-created tmux pane. `None` means
    /// "no command — let tmux's default-shell take over". Used by `ShellRunner`
    /// so opening a Shell session drops you into your login shell directly,
    /// rather than spawning a second shell on top of it.
    fn launch_command(&self) -> Option<&'static str>;
    fn parse_status(&self, pane_content: &str) -> ToolStatus;
    fn extract_session_id(&self, pane_content: &str) -> Option<String>;
    fn restart_command(&self, original_command: &str, tool_data: &str) -> String;

    /// True for runners with a real per-tool impl. False for FallbackRunner
    /// stubs so the new-session picker can hide tools that aren't yet wired
    /// up. Default is true so a new real runner needs no boilerplate.
    fn is_implemented(&self) -> bool {
        true
    }

    /// Install per-tool status-detection hooks into the tool's user config.
    /// Idempotent. Default impl is a no-op for runners without hook support.
    fn install_hooks(&self) -> Result<(), String> {
        Ok(())
    }
}

pub fn runner_for(tool: Tool) -> &'static dyn Runner {
    match tool {
        Tool::Claude => &claude::ClaudeRunner,
        Tool::Codex => &fallback::CODEX,
        Tool::Opencode => &fallback::OPENCODE,
        Tool::Gemini => &fallback::GEMINI,
        Tool::Custom => &fallback::CUSTOM,
        Tool::Shell => &shell::ShellRunner,
    }
}

/// Tools backed by a real `Runner` impl, in `Tool::ALL` order. Drives the
/// new-session overlay's runner picker — adding a new real runner makes
/// it appear here automatically (no picker code change required).
pub fn implemented_tools() -> Vec<Tool> {
    Tool::ALL
        .iter()
        .copied()
        .filter(|t| runner_for(*t).is_implemented())
        .collect()
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

/// Three-tier status composition.
/// - Tier 1a: fresh hook with Running/Waiting/Compacting → use directly.
/// - Tier 1b: fresh hook with Idle → run regex but only let it produce
///   Draft / Paused / Monitoring overlays; otherwise return Idle.
/// - Tier 2: no fresh hook; pane title matches known marker → use it.
/// - Tier 3: regex + resolve_session_status (current behavior).
pub fn compose_status(
    hook: Option<&HookStatus>,
    pane_title_status: Option<crate::types::SessionStatus>,
    pane_content: &str,
    runner: &dyn Runner,
    is_active: bool,
    now: SystemTime,
) -> crate::types::SessionStatus {
    use crate::types::SessionStatus;

    let fresh_hook = hook.filter(|h| {
        now.duration_since(h.received_at)
            .map(|d| d <= HOOK_FRESHNESS)
            .unwrap_or(false)
    });

    if let Some(h) = fresh_hook {
        match h.status {
            SessionStatus::Running | SessionStatus::Waiting | SessionStatus::Compacting => {
                return h.status;
            }
            SessionStatus::Idle => {
                let parsed = runner.parse_status(pane_content);
                if parsed.has_draft {
                    return SessionStatus::Draft;
                }
                if parsed.is_monitoring && parsed.has_idle_prompt {
                    return SessionStatus::Monitoring;
                }
                if parsed.has_idle_prompt && parsed.has_question {
                    return SessionStatus::Paused;
                }
                return SessionStatus::Idle;
            }
            // Hook handler currently never emits Crashed / Stopped / Draft /
            // Paused / Monitoring / Error (Crashed comes from the
            // !session_exists path in the poller; the others are derived from
            // pane parsing). If any of these ever leak through (malformed
            // payload, future schema change), fall through to the title /
            // regex tiers rather than trusting an unexpected hook status.
            _ => {}
        }
    }

    if let Some(s) = pane_title_status {
        return s;
    }

    let parsed = runner.parse_status(pane_content);
    resolve_session_status(&parsed, is_active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SessionStatus, Tool};

    #[test]
    fn test_runner_for_claude() {
        assert_eq!(runner_for(Tool::Claude).name(), "claude");
        assert_eq!(runner_for(Tool::Claude).launch_command(), Some("claude"));
    }

    #[test]
    fn test_launch_commands_per_tool() {
        assert_eq!(runner_for(Tool::Codex).launch_command(), Some("codex"));
        assert_eq!(
            runner_for(Tool::Opencode).launch_command(),
            Some("opencode")
        );
        assert_eq!(runner_for(Tool::Gemini).launch_command(), Some("gemini"));
        assert_eq!(runner_for(Tool::Custom).launch_command(), Some("bash"));
        // Shell defers to tmux's default-shell.
        assert_eq!(runner_for(Tool::Shell).launch_command(), None);
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

    use crate::core::runner::event_watcher::HookStatus;
    use std::time::{Duration, SystemTime};

    fn fresh_hook(status: SessionStatus) -> HookStatus {
        HookStatus {
            status,
            claude_session_id: None,
            event: "test".to_string(),
            received_at: SystemTime::now(),
        }
    }

    fn stale_hook(status: SessionStatus) -> HookStatus {
        HookStatus {
            status,
            claude_session_id: None,
            event: "test".to_string(),
            received_at: SystemTime::now() - Duration::from_secs(10),
        }
    }

    #[test]
    fn test_compose_fresh_running_hook_overrides_regex() {
        let s = compose_status(
            Some(&fresh_hook(SessionStatus::Running)),
            None,
            "\u{276f} \n",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Running);
    }

    #[test]
    fn test_compose_fresh_idle_hook_with_draft_overlay() {
        let s = compose_status(
            Some(&fresh_hook(SessionStatus::Idle)),
            None,
            "\u{276f} fix the bug in\n",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Draft);
    }

    #[test]
    fn test_compose_fresh_idle_hook_with_paused_overlay() {
        let s = compose_status(
            Some(&fresh_hook(SessionStatus::Idle)),
            None,
            "What file should I edit?\n\u{276f} \n",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Paused);
    }

    #[test]
    fn test_compose_fresh_idle_hook_no_overlay_returns_idle() {
        let s = compose_status(
            Some(&fresh_hook(SessionStatus::Idle)),
            None,
            "Done.\n\u{276f} \n",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Idle);
    }

    #[test]
    fn test_compose_stale_hook_falls_back_to_regex() {
        let s = compose_status(
            Some(&stale_hook(SessionStatus::Idle)),
            None,
            "ctrl+c to interrupt\n",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Running);
    }

    #[test]
    fn test_compose_no_hook_uses_pane_title_when_available() {
        let s = compose_status(
            None,
            Some(SessionStatus::Running),
            "doesn't matter",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Running);
    }

    #[test]
    fn test_compose_no_hook_no_title_uses_regex() {
        let s = compose_status(
            None,
            None,
            "ctrl+c to interrupt\n",
            runner_for(Tool::Claude),
            false,
            SystemTime::now(),
        );
        assert_eq!(s, SessionStatus::Running);
    }

    #[test]
    fn test_claude_runner_is_implemented() {
        assert!(runner_for(Tool::Claude).is_implemented());
    }

    #[test]
    fn test_fallback_runners_report_not_implemented() {
        for tool in [Tool::Codex, Tool::Opencode, Tool::Gemini, Tool::Custom] {
            assert!(
                !runner_for(tool).is_implemented(),
                "{:?} should still be a fallback at this stage",
                tool
            );
        }
    }

    #[test]
    fn test_implemented_tools_includes_claude_and_shell() {
        assert_eq!(implemented_tools(), vec![Tool::Claude, Tool::Shell]);
    }

    #[test]
    fn test_runner_for_shell_returns_shell_runner() {
        let r = runner_for(Tool::Shell);
        assert_eq!(r.name(), "shell");
        // None means tmux's default-shell handles the pane — no send-keys.
        assert_eq!(r.launch_command(), None);
        assert!(r.is_implemented());
    }
}
