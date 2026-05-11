//! Codex notify payload parser + event-to-status mapper.
//!
//! Codex's `notify` config invokes its program with a JSON payload (via
//! stdin and/or argv) on lifecycle events. The shape isn't strictly
//! versioned — different Codex releases have used `type`, `event`, or
//! `method` for the event name, and `session_id`, `thread_id`, or
//! `thread-id` for the session id. Field-extraction logic accepts every
//! shape we have observed in the wild (mirrors agent-deck's
//! `mapCodexNotifyToStatus` + `parseCodexNotifyPayload`).

use crate::types::SessionStatus;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct NotifyPayload {
    #[serde(rename = "type")]
    type_field: String,
    event: String,
    method: String,
    session_id: String,
    thread_id: String,
    #[serde(rename = "thread-id")]
    thread_id_dash: String,
    #[serde(rename = "Params")]
    params: Option<Value>,
    #[serde(rename = "Payload")]
    payload: Option<Value>,
}

/// Parse a JSON payload (from stdin or argv) and extract `(event, session_id)`.
/// Returns empty strings on parse failure or absence.
#[allow(dead_code)]
pub fn parse_payload(bytes: &[u8]) -> (String, String) {
    let payload: NotifyPayload = match serde_json::from_slice(bytes) {
        Ok(p) => p,
        Err(_) => return (String::new(), String::new()),
    };

    let event = first_non_empty(&[
        payload.type_field.as_str(),
        payload.event.as_str(),
        payload.method.as_str(),
    ])
    .map(str::to_string)
    .or_else(|| nested_field(&payload.params, &["type", "event", "method"]))
    .or_else(|| nested_field(&payload.payload, &["type", "event", "method"]))
    .unwrap_or_default();

    let session_id = first_non_empty(&[
        payload.session_id.as_str(),
        payload.thread_id.as_str(),
        payload.thread_id_dash.as_str(),
    ])
    .map(str::to_string)
    .or_else(|| {
        nested_field(
            &payload.params,
            &["session_id", "thread_id", "thread-id", "id"],
        )
    })
    .or_else(|| {
        nested_field(
            &payload.payload,
            &["session_id", "thread_id", "thread-id", "id"],
        )
    })
    .or_else(|| {
        std::env::var("CODEX_SESSION_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
    .unwrap_or_default();

    (event.trim().to_string(), session_id.trim().to_string())
}

/// Map a Codex event name to a `SessionStatus`. Accepts `.`, `-`, and `_`
/// as interchangeable separators (`turn.started` ≡ `turn-started` ≡
/// `turn_started`). Returns `None` for events we do not care about.
#[allow(dead_code)]
pub fn map_event_to_status(event: &str) -> Option<SessionStatus> {
    let normalized = event
        .trim()
        .to_ascii_lowercase()
        .replace(['.', '-', '_'], "/");
    if normalized.is_empty() {
        return None;
    }

    // Started a thread or configured a session — agent is idle, ready for input.
    if normalized == "thread/started" || normalized == "session/configured" {
        return Some(SessionStatus::Waiting);
    }

    // Turn started — agent is actively running.
    if (normalized.contains("turn") && normalized.contains("start"))
        || normalized == "agent/turn/start"
        || normalized == "agent/turn/started"
    {
        return Some(SessionStatus::Running);
    }

    // Turn completed/failed/aborted/cancelled — agent is back to idle.
    if normalized.contains("turn")
        && (normalized.contains("complete")
            || normalized.contains("fail")
            || normalized.contains("abort")
            || normalized.contains("cancel"))
    {
        return Some(SessionStatus::Waiting);
    }

    None
}

fn first_non_empty<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .map(str::trim)
        .find(|s| !s.is_empty())
}

fn nested_field(obj: &Option<Value>, keys: &[&str]) -> Option<String> {
    let map = obj.as_ref()?.as_object()?;
    for key in keys {
        if let Some(v) = map.get(*key).and_then(|v| v.as_str()) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_payload_top_level_fields() {
        let json = br#"{"type":"turn.started","session_id":"abc-123"}"#;
        let (event, sid) = parse_payload(json);
        assert_eq!(event, "turn.started");
        assert_eq!(sid, "abc-123");
    }

    #[test]
    fn test_parse_payload_falls_back_to_event_field() {
        let json = br#"{"event":"thread.started","thread_id":"t-1"}"#;
        let (event, sid) = parse_payload(json);
        assert_eq!(event, "thread.started");
        assert_eq!(sid, "t-1");
    }

    #[test]
    fn test_parse_payload_falls_back_to_thread_id_dash() {
        let json = br#"{"method":"session.configured","thread-id":"t-2"}"#;
        let (event, sid) = parse_payload(json);
        assert_eq!(event, "session.configured");
        assert_eq!(sid, "t-2");
    }

    #[test]
    fn test_parse_payload_nested_params() {
        let json = br#"{"Params":{"type":"turn.completed","session_id":"s-3"}}"#;
        let (event, sid) = parse_payload(json);
        assert_eq!(event, "turn.completed");
        assert_eq!(sid, "s-3");
    }

    #[test]
    fn test_parse_payload_invalid_json_returns_empty() {
        let (event, sid) = parse_payload(b"not json");
        assert_eq!(event, "");
        assert_eq!(sid, "");
    }

    #[test]
    fn test_map_thread_started_is_waiting() {
        assert_eq!(
            map_event_to_status("thread.started"),
            Some(SessionStatus::Waiting)
        );
    }

    #[test]
    fn test_map_session_configured_is_waiting() {
        assert_eq!(
            map_event_to_status("session.configured"),
            Some(SessionStatus::Waiting)
        );
    }

    #[test]
    fn test_map_turn_started_is_running() {
        assert_eq!(
            map_event_to_status("turn.started"),
            Some(SessionStatus::Running)
        );
        assert_eq!(
            map_event_to_status("agent-turn-start"),
            Some(SessionStatus::Running)
        );
    }

    #[test]
    fn test_map_turn_completed_is_waiting() {
        assert_eq!(
            map_event_to_status("turn.completed"),
            Some(SessionStatus::Waiting)
        );
        assert_eq!(
            map_event_to_status("agent-turn-complete"),
            Some(SessionStatus::Waiting)
        );
    }

    #[test]
    fn test_map_turn_failed_is_waiting() {
        assert_eq!(
            map_event_to_status("turn.failed"),
            Some(SessionStatus::Waiting)
        );
    }

    #[test]
    fn test_map_separator_variants_equivalent() {
        let canonical = map_event_to_status("turn.completed");
        assert_eq!(map_event_to_status("turn-completed"), canonical);
        assert_eq!(map_event_to_status("turn_completed"), canonical);
    }

    #[test]
    fn test_map_unknown_event_returns_none() {
        assert_eq!(map_event_to_status("random-unrelated-event"), None);
        assert_eq!(map_event_to_status(""), None);
    }
}
