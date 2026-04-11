//! Session lifecycle management with status debouncing and notification logic

use crate::core::notify::{send_notification, NotificationOptions};
use crate::core::storage::Storage;
use crate::core::tmux::SessionCache;
use crate::types::{Session, SessionCreateOptions, SessionStatus};
#[cfg(test)]
use crate::types::Tool;
use std::collections::HashMap;
use std::time::Instant;

// Name generation word lists
const ADJECTIVES: &[&str] = &[
    "swift", "bright", "calm", "deep", "eager", "fair", "gentle", "happy",
    "keen", "light", "mild", "noble", "proud", "quick", "rich", "safe",
    "true", "vivid", "warm", "wise", "bold", "cool", "dark", "fast",
];

const NOUNS: &[&str] = &[
    "fox", "owl", "wolf", "bear", "hawk", "lion", "deer", "crow",
    "dove", "seal", "swan", "hare", "lynx", "moth", "newt", "orca",
    "pike", "rook", "toad", "vole", "wren", "yak", "bass", "crab",
];

fn generate_title() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    let adj = ADJECTIVES[nanos % ADJECTIVES.len()];
    let noun = NOUNS[(nanos / ADJECTIVES.len()) % NOUNS.len()];
    format!("{}-{}", adj, noun)
}

/// Tracks debounce and notification state for status management
pub struct SessionManager {
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

/// Minimum time (ms) a session must be "running" before idle triggers "completed" notification
const MIN_RUNNING_DURATION_MS: u128 = 10_000;
/// Minimum time (ms) a session must be idle before we consider it "completed"
const MIN_IDLE_DURATION_MS: u128 = 8_000;
/// Minimum time (ms) error patterns must persist before showing error status
const MIN_ERROR_DURATION_MS: u128 = 5_000;
/// Minimum time (ms) a new status must persist before the UI updates
const STATUS_DEBOUNCE_MS: u128 = 750;

impl SessionManager {
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
            if !self.error_start_time.contains_key(session_id) {
                self.error_start_time
                    .insert(session_id.to_string(), Instant::now());
            }
            let error_duration = self
                .error_start_time
                .get(session_id)
                .unwrap()
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
                SessionStatus::Waiting | SessionStatus::Paused | SessionStatus::Error
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

    /// Create a new session (creates tmux session and saves to storage)
    pub fn create_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        options: SessionCreateOptions,
    ) -> Result<Session, String> {
        let title = options.title.unwrap_or_else(generate_title);
        let id = uuid::Uuid::new_v4().to_string();
        let tmux_name = crate::core::tmux::generate_session_name(&title);
        let command = options
            .command
            .unwrap_or_else(|| options.tool.command().to_string());

        let now = chrono::Utc::now().timestamp_millis();

        let mut env = HashMap::new();
        env.insert("AGENT_ORCHESTRATOR_SESSION".to_string(), id.clone());

        crate::core::tmux::create_session(
            &tmux_name,
            Some(&command),
            Some(&options.project_path),
            Some(&env),
        )?;

        cache.register(&tmux_name);

        let session = Session {
            id: id.clone(),
            title,
            project_path: options.project_path,
            group_path: options.group_path.unwrap_or_else(|| "my-sessions".to_string()),
            order: storage.load_sessions().unwrap_or_default().len() as i32,
            command,
            wrapper: String::new(),
            tool: options.tool,
            status: SessionStatus::Running,
            tmux_session: tmux_name,
            created_at: now,
            last_accessed: now,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: now,
            restart_count: 0,
            status_history: vec![crate::types::StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: now,
            }],
        };

        storage
            .save_session(&session)
            .map_err(|e| format!("Failed to save session: {}", e))?;
        storage.touch().ok();

