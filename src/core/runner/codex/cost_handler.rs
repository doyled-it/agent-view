//! Codex rollout JSONL parser → cost events.
//!
//! Reads `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` (or `.zst`)
//! and emits per-turn `CodexCostEvent` records. Dedupe is required because
//! Codex emits duplicate token_count snapshots (ccusage issue #884) — we
//! track the previous `total_token_usage` per-file and skip events where it
//! hasn't advanced.

#![allow(dead_code)] // module awaiting wiring in future tasks

use serde::{Deserialize, Serialize};
use std::path::Path;

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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
