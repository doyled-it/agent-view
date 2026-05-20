//! `agent-view gemini-hook` subcommand — invoked by Gemini CLI on each
//! lifecycle event. Writes per-session status JSON.
//!
//! Inferred payload schema: Gemini emits the same JSON shape as Claude Code
//! (snake_case `hook_event_name`, `session_id`). agent-deck's hook receiver
//! handles both with one struct and one switch (see
//! `asheshgoplani/agent-deck:cmd/agent-deck/hook_handler.go::hookPayload`),
//! which is the basis for treating the shapes as identical. If a future
//! Gemini release diverges, the typed deserialization here will fail
//! cleanly and the handler will silently no-op.
//!
//! Unlike Claude's hook handler this does not extract cost events — Gemini
//! sessions are JSON (not JSONL like Claude transcripts) and the schema is
//! different enough that pricing extraction is its own follow-up. Hooks
//! only carry status here.
//!
//! All errors are silent (handler always exits 0) so Gemini is never blocked.

use crate::core::paths;
use crate::core::runner::hook_io::{
    atomic_write, read_payload_from_stdin, validate_instance_id, HookStatusFile, MAX_PAYLOAD_BYTES,
};
use crate::types::SessionStatus;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HookPayload {
    pub hook_event_name: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Map a Gemini CLI hook event to a `SessionStatus`.
///
/// Status semantics:
/// - `SessionStart` → Idle: just configured, at prompt awaiting first input.
/// - `BeforeAgent`  → Running: turn started, model is processing.
/// - `AfterAgent`   → Idle: turn done, back at prompt.
/// - `SessionEnd`   → Idle: pane closing; the poller's session-exists check
///   handles the actual tmux pane death transition.
pub fn map_event_to_status(event: &str) -> Option<SessionStatus> {
    match event {
        "SessionStart" | "AfterAgent" | "SessionEnd" => Some(SessionStatus::Idle),
        "BeforeAgent" => Some(SessionStatus::Running),
        _ => None,
    }
}

pub fn parse_payload(data: &[u8]) -> Option<HookPayload> {
    if data.is_empty() || data.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    serde_json::from_slice(data).ok()
}

fn debug_enabled() -> bool {
    std::env::var("AGENT_VIEW_HOOK_DEBUG")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

fn dbg(msg: &str) {
    if debug_enabled() {
        eprintln!("agent-view gemini-hook: {}", msg);
    }
}

/// Entrypoint: called from main.rs when argv[1] == "gemini-hook". Always exits 0.
pub fn run() {
    let _ = run_inner();
}

fn run_inner() -> Option<()> {
    let instance_id = match std::env::var("AGENT_VIEW_SESSION_ID") {
        Ok(v) => v,
        Err(_) => {
            dbg("AGENT_VIEW_SESSION_ID env not set; skipping");
            return None;
        }
    };
    if !validate_instance_id(&instance_id) {
        dbg(&format!("invalid AGENT_VIEW_SESSION_ID: {:?}", instance_id));
        return None;
    }

    let buf = match read_payload_from_stdin() {
        Ok(b) => b,
        Err(e) => {
            dbg(&format!("stdin read error: {}", e));
            return None;
        }
    };
    let payload = match parse_payload(&buf) {
        Some(p) => p,
        None => {
            dbg(&format!("payload parse failed (size={})", buf.len()));
            return None;
        }
    };
    dbg(&format!(
        "event={} session={:?}",
        payload.hook_event_name, payload.session_id
    ));

    if let Err(e) = paths::ensure_event_dirs() {
        dbg(&format!("ensure_event_dirs failed: {}", e));
        return None;
    }

    let status = match map_event_to_status(&payload.hook_event_name) {
        Some(s) => s,
        None => {
            dbg(&format!(
                "event {} did not map to a status; not writing hook file",
                payload.hook_event_name
            ));
            return Some(());
        }
    };

    let gemini_sid = payload.session_id.clone().unwrap_or_default();
    let file = HookStatusFile {
        status: status.as_str().to_string(),
        tool_session_id: gemini_sid.trim().to_string(),
        event: payload.hook_event_name.clone(),
        ts: chrono::Utc::now().timestamp(),
        transcript_path: String::new(),
    };
    let json = serde_json::to_vec(&file).ok()?;
    let path = paths::hooks_dir().join(format!("{}.json", instance_id));
    match atomic_write(&path, &json) {
        Ok(()) => dbg(&format!(
            "wrote status={} -> {}",
            status.as_str(),
            path.display()
        )),
        Err(e) => dbg(&format!("atomic_write status failed: {}", e)),
    }

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_event_to_status_known_events() {
        assert_eq!(
            map_event_to_status("SessionStart"),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            map_event_to_status("BeforeAgent"),
            Some(SessionStatus::Running)
        );
        assert_eq!(map_event_to_status("AfterAgent"), Some(SessionStatus::Idle));
        assert_eq!(map_event_to_status("SessionEnd"), Some(SessionStatus::Idle));
    }

    #[test]
    fn test_map_event_to_status_unknown_returns_none() {
        assert_eq!(map_event_to_status("Stop"), None);
        assert_eq!(map_event_to_status("UserPromptSubmit"), None);
        assert_eq!(map_event_to_status("MysteryFutureEvent"), None);
        assert_eq!(map_event_to_status(""), None);
    }

    #[test]
    fn test_parse_payload_minimal() {
        let raw = br#"{"hook_event_name":"BeforeAgent"}"#;
        let p = parse_payload(raw).unwrap();
        assert_eq!(p.hook_event_name, "BeforeAgent");
        assert!(p.session_id.is_none());
    }

    #[test]
    fn test_parse_payload_with_session_id() {
        let raw = br#"{"hook_event_name":"AfterAgent","session_id":"4d8fcb4d-1234-5678-90ab-cdef01234567"}"#;
        let p = parse_payload(raw).unwrap();
        assert_eq!(p.hook_event_name, "AfterAgent");
        assert_eq!(
            p.session_id.as_deref(),
            Some("4d8fcb4d-1234-5678-90ab-cdef01234567")
        );
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

    #[test]
    fn test_parse_payload_ignores_unknown_fields() {
        // Forward-compat: extra fields Gemini may add later (e.g.
        // `transcript_path`, `model`, `matcher`) must not break parsing.
        let raw = br#"{"hook_event_name":"BeforeAgent","session_id":"s","extra":"ignored","future_field":42}"#;
        let p = parse_payload(raw).unwrap();
        assert_eq!(p.hook_event_name, "BeforeAgent");
        assert_eq!(p.session_id.as_deref(), Some("s"));
    }
}
