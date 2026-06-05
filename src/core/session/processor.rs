use std::collections::HashMap;
use std::time::Instant;

use crate::core::notify::{send_notification, NotificationOptions};
use crate::types::{Session, SessionStatus};

/// Minimum time (ms) a session must be "running" before idle triggers "completed" notification
const MIN_RUNNING_DURATION_MS: u128 = 10_000;
/// Minimum time (ms) a session must be idle before we consider it "completed"
const MIN_IDLE_DURATION_MS: u128 = 8_000;
/// Minimum time (ms) error patterns must persist before showing error status
const MIN_ERROR_DURATION_MS: u128 = 5_000;
/// Minimum time (ms) a new status must persist before the UI updates
const STATUS_DEBOUNCE_MS: u128 = 750;

/// Tracks debounce and notification state for status processing.
/// Lives in the background thread.
pub struct StatusProcessor {
    /// Last status we notified about per session (prevents repeated notifications)
    last_notified_status: HashMap<String, SessionStatus>,
    /// When a session entered "running" state
    running_start_time: HashMap<String, Instant>,
    /// Last sustained running duration per session
    last_sustained_running: HashMap<String, u128>,
    /// When a session first entered idle
    idle_start_time: HashMap<String, Instant>,
    /// Recently detached sessions (suppress notifications briefly)
    recently_detached: HashMap<String, Instant>,
    /// When a session first showed error patterns
    error_start_time: HashMap<String, Instant>,
    /// Pending status transitions for debouncing
    pending_status: HashMap<String, (SessionStatus, Instant)>,
}

impl StatusProcessor {
    pub fn new() -> Self {
        Self {
            last_notified_status: HashMap::new(),
            running_start_time: HashMap::new(),
            last_sustained_running: HashMap::new(),
            idle_start_time: HashMap::new(),
            recently_detached: HashMap::new(),
            error_start_time: HashMap::new(),
            pending_status: HashMap::new(),
        }
    }

    /// Mark a session as recently detached to suppress notifications
    pub fn suppress_notification(&mut self, tmux_session: &str) {
        self.recently_detached
            .insert(tmux_session.to_string(), Instant::now());
    }

    /// Determine the resolved status for a session given the raw detected status.
    /// Applies error hysteresis and status debouncing.
    /// Returns the status to display (may be the previous status if still debouncing).
    pub fn resolve_status(
        &mut self,
        session_id: &str,
        raw_status: SessionStatus,
        previous_status: SessionStatus,
    ) -> SessionStatus {
        // Error hysteresis: require sustained error before showing
        if raw_status == SessionStatus::Error {
            let error_duration = self
                .error_start_time
                .entry(session_id.to_string())
                .or_insert_with(Instant::now)
                .elapsed()
                .as_millis();
            if error_duration < MIN_ERROR_DURATION_MS {
                return if previous_status == SessionStatus::Error {
                    SessionStatus::Idle
                } else {
                    previous_status
                };
            }
        } else {
            self.error_start_time.remove(session_id);
        }

        // Debounce: statuses that need user attention bypass debounce (immediate)
        if raw_status != previous_status {
            if matches!(
                raw_status,
                SessionStatus::Waiting
                    | SessionStatus::Paused
                    | SessionStatus::Error
                    | SessionStatus::Idle
                    | SessionStatus::Monitoring
                    | SessionStatus::Draft
                    | SessionStatus::Crashed
            ) {
                self.pending_status.remove(session_id);
                return raw_status;
            }

            if let Some((pending_st, pending_since)) = self.pending_status.get(session_id) {
                if *pending_st == raw_status {
                    if pending_since.elapsed().as_millis() >= STATUS_DEBOUNCE_MS {
                        self.pending_status.remove(session_id);
                        return raw_status;
                    }
                    return previous_status; // still debouncing
                }
            }
            // New candidate status
            self.pending_status
                .insert(session_id.to_string(), (raw_status, Instant::now()));
            return previous_status;
        }

        // Status matches current — clear pending
        self.pending_status.remove(session_id);
        raw_status
    }

