//! Tmux subprocess wrapper for session management

mod attach;
mod error;
mod inspect;
mod terminal;

pub use attach::{attach_conductor_workspace_sync, attach_session_sync};
pub use error::{TmuxError, TmuxResult};
pub use inspect::attach_inspect_session_sync;

use std::collections::HashMap;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Instant;

pub const SESSION_PREFIX: &str = "agentorch_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSessionInfo {
    pub name: String,
    pub attached: bool,
}

/// Cache of tmux session activity timestamps
pub struct SessionCache {
    data: HashMap<String, i64>,
    last_refresh: Instant,
}

impl SessionCache {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            last_refresh: Instant::now(),
        }
    }

    /// Refresh cache by querying tmux for all windows
    pub fn refresh(&mut self) {
        let output = Command::new("tmux")
            .args([
                "list-windows",
                "-a",
                "-F",
                "#{session_name}\t#{window_activity}",
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let mut new_data = HashMap::new();

                for line in stdout.trim().lines() {
                    if line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.splitn(2, '\t').collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let name = parts[0];
                    let activity: i64 = parts[1].parse().unwrap_or(0);
                    let existing = new_data.get(name).copied().unwrap_or(0);
                    if activity > existing {
                        new_data.insert(name.to_string(), activity);
                    }
                }

                self.data = new_data;
                self.last_refresh = Instant::now();
            }
            _ => {
                self.data.clear();
                self.last_refresh = Instant::now();
            }
        }
    }

    /// Check if a session exists in the cache
    pub fn session_exists(&self, name: &str) -> bool {
        self.data.contains_key(name)
    }

    /// Check if a session has recent activity
    pub fn is_session_active(&self, name: &str, threshold_seconds: i64) -> bool {
        if let Some(&activity) = self.data.get(name) {
            if activity == 0 {
                return false;
            }
            let now = chrono::Utc::now().timestamp();
            now - activity < threshold_seconds
        } else {
            false
        }
    }

    /// Register a newly created session in cache to prevent race conditions
    pub fn register(&mut self, name: &str) {
        let now = chrono::Utc::now().timestamp();
        self.data.insert(name.to_string(), now);
    }

    /// Remove a session from cache
    pub fn remove(&mut self, name: &str) {
        self.data.remove(name);
    }
}

/// Check if a tmux session exists
pub fn session_exists(name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if tmux is available on the system
pub fn is_tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List tmux sessions with their attachment state.
pub fn list_sessions() -> TmuxResult<Vec<TmuxSessionInfo>> {
    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_attached}",
        ])
        .output()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to list tmux sessions: {}", e)))?;

    if !output.status.success() {
        let detail = command_output_detail(&output.stdout, &output.stderr);
        if detail.contains("no server running") {
            return Ok(Vec::new());
        }
        return Err(TmuxError::CommandFailed(tmux_failure_message(
            "list-sessions",
            &output.status.to_string(),
            &output.stdout,
            &output.stderr,
        )));
    }

    Ok(parse_session_infos(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_session_infos(output: &str) -> Vec<TmuxSessionInfo> {
    output
        .lines()
        .filter_map(|line| {
            let (name, attached_count) = line.split_once('\t')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            let attached = attached_count.trim().parse::<u32>().unwrap_or(0) > 0;
            Some(TmuxSessionInfo {
                name: name.to_string(),
                attached,
            })
        })
        .collect()
}

/// Generate a unique tmux session name from a title
pub fn generate_session_name(title: &str) -> String {
    let safe: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let safe = safe.trim_matches('-');
    let safe = if safe.len() > 20 { &safe[..20] } else { safe };

    let timestamp = chrono::Utc::now().timestamp_millis();
    let ts_base36 = radix_string(timestamp as u64, 36);
    format!("{}{}-{}", SESSION_PREFIX, safe, ts_base36)
}

/// Convert a u64 to a base-36 string (matches JS Date.now().toString(36))
fn radix_string(mut n: u64, radix: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let chars: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
    let mut result = Vec::new();
    while n > 0 {
        result.push(chars[(n % radix) as usize]);
        n /= radix;
    }
    result.reverse();
    result.into_iter().collect()
}

/// Create a new tmux session.
///
/// Env vars in `env` are passed via `-e KEY=VALUE` on `new-session` so they
/// are set on the session BEFORE the initial pane's shell spawns — that
/// shell (and any process it later forks, like `claude`) inherits them.
/// They are ALSO replayed via `set-environment` afterwards so any future
/// panes opened in the same session (e.g., `split-window`) see them too.
///
/// Note: an earlier version of this function only used `set-environment`
/// after `new-session`, which silently failed to populate env for the
/// initial shell. That bug masked the AGENT_VIEW_SESSION_ID injection
/// added for the hook handler — see PR #51.
pub fn create_session(
    name: &str,
    command: Option<&str>,
    cwd: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> TmuxResult<()> {
    let cwd = cwd.unwrap_or("/tmp");

    // Step 1: Create detached session, with env baked into the initial pane.
    let mut args: Vec<String> = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        name.to_string(),
        "-c".to_string(),
        cwd.to_string(),
    ];
    if let Some(env_vars) = env {
        for (key, value) in env_vars {
            args.push("-e".to_string());
            args.push(format!("{}={}", key, value));
        }
    }
    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to spawn tmux: {}", e)))?;

    if !output.status.success() {
        return Err(TmuxError::CommandFailed(tmux_failure_message(
            "new-session",
            &output.status.to_string(),
            &output.stdout,
            &output.stderr,
        )));
    }

    // Step 2: Replay env into the session-level update-environment list so
    // any future panes (split-window, new-window) inherit it as well.
    if let Some(env_vars) = env {
        for (key, value) in env_vars {
            let _ = Command::new("tmux")
                .args(["set-environment", "-t", name, key, value])
                .status();
        }
    }

    // Step 3: Send command via send-keys
    if let Some(cmd) = command {
        let cmd_to_send = if cmd.contains("$(") || cmd.contains("session_id=") {
            let escaped = cmd.replace('\'', "'\"'\"'");
            format!("bash -c '{}'", escaped)
        } else {
            cmd.to_string()
        };

        send_keys(name, &cmd_to_send)?;
    }

    Ok(())
}

fn tmux_failure_message(command: &str, status: &str, stdout: &[u8], stderr: &[u8]) -> String {
    let mut message = format!("tmux {} failed with status {}", command, status);
    let detail = command_output_detail(stdout, stderr);
    if !detail.is_empty() {
        message.push_str(": ");
        message.push_str(&detail);
    }
    message
}

fn command_output_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, false) => format!("{}; stdout: {}", stderr, stdout),
        (false, true) => stderr,
        (true, false) => stdout,
        (true, true) => String::new(),
    }
}

