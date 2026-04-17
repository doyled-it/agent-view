//! Platform abstraction for system job scheduling

#[allow(unused_imports)]
use crate::types::Routine;

#[allow(dead_code)]
pub trait Scheduler {
    fn install(&self, routine: &Routine) -> Result<(), String>;
    fn uninstall(&self, routine_id: &str) -> Result<(), String>;
    fn is_installed(&self, routine_id: &str) -> bool;
    fn has_stale_binary_path(&self, routine_id: &str) -> bool;
}

/// Get the platform-appropriate scheduler
#[allow(dead_code)]
pub fn platform_scheduler() -> Box<dyn Scheduler> {
    #[cfg(target_os = "macos")]
    {
        Box::new(crate::core::scheduler_macos::MacosScheduler::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(crate::core::scheduler_linux::LinuxScheduler::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        compile_error!("Scheduled routines are only supported on macOS and Linux");
    }
}
