use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::app::{
    App, ConfirmAction, ConfirmDialog, MoveForm, NewRoutineForm, Overlay, RenameForm, RenameTarget,
    RoutineListRow,
};
use crate::core::routine::build_promoted_session;
use crate::core::scheduler::platform_scheduler;
use crate::core::storage::Storage;
use crate::core::tmux::{
    attach_inspect_session_sync, create_session, generate_session_name, kill_session,
    session_exists,
};
use crate::types::{RoutineStep, Tool};

/// Handle key input when on the Routines tab (no overlay active)
pub fn handle_routine_list_key(
    app: &mut App,
    key: KeyEvent,
    storage: &Storage,
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
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j'))
            if !app.routine_list_rows.is_empty() =>
        {
            if app.routine_selected_index < app.routine_list_rows.len() - 1 {
                app.routine_selected_index += 1;
            } else {
                app.routine_selected_index = 0;
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

                let scheduler = platform_scheduler();
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
                    app.overlay = Overlay::Confirm(ConfirmDialog {
                        message: format!("Delete routine '{}'?", routine.name),
                        action: ConfirmAction::DeleteRoutine(routine.id.clone()),
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
                    let mut session = build_promoted_session(&run, &routine);

                    let tmux_alive = run
                        .tmux_session
                        .as_ref()
                        .map(|t| session_exists(t))
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
                                RoutineStep::Claude { .. } => Tool::Claude,
                                RoutineStep::Shell { .. } => Tool::Shell,
                            })
                            .unwrap_or(Tool::Shell);

                        let tmux_name =
                            generate_session_name(&format!("promoted_{}", routine.name));
                        let command = match (tool, claude_session_id) {
                            (Tool::Claude, Some(sid)) => Some(format!("claude --resume {}", sid)),
                            _ => None,
                        };
                        let _ = create_session(
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

                    app.toast.message = Some(format!("Promoted to session: {}", session_title));
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                }
            }
        }

        // m: move routine to group
        (KeyModifiers::NONE, KeyCode::Char('m')) => {
            if let Some(RoutineListRow::Routine(routine)) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                let groups: Vec<(String, String)> = app
                    .groups
                    .iter()
                    .map(|g| (g.path.clone(), g.name.clone()))
                    .collect();
                if !groups.is_empty() {
                    app.overlay = Overlay::Move(MoveForm {
                        session_id: routine.id.clone(),
                        session_title: routine.name.clone(),
                        groups,
                        selected: 0,
                    });
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
                app.overlay = Overlay::Rename(RenameForm {
                    target_id: routine.id.clone(),
                    target_type: RenameTarget::Routine,
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
                    let tmux_name = generate_session_name(&format!("inspect_{}", routine.name));

                    let tool_data: serde_json::Value = serde_json::from_str(&run.tool_data)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let claude_session_id = tool_data
                        .get("claude_session_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let last_step_is_claude = routine
                        .steps
                        .last()
                        .map(|s| matches!(s, RoutineStep::Claude { .. }))
                        .unwrap_or(false);

                    let command = if last_step_is_claude {
                        claude_session_id
                            .as_ref()
                            .map(|sid| format!("claude --resume {}", sid))
                    } else {
                        run.log_path.as_ref().map(|p| format!("less {}", p))
                    };

                    if let Err(e) = create_session(
                        &tmux_name,
                        command.as_deref(),
                        Some(&routine.working_dir),
                        None,
                    ) {
                        app.toast.message =
                            Some(format!("Failed to create inspect session: {}", e));
                        app.toast.expire =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                        return;
                    }

                    // Leave TUI
                    let _ = disable_raw_mode();

                    let promote_result = attach_inspect_session_sync(&tmux_name, &run.id);

                    // Re-enter TUI
                    let _ = enable_raw_mode();
                    let _ = execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
                    let _ = terminal.clear();

                    match promote_result {
                        Ok(true) => {
                            // User pressed Ctrl+P — promote the run
                            let session = build_promoted_session(&run, &routine);
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
                                app.routine_runs_cache.insert(run.routine_id.clone(), runs);
                            }
                            app.rebuild_routine_list_rows();
                            storage.touch().ok();

                            app.toast.message =
                                Some(format!("Promoted to session: {}", session_title));
                            app.toast.expire =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                        }
                        Ok(false) => {
                            // Normal detach — kill the ephemeral tmux session
                            let _ = kill_session(&tmux_name);
                        }
                        Err(e) => {
                            let _ = kill_session(&tmux_name);
                            app.toast.message = Some(format!("Inspect failed: {}", e));
                            app.toast.expire =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                        }
                    }
                }
            }
        }

        _ => {}
    }
}
