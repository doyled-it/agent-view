//! Linux systemd timer scheduler implementation (stub — not yet implemented)

use crate::core::scheduler::Scheduler;
use crate::types::Routine;

#[allow(dead_code)]
pub struct LinuxScheduler;

impl LinuxScheduler {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl Scheduler for LinuxScheduler {
    fn install(&self, _routine: &Routine) -> Result<(), String> {
        Err("Linux systemd timer scheduling is not yet implemented. Routines are currently macOS-only.".to_string())
    }

    fn uninstall(&self, _routine_id: &str) -> Result<(), String> {
        Err("Linux systemd timer scheduling is not yet implemented.".to_string())
    }

    fn is_installed(&self, _routine_id: &str) -> bool {
        false
    }

    fn has_stale_binary_path(&self, _routine_id: &str) -> bool {
        false
    }
}
