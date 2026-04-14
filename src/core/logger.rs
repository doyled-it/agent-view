//! Continuous session logging — streams tmux pane output to log files

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024; // 10MB
const MAX_ROTATIONS: usize = 2;

pub fn log_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    home.join(".agent-view").join("logs")
}

pub fn session_log_path(session_id: &str) -> PathBuf {
    log_dir().join(format!("{}.log", session_id))
}

pub fn append_to_log(path: &Path, content: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

pub fn rotate_if_needed(path: &Path, max_size: u64) -> Result<(), std::io::Error> {
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    if metadata.len() <= max_size {
        return Ok(());
    }

    // Delete oldest rotation
    let oldest = format!("{}.{}", path.display(), MAX_ROTATIONS);
    let _ = fs::remove_file(&oldest);

    // Shift rotations down
    for i in (1..MAX_ROTATIONS).rev() {
        let from = format!("{}.{}", path.display(), i);
        let to = format!("{}.{}", path.display(), i + 1);
        let _ = fs::rename(&from, &to);
    }

    // Rotate current
    let rotated = format!("{}.1", path.display());
    fs::rename(path, &rotated)?;

    Ok(())
}

/// Captures output from all active sessions and appends to their log files.
/// Tracks last captured line count to avoid duplicating content.
pub struct SessionLogger {
    last_line_counts: HashMap<String, usize>,
}

impl SessionLogger {
    pub fn new() -> Self {
        Self {
            last_line_counts: HashMap::new(),
        }
    }

    pub fn capture_and_log(&mut self, tmux_session: &str, session_id: &str) {
        let output = match crate::core::tmux::capture_pane(tmux_session, Some(-10000)) {
            Ok(o) => o,
            Err(_) => return,
        };

        let lines: Vec<&str> = output.lines().collect();
        let total_lines = lines.len();
        let last_count = self.last_line_counts.get(session_id).copied().unwrap_or(0);

        if total_lines <= last_count {
            return;
        }

        let new_lines = &lines[last_count..];
        let new_content = new_lines.join("\n") + "\n";

        let log_path = session_log_path(session_id);
        let _ = append_to_log(&log_path, &new_content);
        let _ = rotate_if_needed(&log_path, MAX_LOG_SIZE);

        self.last_line_counts
            .insert(session_id.to_string(), total_lines);
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.last_line_counts.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_append_to_log() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("test.log");
        append_to_log(&log_path, "hello world\n").unwrap();
        append_to_log(&log_path, "second line\n").unwrap();
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("hello world"));
        assert!(content.contains("second line"));
    }

    #[test]
    fn test_rotate_log() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("test.log");
        let chunk = "x".repeat(1024 * 1024);
        for _ in 0..11 {
            append_to_log(&log_path, &chunk).unwrap();
        }
        rotate_if_needed(&log_path, 10 * 1024 * 1024).unwrap();
        assert!(dir.path().join("test.log.1").exists());
    }

    #[test]
    fn test_session_log_path_format() {
        let path = session_log_path("abc-123");
        assert!(path.to_string_lossy().contains("abc-123.log"));
        assert!(path.to_string_lossy().contains(".agent-view"));
    }

    #[test]
    fn test_rotate_noop_when_small() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("test.log");
        append_to_log(&log_path, "small content\n").unwrap();
        rotate_if_needed(&log_path, 10 * 1024 * 1024).unwrap();
        assert!(!dir.path().join("test.log.1").exists());
        assert!(log_path.exists());
    }

    #[test]
    fn test_rotate_noop_when_missing() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("nonexistent.log");
        rotate_if_needed(&log_path, 10 * 1024 * 1024).unwrap();
    }
}
