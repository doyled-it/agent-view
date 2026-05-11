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
    #[serde(default, alias = "claude_session_id")]
    pub tool_session_id: String,
    pub event: String,
    pub ts: i64, // unix seconds
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

/// Read up to `MAX_PAYLOAD_BYTES + 1` from stdin. Returns the captured bytes
/// (empty on read error or when stdin is empty).
pub fn read_payload_from_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock().take(MAX_PAYLOAD_BYTES as u64 + 1);
    let _ = handle.read_to_end(&mut buf);
    buf
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
}
