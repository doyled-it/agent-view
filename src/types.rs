//! Core types for Agent View

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Waiting,
    Paused,
    Compacting,
    Idle,
    Error,
    Stopped,
}

impl SessionStatus {
    pub fn sort_priority(&self) -> u8 {
        match self {
            Self::Waiting => 0,
            Self::Paused => 1,
            Self::Running => 2,
            Self::Compacting => 3,
            Self::Idle => 4,
            Self::Stopped => 5,
            Self::Error => 6,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Compacting => "compacting",
            Self::Idle => "idle",
            Self::Error => "error",
            Self::Stopped => "stopped",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "paused" => Self::Paused,
            "compacting" => Self::Compacting,
            "idle" => Self::Idle,
            "error" => Self::Error,
            "stopped" => Self::Stopped,
            _ => Self::Idle,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Running => "●",
            Self::Waiting => "◐",
            Self::Paused => "◆",
            Self::Compacting => "◌",
            Self::Idle => "○",
            Self::Error => "✗",
            Self::Stopped => "◻",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Opencode,
    Gemini,
    Codex,
    Custom,
    Shell,
}

impl Tool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Custom => "custom",
            Self::Shell => "shell",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "claude" => Self::Claude,
            "opencode" => Self::Opencode,
            "gemini" => Self::Gemini,
            "codex" => Self::Codex,
            "custom" => Self::Custom,
            "shell" => Self::Shell,
            _ => Self::Shell,
        }
    }

    pub fn command(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Custom => "bash",
            Self::Shell => "bash",
        }
    }
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusHistoryEntry {
    pub status: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub project_path: String,
    pub group_path: String,
    pub order: i32,
    pub command: String,
    pub wrapper: String,
    pub tool: Tool,
    pub status: SessionStatus,
    pub tmux_session: String,
    pub created_at: i64,
    pub last_accessed: i64,
    pub parent_session_id: String,
    pub worktree_path: String,
    pub worktree_repo: String,
    pub worktree_branch: String,
    pub tool_data: String,
    pub acknowledged: bool,
    pub notify: bool,
    pub follow_up: bool,
    pub status_changed_at: i64,
    pub restart_count: i32,
    pub status_history: Vec<StatusHistoryEntry>,
    pub pinned: bool,
    pub tokens_used: i64,
}

impl Session {
    pub fn status_history_json(&self) -> String {
        serde_json::to_string(&self.status_history).unwrap_or_else(|_| "[]".to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub path: String,
    pub name: String,
    pub expanded: bool,
    pub order: i32,
    pub default_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    StatusPriority,
    LastActivity,
    Name,
    Created,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            Self::StatusPriority => Self::LastActivity,
            Self::LastActivity => Self::Name,
            Self::Name => Self::Created,
            Self::Created => Self::StatusPriority,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::StatusPriority => "status",
            Self::LastActivity => "activity",
            Self::Name => "name",
            Self::Created => "created",
        }
    }
}

pub struct SessionCreateOptions {
    pub title: Option<String>,
    pub project_path: String,
    pub group_path: Option<String>,
    pub tool: Tool,
    pub command: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub session_title: String,
    #[allow(dead_code)]
    pub old_status: SessionStatus,
    pub new_status: SessionStatus,
    pub timestamp: i64,
    #[allow(dead_code)]
    pub message: Option<String>,
}

impl ActivityEvent {
    #[allow(dead_code)]
    pub fn format_line(&self) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        let ago_ms = now - self.timestamp;
        let ago = if ago_ms < 60_000 {
            "just now".to_string()
        } else if ago_ms < 3_600_000 {
            format!("{}m ago", ago_ms / 60_000)
        } else {
            format!("{}h ago", ago_ms / 3_600_000)
        };

        match &self.message {
            Some(msg) => format!(
                "{:<10} {} -> {} \"{}\"",
                ago,
                self.session_title,
                self.new_status.as_str(),
                msg
            ),
            None => format!(
                "{:<10} {} -> {}",
                ago,
                self.session_title,
                self.new_status.as_str()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_status_roundtrip() {
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Compacting,
            SessionStatus::Idle,
            SessionStatus::Error,
            SessionStatus::Stopped,
        ];
        for s in statuses {
            assert_eq!(SessionStatus::from_str(s.as_str()), s);
        }
    }

    #[test]
    fn test_tool_roundtrip() {
        let tools = [
            Tool::Claude,
            Tool::Opencode,
            Tool::Gemini,
            Tool::Codex,
            Tool::Custom,
            Tool::Shell,
        ];
        for t in tools {
            assert_eq!(Tool::from_str(t.as_str()), t);
        }
    }

    #[test]
    fn test_session_status_unknown_defaults_to_idle() {
        assert_eq!(SessionStatus::from_str("unknown"), SessionStatus::Idle);
    }

    #[test]
    fn test_tool_unknown_defaults_to_shell() {
        assert_eq!(Tool::from_str("unknown"), Tool::Shell);
    }

    #[test]
    fn test_activity_event_format() {
        let event = ActivityEvent {
            session_title: "BIS".to_string(),
            old_status: SessionStatus::Running,
            new_status: SessionStatus::Paused,
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some("Asked a question".to_string()),
        };
        let line = event.format_line();
        assert!(line.contains("BIS"));
        assert!(line.contains("paused"));
        assert!(line.contains("Asked a question"));
    }

    #[test]
    fn test_session_status_icons_are_unique() {
        use std::collections::HashSet;
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Compacting,
            SessionStatus::Idle,
            SessionStatus::Error,
            SessionStatus::Stopped,
        ];
        let icons: HashSet<&str> = statuses.iter().map(|s| s.icon()).collect();
        assert_eq!(
            icons.len(),
            statuses.len(),
            "each status should have a unique icon"
        );
    }

