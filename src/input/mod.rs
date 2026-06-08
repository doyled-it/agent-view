pub mod costs;
pub mod export;
pub mod overlay;
pub mod routine;
pub mod session;

fn reload_sessions_and_groups(app: &mut crate::app::App, storage: &crate::core::storage::Storage) {
    if let Ok(sessions) = storage.load_sessions() {
        app.sessions = sessions;
        app.groups = storage.load_groups().unwrap_or_default();
        app.rebuild_list_rows();
    }
}

fn restore_selected_session(app: &mut crate::app::App, session_id: &str) {
    if let Some(index) = app.list_rows.iter().position(|row| {
        matches!(row, crate::core::groups::ListRow::Session { session, .. } if session.id == session_id)
    }) {
        app.selected_index = index;
    }
}

fn handle_conductor_left(
    app: &mut crate::app::App,
    storage: &crate::core::storage::Storage,
) -> bool {
    if app.selected_session_depth().is_some_and(|depth| depth > 0) {
        if let Some(index) = app.selected_parent_conductor_index() {
            app.selected_index = index;
        }
        return true;
    }

    let Some(session) = app.selected_session() else {
        return false;
    };
    if session.role != crate::types::SessionRole::Conductor || !session.conductor_expanded {
        return false;
    }

    let id = session.id.clone();
    let _ = storage.set_conductor_expanded(&id, false);
    reload_sessions_and_groups(app, storage);
    restore_selected_session(app, &id);
    true
}

