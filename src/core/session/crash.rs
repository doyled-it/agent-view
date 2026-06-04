use crate::core::tmux;
use crate::types::{Session, SessionStatus};

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
            mcp_selection: crate::core::mcp::McpSelection::default(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            user_waiting: false,
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
}
