//! `agent-view hook` subcommand — invoked by Claude Code on each state
//! transition. Writes per-session status JSON; on Stop events also writes
//! per-event cost JSON.
//!
//! All errors are silent (handler always exits 0) so Claude Code is never
//! blocked. The pure parsing/mapping helpers below are unit-tested; the
//! I/O orchestrator is in `run()`.

use crate::core::paths;
use crate::core::storage::CostEvent;
use crate::types::SessionStatus;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PAYLOAD_BYTES: usize = 1 << 20; // 1 MiB

static INSTANCE_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_.\-]*$").expect("static regex must compile")
});

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
pub fn notification_status(matcher: Option<&serde_json::Value>) -> Option<SessionStatus> {
    let m = matcher?.as_str()?;
    if m == "permission_prompt" || m == "elicitation_dialog" {
        Some(SessionStatus::Waiting)
    } else {
        None
    }
}

/// Parse JSON payload from raw bytes. Returns None on size cap or parse error.
pub fn parse_payload(data: &[u8]) -> Option<HookPayload> {
    if data.is_empty() || data.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    serde_json::from_slice(data).ok()
}

/// On-disk hook status file. `status` uses `SessionStatus::as_str()`.
#[derive(Debug, Serialize, Deserialize)]
pub struct HookStatusFile {
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claude_session_id: String,
    pub event: String,
    pub ts: i64, // unix seconds
}

#[derive(Debug, Deserialize)]
struct TranscriptUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
}

#[derive(Debug, Deserialize)]
struct TranscriptMessage {
    #[serde(default)]
    model: String,
    #[serde(default)]
    usage: Option<TranscriptUsage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(rename = "type", default)]
    line_type: String,
    #[serde(default)]
    message: Option<TranscriptMessage>,
}

/// Atomic write: write to `path.tmp` then rename. Returns error on any I/O failure.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(
        path.extension()
            .map(|e| format!("{}.tmp", e.to_string_lossy()))
            .unwrap_or_else(|| "tmp".to_string()),
    );
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Validate that a transcript path is under `~/.claude` after canonical
/// cleaning. Rejects `..` traversal. Returns the cleaned path on success.
pub fn validate_transcript_path(raw: &str) -> Option<PathBuf> {
    if raw.contains("..") {
        return None;
    }
    let p = PathBuf::from(raw);
    let home = dirs::home_dir()?;
    let claude_root = home.join(".claude");
    if !p.starts_with(&claude_root) {
        return None;
    }
    Some(p)
}

/// Read the last non-empty line of a JSONL transcript file. Reads the
/// whole file (transcripts are typically <10 MB; OK to load). Returns None
/// on read error or empty file.
pub fn read_last_jsonl_line(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    contents
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|s| s.to_string())
}

/// Build a CostEvent from a transcript last-line and an agent-view session
/// UUID. Returns None if the line is not an assistant message with usage.
pub fn cost_event_from_transcript_line(
    line: &str,
    agent_view_session_id: &str,
) -> Option<CostEvent> {
    let parsed: TranscriptLine = serde_json::from_str(line).ok()?;
    if parsed.line_type != "assistant" {
        return None;
    }
    let msg = parsed.message?;
    let usage = msg.usage?;
    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        return None;
    }
    let ts_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos() as i64;
    Some(CostEvent {
        session_id: agent_view_session_id.to_string(),
        model: msg.model,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_input_tokens,
        cache_creation_tokens: usage.cache_creation_input_tokens,
        ts: ts_nanos,
    })
}

/// Entrypoint: called from main.rs when argv[1] == "hook". Always exits 0.
pub fn run() {
    let _ = run_inner();
}

