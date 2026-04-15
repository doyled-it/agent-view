//! Session overlay keyboard handlers

pub fn handle_new_session_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::NewSession(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Tab => {
                if form.focused_field == 1 {
                    // Path field: do filesystem completion
                    if !form.completions.is_empty() && form.completions.len() > 1 {
                        // Already have ambiguous completions — cycle through them
                        let idx = match form.completion_index {
                            Some(i) => (i + 1) % form.completions.len(),
                            None => 0,
                        };
                        form.completion_index = Some(idx);
                        // Build the completed path from parent + candidate
                        let expanded = if form.project_path.starts_with('~') {
                            let home = dirs::home_dir()
                                .map(|h| h.to_string_lossy().to_string())
                                .unwrap_or_default();
                            form.project_path.replacen('~', &home, 1)
                        } else {
                            form.project_path.clone()
                        };
                        let parent = if let Some(pos) = expanded.rfind('/') {
                            &expanded[..=pos]
                        } else {
                            ""
                        };
                        let candidate = &form.completions[idx];
                        let new_path = format!("{}{}/", parent, candidate);
                        // Restore ~ if original used it
                        if form.project_path.starts_with('~') {
                            let home = dirs::home_dir()
                                .map(|h| h.to_string_lossy().to_string())
                                .unwrap_or_default();
                            if let Some(rest) = new_path.strip_prefix(&home) {
                                form.project_path = format!("~{}", rest);
                            } else {
                                form.project_path = new_path;
                            }
                        } else {
                            form.project_path = new_path;
                        }
                    } else {
                        // First Tab press — get completions
                        let result =
                            crate::core::path_complete::complete_path(&form.project_path);
                        form.project_path = result.completed;
                        form.completions = result.candidates;
                        form.completion_index = None;
                    }
                } else {
                    // Title field: advance to path field
                    form.focused_field = 1;
                    form.completions.clear();
                    form.completion_index = None;
                }
            }
            KeyCode::BackTab => {
                form.focused_field = if form.focused_field == 0 { 1 } else { 0 };
                form.completions.clear();
                form.completion_index = None;
            }
            KeyCode::Enter => {
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
                };

                let mut cache = crate::core::tmux::SessionCache::new();
                match session_ops.create_session(storage, &mut cache, options) {
                    Ok(_) => {
                        if let Ok(sessions) = storage.load_sessions() {
                            app.sessions = sessions;
                            app.groups = storage.load_groups().unwrap_or_default();
                            app.rebuild_list_rows();
                            // Select the newly created session (last row)
                            if !app.list_rows.is_empty() {
                                app.selected_index = app.list_rows.len() - 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to create session: {}", e);
                    }
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Char(c) => match form.focused_field {
                0 => form.title.push(c),
                1 => {
                    form.project_path.push(c);
                    form.completions.clear();
                    form.completion_index = None;
                }
                _ => {}
            },
            KeyCode::Backspace => match form.focused_field {
                0 => {
                    form.title.pop();
                }
                1 => {
                    form.project_path.pop();
                    form.completions.clear();
                    form.completion_index = None;
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
            KeyCode::Up | KeyCode::Char('k') => {
                if form.selected > 0 {
                    form.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if form.selected < form.groups.len().saturating_sub(1) {
                    form.selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some((ref path, ref name)) = form.groups.get(form.selected).cloned() {
                    let _ = storage.move_session_to_group(&form.session_id.clone(), path);
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                    }
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
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