/// Kill a tmux session
pub fn kill_session(name: &str) -> TmuxResult<()> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output();
    Ok(())
}

/// Send keys to a tmux session (followed by Enter)
pub fn send_keys(name: &str, keys: &str) -> TmuxResult<()> {
    let escaped = keys
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");

    let status = Command::new("tmux")
        .args(["send-keys", "-t", name, &escaped, "Enter"])
        .status()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to send keys: {}", e)))?;

    if !status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux send-keys failed with status {}",
            status
        )));
    }
    Ok(())
}

/// Send raw key names to a tmux session without appending Enter.
/// Use for special keys like "Right", "Left", "Escape", etc.
pub fn send_keys_raw(name: &str, keys: &str) -> TmuxResult<()> {
    let status = Command::new("tmux")
        .args(["send-keys", "-t", name, keys])
        .status()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to send keys: {}", e)))?;

    if !status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux send-keys failed with status {}",
            status
        )));
    }
    Ok(())
}

/// Capture pane content from a tmux session
/// Capture pane content from a tmux session.
/// If `escape` is true, ANSI escape sequences are preserved (-e flag).
pub fn capture_pane(name: &str, start_line: Option<i32>, escape: bool) -> TmuxResult<String> {
    capture_pane_inner(name, start_line, escape, false)
}

/// Like `capture_pane` but passes `-J` so tmux joins lines that wrap at the
/// pane width back into single logical lines. Use this when downstream parsing
/// relies on suffix matches (e.g. "X% used") that would be split by wrap.
pub fn capture_pane_joined(name: &str, start_line: Option<i32>) -> TmuxResult<String> {
    capture_pane_inner(name, start_line, false, true)
}

fn capture_pane_inner(
    name: &str,
    start_line: Option<i32>,
    escape: bool,
    join_wrapped: bool,
) -> TmuxResult<String> {
    let mut args = vec!["capture-pane", "-t", name, "-p"];
    let start_str;

    if escape {
        args.push("-e");
    }
    if join_wrapped {
        args.push("-J");
    }

    if let Some(start) = start_line {
        start_str = start.to_string();
        args.push("-S");
        args.push(&start_str);
    }

    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to capture pane: {}", e)))?;

    if !output.status.success() {
        return Err(TmuxError::CaptureFailed);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Force a tmux window to a fixed size (useful for detached sessions whose
/// pane width would otherwise default to ~80 cols and wrap content).
pub fn resize_window(name: &str, width: u32, height: u32) -> TmuxResult<()> {
    let w = width.to_string();
    let h = height.to_string();
    let status = Command::new("tmux")
        .args(["resize-window", "-t", name, "-x", &w, "-y", &h])
        .status()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to resize-window: {}", e)))?;
    if !status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux resize-window failed with status {}",
            status
        )));
    }
    Ok(())
}

