//! Tmux subprocess wrapper for session management

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

pub const SESSION_PREFIX: &str = "agentorch_";

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

/// Create a new tmux session
pub fn create_session(
    name: &str,
    command: Option<&str>,
    cwd: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> Result<(), String> {
    let cwd = cwd.unwrap_or("/tmp");

    // Step 1: Create detached session
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-c", cwd])
        .status()
        .map_err(|e| format!("Failed to spawn tmux: {}", e))?;

    if !status.success() {
        return Err(format!("tmux new-session failed with status {}", status));
    }

    // Step 2: Set environment variables
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

/// Kill a tmux session
pub fn kill_session(name: &str) -> Result<(), String> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output();
    Ok(())
}

/// Send keys to a tmux session (followed by Enter)
pub fn send_keys(name: &str, keys: &str) -> Result<(), String> {
    let escaped = keys
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");

    let status = Command::new("tmux")
        .args(["send-keys", "-t", name, &escaped, "Enter"])
        .status()
        .map_err(|e| format!("Failed to send keys: {}", e))?;

    if !status.success() {
        return Err(format!("tmux send-keys failed with status {}", status));
    }
    Ok(())
}

/// Capture pane content from a tmux session
pub fn capture_pane(name: &str, start_line: Option<i32>) -> Result<String, String> {
    let mut args = vec!["capture-pane", "-t", name, "-p"];
    let start_str;

    if let Some(start) = start_line {
        start_str = start.to_string();
        args.push("-S");
        args.push(&start_str);
    }

    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to capture pane: {}", e))?;

    if !output.status.success() {
        return Err("capture-pane failed".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get sessions that currently have an attached client
#[allow(dead_code)]
pub fn get_attached_sessions() -> std::collections::HashSet<String> {
    let output = Command::new("tmux")
        .args(["list-clients", "-F", "#{client_session}"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout
                .trim()
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        }
        _ => std::collections::HashSet::new(),
    }
}

/// Attach to a tmux session synchronously (blocks until detach).
/// Sets up Ctrl+Q to detach, Ctrl+K for command palette signal, Ctrl+T for terminal split.
/// Returns true if command palette was requested.
pub fn attach_session_sync(session_name: &str) -> Result<bool, String> {
    use std::io::Write;

    let signal_file = get_signal_file_path();

    // Clear any existing signal
    let _ = std::fs::remove_file(&signal_file);

    // Clear screen + scrollback + show cursor
    // Use both ANSI sequences and the `clear` command for maximum compatibility
    let _ = std::io::stdout().write_all(b"\x1b[3J\x1b[2J\x1b[H\x1b[?25h");
    let _ = std::io::stdout().flush();
    let _ = Command::new("clear").status();

    // Cancel copy-mode (non-fatal)
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", session_name, "-X", "cancel"])
        .output();

    // Batch pre-attach setup
    let status_right = "#[fg=#89b4fa]Ctrl+K#[fg=#6c7086] cmd  #[fg=#89b4fa]Ctrl+T#[fg=#6c7086] terminal  #[fg=#89b4fa]Ctrl+Q#[fg=#6c7086] detach  #[fg=#89b4fa]Ctrl+C#[fg=#6c7086] cancel";

    let _ = Command::new("tmux")
        .args([
            "bind-key",
            "-n",
            "C-q",
            "detach-client",
            ";",
            "bind-key",
            "-n",
            "C-k",
            "run-shell",
            &format!("touch {} && tmux detach-client", signal_file),
            ";",
            "bind-key",
            "-n",
            "C-t",
            "split-window",
            "-v",
            "-c",
            "#{pane_current_path}",
            ";",
            "set-option",
            "-t",
            session_name,
            "status",
            "on",
            ";",
            "set-option",
            "-t",
            session_name,
            "status-position",
            "bottom",
            ";",
            "set-option",
            "-t",
            session_name,
            "status-style",
            "bg=#1e1e2e,fg=#cdd6f4",
            ";",
            "set-option",
            "-t",
            session_name,
            "status-left",
            "",
            ";",
            "set-option",
            "-t",
            session_name,
            "status-right-length",
            "120",
            ";",
            "set-option",
            "-t",
            session_name,
            "status-right",
            status_right,
        ])
        .output();

    // Attach — blocks until detach
    let result = Command::new("tmux")
        .args(["attach-session", "-t", session_name])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::piped())
        .status();

    // Unbind keys
    let _ = Command::new("tmux")
        .args([
            "unbind-key",
            "-n",
            "C-q",
            ";",
            "unbind-key",
            "-n",
            "C-k",
            ";",
            "unbind-key",
            "-n",
            "C-t",
        ])
        .output();

    // Clear screen for TUI return
    let _ = std::io::stdout().write_all(b"\x1b[2J\x1b[H");
    let _ = std::io::stdout().flush();

    match result {
        Ok(status) if !status.success() => Err(
            "tmux attach failed: this is usually caused by a tmux version mismatch. \
                 Run 'tmux kill-server' in a terminal to fix this."
                .to_string(),
        ),
        Err(e) => Err(format!("Failed to attach: {}", e)),
        Ok(_) => {
            // Check if command palette was requested
            let was_requested = std::fs::metadata(&signal_file).is_ok();
            let _ = std::fs::remove_file(&signal_file);
            Ok(was_requested)
        }
    }
}

/// Get the path to the signal file for command palette requests
fn get_signal_file_path() -> String {
    let uid = unsafe { libc::getuid() };
    format!("/tmp/agent-view-cmd-palette-{}", uid)
}

/// Strip ANSI escape sequences from terminal output
pub fn strip_ansi(text: &str) -> String {
    lazy_static::lazy_static! {
        static ref ANSI_RE: regex::Regex = regex::Regex::new(
            r"(\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[PX^_][^\x1b]*\x1b\\)"
        ).unwrap();
    }
    ANSI_RE.replace_all(text, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
