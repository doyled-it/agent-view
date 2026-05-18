//! Codex rollout JSONL parser → cost events.
//!
//! Reads `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` (or `.zst`)
//! and emits per-turn `CodexCostEvent` records. Dedupe is required because
//! Codex emits duplicate token_count snapshots (ccusage issue #884) — we
//! track the previous `total_token_usage` per-file and skip events where it
//! hasn't advanced.

#![allow(dead_code)] // module awaiting wiring in future tasks

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Cost event extracted from a Codex rollout file. Mirrors the on-disk JSON
/// format of `core::storage::CostEvent` so it can be serialized straight to
/// `~/.agent-orchestrator/cost-events/<id>_<ts>.json` and ingested by
/// `event_watcher` without a separate Codex code path on the storage side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexCostEvent {
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Maps from Codex's `cached_input_tokens` (the cache-hit subset of
    /// input_tokens). Codex does NOT report cache-creation separately, so
    /// `cache_creation_tokens` stays 0.
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub ts: i64, // unix nanos
}

/// A single-turn token snapshot, used for dedupe and for delta-from-cumulative
/// fallback when `last_token_usage` is missing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenSnapshot {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

/// Per-rollout-file state persisted across runs so we don't re-emit cost
/// events on restart. Keyed by agent-view session id, stored under
/// `~/.agent-orchestrator/rollout-state/<id>.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CodexRolloutState {
    /// Byte offset to resume reading from next time.
    pub file_offset: u64,
    /// Most-recent `turn_context.payload.model` seen in this file.
    pub last_seen_model: Option<String>,
    /// Previous `total_token_usage` snapshot — used to detect duplicate
    /// snapshots (ccusage #884) and to derive deltas when `last_token_usage`
    /// is absent.
    pub last_total_token_usage: Option<TokenSnapshot>,
}

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct RolloutLine {
    #[serde(rename = "type", default)]
    line_type: String,
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TurnContextPayload {
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventMsgPayload {
    #[serde(rename = "type", default)]
    payload_type: String,
    #[serde(default)]
    info: Option<TokenCountInfo>,
}

#[derive(Debug, Deserialize)]
struct TokenCountInfo {
    #[serde(default)]
    last_token_usage: Option<TokenSnapshot>,
    #[serde(default)]
    total_token_usage: Option<TokenSnapshot>,
}

/// Open the rollout file, transparently decompressing if the path ends in
/// `.zst`. Plain `.jsonl` files are seeked to `seek_offset` for resumption;
/// `.zst` streams are non-seekable so the whole file is re-read each time.
fn open_rollout(path: &Path, seek_offset: u64) -> Option<Box<dyn BufRead>> {
    let mut file = File::open(path).ok()?;
    let is_zst = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zst"));
    if is_zst {
        // zstd streams aren't randomly-seekable; we accept re-reading the
        // whole file each time. The dedupe set covers the rest.
        let decoder = zstd::Decoder::new(file).ok()?;
        Some(Box::new(BufReader::new(decoder)))
    } else {
        file.seek(SeekFrom::Start(seek_offset)).ok()?;
        Some(Box::new(BufReader::new(file)))
    }
}

fn consume_lines<R: BufRead>(
    reader: R,
    agent_view_session_id: &str,
    state: &mut CodexRolloutState,
) -> (Vec<CodexCostEvent>, u64) {
    let mut events = Vec::new();
    let mut bytes_read: u64 = 0;
    for line_result in reader.lines() {
        let Ok(line) = line_result else { continue };
        // Codex rollout files are LF-only; +1 accounts for the stripped '\n'.
        bytes_read += line.len() as u64 + 1;
        let Ok(parsed) = serde_json::from_str::<RolloutLine>(&line) else {
            continue;
        };
        match parsed.line_type.as_str() {
            "turn_context" => {
                if let Some(model) = parsed
                    .payload
                    .and_then(|p| serde_json::from_value::<TurnContextPayload>(p).ok())
                    .and_then(|tc| tc.model)
                {
                    state.last_seen_model = Some(model);
                }
            }
            "event_msg" => {
                let Some(payload) = parsed.payload else {
                    continue;
                };
                let Ok(ev_payload) = serde_json::from_value::<EventMsgPayload>(payload) else {
                    continue;
                };
                if ev_payload.payload_type != "token_count" {
                    continue;
                }
                let Some(info) = ev_payload.info else {
                    continue;
                };
                // Resolve the per-turn snapshot: prefer last_token_usage, fall
                // back to (total - previous_total) when only cumulative is
                // present.
                let total = match info.total_token_usage {
                    Some(t) => t,
                    None => continue, // No way to dedupe or derive delta.
                };
                // ccusage #884 dedupe: skip events where cumulative hasn't
                // advanced since the last emitted event.
                if Some(total) == state.last_total_token_usage {
                    continue;
                }
                let delta = match info.last_token_usage {
                    Some(last) => last,
                    None => subtract(total, state.last_total_token_usage.unwrap_or_default()),
                };
                let Some(model) = state.last_seen_model.clone() else {
                    // No model context yet — record the cumulative and skip
                    // emission. Common at session start.
                    state.last_total_token_usage = Some(total);
                    continue;
                };
                let ts_nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as i64)
                    .unwrap_or(0);
                events.push(CodexCostEvent {
                    session_id: agent_view_session_id.to_string(),
                    model,
                    input_tokens: delta.input_tokens,
                    output_tokens: delta.output_tokens,
                    cache_read_tokens: delta.cached_input_tokens,
                    cache_creation_tokens: 0,
                    ts: ts_nanos,
                });
                state.last_total_token_usage = Some(total);
            }
            _ => {}
        }
    }
    (events, bytes_read)
}

/// Parse new lines from `path` starting at `state.file_offset`. Updates
/// `state` in place (file_offset, last_seen_model, last_total_token_usage).
/// Returns one `CodexCostEvent` per non-duplicate `token_count` event with a
/// resolved model. Lines that fail to parse are skipped silently — the
/// rollout format may evolve and we don't want a stray line to halt ingest.
pub fn parse_new_events(
    path: &Path,
    agent_view_session_id: &str,
    state: &mut CodexRolloutState,
) -> Vec<CodexCostEvent> {
    let is_zst = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zst"));
    if is_zst {
        // Non-resumable; reset offset so reader returns from start.
        state.file_offset = 0;
    }
    let Some(reader) = open_rollout(path, state.file_offset) else {
        return Vec::new();
    };
    let (events, bytes_read) = consume_lines(reader, agent_view_session_id, state);
    if !is_zst {
        state.file_offset += bytes_read;
    }
    events
}

/// Rate-limit summary derived from the most recent token_count event in a
/// rollout file. Mirrors the shape Codex writes under
/// `payload.rate_limits` — either a primary/secondary window pair or, for
/// credits-mode plans with no published limit, an "Unlimited (preview)"
/// state.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RateLimitInfo {
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
    /// True when the plan has credits-mode billing and no specific limit has
    /// been published yet (e.g. Codex business preview).
    pub unlimited_preview: bool,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RateLimitWindow {
    #[serde(default)]
    pub used_percent: Option<f64>,
    #[serde(default)]
    pub window_minutes: Option<i64>,
    #[serde(default)]
    pub resets_in_seconds: Option<i64>,
}

/// Most recent rate_limits block from a token_count event. Returns None if
/// no such event exists in the file.
pub fn current_rate_limits(path: &Path) -> Option<RateLimitInfo> {
    let reader = open_rollout(path, 0)?;
    let mut latest: Option<serde_json::Value> = None;
    for line in reader.lines().map_while(Result::ok) {
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if parsed.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
            continue;
        }
        let Some(payload) = parsed.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(|v| v.as_str()) != Some("token_count") {
            continue;
        }
        if let Some(rl) = payload.get("rate_limits") {
            latest = Some(rl.clone());
        }
    }
    let raw = latest?;
    let primary: Option<RateLimitWindow> = raw
        .get("primary")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let secondary: Option<RateLimitWindow> = raw
        .get("secondary")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let has_credits = raw
        .get("credits")
        .and_then(|c| c.get("has_credits"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let unlimited_preview = primary.is_none() && secondary.is_none() && has_credits;
    let plan_type = raw
        .get("plan_type")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some(RateLimitInfo {
        primary,
        secondary,
        unlimited_preview,
        plan_type,
    })
}

/// Return the `total_tokens` from the most recent `token_count` event with a
/// non-null `total_token_usage` in this rollout file. Reads the whole file
/// (rollouts are bounded — context-window-sized — so this is cheap). Returns
/// `None` if no token_count event has been written yet.
pub fn current_context_tokens(path: &Path) -> Option<i64> {
    let reader = open_rollout(path, 0)?;
    let mut latest: Option<i64> = None;
    for line in reader.lines().map_while(Result::ok) {
        let Ok(parsed) = serde_json::from_str::<RolloutLine>(&line) else {
            continue;
        };
        if parsed.line_type != "event_msg" {
            continue;
        }
        let Some(payload) = parsed.payload else {
            continue;
        };
        let Ok(ev) = serde_json::from_value::<EventMsgPayload>(payload) else {
            continue;
        };
        if ev.payload_type != "token_count" {
            continue;
        }
        if let Some(info) = ev.info {
            if let Some(total) = info.total_token_usage {
                latest = Some(total.total_tokens);
            }
        }
    }
    latest
}

/// Atomic state-file write — write to a sibling tmp then rename.
pub fn save_rollout_state(path: &Path, state: &CodexRolloutState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Load state; returns Default on missing-file (treated as a fresh rollout).
/// Returns an error only for I/O failures other than NotFound, or for parse
/// errors on a present-but-malformed file.
pub fn load_rollout_state(path: &Path) -> std::io::Result<CodexRolloutState> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodexRolloutState::default());
        }
        Err(e) => return Err(e),
    };
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

