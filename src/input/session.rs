//! Session overlay keyboard handlers

pub fn handle_new_session_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{KeyCode, KeyModifiers};

    if let crate::app::Overlay::NewSession(ref mut form) = app.overlay {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                app.overlay = crate::app::Overlay::None;
            }
            // Ctrl+T must precede the generic Char arm so it doesn't append 't'
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                form.worktree_new_branch = !form.worktree_new_branch;
                form.error = None;
            }
            // Ctrl+S or Super+S — submit
            (m, KeyCode::Char('s'))
                if m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::SUPER) =>
            {
                let wt_branch_trimmed = form.worktree_branch.trim().to_string();
                let worktree = if wt_branch_trimmed.is_empty() {
                    None
                } else {
                    if let Some(err) = crate::core::git::validate_branch_name(&wt_branch_trimmed) {
                        form.error = Some(err);
                        return Ok(());
                    }
                    if !crate::core::git::is_git_repo(&form.project_path) {
                        form.error = Some("Project path is not a git repository".to_string());
                        return Ok(());
                    }
                    let exists =
                        crate::core::git::branch_exists(&form.project_path, &wt_branch_trimmed);
                    if form.worktree_new_branch && exists {
                        form.error = Some(format!(
                            "Branch '{}' already exists — toggle to attach (^t)",
                            wt_branch_trimmed
                        ));
                        return Ok(());
                    }
                    if !form.worktree_new_branch && !exists {
                        form.error = Some(format!(
                            "Branch '{}' does not exist — toggle to create (^t)",
                            wt_branch_trimmed
                        ));
                        return Ok(());
                    }
                    let base = form.worktree_base.trim().to_string();
                    Some(crate::types::WorktreeCreateOptions {
                        branch: wt_branch_trimmed,
                        new_branch: form.worktree_new_branch,
                        base: if base.is_empty() { None } else { Some(base) },
                    })
                };

                let title = if form.title.is_empty() {
                    None
                } else {
                    Some(form.title.clone())
                };
                let project_path = form.project_path.clone();

                let options = crate::types::SessionCreateOptions {
                    title,
                    project_path,
                    group_path: None,
                    tool: crate::types::Tool::Claude,
                    command: None,
                    worktree,
                };

                let mut cache = crate::core::tmux::SessionCache::new();
                match session_ops.create_session(storage, &mut cache, options) {
                    Ok((_, warn)) => {
                        if let Some(msg) = warn {
                            app.toast_message = Some(msg);
                            app.toast_expire =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
                        }
                        if let Ok(sessions) = storage.load_sessions() {
                            app.sessions = sessions;
                            app.groups = storage.load_groups().unwrap_or_default();
                            app.rebuild_list_rows();
                            if !app.list_rows.is_empty() {
                                app.selected_index = app.list_rows.len() - 1;
                            }
                        }
                        app.overlay = crate::app::Overlay::None;
                    }
                    Err(e) => {
                        form.error = Some(e);
                    }
                }
            }
            (_, KeyCode::Tab) => {
                match form.focused_field {
                    1 => {
                        // Path field: filesystem completion
                        if !form.completions.is_empty() && form.completions.len() > 1 {
                            // Cycle: rebuild path from the captured base + next candidate.
                            let idx = match form.completion_index {
                                Some(i) => (i + 1) % form.completions.len(),
                                None => 0,
                            };
                            form.completion_index = Some(idx);
                            let candidate = &form.completions[idx];
                            form.project_path = format!("{}{}/", form.completion_base, candidate);
                        } else {
                            // First Tab press — fetch completions and remember the base.
                            let result =
                                crate::core::path_complete::complete_path(&form.project_path);
                            form.project_path = result.completed;
                            form.completions = result.candidates;
                            form.completion_index = None;
                            // Base = directory containing the candidates. If the completed
                            // path ends with '/', it IS the parent. Otherwise strip its last
                            // (partial) segment.
                            form.completion_base = if form.project_path.ends_with('/') {
                                form.project_path.clone()
                            } else if let Some(pos) = form.project_path.rfind('/') {
                                form.project_path[..=pos].to_string()
                            } else {
                                String::new()
                            };
                        }
                    }
                    2 => {
                        // Branch field: local-branch completion
                        if !form.completions.is_empty() && form.completions.len() > 1 {
                            let idx = match form.completion_index {
                                Some(i) => (i + 1) % form.completions.len(),
                                None => 0,
                            };
                            form.completion_index = Some(idx);
                            form.worktree_branch = form.completions[idx].clone();
                        } else {
                            let all = crate::core::git::list_local_branches(&form.project_path)
                                .unwrap_or_default();
                            let prefix = form.worktree_branch.clone();
                            let candidates: Vec<String> =
                                all.into_iter().filter(|b| b.starts_with(&prefix)).collect();
                            match candidates.len() {
                                0 => {} // no-op
                                1 => {
                                    form.worktree_branch = candidates.into_iter().next().unwrap();
                                    form.clear_completions();
                                }
                                _ => {
                                    form.completions = candidates;
                                    form.completion_index = None;
                                }
                            }
                        }
                    }
                    _ => {
                        // Fields 0 and 3: advance focus
                        form.focused_field = (form.focused_field + 1) % 4;
                        form.clear_completions();
                    }
                }
            }
            (_, KeyCode::BackTab) => {
                form.focused_field = (form.focused_field + 3) % 4;
                form.clear_completions();
            }
            (_, KeyCode::Down) => {
                form.focused_field = (form.focused_field + 1) % 4;
                form.clear_completions();
            }
            (_, KeyCode::Up) => {
                form.focused_field = (form.focused_field + 3) % 4;
                form.clear_completions();
            }
            (_, KeyCode::Enter) => {
                // Advance focus forward — NEVER submits
                form.focused_field = (form.focused_field + 1) % 4;
                form.clear_completions();
            }
            // Generic Char arm — guard excludes Ctrl and Super so those don't append
            (m, KeyCode::Char(c))
                if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::SUPER) =>
            {
                match form.focused_field {
                    0 => form.title.push(c),
                    1 => {
                        form.project_path.push(c);
                        form.clear_completions();
                    }
                    2 => {
                        form.worktree_branch.push(c);
                        form.error = None;
                    }
                    3 => {
                        form.worktree_base.push(c);
                        form.error = None;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Backspace) => match form.focused_field {
                0 => {
                    form.title.pop();
                }
                1 => {
                    form.project_path.pop();
                    form.clear_completions();
                }
                2 => {
                    form.worktree_branch.pop();
                    form.error = None;
                }
                3 => {
                    form.worktree_base.pop();
                    form.error = None;
                }
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}

pub fn handle_confirm_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Confirm(ref dialog) = app.overlay.clone() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match &dialog.action {
                    crate::app::ConfirmAction::DeleteSession(id) => {
                        let mut cache = crate::core::tmux::SessionCache::new();
                        let _ = session_ops.delete_session(storage, &mut cache, id);
                    }
                    crate::app::ConfirmAction::StopSession(id) => {
                        let _ = session_ops.stop_session(storage, id);
                    }
                    crate::app::ConfirmAction::BulkDelete => {
                        let ids: Vec<String> = app.bulk_selected.iter().cloned().collect();
                        let mut cache = crate::core::tmux::SessionCache::new();
                        for id in &ids {
                            let _ = session_ops.delete_session(storage, &mut cache, id);
                        }
                        app.clear_bulk_selection();
                    }
                    crate::app::ConfirmAction::BulkStop => {
                        let ids: Vec<String> = app.bulk_selected.iter().cloned().collect();
                        for id in &ids {
                            let _ = session_ops.stop_session(storage, id);
                        }
                        app.clear_bulk_selection();
                    }
                    crate::app::ConfirmAction::DeleteGroup(path) => {
                        // Move sessions in this group to the default group
                        for s in &app.sessions {
                            if s.group_path == *path {
                                let _ = storage.move_session_to_group(&s.id, "my-sessions");
                            }
                        }
                        let _ = storage.delete_group(path);
                        storage.touch().ok();
                    }
                    crate::app::ConfirmAction::DeleteRoutine(id) => {
                        let scheduler = crate::core::scheduler::platform_scheduler();
                        let _ = scheduler.uninstall(id);
                        let _ = storage.delete_routine(id);
                        app.routines = storage.load_routines().unwrap_or_default();
                        app.routine_runs_cache.remove(id);
                        app.rebuild_routine_list_rows();
                        storage.touch().ok();
                    }
                    crate::app::ConfirmAction::FinishSession(id) => {
                        let mut cache = crate::core::tmux::SessionCache::new();
                        match session_ops.finish_session(storage, &mut cache, id, true) {
                            Ok(outcome) => {
                                let msg = match (
                                    outcome.worktree_removed,
                                    outcome.branch_deleted,
                                    outcome.branch_skipped_unmerged,
                                ) {
                                    (true, true, _) => "Worktree removed and branch deleted",
                                    (true, false, true) => {
                                        "Worktree removed; branch kept (not merged)"
                                    }
                                    (true, false, false) => "Worktree removed",
                                    _ => "Session finished",
                                };
                                app.toast_message = Some(msg.to_string());
                                app.toast_expire = Some(
                                    std::time::Instant::now() + std::time::Duration::from_secs(5),
                                );
                            }
                            Err(e) => {
                                app.toast_message = Some(format!("Finish failed: {}", e));
                                app.toast_expire = Some(
                                    std::time::Instant::now() + std::time::Duration::from_secs(6),
                                );
                            }
                        }
                    }
                }
                // Refresh sessions
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn handle_rename_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Rename(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Enter => {
                let new_name = form.input.trim().to_string();
                if !new_name.is_empty() {
                    match form.target_type {
                        crate::app::RenameTarget::Session => {
                            let _ = storage.rename_session(&form.target_id, &new_name);
                        }
                        crate::app::RenameTarget::Group => {
                            if let Ok(groups) = storage.load_groups() {
                                if let Some(mut group) =
                                    groups.into_iter().find(|g| g.path == form.target_id)
                                {
                                    group.name = new_name;
                                    let _ = storage.save_group(&group);
                                }
                            }
                        }
                        crate::app::RenameTarget::Routine => {
                            let _ = storage.rename_routine(&form.target_id, &new_name);
                            app.routines = storage.load_routines().unwrap_or_default();
                            app.rebuild_routine_list_rows();
                            storage.touch().ok();
                        }
                    }
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                    }
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Backspace => {
                form.input.pop();
            }
            KeyCode::Char(c) => {
                form.input.push(c);
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn handle_move_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Move(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Up | KeyCode::Char('k') if form.selected > 0 => {
                form.selected -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if form.selected < form.groups.len().saturating_sub(1) =>
            {
                form.selected += 1;
            }
            KeyCode::Enter => {
                if let Some((ref path, ref name)) = form.groups.get(form.selected).cloned() {
                    match app.active_tab {
                        crate::app::ActiveTab::Sessions => {
                            let _ = storage.move_session_to_group(&form.session_id.clone(), path);
                            if let Ok(sessions) = storage.load_sessions() {
                                app.sessions = sessions;
                            }
                            app.groups = storage.load_groups().unwrap_or_default();
                            app.rebuild_list_rows();
                        }
                        crate::app::ActiveTab::Routines => {
                            let _ = storage.move_routine_to_group(&form.session_id.clone(), path);
                            app.routines = storage.load_routines().unwrap_or_default();
                            app.rebuild_routine_list_rows();
                        }
                    }
                    app.toast_message = Some(format!("Moved to {}", name));
                    app.toast_expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                app.overlay = crate::app::Overlay::None;
            }
            _ => {}
        }
    }
    Ok(())
}
