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

    let _text_style_guard = super::terminal::normalize_text_styles_for_attached_client();

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
        Ok(status) if !status.success() => Err(TmuxError::AttachFailedReason(
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

pub fn build_conductor_workspace_commands(
    tmux_name: &str,
    session_id: &str,
    binary_path: &str,
) -> Vec<Vec<String>> {
    let sidecar_command = format!(
        "{} conductor-panel {}",
        shell_quote_if_needed(binary_path),
        shell_quote_if_needed(session_id)
    );
    vec![
        vec![
            "split-window".to_string(),
            "-h".to_string(),
            "-t".to_string(),
            tmux_name.to_string(),
            sidecar_command,
        ],
        vec![
            "select-pane".to_string(),
            "-t".to_string(),
            format!("{}:.0", tmux_name),
        ],
        vec![
            "attach-session".to_string(),
            "-t".to_string(),
            tmux_name.to_string(),
        ],
    ]
}

pub fn attach_conductor_workspace_sync(tmux_name: &str, session_id: &str) -> TmuxResult<()> {
    let binary_path = std::env::current_exe()?;
    let binary_path = binary_path.to_string_lossy();
    let commands = build_conductor_workspace_commands(tmux_name, session_id, &binary_path);
    attach_conductor_workspace_core(
        || capture_target_pane_id(tmux_name),
        |main_pane| {
            let sidecar_split_command = retarget_tmux_command(&commands[0], main_pane);
            split_conductor_sidecar(&sidecar_split_command)
        },
        |main_pane| run_tmux_command(&build_select_pane_command(main_pane)),
        || attach_session_sync(tmux_name).map(|_| ()),
        cleanup_tmux_pane,
    )
}

fn attach_conductor_workspace_core<C, S, P, A, K>(
    capture_main_pane: C,
    split_sidecar: S,
    select_main_pane: P,
    mut attach_normal: A,
    cleanup_sidecar: K,
) -> TmuxResult<()>
where
    C: FnOnce() -> TmuxResult<String>,
    S: FnOnce(&str) -> TmuxResult<String>,
    P: FnOnce(&str) -> TmuxResult<()>,
    A: FnMut() -> TmuxResult<()>,
    K: FnOnce(&str),
{
    let main_pane = capture_main_pane()?;
    let sidecar_pane = match split_sidecar(&main_pane) {
        Ok(sidecar_pane) => sidecar_pane,
        Err(_) => return attach_normal(),
    };

    let attach_result = (|| {
        select_main_pane(&main_pane)?;
        attach_normal()
    })();
    cleanup_sidecar(&sidecar_pane);
    attach_result
}

fn capture_target_pane_id(tmux_name: &str) -> TmuxResult<String> {
    let args = build_capture_pane_command(tmux_name);
    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to spawn tmux: {}", e)))?;
    if !output.status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux display-message failed with status {}",
            output.status
        )));
    }

    let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pane_id.is_empty() {
        return Err(TmuxError::CommandFailed(
            "tmux display-message did not return a target pane id".to_string(),
        ));
    }
    Ok(pane_id)
}

fn split_conductor_sidecar(split_command: &[String]) -> TmuxResult<String> {
    let args = build_captured_sidecar_split_command(split_command);
    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to spawn tmux: {}", e)))?;
    if !output.status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux split-window failed with status {}",
            output.status
        )));
    }

    let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pane_id.is_empty() {
        return Err(TmuxError::CommandFailed(
            "tmux split-window did not return a sidecar pane id".to_string(),
        ));
    }
    Ok(pane_id)
}

fn run_tmux_command(args: &[String]) -> TmuxResult<()> {
    let status = Command::new("tmux")
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| TmuxError::CommandFailed(format!("Failed to spawn tmux: {}", e)))?;
    if !status.success() {
        return Err(TmuxError::CommandFailed(format!(
            "tmux {} failed with status {}",
            args.first().map(String::as_str).unwrap_or("command"),
            status
        )));
    }
    Ok(())
}

fn build_capture_pane_command(tmux_name: &str) -> Vec<String> {
    vec![
        "display-message".to_string(),
        "-p".to_string(),
        "-t".to_string(),
        tmux_name.to_string(),
        "#{pane_id}".to_string(),
    ]
}

fn build_select_pane_command(pane_id: &str) -> Vec<String> {
    vec![
        "select-pane".to_string(),
        "-t".to_string(),
        pane_id.to_string(),
    ]
}

fn retarget_tmux_command(args: &[String], target: &str) -> Vec<String> {
    let mut command = args.to_vec();
    if let Some(index) = command.iter().position(|arg| arg == "-t") {
        if let Some(target_arg) = command.get_mut(index + 1) {
            *target_arg = target.to_string();
        }
    }
    command
}

fn build_captured_sidecar_split_command(split_command: &[String]) -> Vec<String> {
    let mut command = Vec::with_capacity(split_command.len() + 3);
    if let Some((first, rest)) = split_command.split_first() {
        command.push(first.clone());
        command.push("-P".to_string());
        command.push("-F".to_string());
        command.push("#{pane_id}".to_string());
        command.extend(rest.iter().cloned());
    }
    command
}