fn run_inner() -> Option<()> {
    let instance_id = std::env::var("AGENT_VIEW_SESSION_ID").ok()?;
    if !validate_instance_id(&instance_id) {
        return None;
    }

    let mut buf = Vec::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock().take(MAX_PAYLOAD_BYTES as u64 + 1);
    handle.read_to_end(&mut buf).ok()?;
    let payload = parse_payload(&buf)?;

    paths::ensure_event_dirs().ok()?;

    // Resolve status. For Notification events, gate on matcher.
    let status_opt = if payload.hook_event_name == "Notification" {
        notification_status(payload.matcher.as_ref())
    } else {
        map_event_to_status(&payload.hook_event_name)
    };

    if let Some(status) = status_opt {
        let claude_sid = payload.session_id.clone().unwrap_or_default();
        let file = HookStatusFile {
            status: status.as_str().to_string(),
            claude_session_id: claude_sid.trim().to_string(),
            event: payload.hook_event_name.clone(),
            ts: chrono::Utc::now().timestamp(),
        };
        let json = serde_json::to_vec(&file).ok()?;
        let path = paths::hooks_dir().join(format!("{}.json", instance_id));
        let _ = atomic_write(&path, &json);
    }

    // On Stop events with a transcript_path, also write a cost event.
    if payload.hook_event_name == "Stop" {
        if let Some(raw) = payload.transcript_path.as_deref() {
            if let Some(p) = validate_transcript_path(raw) {
                if let Some(line) = read_last_jsonl_line(&p) {
                    if let Some(event) = cost_event_from_transcript_line(&line, &instance_id) {
                        let filename = format!("{}_{}.json", instance_id, event.ts);
                        let bytes = serde_json::to_vec(&event).ok()?;
                        let path = paths::cost_events_dir().join(filename);
                        let _ = atomic_write(&path, &bytes);
                    }
                }
            }
        }
    }

    Some(())
}

// CostEvent isn't Serialize by default — add a Serialize impl on it
// in this file so the hook handler can write it without leaking the
// dependency into the storage module.
impl Serialize for CostEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CostEvent", 7)?;
        st.serialize_field("session_id", &self.session_id)?;
        st.serialize_field("model", &self.model)?;
        st.serialize_field("input_tokens", &self.input_tokens)?;
        st.serialize_field("output_tokens", &self.output_tokens)?;
        st.serialize_field("cache_read_tokens", &self.cache_read_tokens)?;
        st.serialize_field("cache_creation_tokens", &self.cache_creation_tokens)?;
        st.serialize_field("ts", &self.ts)?;
        st.end()
    }
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

    #[test]
    fn test_atomic_write_creates_file_with_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deep/file.json");
        atomic_write(&path, b"x").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_validate_transcript_path_rejects_outside_claude() {
        let home = dirs::home_dir().unwrap();
        let outside = home.join("Documents/secret.jsonl");
        assert!(validate_transcript_path(outside.to_str().unwrap()).is_none());
    }

    #[test]
    fn test_validate_transcript_path_rejects_traversal() {
        let home = dirs::home_dir().unwrap();
        let bad = format!("{}/.claude/../etc/passwd", home.display());
        assert!(validate_transcript_path(&bad).is_none());
    }

    #[test]
    fn test_validate_transcript_path_accepts_under_claude() {
        let home = dirs::home_dir().unwrap();
        let ok = home.join(".claude/projects/abc/sess.jsonl");
        let res = validate_transcript_path(ok.to_str().unwrap());
        assert!(res.is_some());
    }

    #[test]
    fn test_read_last_jsonl_line_skips_trailing_blank() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "{\"a\":1}\n{\"a\":2}\n\n").unwrap();
        assert_eq!(read_last_jsonl_line(&path).unwrap(), "{\"a\":2}");
    }

    #[test]
    fn test_read_last_jsonl_line_single_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "{\"a\":1}").unwrap();
        assert_eq!(read_last_jsonl_line(&path).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn test_read_last_jsonl_line_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.jsonl");
        std::fs::write(&path, "").unwrap();
        assert!(read_last_jsonl_line(&path).is_none());
    }

    #[test]
    fn test_cost_event_from_transcript_line_assistant_with_usage() {
        let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-7","usage":{"input_tokens":42,"output_tokens":1234,"cache_read_input_tokens":5,"cache_creation_input_tokens":7}}}"#;
        let event = cost_event_from_transcript_line(line, "sess-abc").unwrap();
        assert_eq!(event.session_id, "sess-abc");
        assert_eq!(event.model, "claude-opus-4-7");
        assert_eq!(event.input_tokens, 42);
        assert_eq!(event.output_tokens, 1234);
        assert_eq!(event.cache_read_tokens, 5);
        assert_eq!(event.cache_creation_tokens, 7);
    }

    #[test]
    fn test_cost_event_skips_non_assistant() {
        let line = r#"{"type":"user","message":{"model":"x","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        assert!(cost_event_from_transcript_line(line, "x").is_none());
    }

    #[test]
    fn test_cost_event_skips_zero_usage() {
        let line = r#"{"type":"assistant","message":{"model":"x","usage":{"input_tokens":0,"output_tokens":0}}}"#;
        assert!(cost_event_from_transcript_line(line, "x").is_none());
    }
}