    /// Update running/idle duration tracking for notification logic
    pub fn track_durations(&mut self, session_id: &str, status: SessionStatus) {
        match status {
            SessionStatus::Running => {
                if !self.running_start_time.contains_key(session_id) {
                    self.running_start_time
                        .insert(session_id.to_string(), Instant::now());
                }
                self.idle_start_time.remove(session_id);
            }
            SessionStatus::Idle => {
                if !self.idle_start_time.contains_key(session_id) {
                    self.idle_start_time
                        .insert(session_id.to_string(), Instant::now());
                }
                // Record last running duration before clearing
                if let Some(start) = self.running_start_time.remove(session_id) {
                    self.last_sustained_running
                        .insert(session_id.to_string(), start.elapsed().as_millis());
                }
            }
            _ => {
                self.idle_start_time.remove(session_id);
                self.running_start_time.remove(session_id);
            }
        }

        // Update sustained running duration if still running
        if status == SessionStatus::Running {
            if let Some(start) = self.running_start_time.get(session_id) {
                let duration = start.elapsed().as_millis();
                self.last_sustained_running
                    .insert(session_id.to_string(), duration);
                // Reset notification tracking after sustained running
                if duration >= MIN_RUNNING_DURATION_MS {
                    self.last_notified_status.remove(session_id);
                }
            }
        }
    }

    /// Check if a notification should fire and fire it.
    /// Returns true if a notification was sent.
    pub fn maybe_notify(
        &mut self,
        session: &Session,
        new_status: SessionStatus,
        attached_session: Option<&str>,
        sound: bool,
    ) -> bool {
        if !session.notify {
            return false;
        }
        // Suppress notifications for the session the user is currently looking at
        if let Some(attached) = attached_session {
            if session.tmux_session == attached {
                return false;
            }
        }

        // Check recently detached
        if let Some(detach_time) = self.recently_detached.get(&session.tmux_session) {
            if detach_time.elapsed().as_millis() < 5000 {
                return false;
            }
            self.recently_detached.remove(&session.tmux_session);
        }

        let last = self.last_notified_status.get(&session.id);

        let notified = match new_status {
            SessionStatus::Waiting if last != Some(&SessionStatus::Waiting) => {
                send_notification(NotificationOptions {
                    title: format!("\u{1F7E1} {}", session.title),
                    body: "Needs approval".to_string(),
                    subtitle: None,
                    sound,
                });
                true
            }
            SessionStatus::Paused if last != Some(&SessionStatus::Paused) => {
                send_notification(NotificationOptions {
                    title: format!("\u{1F535} {}", session.title),
                    body: "Asked you a question".to_string(),
                    subtitle: None,
                    sound,
                });
                true
            }
            SessionStatus::Idle if last != Some(&SessionStatus::Idle) => {
                let idle_duration = self
                    .idle_start_time
                    .get(&session.id)
                    .map(|t| t.elapsed().as_millis())
                    .unwrap_or(0);
                let was_running_enough = self
                    .last_sustained_running
                    .get(&session.id)
                    .copied()
                    .unwrap_or(0)
                    >= MIN_RUNNING_DURATION_MS;
                let is_sustained_idle = idle_duration >= MIN_IDLE_DURATION_MS;

                if was_running_enough && is_sustained_idle {
                    send_notification(NotificationOptions {
                        title: format!("\u{2705} {}", session.title),
                        body: "Completed its task".to_string(),
                        subtitle: None,
                        sound,
                    });
                    true
                } else {
                    false
                }
            }
            SessionStatus::Error if last != Some(&SessionStatus::Error) => {
                send_notification(NotificationOptions {
                    title: format!("\u{1F534} {}", session.title),
                    body: "Was interrupted".to_string(),
                    subtitle: None,
                    sound,
                });
                true
            }
            _ => false,
        };

        if notified {
            self.last_notified_status
                .insert(session.id.clone(), new_status);
        }

        notified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tool;

    fn make_test_session(id: &str, notify: bool) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp".to_string(),
            group_path: "my-sessions".to_string(),
            order: 0,
            command: "claude".to_string(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Running,
            tmux_session: format!("agentorch_{}", id),
            created_at: 0,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            mcp_selection: crate::core::mcp::McpSelection::default(),
            acknowledged: false,
            notify,
            follow_up: false,
            user_waiting: false,
            status_changed_at: 0,
            restart_count: 0,
            last_started_at: 0,
            notes: vec![],
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        }
    }

