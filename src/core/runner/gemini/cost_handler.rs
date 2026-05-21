//! Gemini session-JSON parser → cost events.
//!
//! Reads `~/.gemini/tmp/<sha256_hex_of_realpath_cwd>/chats/session-*-<id8>.json`
//! and emits one cost-event per new `type: "gemini"` message. Dedupes by
//! message-array index — Gemini rewrites the whole session document on each
//! save, so a byte-offset resume isn't applicable.
//!
//! Schema is from agent-deck's `internal/session/gemini.go` (the reference
//! consumer):
//!
//! ```json
//! {
//!   "sessionId": "<uuid>",
//!   "startTime": "...",
//!   "lastUpdated": "...",
//!   "messages": [
//!     { "type": "user", ... },
//!     { "type": "gemini", "model": "gemini-2.5-pro",
//!       "tokens": { "input": 100, "output": 50 } },
//!     ...
//!   ]
//! }
//! ```
//!
//! Each `type: "gemini"` message represents one model turn whose input
//! tokens cover the cumulative conversation history (Gemini sends the full
//! transcript per call). Per-turn billing tokens map straight onto
//! `(input_tokens, output_tokens)`; Gemini's session JSON doesn't expose a
//! cache-read or cache-creation split, so those stay zero — pricing the
//! row at the full input rate is a slight overestimate but is the safer
//! direction (we never under-report cost).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Canonical UUID shape. Same regex as Codex's `is_valid_thread_id`; reused
/// here for defence-in-depth against a hostile hook payload.
static SESSION_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .expect("static regex must compile")
});

pub fn is_valid_session_id(s: &str) -> bool {
    SESSION_ID_RE.is_match(s)
}

/// Cost event extracted from a Gemini session JSON. Matches the on-disk
/// wire format consumed by `event_watcher::deserialize_cost_event`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeminiCostEvent {
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Gemini's session JSON doesn't expose cache splits — kept at 0 for
    /// schema parity with the Codex/Claude cost-event shape.
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub ts: i64, // unix nanos
}

/// Per-session state persisted across runs so we don't re-emit cost events
/// on restart. Stored under `~/.agent-orchestrator/rollout-state/<av_sid>.json`
/// (shares Codex's directory — keyed by agent-view session id, one tool
/// per session, so no collision).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeminiSessionState {
    /// Number of messages already processed. Next call starts at this index.
    pub last_processed_message_count: usize,
    /// Most-recent `model` seen on a gemini-type message — used as the
    /// model for any subsequent gemini messages that omit the field.
    pub last_seen_model: Option<String>,
}

/// Find Gemini's session file for `session_id` by walking every project
/// hash under `~/.gemini/tmp/<hash>/chats/`. We can't precompute the right
/// hash from agent-view because Gemini may have been launched from a
/// different cwd than where the hook fires; the cross-project walk
/// (mirrors agent-deck's `findGeminiSessionInAllProjects`) is the robust
/// path. Rejects non-canonical session ids.
pub fn find_session_for_id(session_id: &str, gemini_root: &Path) -> Option<PathBuf> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    // Filename pattern: `session-YYYY-MM-DDTHH-MM-<uuid8>.json` where
    // uuid8 is the first 8 hex chars of the full session UUID.
    let id8 = &session_id[..8];
    let tmp_dir = gemini_root.join("tmp");
    let entries = std::fs::read_dir(&tmp_dir).ok()?;
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for project_entry in entries.flatten() {
        if !project_entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let chats = project_entry.path().join("chats");
        let chat_iter = match std::fs::read_dir(&chats) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for file_entry in chat_iter.flatten() {
            let path = file_entry.path();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.starts_with("session-") || !name.ends_with(".json") || !name.contains(id8) {
                continue;
            }
            let Ok(meta) = file_entry.metadata() else {
                continue;
            };
            let Ok(mtime) = meta.modified() else { continue };
            match &best {
                None => best = Some((path, mtime)),
                Some((_, prev)) if mtime > *prev => best = Some((path, mtime)),
                _ => {}
            }
        }
    }
    best.map(|(p, _)| p)
}

#[derive(Debug, Deserialize)]
struct SessionDoc {
    #[serde(default)]
    messages: Vec<SessionMessage>,
}

#[derive(Debug, Deserialize)]
struct SessionMessage {
    #[serde(rename = "type", default)]
    msg_type: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tokens: Option<MessageTokens>,
}

#[derive(Debug, Deserialize, Default)]
struct MessageTokens {
    #[serde(default)]
    input: i64,
    #[serde(default)]
    output: i64,
}