    #[test]
    fn test_session_status_icons_nonempty() {
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Compacting,
            SessionStatus::Idle,
            SessionStatus::Error,
            SessionStatus::Stopped,
        ];
        for s in statuses {
            assert!(!s.icon().is_empty(), "icon for {:?} should not be empty", s);
        }
    }

    #[test]
    fn test_session_status_display_matches_as_str() {
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Compacting,
            SessionStatus::Idle,
            SessionStatus::Error,
            SessionStatus::Stopped,
        ];
        for s in statuses {
            assert_eq!(format!("{}", s), s.as_str());
        }
    }

    #[test]
    fn test_session_status_sort_priority_ordering() {
        // Waiting < Paused < Running < Compacting < Idle < Stopped < Error
        assert!(SessionStatus::Waiting.sort_priority() < SessionStatus::Paused.sort_priority());
        assert!(SessionStatus::Paused.sort_priority() < SessionStatus::Running.sort_priority());
        assert!(SessionStatus::Running.sort_priority() < SessionStatus::Compacting.sort_priority());
        assert!(SessionStatus::Compacting.sort_priority() < SessionStatus::Idle.sort_priority());
        assert!(SessionStatus::Idle.sort_priority() < SessionStatus::Stopped.sort_priority());
        assert!(SessionStatus::Stopped.sort_priority() < SessionStatus::Error.sort_priority());
    }

    #[test]
    fn test_tool_command_strings() {
        assert_eq!(Tool::Claude.command(), "claude");
        assert_eq!(Tool::Opencode.command(), "opencode");
        assert_eq!(Tool::Gemini.command(), "gemini");
        assert_eq!(Tool::Codex.command(), "codex");
        assert_eq!(Tool::Custom.command(), "bash");
        assert_eq!(Tool::Shell.command(), "bash");
    }

    #[test]
    fn test_tool_display_matches_as_str() {
        let tools = [
            Tool::Claude,
            Tool::Opencode,
            Tool::Gemini,
            Tool::Codex,
            Tool::Custom,
            Tool::Shell,
        ];
        for t in tools {
            assert_eq!(format!("{}", t), t.as_str());
        }
    }

    #[test]
    fn test_session_status_history_json_empty() {
        let session = Session {
            id: "test".to_string(),
            title: "Test".to_string(),
            project_path: "/tmp".to_string(),
            group_path: String::new(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Idle,
            tmux_session: String::new(),
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
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        };
        assert_eq!(session.status_history_json(), "[]");
    }

    #[test]
    fn test_session_status_history_json_with_entries() {
        let session = Session {
            id: "test".to_string(),
            title: "Test".to_string(),
            project_path: "/tmp".to_string(),
            group_path: String::new(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Running,
            tmux_session: String::new(),
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
            status_history: vec![
                StatusHistoryEntry {
                    status: "running".to_string(),
                    timestamp: 1700000000000,
                },
                StatusHistoryEntry {
                    status: "idle".to_string(),
                    timestamp: 1700000001000,
                },
            ],
            pinned: false,
            tokens_used: 0,
        };
        let json = session.status_history_json();
        assert!(json.contains("running"));
        assert!(json.contains("idle"));
        assert!(json.contains("1700000000000"));
    }

    #[test]
    fn test_sort_mode_cycles_through_all_variants() {
        let start = SortMode::StatusPriority;
        let next1 = start.next();
        let next2 = next1.next();
        let next3 = next2.next();
        let back = next3.next();
        assert_eq!(next1, SortMode::LastActivity);
        assert_eq!(next2, SortMode::Name);
        assert_eq!(next3, SortMode::Created);
        assert_eq!(back, SortMode::StatusPriority);
    }

    #[test]
    fn test_sort_mode_labels() {
        assert_eq!(SortMode::StatusPriority.label(), "status");
        assert_eq!(SortMode::LastActivity.label(), "activity");
        assert_eq!(SortMode::Name.label(), "name");
        assert_eq!(SortMode::Created.label(), "created");
    }
}