    #[test]
    fn test_resolve_status_debounces_running() {
        let mut mgr = StatusProcessor::new();
        // Running is debounced — first call starts timer, returns previous
        let result = mgr.resolve_status("s1", SessionStatus::Running, SessionStatus::Idle);
        assert_eq!(result, SessionStatus::Idle); // still debouncing
    }

    #[test]
    fn test_resolve_status_idle_is_immediate() {
        let mut mgr = StatusProcessor::new();
        // Idle bypasses debounce (task completion should show immediately)
        let result = mgr.resolve_status("s1", SessionStatus::Idle, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Idle); // immediate
    }

    #[test]
    fn test_resolve_status_waiting_is_immediate() {
        let mut mgr = StatusProcessor::new();
        let result = mgr.resolve_status("s1", SessionStatus::Waiting, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Waiting); // immediate
    }

    #[test]
    fn test_resolve_status_same_status_clears_pending() {
        let mut mgr = StatusProcessor::new();
        // Start a pending transition (Running is debounced)
        mgr.resolve_status("s1", SessionStatus::Running, SessionStatus::Idle);
        assert!(mgr.pending_status.contains_key("s1"));

        // Same as current — clears pending
        mgr.resolve_status("s1", SessionStatus::Idle, SessionStatus::Idle);
        assert!(!mgr.pending_status.contains_key("s1"));
    }

    #[test]
    fn test_resolve_status_error_hysteresis() {
        let mut mgr = StatusProcessor::new();
        // Error just started — should not immediately show
        let result = mgr.resolve_status("s1", SessionStatus::Error, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Running); // error not sustained yet
    }

    #[test]
    fn test_suppress_notification() {
        let mut mgr = StatusProcessor::new();
        mgr.suppress_notification("agentorch_test");
        assert!(mgr.recently_detached.contains_key("agentorch_test"));
    }

    #[test]
    fn test_maybe_notify_returns_false_when_not_enabled() {
        let mut mgr = StatusProcessor::new();
        let session = make_test_session("s1", false); // notify = false
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, None, false);
        assert!(!result);
    }

    #[test]
    fn test_maybe_notify_returns_false_when_attached() {
        let mut mgr = StatusProcessor::new();
        let session = make_test_session("s1", true);
        let result = mgr.maybe_notify(
            &session,
            SessionStatus::Waiting,
            Some("agentorch_s1"),
            false,
        );
        assert!(!result);
    }

    #[test]
    fn test_track_durations_running() {
        let mut mgr = StatusProcessor::new();
        mgr.track_durations("s1", SessionStatus::Running);
        assert!(mgr.running_start_time.contains_key("s1"));
        assert!(!mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_track_durations_idle_clears_running() {
        let mut mgr = StatusProcessor::new();
        mgr.track_durations("s1", SessionStatus::Running);
        mgr.track_durations("s1", SessionStatus::Idle);
        assert!(!mgr.running_start_time.contains_key("s1"));
        assert!(mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_track_durations_other_clears_both() {
        let mut mgr = StatusProcessor::new();
        mgr.track_durations("s1", SessionStatus::Running);
        mgr.track_durations("s1", SessionStatus::Waiting);
        assert!(!mgr.running_start_time.contains_key("s1"));
        assert!(!mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_maybe_notify_suppresses_attached_session() {
        let mut mgr = StatusProcessor::new();
        let session = make_test_session("s1", true);
        // Attached to this exact session — should suppress
        let result = mgr.maybe_notify(
            &session,
            SessionStatus::Waiting,
            Some("agentorch_s1"),
            false,
        );
        assert!(!result);
    }

    #[test]
    fn test_maybe_notify_allows_other_sessions_when_attached() {
        let mut mgr = StatusProcessor::new();
        let session = make_test_session("s2", true);
        // Attached to a DIFFERENT session — should allow notification
        let result = mgr.maybe_notify(
            &session,
            SessionStatus::Waiting,
            Some("agentorch_s1"),
            false,
        );
        assert!(result);
    }

    #[test]
    fn test_maybe_notify_allows_all_when_not_attached() {
        let mut mgr = StatusProcessor::new();
        let session = make_test_session("s1", true);
        // Not attached to anything — should allow notification
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, None, false);
        assert!(result);
    }
}