/// Parse the session JSON at `path`; emit one `GeminiCostEvent` per new
/// `type: "gemini"` message at index `>= state.last_processed_message_count`.
/// Updates `state` in place (last_processed_message_count, last_seen_model).
///
/// Messages whose `tokens` are absent or zero are skipped — they're either
/// streaming intermediates or non-billing rows. Non-gemini messages
/// (`type: "user"` etc.) advance the cursor but don't emit.
pub fn parse_new_cost_events(
    path: &Path,
    agent_view_session_id: &str,
    state: &mut GeminiSessionState,
) -> Vec<GeminiCostEvent> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let doc: SessionDoc = match serde_json::from_slice(&bytes) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let ts_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);

    let mut events = Vec::new();
    let start = state.last_processed_message_count;
    for (idx, msg) in doc.messages.iter().enumerate().skip(start) {
        if msg.msg_type == "gemini" {
            if let Some(m) = msg.model.clone() {
                state.last_seen_model = Some(m);
            }
            if let Some(tokens) = &msg.tokens {
                if tokens.input > 0 || tokens.output > 0 {
                    if let Some(model) = state.last_seen_model.clone() {
                        // Spread synthetic timestamps so a single replay
                        // doesn't collide on filename `{sid}_{ts}.json`.
                        let event_ts = ts_nanos + idx as i64;
                        events.push(GeminiCostEvent {
                            session_id: agent_view_session_id.to_string(),
                            model,
                            input_tokens: tokens.input,
                            output_tokens: tokens.output,
                            cache_read_tokens: 0,
                            cache_creation_tokens: 0,
                            ts: event_ts,
                        });
                    }
                    // else: no model resolved yet — skip emission, but
                    // still advance the cursor so we don't re-attempt next
                    // tick. agent-deck's analytics-from-disk behaves the
                    // same way: a tokens-without-model row is non-billable.
                }
            }
        }
        state.last_processed_message_count = idx + 1;
    }
    events
}

