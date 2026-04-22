pub fn handle_group_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::GroupManage(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Enter => {
                let name = form.name.trim().to_string();
                if !name.is_empty() {
                    let path = name
                        .to_lowercase()
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '-' })
                        .collect::<String>();
                    let path = path.trim_matches('-').to_string();

                    let order = app.groups.len() as i32;
                    let group = crate::types::Group {
                        path,
                        name,
                        expanded: true,
                        order,
                        default_path: String::new(),
                    };
                    let _ = storage.save_group(&group);
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Backspace => {
                form.name.pop();
            }
            KeyCode::Char(c) => {
                form.name.push(c);
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn handle_palette_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::CommandPalette(ref mut palette) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Up | KeyCode::BackTab if palette.selected > 0 => {
                palette.selected -= 1;
            }
            KeyCode::Down | KeyCode::Tab
                if palette.selected < palette.filtered.len().saturating_sub(1) =>
            {
                palette.selected += 1;
            }
            KeyCode::Enter => {
                if let Some(&idx) = palette.filtered.get(palette.selected) {
                    let action = palette.items[idx].action.clone();
                    app.overlay = crate::app::Overlay::None;
                    execute_command_action(app, action, storage, session_ops)?;
                }
            }
            KeyCode::Backspace => {
                palette.query.pop();
                palette.filter();
            }
            KeyCode::Char(c) => {
                palette.query.push(c);
                palette.filter();
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn execute_command_action(
    app: &mut crate::app::App,
    action: crate::app::CommandAction,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::app::{CommandAction, Overlay};

    match action {
        CommandAction::NewSession => {
            app.overlay = Overlay::NewSession(crate::app::NewSessionForm::new());
        }
        CommandAction::Search => {
            app.search_query = Some(String::new());
        }
        CommandAction::CreateGroup => {
            app.overlay = Overlay::GroupManage(crate::app::GroupForm {
                name: String::new(),
            });
        }
        CommandAction::DeleteGroup => {
            if let Some(group) = app.selected_group() {
                if group.path != "my-sessions" {
                    let msg = format!("Delete group \"{}\"?", group.name);
                    app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                        message: msg,
                        action: crate::app::ConfirmAction::DeleteGroup(group.path.clone()),
                    });
                }
            }
        }
        CommandAction::Quit => {
            app.should_quit = true;
        }
        CommandAction::StopSession => {
            if let Some(session) = app.selected_session() {
                let msg = format!("Stop session \"{}\"?", session.title);
                app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::StopSession(session.id.clone()),
                });
            }
        }
        CommandAction::DeleteSession => {
            if let Some(session) = app.selected_session() {
                let msg = format!("Delete session \"{}\"?", session.title);
                app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::DeleteSession(session.id.clone()),
                });
            }
        }
        CommandAction::RestartSession => {
            if let Some(session) = app.selected_session() {
                let id = session.id.clone();
                let mut cache = crate::core::tmux::SessionCache::new();
                let _ = session_ops.restart_session(storage, &mut cache, &id);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::RenameSession => {
            if let Some(session) = app.selected_session() {
                app.overlay = Overlay::Rename(crate::app::RenameForm {
                    target_id: session.id.clone(),
                    target_type: crate::app::RenameTarget::Session,
                    input: session.title.clone(),
                });
            }
        }
        CommandAction::MoveSession => {
            if let Some(session) = app.selected_session() {
                let groups: Vec<(String, String)> = app
                    .groups
                    .iter()
                    .map(|g| (g.path.clone(), g.name.clone()))
                    .collect();
                if !groups.is_empty() {
                    app.overlay = Overlay::Move(crate::app::MoveForm {
                        session_id: session.id.clone(),
                        session_title: session.title.clone(),
                        groups,
                        selected: 0,
                    });
                }
            }
        }
        CommandAction::ToggleNotify => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.notify;
                let id = session.id.clone();
                let _ = storage.set_notify(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::ToggleFollowUp => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.follow_up;
                let id = session.id.clone();
                let _ = storage.set_follow_up(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::ExportLog => {
            if let Some(session) = app.selected_session() {
                if !session.tmux_session.is_empty() {
                    let tmux_name = session.tmux_session.clone();
                    let title = session.title.clone();
                    let id = session.id.clone();
                    match crate::input::export::export_session_log(&tmux_name, &title, &id) {
                        Ok(path) => {
                            app.toast_message = Some(format!("Exported to {}", path));
                            app.toast_expire =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                        }
                        Err(e) => {
                            app.toast_message = Some(format!("Export failed: {}", e));
                            app.toast_expire =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                        }
                    }
                }
            }
        }
        CommandAction::CycleSort => {
            app.sort_mode = app.sort_mode.next();
            app.rebuild_list_rows();
            let label = app.sort_mode.label();
            app.toast_message = Some(format!("Sort: {}", label));
            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        }
        CommandAction::PinSession => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.pinned;
                let id = session.id.clone();
                let title = session.title.clone();
                let _ = storage.set_pinned(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                let msg = if new_val {
                    format!("Pinned: {}", title)
                } else {
                    format!("Unpinned: {}", title)
                };
                app.toast_message = Some(msg);
                app.toast_expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
        }
        CommandAction::ShowHelp => {
            app.overlay = Overlay::Help;
        }
        CommandAction::SelectTheme => {
            app.overlay = Overlay::ThemeSelect(crate::app::ThemeSelectForm::new(&app.theme_name));
        }
        CommandAction::CyclePanel => {
            app.detail_mode = app.detail_mode.next();
            let mut config = crate::core::config::load_config();
            config.detail_panel_mode = app.detail_mode.as_config_str().to_string();
            let _ = crate::core::config::save_config(&config);
            app.config_changed
                .store(false, std::sync::atomic::Ordering::Relaxed);
            app.preview_content.clear();
            app.preview_last_session = None;
            app.preview_last_capture = None;
            app.toast_message = Some(format!("Panel: {}", app.detail_mode.label()));
            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        }
        CommandAction::NewRoutine => {
            app.active_tab = crate::app::ActiveTab::Routines;
            app.overlay = Overlay::NewRoutine(crate::app::NewRoutineForm::new());
        }
        CommandAction::ToggleRoutine => {
            app.active_tab = crate::app::ActiveTab::Routines;
            if let Some(crate::app::RoutineListRow::Routine(routine)) = app
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
            }
        }
        CommandAction::DeleteRoutine => {
            app.active_tab = crate::app::ActiveTab::Routines;
            if let Some(crate::app::RoutineListRow::Routine(routine)) = app
                .routine_list_rows
                .get(app.routine_selected_index)
                .cloned()
            {
                app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                    message: format!("Delete routine '{}'?", routine.name),
                    action: crate::app::ConfirmAction::DeleteRoutine(routine.id.clone()),
                });
            }
        }
    }
    Ok(())
}