        Ok(session)
    }

    /// Stop a session (kill tmux but keep the record)
    pub fn stop_session(&self, storage: &Storage, session_id: &str) -> Result<(), String> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?
            .ok_or_else(|| "Session not found".to_string())?;

        if !session.tmux_session.is_empty() {
            crate::core::tmux::kill_session(&session.tmux_session)?;
        }

        storage
            .write_status(session_id, SessionStatus::Stopped, session.tool)
            .map_err(|e| format!("DB error: {}", e))?;
        storage.touch().ok();

        Ok(())
    }

    /// Delete a session (kill tmux and remove from storage)
    pub fn delete_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
    ) -> Result<(), String> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?;

        if let Some(session) = session {
            if !session.tmux_session.is_empty() {
                crate::core::tmux::kill_session(&session.tmux_session)?;
                cache.remove(&session.tmux_session);
            }
        }

        storage
            .delete_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?;
        storage.touch().ok();

        Ok(())
    }

    /// Restart a session (kill and recreate tmux session)
    pub fn restart_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
    ) -> Result<Session, String> {
        let mut session = storage
            .get_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?
            .ok_or_else(|| "Session not found".to_string())?;

        if !session.tmux_session.is_empty() {
            crate::core::tmux::kill_session(&session.tmux_session)?;
            cache.remove(&session.tmux_session);
        }

        let new_tmux_name = crate::core::tmux::generate_session_name(&session.title);
        let mut env = HashMap::new();
        env.insert(
            "AGENT_ORCHESTRATOR_SESSION".to_string(),
            session.id.clone(),
        );

        crate::core::tmux::create_session(
            &new_tmux_name,
            Some(&session.command),
            Some(&session.project_path),
            Some(&env),
        )?;

        cache.register(&new_tmux_name);

        session.tmux_session = new_tmux_name;
        session.status = SessionStatus::Running;
        session.last_accessed = chrono::Utc::now().timestamp_millis();

        storage
            .save_session(&session)
            .map_err(|e| format!("DB error: {}", e))?;
        storage
            .increment_restart_count(session_id)
            .map_err(|e| format!("DB error: {}", e))?;
        storage.touch().ok();

        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            acknowledged: false,
            notify,
            follow_up: false,
            status_changed_at: 0,
            restart_count: 0,
            status_history: vec![],
        }
    }

    #[test]
    fn test_resolve_status_debounces_non_waiting() {
        let mut mgr = SessionManager::new();
        // First call: starts debounce timer, returns previous
        let result = mgr.resolve_status("s1", SessionStatus::Idle, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Running); // still debouncing
    }

    #[test]
    fn test_resolve_status_waiting_is_immediate() {
        let mut mgr = SessionManager::new();
        let result = mgr.resolve_status("s1", SessionStatus::Waiting, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Waiting); // immediate
    }

    #[test]
    fn test_resolve_status_same_status_clears_pending() {
        let mut mgr = SessionManager::new();
        // Start a pending transition
        mgr.resolve_status("s1", SessionStatus::Idle, SessionStatus::Running);
        assert!(mgr.pending_status.contains_key("s1"));

        // Same as current — clears pending
        mgr.resolve_status("s1", SessionStatus::Running, SessionStatus::Running);
        assert!(!mgr.pending_status.contains_key("s1"));
    }

    #[test]
    fn test_resolve_status_error_hysteresis() {
        let mut mgr = SessionManager::new();
        // Error just started — should not immediately show
        let result = mgr.resolve_status("s1", SessionStatus::Error, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Running); // error not sustained yet
    }

    #[test]
    fn test_generate_title_format() {
        let title = generate_title();
        let parts: Vec<&str> = title.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(NOUNS.contains(&parts[1]));
    }

    #[test]
    fn test_suppress_notification() {
        let mut mgr = SessionManager::new();
        mgr.suppress_notification("agentorch_test");
        assert!(mgr.recently_detached.contains_key("agentorch_test"));
    }

    #[test]
    fn test_maybe_notify_returns_false_when_not_enabled() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s1", false); // notify = false
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, None, false);
        assert!(!result);
    }

    #[test]
    fn test_maybe_notify_returns_false_when_attached() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s1", true);
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, Some("agentorch_s1"), false);
        assert!(!result);
    }

    #[test]
    fn test_track_durations_running() {
        let mut mgr = SessionManager::new();
        mgr.track_durations("s1", SessionStatus::Running);
        assert!(mgr.running_start_time.contains_key("s1"));
        assert!(!mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_track_durations_idle_clears_running() {
        let mut mgr = SessionManager::new();
        mgr.track_durations("s1", SessionStatus::Running);
        mgr.track_durations("s1", SessionStatus::Idle);
        assert!(!mgr.running_start_time.contains_key("s1"));
        assert!(mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_track_durations_other_clears_both() {
        let mut mgr = SessionManager::new();
        mgr.track_durations("s1", SessionStatus::Running);
        mgr.track_durations("s1", SessionStatus::Waiting);
        assert!(!mgr.running_start_time.contains_key("s1"));
        assert!(!mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_maybe_notify_suppresses_attached_session() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s1", true);
        // Attached to this exact session — should suppress
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, Some("agentorch_s1"), false);
        assert!(!result);
    }

    #[test]
    fn test_maybe_notify_allows_other_sessions_when_attached() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s2", true);
        // Attached to a DIFFERENT session — should allow notification
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, Some("agentorch_s1"), false);
        assert!(result);
    }

    #[test]
    fn test_maybe_notify_allows_all_when_not_attached() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s1", true);
        // Not attached to anything — should allow notification
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, None, false);
        assert!(result);
    }
}
