//! `agent-view hook` subcommand — invoked by Claude Code on each state
//! transition. Writes per-session status JSON; on Stop events also writes
//! per-event cost JSON.
//!
//! All errors are silent (handler always exits 0) so Claude Code is never
//! blocked. The pure parsing/mapping helpers below are unit-tested; the
//! I/O orchestrator is in `run()`.

use crate::core::paths;
use crate::core::runner::hook_io::{
    atomic_write, read_payload_from_stdin, validate_instance_id, HookStatusFile, MAX_PAYLOAD_BYTES,
};
use crate::core::storage::CostEvent;
use crate::types::SessionStatus;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Lexically normalize a path: collapses `.` and `..` components without
/// touching the filesystem. Symlinks are not resolved (intentional — we
/// don't want a symlink under ~/.claude to legitimize a target outside it,
/// but we also don't want filesystem access in a security-critical check).
fn lexical_normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Validate that a transcript path resolves (lexically) to a location under
/// `~/.claude`. Returns the normalized path on success. Rejects relative
/// paths, paths that traverse out of `~/.claude` via `..`, and paths whose
/// home directory cannot be resolved.
///
/// Lexical-only normalization is deliberate: filesystem-based canonicalize
/// would require the file to exist (a TOCTOU on Claude's transcript writes)
/// and would resolve symlinks, which could legitimize paths an attacker
/// planted as symlinks in `~/.claude`.
pub fn validate_transcript_path(raw: &str) -> Option<PathBuf> {
    let p = PathBuf::from(raw);
    if !p.is_absolute() {
        return None;
    }
    let normalized = lexical_normalize(&p);
    let home = dirs::home_dir()?;
    let claude_root = lexical_normalize(&home.join(".claude"));
    if !normalized.starts_with(&claude_root) {
        return None;
    }
    Some(normalized)
}

/// Find the last `type: "assistant"` line in a JSONL transcript whose
/// payload contains usage data. Real Claude transcripts often end with a
/// `type: "system"` marker (session summary, compaction note, etc.) so we
/// can't rely on `read_last_jsonl_line` here — we must scan backward past
/// any trailing non-assistant lines.
///
/// Reads only the trailing 256 KiB of the file (consistent with
/// `read_last_jsonl_line`); the assistant message produced by the most
/// recent turn is essentially always within the last few KiB.
pub fn find_last_assistant_line(path: &Path) -> Option<String> {
    let buf = read_tail(path)?;
    for line in buf.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Cheap pre-filter to skip JSON parsing for obvious non-matches.
        if !trimmed.contains("\"type\":\"assistant\"") {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<TranscriptLine>(trimmed) {
            if parsed.line_type == "assistant"
                && parsed
                    .message
                    .as_ref()
                    .and_then(|m| m.usage.as_ref())
                    .is_some()
            {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn read_tail(path: &Path) -> Option<String> {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};

    const TAIL_BYTES: u64 = 256 * 1024;

    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let read_from = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(read_from)).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    Some(buf)
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
        // Microdollars are computed at ingest time by `event_watcher`, not
        // here — the hook subprocess deliberately stays free of config
        // loading and rate tables.
        cost_microdollars: 0,
    })
}

