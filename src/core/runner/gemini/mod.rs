//! Gemini CLI runner. Launches `gemini`, captures status via the
//! `SessionStart` / `BeforeAgent` / `AfterAgent` / `SessionEnd` hooks
//! (installed into `~/.gemini/settings.json`).
//!
//! Status detection is hook-driven only at this stage. `parse_status`
//! returns the default `ToolStatus` — Gemini 0.9's TUI hasn't been
//! captured for fixtures yet, so pane scraping is deferred to a follow-up
//! when real captures are available. The hook pair `BeforeAgent` →
//! Running, `AfterAgent` → Idle gives turn-level transitions
//! authoritatively, so this is a reasonable starting posture.
//!
//! Gemini CLI 0.9 has no resume flag (no `--resume`, no `--chat <id>`,
//! no `--continue`), so `restart_command` always returns the original
//! command. The session id captured from hooks is preserved in
//! `tool_data` for analytics linking even though it can't be used for
//! relaunch.

pub mod hook_handler;
pub mod hooks;

use super::{Runner, ToolStatus};

pub struct GeminiRunner;

impl Runner for GeminiRunner {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn launch_command(&self) -> Option<&'static str> {
        Some("gemini")
    }

    fn parse_status(&self, _pane_content: &str) -> ToolStatus {
        // Deferred: real pane captures (idle / typing / mid-turn /
        // awaiting-confirmation) are needed before we can write a
        // meaningful parser. Hook pair carries Running/Idle transitions
        // for now; pane scraping is the secondary fallback per
        // compose_status, and an empty ToolStatus there yields Idle.
        ToolStatus::default()
    }

    fn extract_session_id(&self, _pane_content: &str) -> Option<String> {
        // Gemini doesn't print the session id to the pane. Captured from
        // the hook payload's `session_id` field by hook_handler.rs.
        None
    }

    fn restart_command(&self, original_command: &str, _tool_data: &str) -> String {
        // Gemini CLI 0.9 has no resume flag. Re-launch fresh.
        original_command.to_string()
    }

    fn install_hooks(&self) -> Result<(), String> {
        let dir = hooks::gemini_config_dir().ok_or_else(|| "no home directory".to_string())?;
        let cmd = hooks::resolve_hook_command()?;
        hooks::install_hooks_in(&dir, &cmd)
    }

    fn tool_data_session_id_key(&self) -> &'static str {
        "gemini_session_id"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_and_launch_command() {
        let r = GeminiRunner;
        assert_eq!(r.name(), "gemini");
        assert_eq!(r.launch_command(), Some("gemini"));
    }

    #[test]
    fn test_parse_status_returns_default() {
        // Documented: hook-driven only at this stage.
        let s = GeminiRunner.parse_status("anything > here\n");
        assert!(!s.is_busy);
        assert!(!s.has_idle_prompt);
        assert!(!s.has_draft);
        assert!(!s.has_question);
        assert!(!s.has_exited);
    }

    #[test]
    fn test_extract_session_id_returns_none() {
        assert_eq!(GeminiRunner.extract_session_id("anything"), None);
    }

    #[test]
    fn test_restart_command_falls_back_to_original_when_no_tool_data() {
        assert_eq!(GeminiRunner.restart_command("gemini", "{}"), "gemini");
    }

    #[test]
    fn test_restart_command_ignores_tool_data_no_resume_supported() {
        // Even when a session id is present, Gemini 0.9 has no resume CLI,
        // so we relaunch fresh. Pin this behavior so a future change is
        // intentional.
        assert_eq!(
            GeminiRunner.restart_command("gemini", r#"{"gemini_session_id":"abc-123"}"#),
            "gemini"
        );
    }

    #[test]
    fn test_is_implemented_returns_true() {
        assert!(GeminiRunner.is_implemented());
    }

    #[test]
    fn test_tool_data_session_id_key() {
        assert_eq!(GeminiRunner.tool_data_session_id_key(), "gemini_session_id");
    }

    #[test]
    fn test_wants_ansi_escapes_default_false() {
        // Until parse_status needs SGR codes, the default ANSI-stripped
        // capture path is fine.
        assert!(!GeminiRunner.wants_ansi_escapes());
    }
}