/// Find the rollout file for a given Codex thread id, by walking the
/// sessions directory and matching the uuid component of each filename.
/// Returns the path on first match. Walks recursively because the layout
/// is `YYYY/MM/DD/rollout-*.jsonl[.zst]`.
pub fn find_rollout_for_thread(thread_id: &str, sessions_root: &Path) -> Option<PathBuf> {
    fn visit(dir: &Path, thread_id: &str) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = visit(&path, thread_id) {
                    return Some(found);
                }
            } else if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.starts_with("rollout-") && name.contains(thread_id) {
                    return Some(path);
                }
            }
        }
        None
    }
    visit(sessions_root, thread_id)
}

fn subtract(a: TokenSnapshot, b: TokenSnapshot) -> TokenSnapshot {
    TokenSnapshot {
        input_tokens: (a.input_tokens - b.input_tokens).max(0),
        cached_input_tokens: (a.cached_input_tokens - b.cached_input_tokens).max(0),
        output_tokens: (a.output_tokens - b.output_tokens).max(0),
        reasoning_output_tokens: (a.reasoning_output_tokens - b.reasoning_output_tokens).max(0),
        total_tokens: (a.total_tokens - b.total_tokens).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_default_is_fresh() {
        let s = CodexRolloutState::default();
        assert_eq!(s.file_offset, 0);
        assert!(s.last_seen_model.is_none());
        assert!(s.last_total_token_usage.is_none());
    }

    #[test]
    fn event_struct_carries_all_fields() {
        let ev = CodexCostEvent {
            session_id: "av-sess".to_string(),
            model: "gpt-5.5".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 30,
            cache_creation_tokens: 0,
            ts: 1_700_000_000_000_000_000,
        };
        assert_eq!(ev.model, "gpt-5.5");
    }

    use std::io::Write;

    fn write_jsonl(lines: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-test.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        (dir, path)
    }

    #[test]
    fn parse_emits_one_event_per_token_count() {
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"2026-05-14T22:28:23Z","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.5"}}"#,
            r#"{"timestamp":"2026-05-14T22:28:32Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#,
        ]);
        let mut state = CodexRolloutState::default();
        let events = parse_new_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev.session_id, "av-sess");
        assert_eq!(ev.model, "gpt-5.5");
        assert_eq!(ev.input_tokens, 100);
        assert_eq!(ev.output_tokens, 20);
        assert_eq!(ev.cache_read_tokens, 40);
        assert_eq!(ev.cache_creation_tokens, 0);
        assert!(state.file_offset > 0);
    }

    #[test]
    fn parse_skips_token_count_with_null_info() {
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":null}}"#,
        ]);
        let mut state = CodexRolloutState::default();
        let events = parse_new_events(&path, "av-sess", &mut state);
        assert!(events.is_empty(), "null-info events must be skipped");
    }

    #[test]
    fn parse_dedupes_unchanged_total_snapshot() {
        // Codex sometimes emits duplicate token_count events where
        // total_token_usage hasn't advanced. ccusage #884.
        let same = r#"{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}"#;
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            &format!(
                r#"{{"timestamp":"...","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{},"last_token_usage":{}}}}}}}"#,
                same, same
            ),
            &format!(
                r#"{{"timestamp":"...","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{},"last_token_usage":{}}}}}}}"#,
                same, same
            ),
        ]);
        let mut state = CodexRolloutState::default();
        let events = parse_new_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 1, "second duplicate must be skipped");
    }

    #[test]
    fn parse_derives_delta_from_cumulative_when_last_missing() {
        // First event: cumulative total = 100 in, 20 out.
        // Second event: cumulative total = 150 in, 30 out — last_token_usage
        // absent, so we derive the delta (50 in, 10 out).
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":20,"reasoning_output_tokens":0,"total_tokens":120},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":20,"reasoning_output_tokens":0,"total_tokens":120}}}}"#,
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"cached_input_tokens":0,"output_tokens":30,"reasoning_output_tokens":0,"total_tokens":180}}}}"#,
        ]);
        let mut state = CodexRolloutState::default();
        let events = parse_new_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].input_tokens, 50);
        assert_eq!(events[1].output_tokens, 10);
    }

    #[test]
    fn parse_skips_token_count_before_first_turn_context() {
        // Common at session start: token_count fires before turn_context.
        // Without a model we can't emit a cost event — record state and skip.
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":50},"last_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":50}}}}"#,
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":30,"reasoning_output_tokens":0,"total_tokens":130},"last_token_usage":{"input_tokens":50,"cached_input_tokens":0,"output_tokens":30,"reasoning_output_tokens":0,"total_tokens":80}}}}"#,
        ]);
        let mut state = CodexRolloutState::default();
        let events = parse_new_events(&path, "av-sess", &mut state);
        // Only the second token_count (after turn_context) emits.
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "gpt-5.5");
        assert_eq!(events[0].input_tokens, 50);
    }

    #[test]
    fn current_rate_limits_detects_unlimited_preview() {
        let (_dir, path) = write_jsonl(&[
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","plan_type":"business","primary":null,"secondary":null,"credits":{"has_credits":true,"unlimited":false,"balance":null}}}}"#,
        ]);
        let rl = current_rate_limits(&path).unwrap();
        assert!(rl.unlimited_preview);
        assert!(rl.primary.is_none());
        assert_eq!(rl.plan_type.as_deref(), Some("business"));
    }

    #[test]
    fn current_rate_limits_parses_primary_window() {
        let (_dir, path) = write_jsonl(&[
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","plan_type":"business","primary":{"used_percent":42.5,"window_minutes":300,"resets_in_seconds":1500},"secondary":null,"credits":{"has_credits":true,"unlimited":false,"balance":null}}}}"#,
        ]);
        let rl = current_rate_limits(&path).unwrap();
        assert!(!rl.unlimited_preview);
        assert_eq!(rl.primary.as_ref().unwrap().used_percent, Some(42.5));
        assert_eq!(rl.primary.as_ref().unwrap().window_minutes, Some(300));
    }

    #[test]
    fn current_context_tokens_returns_last_total() {
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#,
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":250,"cached_input_tokens":80,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":300},"last_token_usage":{"input_tokens":150,"cached_input_tokens":40,"output_tokens":30,"reasoning_output_tokens":5,"total_tokens":180}}}}"#,
        ]);
        assert_eq!(current_context_tokens(&path), Some(300));
    }

    #[test]
    fn current_context_tokens_none_when_no_token_count() {
        let (_dir, path) = write_jsonl(&[
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
        ]);
        assert_eq!(current_context_tokens(&path), None);
    }

    #[test]
    fn rollout_state_roundtrips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("av-sess.json");
        let original = CodexRolloutState {
            file_offset: 12345,
            last_seen_model: Some("gpt-5.5".to_string()),
            last_total_token_usage: Some(TokenSnapshot {
                input_tokens: 100,
                cached_input_tokens: 40,
                output_tokens: 20,
                reasoning_output_tokens: 5,
                total_tokens: 120,
            }),
        };
        save_rollout_state(&state_path, &original).unwrap();
        let loaded = load_rollout_state(&state_path).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn load_rollout_state_returns_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("nope.json");
        let loaded = load_rollout_state(&state_path).unwrap();
        assert_eq!(loaded, CodexRolloutState::default());
    }

    #[test]
    fn find_rollout_returns_matching_file() {
        let dir = tempfile::tempdir().unwrap();
        let day = dir.path().join("2026").join("05").join("14");
        std::fs::create_dir_all(&day).unwrap();
        let target =
            day.join("rollout-2026-05-14T22-27-25-019e289a-0f2d-73f1-94d3-d15182ff1741.jsonl");
        std::fs::write(&target, "{}").unwrap();
        let unrelated =
            day.join("rollout-2026-05-14T22-30-00-deadbeef-0000-0000-0000-000000000000.jsonl");
        std::fs::write(&unrelated, "{}").unwrap();

        let found = find_rollout_for_thread("019e289a-0f2d-73f1-94d3-d15182ff1741", dir.path());
        assert_eq!(found, Some(target));
    }

    #[test]
    fn find_rollout_returns_none_when_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let day = dir.path().join("2026").join("05").join("14");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(
            day.join("rollout-2026-05-14T22-30-00-deadbeef-0000-0000-0000-000000000000.jsonl"),
            "{}",
        )
        .unwrap();
        let found = find_rollout_for_thread("not-a-real-uuid", dir.path());
        assert!(found.is_none());
    }

    #[test]
    fn parse_handles_zstd_compressed_rollout() {
        // Write a real .jsonl.zst, confirm parser reads it transparently.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-test.jsonl.zst");
        let payload = concat!(
            r#"{"timestamp":"...","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            "\n",
            r#"{"timestamp":"...","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":42,"cached_input_tokens":0,"output_tokens":7,"reasoning_output_tokens":0,"total_tokens":49},"last_token_usage":{"input_tokens":42,"cached_input_tokens":0,"output_tokens":7,"reasoning_output_tokens":0,"total_tokens":49}}}}"#,
            "\n",
        );
        let compressed = zstd::encode_all(payload.as_bytes(), 3).unwrap();
        std::fs::write(&path, compressed).unwrap();

        let mut state = CodexRolloutState::default();
        let events = parse_new_events(&path, "av-sess", &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input_tokens, 42);
        assert_eq!(events[0].output_tokens, 7);
    }
}
