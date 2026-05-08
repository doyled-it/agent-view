//! Stub runner for `Tool` variants without a dedicated impl yet.
//! Each follow-up runner issue replaces one variant with a real impl.

#![allow(dead_code)]

use super::{Runner, ToolStatus};

pub struct FallbackRunner {
    name: &'static str,
    launch: &'static str,
}

impl Runner for FallbackRunner {
    fn name(&self) -> &'static str {
        self.name
    }
    fn launch_command(&self) -> &'static str {
        self.launch
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

pub static CODEX: FallbackRunner = FallbackRunner {
    name: "codex",
    launch: "codex",
};
pub static OPENCODE: FallbackRunner = FallbackRunner {
    name: "opencode",
    launch: "opencode",
};
pub static GEMINI: FallbackRunner = FallbackRunner {
    name: "gemini",
    launch: "gemini",
};
pub static CUSTOM: FallbackRunner = FallbackRunner {
    name: "custom",
    launch: "bash",
};
pub static SHELL: FallbackRunner = FallbackRunner {
    name: "shell",
    launch: "bash",
};
