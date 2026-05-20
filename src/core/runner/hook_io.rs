//! Runner-agnostic hook I/O: the on-disk `HookStatusFile` struct, instance-id
//! validation, atomic writes, and stdin payload reading. Shared between
//! `claude::hook_handler` and (forthcoming) `codex::notify_handler`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

/// Max bytes accepted from a single hook stdin payload. Protects against a
/// runaway producer flooding the handler.
pub const MAX_PAYLOAD_BYTES: usize = 1 << 20; // 1 MiB

static INSTANCE_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_.\-]*$").expect("static regex must compile")
});

/// Status snapshot written to `{hooks_dir}/{instance_id}.json` by hook
/// handlers and consumed by `event_watcher`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookStatusFile {
    pub status: String,
    #[serde(
        default,
        alias = "claude_session_id",
        skip_serializing_if = "String::is_empty"
    )]
    pub tool_session_id: String,
    pub event: String,
    pub ts: i64, // unix seconds
    /// Claude transcript path (when known). Empty for Codex / non-Claude
    /// runners. Consumed by `event_watcher` to surface to UI panes that
    /// read the transcript directly (e.g. live context-size).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transcript_path: String,
}

/// Validate an `AGENT_VIEW_SESSION_ID` env value. Returns `true` if safe to
/// use as a filename component. Rejects empty / `..` / unusual chars.
pub fn validate_instance_id(id: &str) -> bool {
    if id.is_empty() || id.contains("..") {
        return false;
    }
    INSTANCE_ID_RE.is_match(id)
}

/// Atomic write: write `bytes` to a sibling `<file>.<ext>.tmp` (or `<file>.tmp`
/// when the path has no extension), then rename onto `path`. Returns the first
/// I/O error encountered. Parent dirs are created on demand.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(
        path.extension()
            .map(|e| format!("{}.tmp", e.to_string_lossy()))
            .unwrap_or_else(|| "tmp".to_string()),
    );
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read up to `MAX_PAYLOAD_BYTES + 1` from stdin. Returns the captured bytes,
/// or the underlying I/O error so callers can choose to log it (Claude's hook
/// handler debug-logs read errors; Codex's notify handler silently falls
/// through to argv).
pub fn read_payload_from_stdin() -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock().take(MAX_PAYLOAD_BYTES as u64 + 1);
    handle.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Process-wide mutex for tests that mutate environment variables. `cargo test`
/// runs tests in parallel within a binary, and `std::env::set_var` /
/// `remove_var` are global. Tests that touch `CODEX_HOME`, `CODEX_SESSION_ID`,
/// etc. should acquire this before mutating to avoid cross-test races.
#[cfg(test)]
pub(crate) static ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire the env test mutex, surviving poisoning (a panicked test should
/// not freeze the rest of the suite).
#[cfg(test)]
pub(crate) fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_hook_status_file_deserializes_legacy_claude_session_id() {
        // Old in-flight hook files written before the field rename used
        // `claude_session_id`. The serde alias must keep them readable.
        let legacy =
            br#"{"status":"waiting","claude_session_id":"legacy-sid","event":"Stop","ts":1700000000}"#;
        let parsed: HookStatusFile = serde_json::from_slice(legacy).unwrap();
        assert_eq!(parsed.tool_session_id, "legacy-sid");
        assert_eq!(parsed.status, "waiting");
        assert_eq!(parsed.event, "Stop");
    }

    #[test]
    fn test_hook_status_file_omits_empty_tool_session_id() {
        // skip_serializing_if keeps the JSON tidy when no sid was captured.
        let file = HookStatusFile {
            status: "running".to_string(),
            tool_session_id: String::new(),
            event: "turn.started".to_string(),
            ts: 1700000000,
            transcript_path: String::new(),
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("tool_session_id"), "got: {}", json);
        assert!(!json.contains("claude_session_id"), "got: {}", json);
    }
}
