//! Platform abstraction for system job scheduling

use std::io;

#[allow(unused_imports)]
use crate::types::Routine;

#[derive(thiserror::Error, Debug)]
pub enum SchedulerError {
    #[error("scheduler command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

pub type SchedulerResult<T> = Result<T, SchedulerError>;

pub trait Scheduler {
    fn install(&self, routine: &Routine) -> SchedulerResult<()>;
    fn uninstall(&self, routine_id: &str) -> SchedulerResult<()>;
    fn is_installed(&self, routine_id: &str) -> bool;
    fn has_stale_binary_path(&self, routine_id: &str) -> bool;
}

/// Get the platform-appropriate scheduler
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
