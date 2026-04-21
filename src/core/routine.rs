//! Routine execution logic — runs steps sequentially in a tmux session

use crate::core::storage::Storage;
use crate::types::{RoutineRun, RoutineStep, RunStatus};

/// Execute a routine by ID. Called by the `exec-routine` CLI subcommand.
/// This is a blocking function that runs all steps sequentially.
pub fn exec_routine(routine_id: &str) -> Result<(), String> {
    let storage = Storage::open_default().map_err(|e| format!("Failed to open storage: {}", e))?;
    storage
        .migrate()
        .map_err(|e| format!("Migration failed: {}", e))?;

    let routine = storage
        .get_routine(routine_id)
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| format!("Routine '{}' not found", routine_id))?;

    // Concurrency guard
    if storage
        .has_active_run(routine_id)
        .map_err(|e| format!("DB error: {}", e))?
    {
        eprintln!(
            "Routine '{}' already has an active run, skipping",
            routine.name
        );
        return Ok(());
    }

    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let tmux_name = crate::core::tmux::generate_session_name(&format!("routine_{}", routine.name));

    // Create tmux session
    crate::core::tmux::create_session(&tmux_name, None, Some(&routine.working_dir), None)?;

    // Insert run record
    let run = RoutineRun {
        id: run_id.clone(),
        routine_id: routine_id.to_string(),
        started_at: now,
        finished_at: None,
        status: RunStatus::Running,
        steps_completed: 0,
        steps_total: routine.steps.len() as i32,
        log_path: None,
        tmux_session: Some(tmux_name.clone()),
        tool_data: "{}".to_string(),
        promoted_session_id: None,
    };
    storage
        .save_routine_run(&run)
        .map_err(|e| format!("DB error: {}", e))?;

    let timeout = std::time::Duration::from_secs(routine.step_timeout_secs as u64);
    let mut final_status = RunStatus::Completed;
    let mut current_tool_data = "{}".to_string();

    for (i, step) in routine.steps.iter().enumerate() {
        let command = match step {
            RoutineStep::Claude { prompt } => {
                format!(
                    "claude --permission-mode bypassPermissions '{}'",
                    prompt.replace('\'', "'\\''")
                )
            }
            RoutineStep::Shell { command } => command.clone(),
        };

        // Send command to tmux
        if let Err(e) = crate::core::tmux::send_keys(&tmux_name, &command) {
            eprintln!("Failed to send keys for step {}: {}", i + 1, e);
            final_status = RunStatus::Failed;
            break;
        }

        // Wait for command to start before polling for completion.
        // Without this, the idle prompt detector sees the shell prompt
        // before the command has loaded and thinks the step is done.
        std::thread::sleep(std::time::Duration::from_secs(3));

        // For Claude with bypassPermissions: auto-accept the confirmation prompt.
        // Send Down (select "Yes, I accept") then Enter.
        if matches!(step, RoutineStep::Claude { .. }) {
            let _ = std::process::Command::new("tmux")
                .args(["send-keys", "-t", &tmux_name, "Down", "Enter"])
                .output();
            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        // Poll for completion
        let start = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            if start.elapsed() > timeout {
                eprintln!("Step {} timed out", i + 1);
                final_status = RunStatus::TimedOut;
                break;
            }

            // Check if tmux session still exists
            if !crate::core::tmux::session_exists(&tmux_name) {
                eprintln!("tmux session died during step {}", i + 1);
                final_status = RunStatus::Crashed;
                break;
            }

            // Check if step completed by detecting idle prompt
            match crate::core::tmux::capture_pane(&tmux_name, Some(-100), false) {
                Ok(output) => {
                    let tool_str = match step {
                        RoutineStep::Claude { .. } => Some("claude"),
                        RoutineStep::Shell { .. } => None,
                    };
                    let parsed = crate::core::status::parse_tool_status(&output, tool_str);

                    // For Claude: capture session ID
                    if matches!(step, RoutineStep::Claude { .. }) {
                        if let Some(session_id) =
                            crate::core::status::extract_claude_session_id(&output)
                        {
                            let mut data: serde_json::Value =
                                serde_json::from_str(&current_tool_data)
                                    .unwrap_or_else(|_| serde_json::json!({}));
                            data["claude_session_id"] = serde_json::Value::String(session_id);
                            current_tool_data = data.to_string();
                            let _ = storage.update_run_tool_data(&run_id, &current_tool_data);
                        }
                    }

                    if parsed.has_error {
                        eprintln!("Step {} encountered an error", i + 1);
                        final_status = RunStatus::Failed;
                        break;
                    }

                    if parsed.has_idle_prompt || parsed.has_exited {
                        break; // Step completed
                    }
                }
                Err(_) => continue,
            }
        }

        if final_status != RunStatus::Completed {
            break;
        }

        // Mark step completed
        let _ = storage.increment_run_steps_completed(&run_id);
    }

    // Capture log
    let log_dir = dirs::home_dir()
        .expect("Cannot determine home dir")
        .join(".agent-view")
        .join("routine-logs")
        .join(routine_id);
    std::fs::create_dir_all(&log_dir).ok();
    let log_path = log_dir.join(format!("{}.log", run_id));
    if let Ok(output) = crate::core::tmux::capture_pane(&tmux_name, None, false) {
        let _ = std::fs::write(&log_path, &output);
    }

    // Finalize run
    let finished_at = chrono::Utc::now().timestamp_millis();
    let _ = storage.update_routine_run_status(&run_id, final_status, Some(finished_at));

    // Update log_path on run
    let _ = storage.conn().execute(
        "UPDATE routine_runs SET log_path = ?1 WHERE id = ?2",
        rusqlite::params![log_path.to_string_lossy().to_string(), run_id],
    );

    // Update routine metadata
    let next = crate::core::schedule::next_run(&routine.schedule);
    let _ = storage.record_routine_execution(routine_id, finished_at, next);

    // Kill tmux session
    let _ = crate::core::tmux::kill_session(&tmux_name);

    // Notify if enabled
    if routine.notify {
        let (title, body) = match final_status {
            RunStatus::Completed => (
                format!("{} completed", routine.name),
                "Routine completed successfully".to_string(),
            ),
            RunStatus::Failed => (
                format!("{} failed", routine.name),
                "Routine failed".to_string(),
            ),
            RunStatus::TimedOut => (
                format!("{} timed out", routine.name),
                "Routine timed out".to_string(),
            ),
            _ => (routine.name.clone(), "Routine finished".to_string()),
        };
        crate::core::notify::send_notification(crate::core::notify::NotificationOptions {
            title,
            body,
            subtitle: None,
            sound: false,
        });
    }

    storage.touch().ok();

    Ok(())
}

