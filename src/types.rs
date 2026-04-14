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
}
