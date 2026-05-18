//! Entrypoint for the `agent-view codex-notify` argv subcommand. Codex
//! invokes this from its `notify` config line on lifecycle events.
//! Always exits 0 so a malformed payload never breaks Codex itself.
//!
//! `handle_notify_with_paths` is the testable core: given an already-parsed
//! payload value and explicit directory paths, it writes a cost-event JSON
//! file for each new token_count entry found in the matching rollout file.

use super::notify;
use crate::core::paths;
use crate::core::runner::codex::cost_handler::{
    find_rollout_for_thread, load_rollout_state, parse_new_events, save_rollout_state,
};
use crate::core::runner::hook_io::{
    atomic_write, read_payload_from_stdin, validate_instance_id, HookStatusFile,
};

/// Entrypoint called from main.rs when argv[1] == "codex-notify". Always
/// exits 0; failures (parse error, missing env, etc.) are silent so Codex
/// itself never sees a broken notify return.
pub fn run() {
    let _ = run_inner();
}

fn run_inner() -> Option<()> {
    let instance_id = std::env::var("AGENT_VIEW_SESSION_ID").ok()?;
    if !validate_instance_id(&instance_id) {
        return None;
    }

    // Codex may pass payload via stdin OR argv. Prefer stdin. A read error
    // is treated the same as an empty stdin — fall through to argv parsing
    // rather than aborting; Codex itself never sees a non-zero exit.
    let mut bytes = read_payload_from_stdin().unwrap_or_default();
    if bytes.is_empty() {
        // Look for the first JSON-shaped argv (starts with `{`).
        if let Some(arg) = std::env::args()
            .skip(2)
            .find(|a| a.trim_start().starts_with('{'))
        {
            bytes = arg.into_bytes();
        }
    }

    // If still empty, try plain event-name argv (Codex's older behavior).
    let (event, session_id) = if bytes.is_empty() {
        let event_arg = std::env::args()
            .skip(2)
            .find(|a| !a.trim().is_empty())
            .unwrap_or_default();
        (event_arg, String::new())
    } else {
        notify::parse_payload(&bytes)
    };

    let status = notify::map_event_to_status(&event)?;

    paths::ensure_event_dirs().ok()?;

    // Trim event/session_id defensively — Codex's notify payload is
    // free-form JSON with no documented field-shape contract, so values may
    // arrive with stray whitespace. (Claude's hook handler doesn't trim
    // because its payload comes from a structured `HookPayload` enum.)
    let file = HookStatusFile {
        status: status.as_str().to_string(),
        tool_session_id: session_id.trim().to_string(),
        event: event.trim().to_string(),
        ts: chrono::Utc::now().timestamp(),
        transcript_path: String::new(),
    };

    let json = serde_json::to_vec(&file).ok()?;
    let path = paths::hooks_dir().join(format!("{}.json", instance_id));
    let _ = atomic_write(&path, &json);

    // Cost-event side-effect: parse the rollout file for the thread and emit
    // one cost-event JSON per new token_count entry.
    let sessions_root = dirs::home_dir()
        .map(|h| h.join(".codex").join("sessions"))
        .unwrap_or_default();
    if let Ok(payload_value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
        handle_notify_with_paths(
            &instance_id,
            &payload_value,
            &sessions_root,
            &paths::cost_events_dir(),
            &paths::rollout_state_dir(),
        );
    }

    Some(())
}

/// Cost-event side-effect of a Codex notify call. Given an already-parsed
/// payload value and explicit directory paths, locates the rollout file for
/// the notify's `thread-id`, parses any new token_count entries, and writes
/// one cost-event JSON per new entry. Idempotent: per-session state tracks
/// the file offset and last cumulative snapshot so repeated calls don't
/// re-emit.
pub fn handle_notify_with_paths(
    agent_view_session_id: &str,
    payload: &serde_json::Value,
    sessions_root: &std::path::Path,
    cost_events_dir: &std::path::Path,
    state_dir: &std::path::Path,
) {
    let Some(thread_id) = payload.get("thread-id").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(rollout_path) = find_rollout_for_thread(thread_id, sessions_root) else {
        return;
    };
    let state_path = state_dir.join(format!("{}.json", agent_view_session_id));
    let mut state = load_rollout_state(&state_path).unwrap_or_default();
    let events = parse_new_events(&rollout_path, agent_view_session_id, &mut state);
    for event in &events {
        let filename = format!("{}_{}.json", agent_view_session_id, event.ts);
        let path = cost_events_dir.join(filename);
        if let Ok(bytes) = serde_json::to_vec(event) {
            let _ = atomic_write(&path, &bytes);
        }
    }
    let _ = save_rollout_state(&state_path, &state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_handler_emits_cost_event_when_rollout_has_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_root = dir.path().join("codex_sessions");
        let day = sessions_root.join("2026").join("05").join("18");
        std::fs::create_dir_all(&day).unwrap();
        let thread_id = "019e289a-0f2d-73f1-94d3-d15182ff1741";
        let rollout = day.join(format!("rollout-2026-05-18T10-00-00-{}.jsonl", thread_id));
        let content = concat!(
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            "\n",
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150}}}}"#,
            "\n",
        );
        std::fs::write(&rollout, content).unwrap();

        let cost_events_dir = dir.path().join("cost-events");
        std::fs::create_dir_all(&cost_events_dir).unwrap();
        let state_dir = dir.path().join("rollout-state");
        std::fs::create_dir_all(&state_dir).unwrap();

        let payload = serde_json::json!({
            "thread-id": thread_id,
            "type": "agent-turn-complete",
        });

        handle_notify_with_paths(
            "av-sess-test",
            &payload,
            &sessions_root,
            &cost_events_dir,
            &state_dir,
        );

        let files: Vec<_> = std::fs::read_dir(&cost_events_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .collect();
        assert_eq!(files.len(), 1, "expected exactly one cost-event JSON");
        let body: serde_json::Value =
            serde_json::from_slice(&std::fs::read(files[0].path()).unwrap()).unwrap();
        assert_eq!(body["session_id"], "av-sess-test");
        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["input_tokens"], 100);
        assert_eq!(body["output_tokens"], 50);
    }

    #[test]
    fn notify_handler_does_not_re_emit_on_repeat_call() {
        let dir = tempfile::tempdir().unwrap();
        let sessions_root = dir.path().join("codex_sessions");
        let day = sessions_root.join("2026").join("05").join("18");
        std::fs::create_dir_all(&day).unwrap();
        let thread_id = "019e289a-0f2d-73f1-94d3-d15182ff1741";
        let rollout = day.join(format!("rollout-2026-05-18T10-00-00-{}.jsonl", thread_id));
        let content = concat!(
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            "\n",
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150}}}}"#,
            "\n",
        );
        std::fs::write(&rollout, content).unwrap();

        let cost_events_dir = dir.path().join("cost-events");
        std::fs::create_dir_all(&cost_events_dir).unwrap();
        let state_dir = dir.path().join("rollout-state");
        std::fs::create_dir_all(&state_dir).unwrap();

        let payload = serde_json::json!({
            "thread-id": thread_id,
            "type": "agent-turn-complete",
        });

        handle_notify_with_paths(
            "av-sess-restart",
            &payload,
            &sessions_root,
            &cost_events_dir,
            &state_dir,
        );
        handle_notify_with_paths(
            "av-sess-restart",
            &payload,
            &sessions_root,
            &cost_events_dir,
            &state_dir,
        );

        let files: Vec<_> = std::fs::read_dir(&cost_events_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .collect();
        assert_eq!(files.len(), 1, "second call must not re-emit");
    }
}
