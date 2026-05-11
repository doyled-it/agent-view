//! Entrypoint for the `agent-view codex-notify` argv subcommand. Codex
//! invokes this from its `notify` config line on lifecycle events.
//! Always exits 0 so a malformed payload never breaks Codex itself.

use super::notify;
use crate::core::paths;
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

    // Codex may pass payload via stdin OR argv. Prefer stdin.
    let mut bytes = read_payload_from_stdin();
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

    let file = HookStatusFile {
        status: status.as_str().to_string(),
        tool_session_id: session_id.trim().to_string(),
        event: event.trim().to_string(),
        ts: chrono::Utc::now().timestamp(),
    };

    let json = serde_json::to_vec(&file).ok()?;
    let path = paths::hooks_dir().join(format!("{}.json", instance_id));
    let _ = atomic_write(&path, &json);
    Some(())
}
