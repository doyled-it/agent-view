//! Usage monitoring for Claude Code — background thread and output parser.

use std::sync::{Arc, Mutex};

use crate::core::tmux;
use crate::types::UsageData;

mod monitor;
mod parser;

const META_SESSION_NAME: &str = "__agentview_meta_usage";
pub const META_SESSION_PREFIX: &str = "__agentview_meta_";

/// Shared usage data between the monitor thread and the main UI thread.
pub type SharedUsageData = Arc<Mutex<Option<UsageData>>>;

/// Spawn the usage monitor background thread.
/// Returns the shared data handle and the thread join handle.
pub fn spawn_monitor() -> (SharedUsageData, std::thread::JoinHandle<()>) {
    let shared: SharedUsageData = Arc::new(Mutex::new(None));
    let shared_clone = Arc::clone(&shared);

    let handle = std::thread::spawn(move || {
        monitor::monitor_loop(shared_clone);
    });

    (shared, handle)
}

/// Kill the usage monitor tmux session (call on app shutdown).
pub fn kill_monitor() {
    if tmux::session_exists(META_SESSION_NAME) {
        let _ = tmux::kill_session(META_SESSION_NAME);
    }
}
