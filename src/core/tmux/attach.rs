use super::error::{TmuxError, TmuxResult};
use std::io::Write;
use std::process::Command;

/// Attach to a tmux session synchronously (blocks until detach).
/// Sets up Ctrl+Q to detach, Ctrl+K for command palette signal, Ctrl+T for terminal split.
/// Returns true if command palette was requested.
pub fn attach_session_sync(session_name: &str) -> TmuxResult<bool> {
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
        Ok(status) if !status.success() => Err(TmuxError::AttachFailed(
            "this is usually caused by a tmux version mismatch. \
                 Run 'tmux kill-server' in a terminal to fix this."
                .to_string(),
        )),
        Err(e) => Err(TmuxError::Io(e)),
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
