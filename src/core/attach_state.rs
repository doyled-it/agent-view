//! Shared state between main thread and background thread for attach tracking.

/// Communicates attach state from the main thread to the background thread.
/// Protected by Arc<Mutex<>>.
pub struct AttachState {
    /// Which tmux session the user is currently inside (None = on home screen)
    pub attached_session: Option<String>,
    /// Tmux session names to add to recently_detached suppression
    pub suppress_queue: Vec<String>,
}

impl AttachState {
    pub fn new() -> Self {
        Self {
            attached_session: None,
            suppress_queue: Vec::new(),
        }
    }
}