/// Clear the scrollback history for a session's pane.
pub fn clear_history(name: &str) -> TmuxResult<()> {
    let status = Command::new("tmux")
        .args(["clear-history", "-t", name])
        .status()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to clear-history: {}", e)))?;
    if !status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux clear-history failed with status {}",
            status
        )));
    }
    Ok(())
}

static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[PX^_][^\x1b]*\x1b\\)")
        .expect("static regex must compile")
});

/// Strip ANSI escape sequences from terminal output
pub fn strip_ansi(text: &str) -> String {
    ANSI_RE.replace_all(text, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_keys_raw_builds_correct_command() {
        // Just test that the function exists and has the right signature
        // (actual tmux interaction tested manually)
        let result = send_keys_raw("nonexistent_session_xyz", "Right");
        assert!(result.is_err()); // session doesn't exist
    }

    #[test]
    fn tmux_failure_message_includes_stderr_detail() {
        let message = tmux_failure_message(
            "new-session",
            "exit status: 1",
            b"",
            b"create window failed: fork failed: Device not configured\n",
        );

        assert_eq!(
            message,
            "tmux new-session failed with status exit status: 1: create window failed: fork failed: Device not configured"
        );
    }

    #[test]
    fn test_parse_session_infos_reads_attachment_state() {
        let sessions = parse_session_infos(
            "agentorch_known\t0\nagentorch_attached\t2\nmissing-tab\n\t1\npersonal\tbad\n",
        );

        assert_eq!(
            sessions,
            vec![
                TmuxSessionInfo {
                    name: "agentorch_known".to_string(),
                    attached: false,
                },
                TmuxSessionInfo {
                    name: "agentorch_attached".to_string(),
                    attached: true,
                },
                TmuxSessionInfo {
                    name: "personal".to_string(),
                    attached: false,
                },
            ]
        );
    }

    #[test]
    fn test_generate_session_name_format() {
        let name = generate_session_name("My Test Session");
        assert!(name.starts_with("agentorch_"));
        assert!(name.contains("my-test-session"));
    }

    #[test]
    fn test_generate_session_name_truncates_long_titles() {
        let name = generate_session_name("this is a very long title that should be truncated");
        // The safe part should be at most 20 chars
        let after_prefix = &name["agentorch_".len()..];
        let parts: Vec<&str> = after_prefix.rsplitn(2, '-').collect();
        // parts[1] is the safe title part, parts[0] is the timestamp
        assert!(parts.len() == 2);
        assert!(parts[1].len() <= 20);
    }

    #[test]
    fn test_generate_session_name_sanitizes_special_chars() {
        let name = generate_session_name("hello@world!#$%");
        assert!(name.starts_with("agentorch_"));
        // Should not contain special characters
        let after_prefix = &name["agentorch_".len()..];
        assert!(!after_prefix.contains('@'));
        assert!(!after_prefix.contains('!'));
    }

    #[test]
    fn test_strip_ansi_removes_color_codes() {
        let input = "\x1b[31mHello\x1b[0m World";
        assert_eq!(strip_ansi(input), "Hello World");
    }

    #[test]
    fn test_strip_ansi_removes_osc_sequences() {
        let input = "Hello\x1b]0;title\x07World";
        assert_eq!(strip_ansi(input), "HelloWorld");
    }

    #[test]
    fn test_strip_ansi_preserves_normal_text() {
        let input = "Hello World";
        assert_eq!(strip_ansi(input), "Hello World");
    }

    #[test]
    fn test_radix_string_base36() {
        assert_eq!(radix_string(0, 36), "0");
        assert_eq!(radix_string(35, 36), "z");
        assert_eq!(radix_string(36, 36), "10");
    }

    #[test]
    fn test_session_cache_register_and_exists() {
        let mut cache = SessionCache::new();
        assert!(!cache.session_exists("test"));
        cache.register("test");
        assert!(cache.session_exists("test"));
    }

    #[test]
    fn test_session_cache_remove() {
        let mut cache = SessionCache::new();
        cache.register("test");
        cache.remove("test");
        assert!(!cache.session_exists("test"));
    }

    #[test]
    fn test_session_exists_nonexistent() {
        assert!(!session_exists("agentorch_nonexistent_test_session_xyz"));
    }
}