/// Atomic save of `state` to `path`. Mirrors codex/cost_handler's helper —
/// shared state directory but per-session state shape.
pub fn save_state(path: &Path, state: &GeminiSessionState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Load state; missing-file = fresh session (default state).
pub fn load_state(path: &Path) -> std::io::Result<GeminiSessionState> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(GeminiSessionState::default());
        }
        Err(e) => return Err(e),
    };
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_session(dir: &Path, id8: &str, content: &str) -> PathBuf {
        let chats = dir.join("chats");
        fs::create_dir_all(&chats).unwrap();
        let path = chats.join(format!("session-2026-05-20T10-00-{}.json", id8));
        fs::write(&path, content).unwrap();
        path
    }

    fn make_gemini_root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn is_valid_session_id_accepts_canonical_uuids() {
        assert!(is_valid_session_id("019e289a-0f2d-73f1-94d3-d15182ff1741"));
        assert!(is_valid_session_id("4d8fcb4d-1234-5678-90ab-cdef01234567"));
    }

    #[test]
    fn is_valid_session_id_rejects_malformed() {
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("not-a-uuid"));
        assert!(!is_valid_session_id("../etc/passwd"));
        assert!(!is_valid_session_id(
            "g19e289a-0f2d-73f1-94d3-d15182ff1741" // non-hex
        ));
    }

    #[test]
    fn find_session_locates_file_under_any_project_hash() {
        let root = make_gemini_root();
        let tmp = root.path().join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        // Two project hashes — only one has the session.
        let project_a = tmp.join("aaaa1111");
        let project_b = tmp.join("bbbb2222");
        fs::create_dir_all(&project_a).unwrap();
        write_session(&project_a, "4d8fcb4d", "{\"messages\":[]}");
        fs::create_dir_all(&project_b).unwrap();
        write_session(&project_b, "ffffffff", "{\"messages\":[]}");

        let found = find_session_for_id("4d8fcb4d-1234-5678-90ab-cdef01234567", root.path());
        assert!(found.is_some());
        assert!(found.unwrap().to_string_lossy().contains("aaaa1111"));
    }

    #[test]
    fn find_session_returns_none_when_id_absent() {
        let root = make_gemini_root();
        fs::create_dir_all(root.path().join("tmp").join("aaaa1111").join("chats")).unwrap();
        assert!(find_session_for_id("00000000-0000-0000-0000-000000000001", root.path()).is_none());
    }

    #[test]
    fn find_session_rejects_invalid_id_without_walking_fs() {
        let root = make_gemini_root();
        assert!(find_session_for_id("../escape", root.path()).is_none());
        assert!(find_session_for_id("", root.path()).is_none());
    }

    #[test]
    fn parse_emits_one_event_per_new_gemini_message() {
        let root = make_gemini_root();
        let path = write_session(
            root.path(),
            "4d8fcb4d",
            r#"{
                "sessionId": "4d8fcb4d-1234-5678-90ab-cdef01234567",
                "messages": [
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 1000, "output": 250 } },
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 1500, "output": 400 } }
                ]
            }"#,
        );
        let mut state = GeminiSessionState::default();
        let events = parse_new_cost_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].input_tokens, 1000);
        assert_eq!(events[0].output_tokens, 250);
        assert_eq!(events[0].model, "gemini-2.5-pro");
        assert_eq!(events[1].input_tokens, 1500);
        assert_eq!(events[1].output_tokens, 400);
        assert_eq!(state.last_processed_message_count, 4);
        assert_eq!(state.last_seen_model.as_deref(), Some("gemini-2.5-pro"));
    }

    #[test]
    fn parse_resumes_from_cursor_no_re_emit() {
        let root = make_gemini_root();
        let path = write_session(
            root.path(),
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 200, "output": 100 } }
                ]
            }"#,
        );
        let mut state = GeminiSessionState::default();
        let first = parse_new_cost_events(&path, "av-sess", &mut state);
        assert_eq!(first.len(), 2);
        let second = parse_new_cost_events(&path, "av-sess", &mut state);
        assert!(
            second.is_empty(),
            "second call with unchanged file must not re-emit"
        );
    }

    #[test]
    fn parse_picks_up_new_messages_after_resume() {
        let root = make_gemini_root();
        let path = write_session(
            root.path(),
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } }
                ]
            }"#,
        );
        let mut state = GeminiSessionState::default();
        let first = parse_new_cost_events(&path, "av-sess", &mut state);
        assert_eq!(first.len(), 1);

        // Simulate the session file being rewritten with a new turn.
        fs::write(
            &path,
            r#"{
                "messages": [
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } },
                    { "type": "user" },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 250, "output": 80 } }
                ]
            }"#,
        )
        .unwrap();
        let second = parse_new_cost_events(&path, "av-sess", &mut state);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].input_tokens, 250);
    }

    #[test]
    fn parse_skips_messages_with_zero_tokens() {
        let root = make_gemini_root();
        let path = write_session(
            root.path(),
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 0, "output": 0 } },
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } }
                ]
            }"#,
        );
        let mut state = GeminiSessionState::default();
        let events = parse_new_cost_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input_tokens, 100);
        // Cursor must still advance over the zero-token row.
        assert_eq!(state.last_processed_message_count, 2);
    }

    #[test]
    fn parse_skips_messages_with_no_model_yet() {
        let root = make_gemini_root();
        let path = write_session(
            root.path(),
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "gemini",
                      "tokens": { "input": 100, "output": 50 } }
                ]
            }"#,
        );
        let mut state = GeminiSessionState::default();
        let events = parse_new_cost_events(&path, "av-sess", &mut state);
        assert!(
            events.is_empty(),
            "tokens-without-model is non-billable and must not emit"
        );
        assert_eq!(state.last_processed_message_count, 1);
    }

    #[test]
    fn parse_carries_last_seen_model_to_later_messages() {
        let root = make_gemini_root();
        let path = write_session(
            root.path(),
            "4d8fcb4d",
            r#"{
                "messages": [
                    { "type": "gemini", "model": "gemini-2.5-pro",
                      "tokens": { "input": 100, "output": 50 } },
                    { "type": "gemini",
                      "tokens": { "input": 200, "output": 100 } }
                ]
            }"#,
        );
        let mut state = GeminiSessionState::default();
        let events = parse_new_cost_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].model, "gemini-2.5-pro");
    }

    #[test]
    fn parse_handles_missing_file_returns_empty() {
        let root = make_gemini_root();
        let path = root.path().join("does-not-exist.json");
        let mut state = GeminiSessionState::default();
        let events = parse_new_cost_events(&path, "av-sess", &mut state);
        assert!(events.is_empty());
        assert_eq!(state.last_processed_message_count, 0);
    }

    #[test]
    fn parse_handles_malformed_json_returns_empty() {
        let root = make_gemini_root();
        let path = write_session(root.path(), "4d8fcb4d", "not valid json");
        let mut state = GeminiSessionState::default();
        let events = parse_new_cost_events(&path, "av-sess", &mut state);
        assert!(events.is_empty());
    }

    #[test]
    fn state_roundtrips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("av-sess.json");
        let original = GeminiSessionState {
            last_processed_message_count: 7,
            last_seen_model: Some("gemini-2.5-pro".to_string()),
        };
        save_state(&path, &original).unwrap();
        let loaded = load_state(&path).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn load_state_returns_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_state(&dir.path().join("nope.json")).unwrap();
        assert_eq!(loaded, GeminiSessionState::default());
    }

    #[test]
    fn cost_event_serializes_to_wire_format() {
        // Pin the on-disk JSON shape — event_watcher::deserialize_cost_event
        // requires exactly these snake_case fields.
        let ev = GeminiCostEvent {
            session_id: "av-sess".to_string(),
            model: "gemini-2.5-pro".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts: 1_700_000_000_000_000_000,
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["session_id"], "av-sess");
        assert_eq!(json["model"], "gemini-2.5-pro");
        assert_eq!(json["input_tokens"], 100);
        assert_eq!(json["output_tokens"], 50);
        assert_eq!(json["cache_read_tokens"], 0);
        assert_eq!(json["cache_creation_tokens"], 0);
        assert_eq!(json["ts"], 1_700_000_000_000_000_000i64);
    }
}
