//! Input handling for routine-related overlays

use crate::app::{App, NewRoutineForm, Overlay, RoutineListRow, ScheduleFrequency};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle key input when on the Routines tab (no overlay active)
pub fn handle_routine_list_key(
    app: &mut App,
    key: KeyEvent,
    storage: &crate::core::storage::Storage,
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
) {
    match (key.modifiers, key.code) {
        // Navigation
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            if app.routine_selected_index > 0 {
                app.routine_selected_index -= 1;
            } else if !app.routine_list_rows.is_empty() {
                app.routine_selected_index = app.routine_list_rows.len() - 1;
            }
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            if !app.routine_list_rows.is_empty() {
                if app.routine_selected_index < app.routine_list_rows.len() - 1 {
                    app.routine_selected_index += 1;
                } else {
                    app.routine_selected_index = 0;
                }
            }
        }

        // Enter: expand/collapse routine to show runs, or toggle group
        (KeyModifiers::NONE, KeyCode::Enter) => {
            match app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                Some(RoutineListRow::Group { group, .. }) => {
                    let path = group.path.clone();
                    if let Some(g) = app.groups.iter_mut().find(|g| g.path == path) {
                        g.expanded = !g.expanded;
                    }
                    app.rebuild_routine_list_rows();
                }
                Some(RoutineListRow::Routine(routine)) => {
                    let routine_id = routine.id.clone();
                    if let Some(r) = app.routines.iter_mut().find(|r| r.id == routine_id) {
                        r.expanded = !r.expanded;
                        if r.expanded && !app.routine_runs_cache.contains_key(&routine_id) {
                            if let Ok(runs) = storage.load_routine_runs(&routine_id) {
                                app.routine_runs_cache.insert(routine_id.clone(), runs);
                            }
                        }
                    }
                    app.rebuild_routine_list_rows();
                }
                _ => {}
            }
        }

        // Space: toggle enabled/disabled
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            if let Some(RoutineListRow::Routine(routine)) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                let new_enabled = !routine.enabled;
                let _ = storage.set_routine_enabled(&routine.id, new_enabled);

                let scheduler = crate::core::scheduler::platform_scheduler();
                if new_enabled {
                    if let Some(r) = app.routines.iter().find(|r| r.id == routine.id) {
                        let _ = scheduler.install(r);
                    }
                } else {
                    let _ = scheduler.uninstall(&routine.id);
                }

                app.routines = storage.load_routines().unwrap_or_default();
                app.rebuild_routine_list_rows();
                storage.touch().ok();
            }
        }

        // d: delete routine or run
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            match app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                Some(RoutineListRow::Routine(routine)) => {
                    app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                        message: format!("Delete routine '{}'?", routine.name),
                        action: crate::app::ConfirmAction::DeleteRoutine(routine.id.clone()),
                    });
                }
                Some(RoutineListRow::Run { run, .. }) => {
                    let _ = storage.delete_routine_run(&run.id);
                    if let Some(ref log_path) = run.log_path {
                        let _ = std::fs::remove_file(log_path);
                    }
                    if let Ok(runs) = storage.load_routine_runs(&run.routine_id) {
                        app.routine_runs_cache.insert(run.routine_id.clone(), runs);
                    }
                    app.rebuild_routine_list_rows();
                    storage.touch().ok();
                }
                _ => {}
            }
        }

        // e: edit routine
        (KeyModifiers::NONE, KeyCode::Char('e')) => {
            if let Some(RoutineListRow::Routine(routine)) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                app.overlay = Overlay::NewRoutine(NewRoutineForm::from_routine(&routine));
            }
        }

        // p: pin/unpin routine
        (KeyModifiers::NONE, KeyCode::Char('p')) => {
            if let Some(RoutineListRow::Routine(routine)) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                let new_pinned = !routine.pinned;
                let _ = storage.set_routine_pinned(&routine.id, new_pinned);
                app.routines = storage.load_routines().unwrap_or_default();
                app.rebuild_routine_list_rows();
                storage.touch().ok();
            }
        }

        // P: promote run to session
        (KeyModifiers::SHIFT, KeyCode::Char('P')) => {
            if let Some(RoutineListRow::Run { run, .. }) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                if let Some(routine) = app
                    .routines
                    .iter()
                    .find(|r| r.id == run.routine_id)
                    .cloned()
                {
                    let mut session = crate::core::routine::build_promoted_session(&run, &routine);

                    let tmux_alive = run
                        .tmux_session
                        .as_ref()
                        .map(|t| crate::core::tmux::session_exists(t))
                        .unwrap_or(false);

                    if !tmux_alive {
                        let tool_data: serde_json::Value = serde_json::from_str(&run.tool_data)
                            .unwrap_or_else(|_| serde_json::json!({}));
                        let claude_session_id = tool_data
                            .get("claude_session_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let tool = routine
                            .steps
                            .last()
                            .map(|s| match s {
                                crate::types::RoutineStep::Claude { .. } => {
                                    crate::types::Tool::Claude
                                }
                                crate::types::RoutineStep::Shell { .. } => {
                                    crate::types::Tool::Shell
                                }
                            })
                            .unwrap_or(crate::types::Tool::Shell);

                        let tmux_name = crate::core::tmux::generate_session_name(&format!(
                            "promoted_{}",
                            routine.name
                        ));
                        let command = match (tool, claude_session_id) {
                            (crate::types::Tool::Claude, Some(sid)) => {
                                Some(format!("claude --resume {}", sid))
                            }
                            _ => None,
                        };
                        let _ = crate::core::tmux::create_session(
                            &tmux_name,
                            command.as_deref(),
                            Some(&routine.working_dir),
                            None,
                        );
                        session.tmux_session = tmux_name;
                    }

                    let session_title = session.title.clone();
                    let _ = storage.save_session(&session);
                    let _ = storage.set_run_promoted(&run.id, &session.id);

                    app.sessions = storage.load_sessions().unwrap_or_default();
                    app.rebuild_list_rows();
                    if let Ok(runs) = storage.load_routine_runs(&run.routine_id) {
                        app.routine_runs_cache.insert(run.routine_id.clone(), runs);
                    }
                    app.rebuild_routine_list_rows();
                    storage.touch().ok();

                    app.toast_message = Some(format!("Promoted to session: {}", session_title));
                    app.toast_expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                }
            }
        }

        // R: rename routine
        (KeyModifiers::SHIFT, KeyCode::Char('R')) => {
            if let Some(RoutineListRow::Routine(routine)) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                app.overlay = Overlay::Rename(crate::app::RenameForm {
                    target_id: routine.id.clone(),
                    target_type: crate::app::RenameTarget::Routine,
                    input: routine.name.clone(),
                });
            }
        }

        // r: resume/inspect a run
        (KeyModifiers::NONE, KeyCode::Char('r')) => {
            if let Some(RoutineListRow::Run { run, .. }) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                if let Some(routine) = app
                    .routines
                    .iter()
                    .find(|r| r.id == run.routine_id)
                    .cloned()
                {
                    let tmux_name = crate::core::tmux::generate_session_name(&format!(
                        "inspect_{}",
                        routine.name
                    ));

                    let tool_data: serde_json::Value = serde_json::from_str(&run.tool_data)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let claude_session_id = tool_data
                        .get("claude_session_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let last_step_is_claude = routine
                        .steps
                        .last()
                        .map(|s| matches!(s, crate::types::RoutineStep::Claude { .. }))
                        .unwrap_or(false);

                    let command = if last_step_is_claude {
                        claude_session_id
                            .as_ref()
                            .map(|sid| format!("claude --resume {}", sid))
                    } else {
                        run.log_path
                            .as_ref()
                            .map(|p| format!("less {}", p))
                    };

                    if let Err(e) = crate::core::tmux::create_session(
                        &tmux_name,
                        command.as_deref(),
                        Some(&routine.working_dir),
                        None,
                    ) {
                        app.toast_message =
                            Some(format!("Failed to create inspect session: {}", e));
                        app.toast_expire = Some(
                            std::time::Instant::now() + std::time::Duration::from_secs(3),
                        );
                        return;
                    }

                    // Leave TUI
                    use crossterm::terminal::disable_raw_mode;
                    let _ = disable_raw_mode();

                    let promote_result =
                        crate::core::tmux::attach_inspect_session_sync(&tmux_name, &run.id);

                    // Re-enter TUI
                    use crossterm::execute;
                    use crossterm::terminal::enable_raw_mode;
                    let _ = enable_raw_mode();
                    let _ = execute!(
                        std::io::stdout(),
                        crossterm::terminal::EnterAlternateScreen
                    );
                    let _ = terminal.clear();

                    match promote_result {
                        Ok(true) => {
                            // User pressed Ctrl+P — promote the run
                            let session =
                                crate::core::routine::build_promoted_session(&run, &routine);
                            let session_title = session.title.clone();
                            let session_id = session.id.clone();

                            // Keep the inspect session alive as the promoted session's tmux session
                            let mut promoted_session = session;
                            promoted_session.tmux_session = tmux_name;
                            let _ = storage.save_session(&promoted_session);
                            let _ = storage.set_run_promoted(&run.id, &session_id);

                            app.sessions = storage.load_sessions().unwrap_or_default();
                            app.rebuild_list_rows();
                            if let Ok(runs) = storage.load_routine_runs(&run.routine_id) {
                                app.routine_runs_cache
                                    .insert(run.routine_id.clone(), runs);
                            }
                            app.rebuild_routine_list_rows();
                            storage.touch().ok();

                            app.toast_message =
                                Some(format!("Promoted to session: {}", session_title));
                            app.toast_expire = Some(
                                std::time::Instant::now() + std::time::Duration::from_secs(3),
                            );
                        }
                        Ok(false) => {
                            // Normal detach — kill the ephemeral tmux session
                            let _ = crate::core::tmux::kill_session(&tmux_name);
                        }
                        Err(e) => {
                            let _ = crate::core::tmux::kill_session(&tmux_name);
                            app.toast_message = Some(format!("Inspect failed: {}", e));
                            app.toast_expire = Some(
                                std::time::Instant::now() + std::time::Duration::from_secs(3),
                            );
                        }
                    }
                }
            }
        }

        _ => {}
    }
}

