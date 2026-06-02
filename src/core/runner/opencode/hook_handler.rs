//! `agent-view opencode-hook` subcommand — invoked by the OpenCode plugin
//! installed into `~/.config/opencode/plugins/agent-view.js`. The plugin
//! normalizes OpenCode event objects into a small JSON payload, and this
//! handler writes the latest per-session status file consumed by
//! `event_watcher`.

use crate::core::runner::hook_io::{
    atomic_write, read_payload_from_stdin, validate_instance_id, HookStatusFile, MAX_PAYLOAD_BYTES,
};
use crate::types::SessionStatus;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPayload {
    pub event: String,
    pub session_id: Option<String>,
    pub status: Option<String>,
}

pub fn parse_payload(data: &[u8]) -> Option<HookPayload> {
    if data.is_empty() || data.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(data).ok()?;
    let event = string_field(&value, &["event", "type"])
        .or_else(|| nested_string_field(&value, "properties", &["event", "type"]))?;
    let session_id = string_field(&value, &["session_id", "sessionID"])
        .or_else(|| nested_string_field(&value, "properties", &["session_id", "sessionID"]))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let status = status_field(value.get("status"))
        .or_else(|| status_field(value.get("properties").and_then(|p| p.get("status"))));

    Some(HookPayload {
        event: event.trim().to_string(),
        session_id,
        status,
    })
}

pub fn map_event_to_status(event: &str, status: Option<&str>) -> Option<SessionStatus> {
    let event = event.trim().to_ascii_lowercase();
    let status = status.map(|s| s.trim().to_ascii_lowercase());
    match event.as_str() {
        "session.status" => match status.as_deref() {
            Some("busy") | Some("active") | Some("running") => Some(SessionStatus::Running),
            Some("idle") => Some(SessionStatus::Idle),
            Some("retry") | Some("error") => Some(SessionStatus::Error),
            _ => None,
        },
        "session.created" | "session.idle" => Some(SessionStatus::Idle),
        "session.compacted" => Some(SessionStatus::Compacting),
        "session.error" => Some(SessionStatus::Error),
        "permission.asked" | "permission.updated" => Some(SessionStatus::Waiting),
        "permission.replied" => Some(SessionStatus::Running),
        _ => None,
    }
}

pub fn handle_payload_with_paths(instance_id: &str, data: &[u8], hooks_dir: &Path) {
    if !validate_instance_id(instance_id) {
        return;
    }
    let Some(payload) = parse_payload(data) else {
        return;
    };
    let Some(status) = map_event_to_status(&payload.event, payload.status.as_deref()) else {
        return;
    };

    let file = HookStatusFile {
        status: status.as_str().to_string(),
        tool_session_id: payload.session_id.unwrap_or_default(),
        event: payload.event,
        ts: chrono::Utc::now().timestamp(),
        transcript_path: String::new(),
    };
    if let Ok(json) = serde_json::to_vec(&file) {
        let path = hooks_dir.join(format!("{}.json", instance_id));
        let _ = atomic_write(&path, &json);
    }
}

pub fn run(args: &[String]) {
    let _ = run_inner(args);
}

fn run_inner(args: &[String]) -> Option<()> {
    let instance_id = std::env::var("AGENT_VIEW_SESSION_ID").ok()?;
    if !validate_instance_id(&instance_id) {
        return None;
    }

    let payload = if args.is_empty() {
        read_payload_from_stdin().ok()?
    } else {
        args.join(" ").into_bytes()
    };

    if crate::core::paths::ensure_event_dirs().is_err() {
        return None;
    }
    handle_payload_with_paths(&instance_id, &payload, &crate::core::paths::hooks_dir());
    Some(())
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    let obj = value.as_object()?;
    for key in keys {
        if let Some(s) = obj.get(*key).and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn nested_string_field(value: &Value, object_key: &str, keys: &[&str]) -> Option<String> {
    string_field(value.get(object_key)?, keys)
}

fn status_field(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        Value::Object(obj) => obj.get("type").and_then(|v| v.as_str()).and_then(|s| {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runner::hook_io::HookStatusFile;
    use crate::types::SessionStatus;
    use std::fs;

    #[test]
    fn map_session_status_busy_is_running() {
        assert_eq!(
            map_event_to_status("session.status", Some("busy")),
            Some(SessionStatus::Running)
        );
    }

    #[test]
    fn map_session_status_idle_is_idle() {
        assert_eq!(
            map_event_to_status("session.status", Some("idle")),
            Some(SessionStatus::Idle)
        );
    }

    #[test]
    fn map_session_status_retry_is_error() {
        assert_eq!(
            map_event_to_status("session.status", Some("retry")),
            Some(SessionStatus::Error)
        );
    }

    #[test]
    fn map_session_idle_is_idle() {
        assert_eq!(
            map_event_to_status("session.idle", None),
            Some(SessionStatus::Idle)
        );
    }

    #[test]
    fn map_permission_asked_is_waiting() {
        assert_eq!(
            map_event_to_status("permission.asked", None),
            Some(SessionStatus::Waiting)
        );
    }

    #[test]
    fn map_session_error_is_error() {
        assert_eq!(
            map_event_to_status("session.error", None),
            Some(SessionStatus::Error)
        );
    }

    #[test]
    fn parse_plugin_payload_extracts_event_status_and_session() {
        let payload = parse_payload(
            br#"{"event":"session.status","session_id":"ses_abc123","status":"busy"}"#,
        )
        .unwrap();
        assert_eq!(payload.event, "session.status");
        assert_eq!(payload.session_id.as_deref(), Some("ses_abc123"));
        assert_eq!(payload.status.as_deref(), Some("busy"));
    }

    #[test]
    fn handle_payload_writes_hook_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        handle_payload_with_paths(
            "agent-view-session",
            br#"{"event":"session.status","session_id":"ses_abc123","status":"busy"}"#,
            &hooks,
        );

        let raw = fs::read(hooks.join("agent-view-session.json")).unwrap();
        let file: HookStatusFile = serde_json::from_slice(&raw).unwrap();
        assert_eq!(file.status, "running");
        assert_eq!(file.tool_session_id, "ses_abc123");
        assert_eq!(file.event, "session.status");
        assert_eq!(file.transcript_path, "");
    }

    #[test]
    fn handle_payload_ignores_unknown_event() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        handle_payload_with_paths(
            "agent-view-session",
            br#"{"event":"file.edited","session_id":"ses_abc123"}"#,
            &hooks,
        );

        assert!(!hooks.join("agent-view-session.json").exists());
    }
}