/// Build a Session struct from a routine run for promotion
pub fn build_promoted_session(
    run: &crate::types::RoutineRun,
    routine: &crate::types::Routine,
) -> crate::types::Session {
    let tool = routine
        .steps
        .last()
        .map(|s| match s {
            RoutineStep::Claude { .. } => crate::types::Tool::Claude,
            RoutineStep::Shell { .. } => crate::types::Tool::Shell,
        })
        .unwrap_or(crate::types::Tool::Shell);

    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();

    crate::types::Session {
        id,
        title: format!("{} (promoted)", routine.name),
        project_path: routine.working_dir.clone(),
        group_path: "my-sessions".to_string(),
        order: 0,
        command: String::new(),
        wrapper: String::new(),
        tool,
        status: crate::types::SessionStatus::Idle,
        tmux_session: run.tmux_session.clone().unwrap_or_default(),
        created_at: now,
        last_accessed: now,
        parent_session_id: String::new(),
        worktree_path: String::new(),
        worktree_repo: String::new(),
        worktree_branch: String::new(),
        tool_data: run.tool_data.clone(),
        acknowledged: false,
        notify: false,
        follow_up: false,
        status_changed_at: now,
        restart_count: 0,
        last_started_at: now,
        notes: vec![],
        status_history: vec![],
        pinned: false,
        tokens_used: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_promoted_session_has_correct_fields() {
        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: crate::types::RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: Some("/tmp/log".to_string()),
            tmux_session: Some("agentorch_routine_test".to_string()),
            tool_data: r#"{"claude_session_id": "abc123"}"#.to_string(),
            promoted_session_id: None,
        };
        let routine = crate::types::Routine {
            id: "r1".to_string(),
            name: "Test".to_string(),
            group_path: "my-routines".to_string(),
            sort_order: 0,
            working_dir: "/tmp/project".to_string(),
            default_tool: "claude".to_string(),
            schedule: "0 9 * * *".to_string(),
            steps: vec![crate::types::RoutineStep::Claude {
                prompt: "test".to_string(),
            }],
            enabled: false,
            created_at: 0,
            last_run_at: None,
            next_run_at: None,
            run_count: 0,
            pinned: false,
            notify: true,
            step_timeout_secs: 1800,
            expanded: false,
        };

        let session = build_promoted_session(&run, &routine);
        assert_eq!(session.project_path, "/tmp/project");
        assert_eq!(session.tool, crate::types::Tool::Claude);
        assert!(session.tool_data.contains("abc123"));
    }
}