fn build_kill_pane_command(pane_id: &str) -> Vec<String> {
    vec![
        "kill-pane".to_string(),
        "-t".to_string(),
        pane_id.to_string(),
    ]
}

fn cleanup_tmux_pane(pane_id: &str) {
    let _ = Command::new("tmux")
        .args(build_kill_pane_command(pane_id))
        .status();
}

fn shell_quote_if_needed(value: &str) -> String {
    if !value.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\'' | '"' | '\\' | '$' | '`' | '!' | '&' | '|' | ';' | '<' | '>' | '(' | ')'
            )
    }) {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Get the path to the signal file for command palette requests
fn get_signal_file_path() -> String {
    let uid = unsafe { libc::getuid() };
    format!("/tmp/agent-view-cmd-palette-{}", uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conductor_workspace_commands_split_sidecar_panel() {
        let cmds = build_conductor_workspace_commands(
            "agentorch_release",
            "c1",
            "/usr/local/bin/agent-view",
        );
        assert_eq!(
            cmds,
            vec![
                vec![
                    "split-window",
                    "-h",
                    "-t",
                    "agentorch_release",
                    "/usr/local/bin/agent-view conductor-panel c1"
                ],
                vec!["select-pane", "-t", "agentorch_release:.0"],
                vec!["attach-session", "-t", "agentorch_release"],
            ]
        );
    }

    #[test]
    fn conductor_sidecar_split_command_captures_pane_id() {
        let cmds = build_conductor_workspace_commands(
            "agentorch_release",
            "c1",
            "/usr/local/bin/agent-view",
        );

        assert_eq!(
            build_captured_sidecar_split_command(&cmds[0]),
            vec![
                "split-window",
                "-P",
                "-F",
                "#{pane_id}",
                "-h",
                "-t",
                "agentorch_release",
                "/usr/local/bin/agent-view conductor-panel c1",
            ]
        );
    }

    #[test]
    fn conductor_runtime_commands_target_captured_main_pane() {
        let cmds = build_conductor_workspace_commands(
            "agentorch_release",
            "c1",
            "/usr/local/bin/agent-view",
        );

        assert_eq!(
            build_capture_pane_command("agentorch_release"),
            vec![
                "display-message",
                "-p",
                "-t",
                "agentorch_release",
                "#{pane_id}",
            ]
        );
        assert_eq!(
            retarget_tmux_command(&cmds[0], "%7"),
            vec![
                "split-window",
                "-h",
                "-t",
                "%7",
                "/usr/local/bin/agent-view conductor-panel c1",
            ]
        );
        assert_eq!(
            build_select_pane_command("%7"),
            vec!["select-pane", "-t", "%7"]
        );
    }

    #[test]
    fn conductor_sidecar_cleanup_targets_captured_pane() {
        assert_eq!(
            build_kill_pane_command("%42"),
            vec!["kill-pane", "-t", "%42"]
        );
    }

    #[test]
    fn conductor_workspace_falls_back_to_normal_attach_when_sidecar_split_fails() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let events = Rc::new(RefCell::new(Vec::new()));
        let result = attach_conductor_workspace_core(
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push("capture".to_string());
                    Ok("%1".to_string())
                }
            },
            {
                let events = Rc::clone(&events);
                move |pane| {
                    events.borrow_mut().push(format!("split:{pane}"));
                    Err(TmuxError::CommandFailed("out of ptys".to_string()))
                }
            },
            {
                let events = Rc::clone(&events);
                move |pane| {
                    events.borrow_mut().push(format!("select:{pane}"));
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push("attach".to_string());
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move |pane| {
                    events.borrow_mut().push(format!("cleanup:{pane}"));
                }
            },
        );

        assert!(result.is_ok());
        assert_eq!(
            events.borrow().as_slice(),
            ["capture", "split:%1", "attach"]
        );
    }

    #[test]
    fn conductor_workspace_selects_attaches_and_cleans_up_sidecar_when_split_succeeds() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let events = Rc::new(RefCell::new(Vec::new()));
        let result = attach_conductor_workspace_core(
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push("capture".to_string());
                    Ok("%1".to_string())
                }
            },
            {
                let events = Rc::clone(&events);
                move |pane| {
                    events.borrow_mut().push(format!("split:{pane}"));
                    Ok("%2".to_string())
                }
            },
            {
                let events = Rc::clone(&events);
                move |pane| {
                    events.borrow_mut().push(format!("select:{pane}"));
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push("attach".to_string());
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move |pane| {
                    events.borrow_mut().push(format!("cleanup:{pane}"));
                }
            },
        );

        assert!(result.is_ok());
        assert_eq!(
            events.borrow().as_slice(),
            ["capture", "split:%1", "select:%1", "attach", "cleanup:%2"]
        );
    }
}
