//! `agent-view gemini-hook` subcommand — invoked by Gemini CLI on each
//! lifecycle event. Writes a per-session status JSON and, on turn-end
//! events, walks the Gemini session JSON for any new `type: "gemini"`
//! messages and emits one cost-event JSON per turn.
//!
//! Inferred payload schema: Gemini emits the same JSON shape as Claude Code
//! (snake_case `hook_event_name`, `session_id`). agent-deck's hook receiver
//! handles both with one struct and one switch (see
//! `asheshgoplani/agent-deck:cmd/agent-deck/hook_handler.go::hookPayload`),
//! which is the basis for treating the shapes as identical. If a future
//! Gemini release diverges, the typed deserialization here will fail
//! cleanly and the handler will silently no-op.
//!
//! Cost extraction triggers on `AfterAgent` (turn done) and `SessionEnd`
//! (final flush). Session JSON locator + parser live in `cost_handler.rs`;
//! the entrypoint here just wires payload → status file → optional cost
//! sweep. The architecture mirrors Codex's `notify_handler` + `cost_handler`
//! split.
//!
//! All errors are silent (handler always exits 0) so Gemini is never blocked.

use crate::core::paths;
use crate::core::runner::gemini::cost_handler::{
    find_session_for_id, is_valid_session_id, load_state, parse_new_cost_events, save_state,
    GeminiCostEvent,
};
use crate::core::runner::hook_io::{
    atomic_write, read_payload_from_stdin, validate_instance_id, HookStatusFile, MAX_PAYLOAD_BYTES,
};
use crate::types::SessionStatus;
use serde::Deserialize;
use std::path::Path;

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

/// True for events that should trigger a cost-event sweep of the Gemini
/// session JSON. We sweep on `AfterAgent` (typical turn boundary) and
/// `SessionEnd` (catches anything not yet flushed at the previous
/// AfterAgent — Gemini writes the session document asynchronously).
pub fn is_cost_sweep_event(event: &str) -> bool {
    matches!(event, "AfterAgent" | "SessionEnd")
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

    if let Some(status) = map_event_to_status(&payload.hook_event_name) {
        let gemini_sid = payload.session_id.clone().unwrap_or_default();
        let file = HookStatusFile {
            status: status.as_str().to_string(),
            tool_session_id: gemini_sid.trim().to_string(),
            event: payload.hook_event_name.clone(),
            ts: chrono::Utc::now().timestamp(),
            transcript_path: String::new(),
        };
        if let Ok(json) = serde_json::to_vec(&file) {
            let path = paths::hooks_dir().join(format!("{}.json", instance_id));
            match atomic_write(&path, &json) {
                Ok(()) => dbg(&format!(
                    "wrote status={} -> {}",
                    status.as_str(),
                    path.display()
                )),
                Err(e) => dbg(&format!("atomic_write status failed: {}", e)),
            }
        }
    } else {
        dbg(&format!(
            "event {} did not map to a status; not writing hook file",
            payload.hook_event_name
        ));
    }

    // Cost-event sweep on turn-end events. Skipped when the payload omits
    // session_id (no way to locate the session file).
    if is_cost_sweep_event(&payload.hook_event_name) {
        let gemini_root = dirs::home_dir().map(|h| h.join(".gemini"));
        if let (Some(gemini_sid), Some(gemini_root)) = (
            payload.session_id.as_ref().filter(|s| !s.is_empty()),
            gemini_root,
        ) {
            handle_cost_sweep_with_paths(
                &instance_id,
                gemini_sid,
                &gemini_root,
                &paths::cost_events_dir(),
                &paths::rollout_state_dir(),
            );
        } else {
            dbg("cost sweep skipped: no session_id or no home dir");
        }
    }

    Some(())
}

