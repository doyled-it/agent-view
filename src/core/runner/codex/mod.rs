//! Codex runner. Launches `codex`, captures session ids from notify
//! payloads (see `notify.rs`), and resumes via `codex resume <sid>`
//! gated on the on-disk rollout file (agent-deck issue #756).

pub mod hooks;
pub mod notify;

use super::{Runner, ToolStatus};
use std::path::PathBuf;

pub struct CodexRunner;

impl Runner for CodexRunner {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn launch_command(&self) -> Option<&'static str> {
        Some("codex")
    }

    fn parse_status(&self, _pane_content: &str) -> ToolStatus {
        // Defer to hooks. The active-pane heuristic in resolve_session_status
        // handles the pre-first-notify window correctly.
        ToolStatus::default()
    }

    fn extract_session_id(&self, _pane_content: &str) -> Option<String> {
        // Codex doesn't print its session id to the pane. Captured from
        // notify payloads by notify_handler.rs.
        None
    }

    fn restart_command(&self, original_command: &str, tool_data: &str) -> String {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(tool_data) {
            if let Some(sid) = data.get("codex_session_id").and_then(|v| v.as_str()) {
                if codex_rollout_exists(sid) {
                    return format!("codex resume {}", sid);
                }
                // Stale sid: fall through to fresh launch. The poller clears
                // the sid on the next hook tick.
            }
        }
        original_command.to_string()
    }

    fn install_hooks(&self) -> Result<(), String> {
        let dir = hooks::codex_config_dir().ok_or_else(|| "no home directory".to_string())?;
        let cmd = hooks::resolve_notify_command()?;
        hooks::install_hooks_in(&dir, &cmd)
    }

    fn tool_data_session_id_key(&self) -> &'static str {
        "codex_session_id"
    }
}

/// True if Codex has flushed a rollout JSONL for the given session id under
/// `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<sid>.jsonl`. Used to gate
/// `codex resume <sid>` — without this check, a session that died before its
/// first rollout flush would loop forever (issue #756 in agent-deck).
pub(crate) fn codex_rollout_exists(sid: &str) -> bool {
    let sid = sid.trim();
    if sid.is_empty() {
        return false;
    }
    let codex_home = std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_default();
    let pattern = codex_home
        .join("sessions")
        .join("*")
        .join("*")
        .join("*")
        .join(format!("rollout-*-{}.jsonl", sid));
    glob::glob(pattern.to_str().unwrap_or(""))
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_name_and_launch_command() {
        let r = CodexRunner;
        assert_eq!(r.name(), "codex");
        assert_eq!(r.launch_command(), Some("codex"));
    }

    #[test]
    fn test_parse_status_returns_default() {
        let s = CodexRunner.parse_status("any pane content");
        assert!(!s.is_busy);
        assert!(!s.is_waiting);
        assert!(!s.has_error);
    }

    #[test]
    fn test_extract_session_id_returns_none() {
        assert_eq!(CodexRunner.extract_session_id("anything"), None);
    }

    #[test]
    fn test_is_implemented_returns_true() {
        assert!(CodexRunner.is_implemented());
    }

    #[test]
    fn test_tool_data_session_id_key() {
        assert_eq!(CodexRunner.tool_data_session_id_key(), "codex_session_id");
    }

    #[test]
    fn test_restart_command_falls_back_when_no_tool_data() {
        assert_eq!(CodexRunner.restart_command("codex", "{}"), "codex");
    }

    #[test]
    fn test_restart_command_falls_back_when_rollout_missing() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("CODEX_HOME", dir.path());
        let cmd = CodexRunner.restart_command("codex", r#"{"codex_session_id": "stale-uuid-xyz"}"#);
        std::env::remove_var("CODEX_HOME");
        assert_eq!(cmd, "codex");
    }

    #[test]
    fn test_restart_command_resumes_when_rollout_exists() {
        let dir = TempDir::new().unwrap();
        let sid = "abc-123";
        let rollout_dir = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("05")
            .join("10");
        fs::create_dir_all(&rollout_dir).unwrap();
        fs::write(
            rollout_dir.join(format!("rollout-1234567890-{}.jsonl", sid)),
            "",
        )
        .unwrap();

        std::env::set_var("CODEX_HOME", dir.path());
        let cmd =
            CodexRunner.restart_command("codex", &format!(r#"{{"codex_session_id": "{}"}}"#, sid));
        std::env::remove_var("CODEX_HOME");
        assert_eq!(cmd, format!("codex resume {}", sid));
    }
}