fn handle_conductor_right(
    app: &mut crate::app::App,
    storage: &crate::core::storage::Storage,
) -> bool {
    let Some(session) = app.selected_session() else {
        return false;
    };
    if session.role != crate::types::SessionRole::Conductor || session.conductor_expanded {
        return false;
    }

    let id = session.id.clone();
    let _ = storage.set_conductor_expanded(&id, true);
    reload_sessions_and_groups(app, storage);
    restore_selected_session(app, &id);
    true
}

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

    // Handle routine overlay input before the main key dispatch
    if let crate::app::Overlay::NewRoutine(_) = &app.overlay {
        crate::input::routine::handle_new_routine_key(app, key, storage);
        return Ok(());
    }

    // When on Routines tab, delegate to routine-specific handler for most keys
    if app.active_tab == crate::app::ActiveTab::Routines && app.overlay == crate::app::Overlay::None
    {
        let pass_through = matches!(
            (key.modifiers, key.code),
            (KeyModifiers::NONE, KeyCode::Char('q'))
                | (KeyModifiers::CONTROL, KeyCode::Char('c'))
                | (KeyModifiers::NONE, KeyCode::Tab)
                | (KeyModifiers::NONE, KeyCode::Char('?'))
                | (KeyModifiers::CONTROL, KeyCode::Char('k'))
                | (KeyModifiers::SHIFT, KeyCode::Char('M'))
                | (KeyModifiers::NONE, KeyCode::Char('n'))
                | (KeyModifiers::NONE, KeyCode::Char('v'))
                | (KeyModifiers::NONE, KeyCode::Char('/'))
        );
        if !pass_through {
            crate::input::routine::handle_routine_list_key(app, key, storage, terminal);
            return Ok(());
        }
    }

    if app.active_tab == crate::app::ActiveTab::Costs
        && app.overlay == crate::app::Overlay::None
        && costs::handle_costs_key(app, key)
    {
        return Ok(());
    }

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.should_quit = true;
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.toggle_tab();
        }
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            app.move_selection_up();
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            app.move_selection_down();
        }
        (KeyModifiers::NONE, KeyCode::Char('n')) => match app.active_tab {
            crate::app::ActiveTab::Sessions => {
                crate::input::overlay::open_new_session_overlay(app);
            }
            crate::app::ActiveTab::Routines => {
                app.overlay = crate::app::Overlay::NewRoutine(crate::app::NewRoutineForm::new());
            }
            crate::app::ActiveTab::Costs => {
                // No-op on Costs tab.
            }
        },
        (KeyModifiers::SHIFT, KeyCode::Char('N')) => {
            if let Some(session) = app.selected_session() {
                app.overlay = crate::app::Overlay::AddNote(crate::app::NoteForm {
                    session_id: session.id.clone(),
                    text: String::new(),
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::NONE, KeyCode::Char('l')) => {
            if handle_conductor_right(app, storage) {
                // handled
            } else if let Some(group) = app.selected_group() {
                if !group.expanded {
                    let path = group.path.clone();
                    let _ = storage.toggle_group_expanded(&path);
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
            if handle_conductor_left(app, storage) {
                // handled
            } else if let Some(group) = app.selected_group() {
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
                            matches!(row, crate::core::groups::ListRow::Session { session, .. } if session.tmux_session == tmux_name)
                        }) {
                            app.selected_index = pos;
                        }
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            if !app.bulk.selected.is_empty() {
                let count = app.bulk.selected.len();
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
            if !app.bulk.selected.is_empty() {
                let count = app.bulk.selected.len();
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
            } else if let Some(group) = app.selected_group() {
                if group.path != crate::core::groups::DEFAULT_GROUP_PATH {
                    let msg = format!("Delete group \"{}\"?", group.name);
                    app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                        message: msg,
                        action: crate::app::ConfirmAction::DeleteGroup(group.path.clone()),
                    });
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('f')) => {
            if let Some(session) = app.selected_session() {
                if !session.worktree_path.is_empty() {
                    app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                        message: format!(
                            "Finish '{}'? Removes worktree {} and (if merged into main/master) branch {}.",
                            session.title, session.worktree_path, session.worktree_branch
                        ),
                        action: crate::app::ConfirmAction::FinishSession(session.id.clone()),
                    });
                } else {
                    app.toast.message =
                        Some("Session has no worktree — use 'd' to delete".to_string());
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                }
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
        (KeyModifiers::NONE, KeyCode::Esc) if !app.bulk.selected.is_empty() => {
            app.clear_bulk_selection();
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
                app.toast.message = Some(msg);
                app.toast.expire =
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
        (KeyModifiers::NONE, KeyCode::Char('w')) => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.user_waiting;
                let id = session.id.clone();
                let title = session.title.clone();
                let _ = storage.set_user_waiting(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                let msg = if new_val {
                    format!("Waiting on: {}", title)
                } else {
                    format!("No longer waiting: {}", title)
                };
                app.toast.message = Some(msg);
                app.toast.expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
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
                            app.toast.message = Some(format!("Exported to {}", path));
                        }
                        Err(e) => {
                            app.toast.message = Some(format!("Export failed: {}", e));
                        }
                    }
                    app.toast.expire =
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
        (KeyModifiers::SHIFT, KeyCode::Char('M')) => {
            crate::input::overlay::open_mcp_profiles_overlay(app);
        }
        (KeyModifiers::SHIFT, KeyCode::Char('S')) => {
            app.sort_mode = app.sort_mode.next();
            app.rebuild_list_rows();
            let label = app.sort_mode.label();
            app.toast.message = Some(format!("Sort: {}", label));
            app.toast.expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
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
                app.toast.message = Some(msg);
                app.toast.expire =
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
                        if let Some(idx) = app.list_rows.iter().position(|r| {
                            matches!(r, crate::core::groups::ListRow::Group { group, .. } if group.path == path)
                        }) {
                            app.selected_index = idx;
                        }
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
                        if let Some(idx) = app.list_rows.iter().position(|r| {
                            matches!(r, crate::core::groups::ListRow::Group { group, .. } if group.path == path)
                        }) {
                            app.selected_index = idx;
                        }
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
        (KeyModifiers::NONE, KeyCode::Char('v')) => {
            app.detail_mode = app.detail_mode.next();
            // Persist to config
            let mut config = crate::core::config::load_config();
            config.detail_panel_mode = app.detail_mode.as_config_str().to_string();
            let _ = crate::core::config::save_config(&config);
            // Suppress config watcher from re-applying (we just wrote it)
            app.config_changed
                .store(false, std::sync::atomic::Ordering::Relaxed);
            // Clear preview state on mode change
            app.preview.content.clear();
            app.preview.last_session = None;
            app.preview.last_capture = None;
            // Toast
            app.toast.message = Some(format!("Panel: {}", app.detail_mode.label()));
            app.toast.expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        }
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            app.activity.show_feed = !app.activity.show_feed;
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal, TerminalOptions, Viewport};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    }

    fn main_test_terminal() -> Terminal<CrosstermBackend<std::io::Stdout>> {
        let backend = CrosstermBackend::new(std::io::stdout());
        let options = TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
        };
        Terminal::with_options(backend, options).unwrap()
    }

    fn conductor_tree_fixture(
        parent_expanded: bool,
    ) -> (
        crate::app::App,
        crate::core::storage::Storage,
        tempfile::TempDir,
    ) {
        let (storage, storage_dir) = crate::core::storage::test_helpers::test_storage();
        let mut parent = crate::core::storage::test_helpers::make_test_session("parent");
        parent.title = "Parent Conductor".to_string();
        parent.role = crate::types::SessionRole::Conductor;
        parent.conductor_expanded = parent_expanded;
        let mut child = crate::core::storage::test_helpers::make_test_session("child");
        child.title = "Child Session".to_string();
        child.parent_session_id = parent.id.clone();
        storage.save_session(&parent).unwrap();
        storage.save_session(&child).unwrap();

        let mut app = crate::app::App::new(false);
        app.sessions = storage.load_sessions().unwrap();
        app.groups = storage.load_groups().unwrap_or_default();
        app.rebuild_list_rows();

        (app, storage, storage_dir)
    }

    fn dispatch_main_key(
        app: &mut crate::app::App,
        key: KeyEvent,
        storage: &crate::core::storage::Storage,
    ) {
        let session_ops = crate::core::session::SessionOps;
        let mut terminal = main_test_terminal();
        let attach_state = std::sync::Arc::new(std::sync::Mutex::new(
            crate::core::attach_state::AttachState::new(),
        ));

        super::handle_main_key(
            app,
            key,
            storage,
            &session_ops,
            &mut terminal,
            &attach_state,
        )
        .unwrap();
    }

    fn form_in_overlay(app: &crate::app::App) -> &crate::app::NewSessionForm {
        match &app.overlay {
            crate::app::Overlay::NewSession(form) => form,
            _ => panic!("expected new session overlay"),
        }
    }

    fn assert_server_ids(
        servers: &[crate::core::mcp::catalog::McpServerCatalogEntry],
        expected: &[&str],
    ) {
        let actual: std::collections::BTreeSet<_> =
            servers.iter().map(|server| server.id.as_str()).collect();
        let expected: std::collections::BTreeSet<_> = expected.iter().copied().collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_left_collapses_selected_expanded_conductor() {
        let (mut app, storage, _storage_dir) = conductor_tree_fixture(true);
        app.selected_index = app
            .list_rows
            .iter()
            .position(
                |row| matches!(row, crate::core::groups::ListRow::Session { session, depth: 0 } if session.id == "parent"),
            )
            .unwrap();

        dispatch_main_key(&mut app, key(KeyCode::Left), &storage);

        let parent = app
            .sessions
            .iter()
            .find(|session| session.id == "parent")
            .unwrap();
        assert!(!parent.conductor_expanded);
        assert!(app.list_rows.iter().all(|row| {
            !matches!(row, crate::core::groups::ListRow::Session { session, .. } if session.id == "child")
        }));
    }

    #[test]
    fn test_left_on_child_selects_parent_conductor() {
        let (mut app, storage, _storage_dir) = conductor_tree_fixture(true);
        app.selected_index = app
            .list_rows
            .iter()
            .position(
                |row| matches!(row, crate::core::groups::ListRow::Session { session, depth: 1 } if session.id == "child"),
            )
            .unwrap();

        dispatch_main_key(&mut app, key(KeyCode::Left), &storage);

        let selected = app.selected_session().unwrap();
        assert_eq!(selected.id, "parent");
    }

    #[test]
    fn test_right_expands_selected_collapsed_conductor() {
        let (mut app, storage, _storage_dir) = conductor_tree_fixture(false);
        app.selected_index = app
            .list_rows
            .iter()
            .position(
                |row| matches!(row, crate::core::groups::ListRow::Session { session, depth: 0 } if session.id == "parent"),
            )
            .unwrap();

        dispatch_main_key(&mut app, key(KeyCode::Right), &storage);

        let parent = app
            .sessions
            .iter()
            .find(|session| session.id == "parent")
            .unwrap();
        assert!(parent.conductor_expanded);
        assert!(app.list_rows.iter().any(|row| {
            matches!(row, crate::core::groups::ListRow::Session { session, depth: 1 } if session.id == "child")
        }));
    }

    struct EnvRestore {
        claude_config_dir: Option<std::ffi::OsString>,
        codex_home: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR"),
                codex_home: std::env::var_os("CODEX_HOME"),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(value) = &self.claude_config_dir {
                std::env::set_var("CLAUDE_CONFIG_DIR", value);
            } else {
                std::env::remove_var("CLAUDE_CONFIG_DIR");
            }
            if let Some(value) = &self.codex_home {
                std::env::set_var("CODEX_HOME", value);
            } else {
                std::env::remove_var("CODEX_HOME");
            }
        }
    }

    #[test]
    fn test_new_session_shortcut_auto_syncs_mcp_servers_before_loading_catalog() {
        let _env_lock = crate::core::runner::hook_io::lock_env();
        let _env_restore = EnvRestore::capture();
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join("claude");
        let codex_dir = dir.path().join("codex");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", &claude_dir);
        std::env::set_var("CODEX_HOME", &codex_dir);
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{"mcpServers":{"wavecrest":{"command":"uvx","args":["wavecrest-mcp"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            r#"[mcp_servers.GitLabMITRE]
url = "https://gitlab.example.test/api/v4/mcp"
"#,
        )
        .unwrap();
        let (storage, _storage_dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = crate::core::session::SessionOps;
        let mut app = crate::app::App::new(false);
        let backend = CrosstermBackend::new(std::io::stdout());
        let options = TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
        };
        let mut terminal = Terminal::with_options(backend, options).unwrap();
        let attach_state = std::sync::Arc::new(std::sync::Mutex::new(
            crate::core::attach_state::AttachState::new(),
        ));

        super::handle_main_key(
            &mut app,
            key(KeyCode::Char('n')),
            &storage,
            &session_ops,
            &mut terminal,
            &attach_state,
        )
        .unwrap();

        let form = form_in_overlay(&app);
        assert_eq!(form.runner, crate::types::Tool::Claude);
        assert_server_ids(&form.mcp_servers, &["GitLabMITRE", "wavecrest"]);

        let mut form = form.clone();
        while form.runner != crate::types::Tool::Codex {
            form.cycle_runner_next();
        }
        assert_server_ids(&form.mcp_servers, &["GitLabMITRE", "wavecrest"]);
    }

    #[test]
    fn test_shift_m_opens_mcp_profile_manager() {
        let (storage, _storage_dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = crate::core::session::SessionOps;
        let mut app = crate::app::App::new(false);
        app.config.mcp_profiles = vec![crate::core::mcp::McpProfile {
            id: "rust".to_string(),
            name: "Rust".to_string(),
            selection: crate::core::mcp::McpSelection::default(),
        }];
        let backend = CrosstermBackend::new(std::io::stdout());
        let options = TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
        };
        let mut terminal = Terminal::with_options(backend, options).unwrap();
        let attach_state = std::sync::Arc::new(std::sync::Mutex::new(
            crate::core::attach_state::AttachState::new(),
        ));

        super::handle_main_key(
            &mut app,
            shift_key('M'),
            &storage,
            &session_ops,
            &mut terminal,
            &attach_state,
        )
        .unwrap();

        match app.overlay {
            crate::app::Overlay::McpProfiles(form) => {
                assert_eq!(form.profiles.len(), 1);
                assert_eq!(form.profiles[0].id, "rust");
            }
            _ => panic!("expected MCP profiles overlay"),
        }
    }

    #[test]
    fn test_shift_m_opens_mcp_profile_manager_from_routines_tab() {
        let (storage, _storage_dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = crate::core::session::SessionOps;
        let mut app = crate::app::App::new(false);
        app.active_tab = crate::app::ActiveTab::Routines;
        let backend = CrosstermBackend::new(std::io::stdout());
        let options = TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
        };
        let mut terminal = Terminal::with_options(backend, options).unwrap();
        let attach_state = std::sync::Arc::new(std::sync::Mutex::new(
            crate::core::attach_state::AttachState::new(),
        ));

        super::handle_main_key(
            &mut app,
            shift_key('M'),
            &storage,
            &session_ops,
            &mut terminal,
            &attach_state,
        )
        .unwrap();

        assert!(matches!(app.overlay, crate::app::Overlay::McpProfiles(_)));
    }
}