/// Cost-sweep side effect: locate the Gemini session JSON for `gemini_sid`,
/// parse any new `type: "gemini"` messages since the last call (state lives
/// at `state_dir/<agent_view_session_id>.json`), and write one cost-event
/// JSON per emitted message into `cost_events_dir`. Idempotent: repeated
/// calls with no new turns produce zero events.
pub fn handle_cost_sweep_with_paths(
    agent_view_session_id: &str,
    gemini_sid: &str,
    gemini_root: &Path,
    cost_events_dir: &Path,
    state_dir: &Path,
) {
    if !is_valid_session_id(gemini_sid) {
        dbg(&format!("invalid gemini session_id: {:?}", gemini_sid));
        return;
    }
    let Some(session_path) = find_session_for_id(gemini_sid, gemini_root) else {
        dbg(&format!(
            "no session file found for {} under {}",
            gemini_sid,
            gemini_root.display()
        ));
        return;
    };
    let state_path = state_dir.join(format!("{}.json", agent_view_session_id));
    let mut state = load_state(&state_path).unwrap_or_default();
    let events = parse_new_cost_events(&session_path, agent_view_session_id, &mut state);
    for event in &events {
        let filename = format!("{}_{}.json", agent_view_session_id, event.ts);
        let path = cost_events_dir.join(filename);
        if let Ok(bytes) = serde_json::to_vec::<GeminiCostEvent>(event) {
            let _ = atomic_write(&path, &bytes);
        }
    }
    if let Err(e) = save_state(&state_path, &state) {
        dbg(&format!("save_state failed: {}", e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
    fn test_is_cost_sweep_event() {
        // Only turn-end events trigger cost sweep (BeforeAgent is mid-turn,
        // SessionStart is pre-turn).
        assert!(is_cost_sweep_event("AfterAgent"));
        assert!(is_cost_sweep_event("SessionEnd"));
        assert!(!is_cost_sweep_event("BeforeAgent"));
        assert!(!is_cost_sweep_event("SessionStart"));
        assert!(!is_cost_sweep_event("Stop"));
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
        let raw = br#"{"hook_event_name":"BeforeAgent","session_id":"s","extra":"ignored","future_field":42}"#;
        let p = parse_payload(raw).unwrap();
        assert_eq!(p.hook_event_name, "BeforeAgent");
        assert_eq!(p.session_id.as_deref(), Some("s"));
    }

    fn write_gemini_session(gemini_root: &Path, id8: &str, body: &str) {
        let chats = gemini_root
            .join("tmp")
            .join("project-hash-aaa")
            .join("chats");
        fs::create_dir_all(&chats).unwrap();
        let path = chats.join(format!("session-2026-05-20T10-00-{}.json", id8));
        fs::write(&path, body).unwrap();
    }

    #[test]
    fn cost_sweep_emits_event_when_session_has_gemini_message() {
        let dir = tempfile::tempdir().unwrap();
        let gemini_root = dir.path().join("gemini");
        let cost_dir = dir.path().join("cost-events");
        let state_dir = dir.path().join("rollout-state");
        fs::create_dir_all(&cost_dir).unwrap();
        fs::create_dir_all(&state_dir).unwrap();

        let gemini_sid = "4d8fcb4d-1234-5678-90ab-cdef01234567";
        write_gemini_session(
            &gemini_root,
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } }
                ]
            }"#,
        );

        handle_cost_sweep_with_paths(
            "av-sess-cost",
            gemini_sid,
            &gemini_root,
            &cost_dir,
            &state_dir,
        );

        let files: Vec<_> = fs::read_dir(&cost_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .collect();
        assert_eq!(files.len(), 1, "expected exactly one cost-event file");
        let body: serde_json::Value =
            serde_json::from_slice(&fs::read(files[0].path()).unwrap()).unwrap();
        assert_eq!(body["session_id"], "av-sess-cost");
        assert_eq!(body["model"], "gemini-2.5-pro");
        assert_eq!(body["input_tokens"], 100);
        assert_eq!(body["output_tokens"], 50);
        // State file persists for next sweep.
        assert!(state_dir.join("av-sess-cost.json").exists());
    }

    #[test]
    fn cost_sweep_does_not_re_emit_on_repeat_call() {
        let dir = tempfile::tempdir().unwrap();
        let gemini_root = dir.path().join("gemini");
        let cost_dir = dir.path().join("cost-events");
        let state_dir = dir.path().join("rollout-state");
        fs::create_dir_all(&cost_dir).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        let gemini_sid = "4d8fcb4d-1234-5678-90ab-cdef01234567";
        write_gemini_session(
            &gemini_root,
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } }
                ]
            }"#,
        );

        handle_cost_sweep_with_paths(
            "av-sess-replay",
            gemini_sid,
            &gemini_root,
            &cost_dir,
            &state_dir,
        );
        handle_cost_sweep_with_paths(
            "av-sess-replay",
            gemini_sid,
            &gemini_root,
            &cost_dir,
            &state_dir,
        );

        let count = fs::read_dir(&cost_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .count();
        assert_eq!(count, 1, "second sweep must not re-emit");
    }

    #[test]
    fn cost_sweep_rejects_malformed_gemini_session_id() {
        // Defence-in-depth: bogus session_id mustn't trigger fs walks.
        let dir = tempfile::tempdir().unwrap();
        let gemini_root = dir.path().join("gemini");
        let cost_dir = dir.path().join("cost-events");
        let state_dir = dir.path().join("rollout-state");
        fs::create_dir_all(&cost_dir).unwrap();
        fs::create_dir_all(&state_dir).unwrap();

        for bogus in ["../etc/passwd", "", "not-a-uuid"] {
            handle_cost_sweep_with_paths("av-sess", bogus, &gemini_root, &cost_dir, &state_dir);
        }
        let count = fs::read_dir(&cost_dir).map(|d| d.count()).unwrap_or(0);
        assert_eq!(count, 0);
    }
}
