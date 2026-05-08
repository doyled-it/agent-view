//! Claude Code runner. Filled in by Task 2.

#![allow(dead_code)]

use super::{Runner, ToolStatus};

pub struct ClaudeRunner;

impl Runner for ClaudeRunner {
    fn name(&self) -> &'static str {
        "claude"
    }
    fn launch_command(&self) -> &'static str {
        "claude"
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
}
