use super::error::{TmuxError, TmuxResult};
use std::io::Write;
use std::process::Command;

/// Attach to a tmux session for routine inspection.
/// Sets up Ctrl+Q to detach, Ctrl+P to promote (signal + detach).
/// Returns true if the user pressed Ctrl+P (promote), false for normal detach.
pub fn attach_inspect_session_sync(session_name: &str, run_id: &str) -> TmuxResult<bool> {
    let promote_signal = get_promote_signal_path(run_id);

    // Clear any existing signal
    let _ = std::fs::remove_file(&promote_signal);

    // Clear screen + scrollback + show cursor
    let _ = std::io::stdout().write_all(b"\x1b[3J\x1b[2J\x1b[H\x1b[?25h");
    let _ = std::io::stdout().flush();
    let _ = Command::new("clear").status();

    // Cancel copy-mode (non-fatal)
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", session_name, "-X", "cancel"])
        .output();

    // Set up bindings and status bar — mirrors attach_session_sync but adds Ctrl+P promote
    let status_right = "#[fg=#89b4fa]Ctrl+P#[fg=#6c7086] promote  #[fg=#89b4fa]Ctrl+K#[fg=#6c7086] back  #[fg=#89b4fa]Ctrl+T#[fg=#6c7086] terminal  #[fg=#89b4fa]Ctrl+Q#[fg=#6c7086] detach  #[fg=#89b4fa]Ctrl+C#[fg=#6c7086] cancel";

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
            "detach-client",
            ";",
            "bind-key",
            "-n",
            "C-p",
            "run-shell",
            &format!("touch {} && tmux detach-client", promote_signal),
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
            "C-p",
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
        Ok(status) if !status.success() => {
            Err(TmuxError::AttachFailed("tmux attach failed".to_string()))
        }
        Err(e) => Err(TmuxError::Io(e)),
        Ok(_) => {
            let was_promoted = std::fs::metadata(&promote_signal).is_ok();
            let _ = std::fs::remove_file(&promote_signal);
            Ok(was_promoted)
        }
    }
}

fn get_promote_signal_path(run_id: &str) -> String {
    let uid = unsafe { libc::getuid() };
    format!("/tmp/agent-view-promote-{}-{}", uid, run_id)
}
