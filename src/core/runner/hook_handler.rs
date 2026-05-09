//! `agent-view hook` subcommand — invoked by Claude Code on each state
//! transition. Writes per-session status JSON; on Stop events also writes
//! per-event cost JSON.
//!
//! All errors are silent (handler always exits 0) so Claude Code is never
//! blocked. The pure parsing/mapping helpers below are unit-tested; the
//! I/O orchestrator is in `run()`.

use crate::types::SessionStatus;
use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;

#[allow(dead_code)]
pub const MAX_PAYLOAD_BYTES: usize = 1 << 20; // 1 MiB

static INSTANCE_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_.\-]*$").expect("static regex must compile")
});

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    pub hook_event_name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub matcher: Option<serde_json::Value>,
}

/// Validate an `AGENT_VIEW_SESSION_ID` env value. Returns `true` if safe to
/// use as a filename component. Rejects empty / `..` / unusual chars.
#[allow(dead_code)]
pub fn validate_instance_id(id: &str) -> bool {
    if id.is_empty() || id.contains("..") {
        return false;
    }
    INSTANCE_ID_RE.is_match(id)
}

/// Map a Claude Code hook event to a `SessionStatus`. Returns `None` for
/// events that should not change status (unknown events, or Notification
/// events with non-permission matchers — Notification is filtered separately
/// by the caller).
#[allow(dead_code)]
pub fn map_event_to_status(event: &str) -> Option<SessionStatus> {
    match event {
        "SessionStart" | "Stop" | "SessionEnd" => Some(SessionStatus::Idle),
        "UserPromptSubmit" => Some(SessionStatus::Running),
        "PermissionRequest" => Some(SessionStatus::Waiting),
        "PreCompact" => Some(SessionStatus::Compacting),
        _ => None,
    }
}

/// For Notification events: only map to Waiting when the matcher is one
/// of the permission/elicitation prompts. Otherwise None.
#[allow(dead_code)]
pub fn notification_status(matcher: Option<&serde_json::Value>) -> Option<SessionStatus> {
    let m = matcher?.as_str()?;
    if m == "permission_prompt" || m == "elicitation_dialog" {
        Some(SessionStatus::Waiting)
    } else {
        None
    }
}

/// Parse JSON payload from raw bytes. Returns None on size cap or parse error.
#[allow(dead_code)]
pub fn parse_payload(data: &[u8]) -> Option<HookPayload> {
    if data.is_empty() || data.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    serde_json::from_slice(data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_instance_id_accepts_uuid() {
        assert!(validate_instance_id("7a3f2b1e-4c5d-6e7f-8a9b-0c1d2e3f4a5b"));
    }

    #[test]
    fn test_validate_instance_id_rejects_empty() {
        assert!(!validate_instance_id(""));
    }

    #[test]
    fn test_validate_instance_id_rejects_path_traversal() {
        assert!(!validate_instance_id("../etc/passwd"));
        assert!(!validate_instance_id("foo..bar"));
    }

    #[test]
    fn test_validate_instance_id_rejects_slash() {
        assert!(!validate_instance_id("foo/bar"));
    }

    #[test]
    fn test_map_event_to_status_known_events() {
        assert_eq!(
            map_event_to_status("SessionStart"),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            map_event_to_status("UserPromptSubmit"),
            Some(SessionStatus::Running)
        );
        assert_eq!(map_event_to_status("Stop"), Some(SessionStatus::Idle));
        assert_eq!(
            map_event_to_status("PermissionRequest"),
            Some(SessionStatus::Waiting)
        );
        assert_eq!(
            map_event_to_status("PreCompact"),
            Some(SessionStatus::Compacting)
        );
        assert_eq!(map_event_to_status("SessionEnd"), Some(SessionStatus::Idle));
    }

    #[test]
    fn test_map_event_to_status_unknown_returns_none() {
        assert_eq!(map_event_to_status("Notification"), None);
        assert_eq!(map_event_to_status("MysteryFutureEvent"), None);
    }

    #[test]
    fn test_notification_status_permission_prompt() {
        let m = json!("permission_prompt");
        assert_eq!(notification_status(Some(&m)), Some(SessionStatus::Waiting));
    }

    #[test]
    fn test_notification_status_elicitation_dialog() {
        let m = json!("elicitation_dialog");
        assert_eq!(notification_status(Some(&m)), Some(SessionStatus::Waiting));
    }

    #[test]
    fn test_notification_status_other_returns_none() {
        let m = json!("info_only");
        assert_eq!(notification_status(Some(&m)), None);
        assert_eq!(notification_status(None), None);
    }

    #[test]
    fn test_parse_payload_minimal() {
        let raw = br#"{"hook_event_name":"Stop"}"#;
        let p = parse_payload(raw).unwrap();
        assert_eq!(p.hook_event_name, "Stop");
        assert!(p.session_id.is_none());
    }

    #[test]
    fn test_parse_payload_full() {
        let raw = br#"{"hook_event_name":"Stop","session_id":"abc","transcript_path":"/foo.jsonl","matcher":"x"}"#;
        let p = parse_payload(raw).unwrap();
        assert_eq!(p.session_id.as_deref(), Some("abc"));
        assert_eq!(p.transcript_path.as_deref(), Some("/foo.jsonl"));
    }

    #[test]
    fn test_parse_payload_empty_returns_none() {
        assert!(parse_payload(b"").is_none());
    }

    #[test]
    fn test_parse_payload_oversize_returns_none() {
        let huge = vec![b'a'; MAX_PAYLOAD_BYTES + 1];
        assert!(parse_payload(&huge).is_none());
    }

    #[test]
    fn test_parse_payload_malformed_returns_none() {
        assert!(parse_payload(b"not json at all").is_none());
    }
}
