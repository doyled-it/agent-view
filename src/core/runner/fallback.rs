//! Stub runner for `Tool` variants without a dedicated impl yet.
//! Each follow-up runner issue replaces one variant with a real impl.

use super::{Runner, ToolStatus};

pub struct FallbackRunner {
    #[allow(dead_code)]
    // exposed via Runner::name(); used by tests and reserved for future runners
    name: &'static str,
    launch: &'static str,
}

impl Runner for FallbackRunner {
    fn name(&self) -> &'static str {
        self.name
    }
    fn launch_command(&self) -> Option<&'static str> {
        Some(self.launch)
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
    fn is_implemented(&self) -> bool {
        false
    }
}

pub static CUSTOM: FallbackRunner = FallbackRunner {
    name: "custom",
    launch: "bash",
};