pub fn handle_theme_select_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::ThemeSelect(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                let original = form.original_theme_name.clone();
                app.theme = crate::ui::theme::Theme::from_name(&original);
                app.theme_name = original;
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Enter => {
                let chosen = form.options[form.selected].clone();
                let mut config = crate::core::config::load_config();
                config.theme = chosen.clone();
                let _ = crate::core::config::save_config(&config);
                app.config_changed
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                app.theme_name = chosen.clone();
                app.overlay = crate::app::Overlay::None;
                app.toast_message = Some(format!("Theme: {}", chosen));
                app.toast_expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if form.selected > 0 {
                    form.selected -= 1;
                }
                app.theme = crate::ui::theme::Theme::from_name(&form.options[form.selected]);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if form.selected < form.options.len() - 1 {
                    form.selected += 1;
                }
                app.theme = crate::ui::theme::Theme::from_name(&form.options[form.selected]);
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn handle_add_note_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    use crossterm::event::KeyModifiers;

    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => {
            app.overlay = crate::app::Overlay::None;
        }
        (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
            if let crate::app::Overlay::AddNote(ref mut form) = app.overlay {
                form.text.push('\n');
            }
        }
        (_, KeyCode::Enter) => {
            let (text, session_id) = if let crate::app::Overlay::AddNote(ref form) = app.overlay {
                (form.text.trim().to_string(), form.session_id.clone())
            } else {
                return Ok(());
            };
            if !text.is_empty() {
                let note = crate::types::NoteEntry {
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    text,
                };
                if let Some(session) = app.sessions.iter_mut().find(|s| s.id == session_id) {
                    session.notes.push(note);
                    let _ = storage.save_session(session);
                    storage.touch().ok();
                }
                app.rebuild_list_rows();
            }
            app.overlay = crate::app::Overlay::None;
        }
        (_, KeyCode::Backspace) => {
            if let crate::app::Overlay::AddNote(ref mut form) = app.overlay {
                form.text.pop();
            }
        }
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            if let crate::app::Overlay::AddNote(ref mut form) = app.overlay {
                form.text.push(c);
            }
        }
        _ => {}
    }
    Ok(())
}
