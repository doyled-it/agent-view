pub mod export;
pub mod overlay;
pub mod session;

pub fn handle_main_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
    attach_state: &std::sync::Arc<std::sync::Mutex<crate::core::attach_state::AttachState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    use crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen},
    };

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.should_quit = true;
        }
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            app.move_selection_up();
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            app.move_selection_down();
        }
        (KeyModifiers::NONE, KeyCode::Char('n')) => {
            app.overlay = crate::app::Overlay::NewSession(crate::app::NewSessionForm::new());
        }
        (KeyModifiers::SHIFT, KeyCode::Char('N')) => {
            if let Some(session) = app.selected_session() {
                app.overlay = crate::app::Overlay::AddNote(crate::app::NoteForm {
                    session_id: session.id.clone(),
                    text: String::new(),
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::NONE, KeyCode::Char('l')) => {
            if let Some(group) = app.selected_group() {
                if !group.expanded {
                    let path = group.path.clone();
                    let _ = storage.toggle_group_expanded(&path);
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
            if let Some(group) = app.selected_group() {
                if group.expanded {
                    let path = group.path.clone();
                    let _ = storage.toggle_group_expanded(&path);
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Toggle group expand/collapse, or attach to selected session
            if let Some(group) = app.selected_group() {
                let path = group.path.clone();
                let _ = storage.toggle_group_expanded(&path);
                app.groups = storage.load_groups().unwrap_or_default();
                app.rebuild_list_rows();
            } else if let Some(session) = app.selected_session() {
                if !session.tmux_session.is_empty()
                    && session.status != crate::types::SessionStatus::Stopped
                {
                    let tmux_name = session.tmux_session.clone();
                    if let Ok(mut guard) = attach_state.lock() {
                        guard.attached_session = Some(tmux_name.clone());
                    }

                    // Leave TUI for attach
                    disable_raw_mode()?;
                    // Full terminal reset (\033c) clears screen, scrollback,
                    // alternate screen state, and all attributes in one shot.
                    // This prevents the scroll-to-bottom effect while also
                    // restoring normal terminal mode for paste etc.
                    let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\x1bc");
                    let _ = std::io::Write::flush(&mut std::io::stdout());

                    let _ = crate::core::tmux::attach_session_sync(&tmux_name);

                    if let Ok(mut guard) = attach_state.lock() {
                        guard.suppress_queue.push(tmux_name.clone());
                        guard.attached_session = None;
                    }

                    // Re-enter TUI
                    enable_raw_mode()?;
                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                    terminal.clear()?;

                    // Fresh reload after returning
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                        app.groups = storage.load_groups().unwrap_or_default();
                        app.rebuild_list_rows();
                        // Restore cursor to the session we just detached from
                        if let Some(pos) = app.list_rows.iter().position(|row| {
                            matches!(row, crate::core::groups::ListRow::Session(s) if s.tmux_session == tmux_name)
                        }) {
                            app.selected_index = pos;
                        }
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            if !app.bulk_selected.is_empty() {
                let count = app.bulk_selected.len();
                app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                    message: format!("Stop {} selected sessions?", count),
                    action: crate::app::ConfirmAction::BulkStop,
                });
            } else if let Some(session) = app.selected_session() {
                if session.status != crate::types::SessionStatus::Stopped {
                    let msg = format!("Stop session \"{}\"?", session.title);
                    app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                        message: msg,
                        action: crate::app::ConfirmAction::StopSession(session.id.clone()),
                    });
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            if !app.bulk_selected.is_empty() {
                let count = app.bulk_selected.len();
                app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                    message: format!("Delete {} selected sessions?", count),
                    action: crate::app::ConfirmAction::BulkDelete,
                });
            } else if let Some(session) = app.selected_session() {
                let msg = format!("Delete session \"{}\"?", session.title);
                app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::DeleteSession(session.id.clone()),
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            if let Some(session) = app.selected_session() {
                let id = session.id.clone();
                app.toggle_bulk_select(&id);
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
            app.select_all_visible();
        }
        (KeyModifiers::NONE, KeyCode::Esc) => {
            if !app.bulk_selected.is_empty() {
                app.clear_bulk_selection();
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('r')) => {
            // Restart selected session
            if let Some(session) = app.selected_session() {
                let id = session.id.clone();
                let mut cache = crate::core::tmux::SessionCache::new();
                let _ = session_ops.restart_session(storage, &mut cache, &id);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('!')) => {
            // Toggle notifications for selected session
            if let Some(session) = app.selected_session() {
                let new_val = !session.notify;
                let id = session.id.clone();
                let title = session.title.clone();
                let _ = storage.set_notify(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                let msg = if new_val {
                    format!("Notifications on: {}", title)
                } else {
                    format!("Notifications off: {}", title)
                };
                app.toast_message = Some(msg);
                app.toast_expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('i')) => {
            // Toggle follow-up mark for selected session
            if let Some(session) = app.selected_session() {
                let new_val = !session.follow_up;
                let id = session.id.clone();
                let _ = storage.set_follow_up(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('e')) => {
            if let Some(session) = app.selected_session() {
                if !session.tmux_session.is_empty() {
                    let tmux_name = session.tmux_session.clone();
                    let title = session.title.clone();
                    let id = session.id.clone();
                    match crate::input::export::export_session_log(&tmux_name, &title, &id) {
                        Ok(path) => {
                            app.toast_message = Some(format!("Exported to {}", path));
                        }
                        Err(e) => {
                            app.toast_message = Some(format!("Export failed: {}", e));
                        }
                    }
                    app.toast_expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.search_query = Some(String::new());
        }
        (KeyModifiers::NONE, KeyCode::Char('m')) => {
            if let Some(session) = app.selected_session() {
                let groups: Vec<(String, String)> = app
                    .groups
                    .iter()
                    .map(|g| (g.path.clone(), g.name.clone()))
                    .collect();
                if !groups.is_empty() {
                    app.overlay = crate::app::Overlay::Move(crate::app::MoveForm {
                        session_id: session.id.clone(),
                        session_title: session.title.clone(),
                        groups,
                        selected: 0,
                    });
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('g')) => {
            app.overlay = crate::app::Overlay::GroupManage(crate::app::GroupForm {
                name: String::new(),
            });
        }
        (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
            app.overlay = crate::app::Overlay::CommandPalette(crate::app::CommandPalette::new());
        }
        (KeyModifiers::SHIFT, KeyCode::Char('S')) => {
            app.sort_mode = app.sort_mode.next();
            app.rebuild_list_rows();
            let label = app.sort_mode.label();
            app.toast_message = Some(format!("Sort: {}", label));
            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        }
        (KeyModifiers::NONE, KeyCode::Char('p')) => {
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
        (KeyModifiers::SHIFT, KeyCode::Char('K')) => {
            if let Some(group) = app.selected_group() {
                let path = group.path.clone();
                let groups = storage.load_groups().unwrap_or_default();
                if let Some(pos) = groups.iter().position(|g| g.path == path) {
                    if pos > 0 {
                        let prev_path = groups[pos - 1].path.clone();
                        let _ = storage.swap_group_order(&path, &prev_path);
                        app.groups = storage.load_groups().unwrap_or_default();
                        app.rebuild_list_rows();
                        app.move_selection_up();
                        let _ = storage.touch();
                    }
                }
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Char('J')) => {
            if let Some(group) = app.selected_group() {
                let path = group.path.clone();
                let groups = storage.load_groups().unwrap_or_default();
                if let Some(pos) = groups.iter().position(|g| g.path == path) {
                    if pos < groups.len() - 1 {
                        let next_path = groups[pos + 1].path.clone();
                        let _ = storage.swap_group_order(&path, &next_path);
                        app.groups = storage.load_groups().unwrap_or_default();
                        app.rebuild_list_rows();
                        app.move_selection_down();
                        let _ = storage.touch();
                    }
                }
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Char('R')) => {
            if let Some(session) = app.selected_session() {
                app.overlay = crate::app::Overlay::Rename(crate::app::RenameForm {
                    target_id: session.id.clone(),
                    target_type: crate::app::RenameTarget::Session,
                    input: session.title.clone(),
                });
            } else if let Some(group) = app.selected_group() {
                app.overlay = crate::app::Overlay::Rename(crate::app::RenameForm {
                    target_id: group.path.clone(),
                    target_type: crate::app::RenameTarget::Group,
                    input: group.name.clone(),
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            app.show_activity_feed = !app.show_activity_feed;
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) => {
            app.overlay = crate::app::Overlay::Help;
        }
        (KeyModifiers::NONE, KeyCode::Char('t')) => {
            app.overlay =
                crate::app::Overlay::ThemeSelect(crate::app::ThemeSelectForm::new(&app.theme_name));
        }
        _ => {}
    }

    Ok(())
}