/// True when `AGENT_VIEW_HOOK_DEBUG` is set to a non-empty value. When on,
/// the hook handler emits step-by-step trace lines on stderr — useful when
/// Claude Code reports `Hook command exited with code N` or when status
/// updates aren't reaching the TUI.
fn debug_enabled() -> bool {
    std::env::var("AGENT_VIEW_HOOK_DEBUG")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

fn dbg(msg: &str) {
    if debug_enabled() {
        eprintln!("agent-view hook: {}", msg);
    }
}

/// Entrypoint: called from main.rs when argv[1] == "hook". Always exits 0.
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
        "event={} session={:?} transcript={:?}",
        payload.hook_event_name, payload.session_id, payload.transcript_path
    ));

    if let Err(e) = paths::ensure_event_dirs() {
        dbg(&format!("ensure_event_dirs failed: {}", e));
        return None;
    }

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
            tool_session_id: claude_sid.trim().to_string(),
            event: payload.hook_event_name.clone(),
            ts: chrono::Utc::now().timestamp(),
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
    } else {
        dbg(&format!(
            "event {} did not map to a status; not writing hook file",
            payload.hook_event_name
        ));
    }

    // On Stop events with a transcript_path, also write a cost event.
    if payload.hook_event_name == "Stop" {
        let raw = match payload.transcript_path.as_deref() {
            Some(r) => r,
            None => {
                dbg("Stop event without transcript_path; skipping cost write");
                return Some(());
            }
        };
        let p = match validate_transcript_path(raw) {
            Some(p) => p,
            None => {
                dbg(&format!("transcript_path rejected by validator: {}", raw));
                return Some(());
            }
        };
        let line = match find_last_assistant_line(&p) {
            Some(l) => l,
            None => {
                dbg(&format!(
                    "transcript {} contains no assistant line with usage in tail window",
                    p.display()
                ));
                return Some(());
            }
        };
        let event = match cost_event_from_transcript_line(&line, &instance_id) {
            Some(e) => e,
            None => {
                dbg("found assistant line but cost_event_from_transcript_line rejected it");
                return Some(());
            }
        };
        let filename = format!("{}_{}.json", instance_id, event.ts);
        let bytes = serde_json::to_vec(&event).ok()?;
        let path = paths::cost_events_dir().join(filename);
        match atomic_write(&path, &bytes) {
            Ok(()) => dbg(&format!(
                "wrote cost in={} out={} -> {}",
                event.input_tokens,
                event.output_tokens,
                path.display()
            )),
            Err(e) => dbg(&format!("atomic_write cost failed: {}", e)),
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
        let mut st = s.serialize_struct("CostEvent", 8)?;
        st.serialize_field("session_id", &self.session_id)?;
        st.serialize_field("model", &self.model)?;
        st.serialize_field("input_tokens", &self.input_tokens)?;
        st.serialize_field("output_tokens", &self.output_tokens)?;
        st.serialize_field("cache_read_tokens", &self.cache_read_tokens)?;
        st.serialize_field("cache_creation_tokens", &self.cache_creation_tokens)?;
        st.serialize_field("ts", &self.ts)?;
        st.serialize_field("cost_microdollars", &self.cost_microdollars)?;
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
    fn test_validate_transcript_path_rejects_relative() {
        assert!(validate_transcript_path(".claude/projects/x/s.jsonl").is_none());
        assert!(validate_transcript_path("relative.jsonl").is_none());
    }

    #[test]
    fn test_validate_transcript_path_accepts_under_claude() {
        let home = dirs::home_dir().unwrap();
        let ok = home.join(".claude/projects/abc/sess.jsonl");
        let res = validate_transcript_path(ok.to_str().unwrap());
        assert!(res.is_some());
    }

    #[test]
    fn test_validate_transcript_path_accepts_double_dot_in_segment() {
        // A segment like "some..proj" is a single path component, NOT a
        // traversal — must be accepted. Earlier substring-based rejection
        // would have falsely blocked this.
        let home = dirs::home_dir().unwrap();
        let ok = home.join(".claude/projects/some..proj/sess.jsonl");
        let res = validate_transcript_path(ok.to_str().unwrap());
        assert!(res.is_some(), "double-dot inside a segment must be allowed");
    }

    #[test]
    fn test_validate_transcript_path_normalizes_redundant_components() {
        // ~/.claude/./projects/x/sess.jsonl normalizes to ~/.claude/projects/x/sess.jsonl
        let home = dirs::home_dir().unwrap();
        let raw = format!("{}/.claude/./projects/x/sess.jsonl", home.display());
        let res = validate_transcript_path(&raw).unwrap();
        assert_eq!(res, home.join(".claude/projects/x/sess.jsonl"));
    }

    #[test]
    fn test_find_last_assistant_line_handles_large_file() {
        // The tail-window read must reach back far enough to find the last
        // assistant line in a >256 KiB transcript.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jsonl");
        let mut contents = String::new();
        let asst = r#"{"type":"assistant","message":{"model":"m","usage":{"input_tokens":1,"output_tokens":2}}}"#;
        for _ in 0..3_000 {
            contents.push_str(asst);
            contents.push('\n');
        }
        contents.push_str(r#"{"type":"system","subtype":"summary"}"#);
        contents.push('\n');
        std::fs::write(&path, &contents).unwrap();
        assert!(
            contents.len() > 256 * 1024,
            "test setup invariant: file must exceed the tail-read window (got {} bytes)",
            contents.len()
        );
        assert!(find_last_assistant_line(&path).is_some());
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

    #[test]
    fn test_find_last_assistant_line_skips_trailing_system() {
        // Reproduces the real-world bug: Claude transcripts often end with
        // a `type: "system"` marker after the final assistant message. The
        // hook handler must walk back past it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let user = r#"{"type":"user","message":{"role":"user","content":"hi"}}"#;
        let asst = r#"{"type":"assistant","message":{"model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":2}}}"#;
        let sys = r#"{"type":"system","subtype":"summary"}"#;
        std::fs::write(&path, format!("{}\n{}\n{}\n", user, asst, sys)).unwrap();

        let line = find_last_assistant_line(&path).expect("should find the assistant line");
        let event = cost_event_from_transcript_line(&line, "sess-X").unwrap();
        assert_eq!(event.input_tokens, 1);
        assert_eq!(event.output_tokens, 2);
    }

    #[test]
    fn test_find_last_assistant_line_picks_most_recent_when_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let older = r#"{"type":"assistant","message":{"model":"m","usage":{"input_tokens":10,"output_tokens":100}}}"#;
        let newer = r#"{"type":"assistant","message":{"model":"m","usage":{"input_tokens":20,"output_tokens":200}}}"#;
        let sys = r#"{"type":"system","subtype":"x"}"#;
        std::fs::write(&path, format!("{}\n{}\n{}\n", older, newer, sys)).unwrap();

        let line = find_last_assistant_line(&path).unwrap();
        let event = cost_event_from_transcript_line(&line, "s").unwrap();
        assert_eq!(event.input_tokens, 20);
        assert_eq!(event.output_tokens, 200);
    }

    #[test]
    fn test_find_last_assistant_line_skips_assistant_without_usage() {
        // A streaming-in-progress entry might lack usage; we should keep
        // walking back to find one that has it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let with_usage = r#"{"type":"assistant","message":{"model":"m","usage":{"input_tokens":5,"output_tokens":50}}}"#;
        let no_usage = r#"{"type":"assistant","message":{"model":"m"}}"#;
        std::fs::write(&path, format!("{}\n{}\n", with_usage, no_usage)).unwrap();

        let line = find_last_assistant_line(&path).unwrap();
        let event = cost_event_from_transcript_line(&line, "s").unwrap();
        assert_eq!(event.input_tokens, 5);
    }

    #[test]
    fn test_find_last_assistant_line_none_when_no_assistant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, "{\"type\":\"user\",\"message\":{}}\n").unwrap();
        assert!(find_last_assistant_line(&path).is_none());
    }
}