/// Handle key input for the NewRoutine overlay form
pub fn handle_new_routine_key(
    app: &mut App,
    key: KeyEvent,
    storage: &crate::core::storage::Storage,
) {
    let form = match &mut app.overlay {
        Overlay::NewRoutine(f) => f,
        _ => return,
    };

    match (key.modifiers, key.code) {
        // Escape: cancel
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.overlay = Overlay::None;
        }
        // Tab: next field
        (KeyModifiers::NONE, KeyCode::Tab) => {
            form.focused_field = (form.focused_field + 1) % 8;
            form.completions.clear();
            form.completion_index = None;
        }
        // Shift+Tab: previous field
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            form.focused_field = if form.focused_field == 0 {
                7
            } else {
                form.focused_field - 1
            };
            form.completions.clear();
            form.completion_index = None;
        }
        // Enter: submit form or add step
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // If we're editing a step, add it
            if form.focused_field == 4 {
                if let Some(ref text) = form.editing_step.clone() {
                    if !text.is_empty() {
                        let step = if form.default_tool == "claude" {
                            crate::types::RoutineStep::Claude {
                                prompt: text.clone(),
                            }
                        } else {
                            crate::types::RoutineStep::Shell {
                                command: text.clone(),
                            }
                        };
                        form.steps.push(step);
                        form.editing_step = None;
                    }
                } else {
                    form.editing_step = Some(String::new());
                }
                return;
            }

            // Submit the form
            if form.name.is_empty() {
                return; // Don't submit with empty name
            }
            let cron = form.cron_expression();
            if cron.is_empty() {
                return;
            }

            let now = chrono::Utc::now().timestamp_millis();
            let next = crate::core::schedule::next_run(&cron);

            if let Some(ref edit_id) = form.edit_routine_id.clone() {
                // Editing existing routine
                let routine = crate::types::Routine {
                    id: edit_id.clone(),
                    name: form.name.clone(),
                    group_path: "my-routines".to_string(),
                    sort_order: 0,
                    working_dir: form.working_dir.clone(),
                    default_tool: form.default_tool.clone(),
                    schedule: cron,
                    steps: form.steps.clone(),
                    enabled: false,
                    created_at: now,
                    last_run_at: None,
                    next_run_at: next,
                    run_count: 0,
                    pinned: false,
                    notify: form.notify,
                    step_timeout_secs: form.step_timeout_secs,
                    expanded: false,
                };
                let _ = storage.save_routine(&routine);
            } else {
                // New routine
                let routine = crate::types::Routine {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: form.name.clone(),
                    group_path: "my-routines".to_string(),
                    sort_order: 0,
                    working_dir: form.working_dir.clone(),
                    default_tool: form.default_tool.clone(),
                    schedule: cron,
                    steps: form.steps.clone(),
                    enabled: false,
                    created_at: now,
                    last_run_at: None,
                    next_run_at: next,
                    run_count: 0,
                    pinned: false,
                    notify: form.notify,
                    step_timeout_secs: form.step_timeout_secs,
                    expanded: false,
                };
                let _ = storage.save_routine(&routine);
            }

            // Reload and close
            app.routines = storage.load_routines().unwrap_or_default();
            app.rebuild_routine_list_rows();
            app.overlay = Overlay::None;
            storage.touch().ok();
        }
        _ => {
            // Field-specific input
            match form.focused_field {
                0 => handle_text_input(&mut form.name, key), // Name
                1 => {
                    // Default tool toggle
                    if key.code == KeyCode::Left
                        || key.code == KeyCode::Right
                        || key.code == KeyCode::Char(' ')
                    {
                        form.default_tool = if form.default_tool == "claude" {
                            "shell".to_string()
                        } else {
                            "claude".to_string()
                        };
                    }
                }
                2 => {
                    // Working dir with autocomplete
                    handle_path_input(form, key);
                }
                3 => {
                    // Schedule frequency and params
                    handle_schedule_input(form, key);
                }
                4 => {
                    // Steps
                    handle_steps_input(form, key);
                }
                5 => {
                    // Notifications toggle
                    if key.code == KeyCode::Char(' ')
                        || key.code == KeyCode::Left
                        || key.code == KeyCode::Right
                    {
                        form.notify = !form.notify;
                    }
                }
                6 => {
                    // Step timeout
                    match key.code {
                        KeyCode::Left => {
                            form.step_timeout_secs = (form.step_timeout_secs - 300).max(60);
                        }
                        KeyCode::Right => {
                            form.step_timeout_secs = (form.step_timeout_secs + 300).min(7200);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_text_input(text: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => text.push(c),
        KeyCode::Backspace => {
            text.pop();
        }
        _ => {}
    }
}

fn handle_path_input(form: &mut NewRoutineForm, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            form.working_dir.push(c);
            form.completions =
                crate::core::path_complete::complete_path(&form.working_dir).candidates;
            form.completion_index = None;
        }
        KeyCode::Backspace => {
            form.working_dir.pop();
            form.completions =
                crate::core::path_complete::complete_path(&form.working_dir).candidates;
            form.completion_index = None;
        }
        KeyCode::Down => {
            if !form.completions.is_empty() {
                form.completion_index = Some(
                    form.completion_index
                        .map(|i| (i + 1) % form.completions.len())
                        .unwrap_or(0),
                );
                if let Some(idx) = form.completion_index {
                    form.working_dir = form.completions[idx].clone();
                }
            }
        }
        KeyCode::Up => {
            if !form.completions.is_empty() {
                form.completion_index = Some(
                    form.completion_index
                        .map(|i| {
                            if i == 0 {
                                form.completions.len() - 1
                            } else {
                                i - 1
                            }
                        })
                        .unwrap_or(0),
                );
                if let Some(idx) = form.completion_index {
                    form.working_dir = form.completions[idx].clone();
                }
            }
        }
        _ => {}
    }
}

fn handle_schedule_input(form: &mut NewRoutineForm, key: KeyEvent) {
    match form.frequency {
        ScheduleFrequency::Advanced => match key.code {
            KeyCode::Char(c) => form.cron_raw.push(c),
            KeyCode::Backspace => {
                form.cron_raw.pop();
            }
            KeyCode::Left => form.frequency = form.frequency.prev(),
            KeyCode::Right => form.frequency = form.frequency.next(),
            _ => {}
        },
        _ => match key.code {
            KeyCode::Left => form.frequency = form.frequency.prev(),
            KeyCode::Right => form.frequency = form.frequency.next(),
            KeyCode::Up => {
                form.hour = if form.hour == 23 { 0 } else { form.hour + 1 };
            }
            KeyCode::Down => {
                form.hour = if form.hour == 0 { 23 } else { form.hour - 1 };
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                // Adjust minute
                let digit = c.to_digit(10).unwrap() as u8;
                form.minute = ((form.minute * 10 + digit) % 60).min(59);
            }
            KeyCode::Char(' ') => {
                // Toggle weekday (for weekly) — cycle through days 0-6
                if form.frequency == ScheduleFrequency::Weekly {
                    let idx = form.month_day as usize % 7;
                    form.weekdays[idx] = !form.weekdays[idx];
                    form.month_day = ((form.month_day as usize + 1) % 7) as u8;
                }
            }
            _ => {}
        },
    }
}

fn handle_steps_input(form: &mut NewRoutineForm, key: KeyEvent) {
    if let Some(ref mut text) = form.editing_step {
        match key.code {
            KeyCode::Char(c) => text.push(c),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Esc => form.editing_step = None,
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Char('a') => form.editing_step = Some(String::new()),
            KeyCode::Char('d') => {
                if !form.steps.is_empty() {
                    form.steps.pop();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{NewRoutineForm, ScheduleFrequency};

    #[test]
    fn test_schedule_frequency_next_cycles() {
        assert_eq!(ScheduleFrequency::Hourly.next(), ScheduleFrequency::Daily);
        assert_eq!(ScheduleFrequency::Daily.next(), ScheduleFrequency::Weekly);
        assert_eq!(ScheduleFrequency::Weekly.next(), ScheduleFrequency::Monthly);
        assert_eq!(ScheduleFrequency::Monthly.next(), ScheduleFrequency::Yearly);
        assert_eq!(
            ScheduleFrequency::Yearly.next(),
            ScheduleFrequency::Advanced
        );
        assert_eq!(
            ScheduleFrequency::Advanced.next(),
            ScheduleFrequency::Hourly
        );
    }

    #[test]
    fn test_schedule_frequency_prev_cycles() {
        assert_eq!(ScheduleFrequency::Daily.prev(), ScheduleFrequency::Hourly);
        assert_eq!(
            ScheduleFrequency::Hourly.prev(),
            ScheduleFrequency::Advanced
        );
        assert_eq!(
            ScheduleFrequency::Advanced.prev(),
            ScheduleFrequency::Yearly
        );
    }

    #[test]
    fn test_new_routine_form_defaults() {
        let form = NewRoutineForm::new();
        assert_eq!(form.default_tool, "claude");
        assert_eq!(form.frequency, ScheduleFrequency::Daily);
        assert_eq!(form.hour, 9);
        assert_eq!(form.minute, 0);
        assert!(form.notify);
        assert_eq!(form.step_timeout_secs, 1800);
        assert!(form.steps.is_empty());
        assert!(form.edit_routine_id.is_none());
    }

    #[test]
    fn test_cron_expression_daily() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Daily;
        form.hour = 9;
        form.minute = 0;
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
        assert!(expr.contains("9"));
    }

    #[test]
    fn test_cron_expression_hourly() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Hourly;
        form.minute = 30;
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
        assert!(expr.contains("30"));
    }

    #[test]
    fn test_cron_expression_advanced_returns_raw() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Advanced;
        form.cron_raw = "0 */6 * * *".to_string();
        assert_eq!(form.cron_expression(), "0 */6 * * *");
    }

    #[test]
    fn test_cron_expression_weekly_no_days_falls_back_to_daily() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Weekly;
        form.weekdays = [false; 7];
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
    }
}
