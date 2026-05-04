use crate::core::tmux;
use crate::types::{Session, SessionStatus, Tool};

/// Detect sessions whose tmux sessions no longer exist.
/// Returns IDs of sessions that should be marked as Crashed.
pub fn detect_crashed_statuses(sessions: &[Session]) -> Vec<String> {
    sessions
        .iter()
        .filter(|s| {
            matches!(
                s.status,
                SessionStatus::Running
                    | SessionStatus::Waiting
                    | SessionStatus::Paused
                    | SessionStatus::Compacting
            ) && !s.tmux_session.is_empty()
                && !tmux::session_exists(&s.tmux_session)
        })
        .map(|s| s.id.clone())
        .collect()
}

/// Build the command to use when restarting a session.
/// For Claude: uses --resume <id> if we captured the session ID, otherwise --continue.
/// For other tools: re-runs the original command.
pub(super) fn build_restart_command(tool: Tool, original_command: &str, tool_data: &str) -> String {
    if tool == Tool::Claude {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(tool_data) {
            if let Some(session_id) = data.get("claude_session_id").and_then(|v| v.as_str()) {
                return format!("claude --resume {}", session_id);
            }
        }
        return "claude --continue".to_string();
    }
    original_command.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tool;

    fn make_test_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp".to_string(),
            group_path: "my-sessions".to_string(),
            order: 0,
            command: "claude".to_string(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Running,
            tmux_session: format!("agentorch_{}", id),
            created_at: 0,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: 0,
            restart_count: 0,
            last_started_at: 0,
            notes: vec![],
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        }
    }

    #[test]
    fn test_detect_crashed_sessions() {
        let mut session = make_test_session("crash-test");
        session.status = SessionStatus::Running;
        session.tmux_session = "agentorch_nonexistent_session_xyz".to_string();

        let crashed = detect_crashed_statuses(&[session]);
        assert_eq!(crashed.len(), 1);
        assert_eq!(crashed[0], "crash-test");
    }

    #[test]
    fn test_stopped_sessions_not_detected_as_crashed() {
        let mut session = make_test_session("stopped-test");
        session.status = SessionStatus::Stopped;
        session.tmux_session = "agentorch_nonexistent_session_xyz".to_string();

        let crashed = detect_crashed_statuses(&[session]);
        assert!(crashed.is_empty());
    }

    #[test]
    fn test_build_restart_command_claude_with_session_id() {
        let tool_data = r#"{"claude_session_id": "abc123"}"#;
        let cmd = build_restart_command(Tool::Claude, "claude", tool_data);
        assert_eq!(cmd, "claude --resume abc123");
    }

    #[test]
    fn test_build_restart_command_claude_without_session_id() {
        let tool_data = "{}";
        let cmd = build_restart_command(Tool::Claude, "claude", tool_data);
        assert_eq!(cmd, "claude --continue");
    }

    #[test]
    fn test_build_restart_command_non_claude() {
        let tool_data = "{}";
        let cmd = build_restart_command(Tool::Gemini, "gemini", tool_data);
        assert_eq!(cmd, "gemini");
    }
}
