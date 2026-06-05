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
            app.overlay =
                Overlay::NewSession(crate::app::NewSessionForm::from_app_config(&app.config));
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
        CommandAction::FinishSession => {
            if let Some(session) = app.selected_session() {
                if !session.worktree_path.is_empty() {
                    let title = session.title.clone();
                    let id = session.id.clone();
                    let wt_path = session.worktree_path.clone();
                    let branch = session.worktree_branch.clone();
                    app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                        message: format!(
                            "Finish '{}'? Removes worktree {} and (if merged into main/master) branch {}.",
                            title, wt_path, branch
                        ),
                        action: crate::app::ConfirmAction::FinishSession(id),
                    });
                } else {
                    app.toast.message =
                        Some("Session has no worktree — use 'd' to delete".to_string());
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                }
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
        CommandAction::ToggleUserWaiting => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.user_waiting;
                let id = session.id.clone();
                let _ = storage.set_user_waiting(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::SyncMcpServers => {
            match crate::core::mcp::default_sync_config_paths().and_then(|paths| {
                crate::core::mcp::load_sync_plan_from_paths(&paths).map(|plan| (paths, plan))
            }) {
                Ok((paths, plan)) => {
                    app.overlay = Overlay::McpSync(crate::app::McpSyncForm::new(paths, plan));
                }
                Err(e) => {
                    app.toast.message = Some(format!("MCP sync failed: {}", e));
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
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
                            app.toast.message = Some(format!("Exported to {}", path));
                            app.toast.expire =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                        }
                        Err(e) => {
                            app.toast.message = Some(format!("Export failed: {}", e));
                            app.toast.expire =
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
            app.toast.message = Some(format!("Sort: {}", label));
            app.toast.expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
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
                app.toast.message = Some(msg);
                app.toast.expire =
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
            app.preview.content.clear();
            app.preview.last_session = None;
            app.preview.last_capture = None;
            app.toast.message = Some(format!("Panel: {}", app.detail_mode.label()));
            app.toast.expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        }
        CommandAction::NewRoutine => {
            app.active_tab = crate::app::ActiveTab::Routines;
            app.overlay = Overlay::NewRoutine(crate::app::NewRoutineForm::new());
        }
        CommandAction::ToggleRoutine => {
            app.active_tab = crate::app::ActiveTab::Routines;
            if let Some(crate::app::RoutineListRow::Routine(routine)) = app
                .routine_state
                .list_rows
                .get(app.routine_state.selected_index)
                .cloned()
            {
                let new_enabled = !routine.enabled;
                let _ = storage.set_routine_enabled(&routine.id, new_enabled);
                let scheduler = crate::core::scheduler::platform_scheduler();
                if new_enabled {
                    if let Some(r) = app
                        .routine_state
                        .routines
                        .iter()
                        .find(|r| r.id == routine.id)
                    {
                        let _ = scheduler.install(r);
                    }
                } else {
                    let _ = scheduler.uninstall(&routine.id);
                }
                app.routine_state.routines = storage.load_routines().unwrap_or_default();
                app.rebuild_routine_list_rows();
            }
        }
        CommandAction::DeleteRoutine => {
            app.active_tab = crate::app::ActiveTab::Routines;
            if let Some(crate::app::RoutineListRow::Routine(routine)) = app
                .routine_state
                .list_rows
                .get(app.routine_state.selected_index)
                .cloned()
            {
                app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                    message: format!("Delete routine '{}'?", routine.name),
                    action: crate::app::ConfirmAction::DeleteRoutine(routine.id.clone()),
                });
            }
        }
        CommandAction::SweepOrphanWorktrees => {
            let repo = app
                .selected_session()
                .map(|s| {
                    if s.worktree_repo.is_empty() {
                        s.project_path.clone()
                    } else {
                        s.worktree_repo.clone()
                    }
                })
                .unwrap_or_default();
            if repo.is_empty() {
                app.toast.message = Some("Select a session in the target repo first".to_string());
                app.toast.expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                return Ok(());
            }
            match session_ops.find_orphan_worktrees(storage, &repo) {
                Ok(orphans) if orphans.is_empty() => {
                    app.toast.message = Some("No orphan worktrees".to_string());
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                }
                Ok(orphans) => {
                    let mut removed = 0usize;
                    let mut failed: Vec<String> = Vec::new();
                    for path in &orphans {
                        match session_ops.remove_orphan_worktree(&repo, path) {
                            Ok(()) => removed += 1,
                            Err(e) => failed.push(format!("{}: {}", path, e)),
                        }
                    }
                    let msg = if failed.is_empty() {
                        format!("Removed {} orphan worktree(s)", removed)
                    } else {
                        format!(
                            "Removed {} orphans; {} failed: {}",
                            removed,
                            failed.len(),
                            failed.join("; ")
                        )
                    };
                    app.toast.message = Some(msg);
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(8));
                }
                Err(e) => {
                    app.toast.message = Some(format!("Sweep failed: {}", e));
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
                }
            }
        }
    }
    Ok(())
}

pub fn handle_mcp_sync_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    let mut apply: Option<(
        Vec<crate::core::mcp::McpSyncProposal>,
        crate::core::mcp::McpSyncConfigPaths,
    )> = None;

    if let crate::app::Overlay::McpSync(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                if form.confirming || form.confirming_all {
                    form.clear_confirmation();
                } else {
                    app.overlay = crate::app::Overlay::None;
                }
            }
            KeyCode::Char('n') if form.confirming || form.confirming_all => {
                form.clear_confirmation();
            }
            KeyCode::Up | KeyCode::Char('k') if !form.confirming && !form.confirming_all => {
                form.move_up();
            }
            KeyCode::Down | KeyCode::Char('j') if !form.confirming && !form.confirming_all => {
                form.move_down();
            }
            KeyCode::Char('a')
                if !form.confirming && !form.confirming_all && !form.plan.proposals.is_empty() =>
            {
                form.selected = 0;
                form.confirming_all = true;
                form.confirming = false;
            }
            KeyCode::Enter if form.confirming_all => {
                apply = Some((form.all_proposals(), form.paths.clone()));
            }
            KeyCode::Char('y') if form.confirming_all => {
                apply = Some((form.all_proposals(), form.paths.clone()));
            }
            KeyCode::Enter if form.confirming => {
                if let Some(proposal) = form.selected_proposal().cloned() {
                    apply = Some((vec![proposal], form.paths.clone()));
                }
            }
            KeyCode::Char('y') if form.confirming => {
                if let Some(proposal) = form.selected_proposal().cloned() {
                    apply = Some((vec![proposal], form.paths.clone()));
                }
            }
            KeyCode::Enter if form.action_count() > 0 => {
                form.begin_confirming_selected();
            }
            _ => {}
        }
    }

    if let Some((proposals, paths)) = apply {
        let mut applied_count = 0usize;
        let mut error = None;
        for proposal in &proposals {
            match crate::core::mcp::apply_sync_proposal_to_paths(proposal, &paths) {
                Ok(()) => applied_count += 1,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }

        match error {
            None => {
                let message = if applied_count == 1 {
                    let proposal = &proposals[0];
                    format!(
                        "Synced MCP server {} to {}",
                        proposal.server_id, proposal.target
                    )
                } else {
                    format!("Synced {applied_count} MCP servers across runners")
                };
                app.toast.message = Some(message);
                app.toast.expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                match crate::core::mcp::load_sync_plan_from_paths(&paths) {
                    Ok(plan) if plan.proposals.is_empty() => {
                        app.overlay = crate::app::Overlay::None;
                    }
                    Ok(plan) => {
                        if let crate::app::Overlay::McpSync(ref mut form) = app.overlay {
                            form.replace_plan(plan);
                        }
                    }
                    Err(e) => {
                        app.overlay = crate::app::Overlay::None;
                        app.toast.message = Some(format!("MCP sync refresh failed: {}", e));
                        app.toast.expire =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
                    }
                }
            }
            Some(e) => {
                if let crate::app::Overlay::McpSync(ref mut form) = app.overlay {
                    form.clear_confirmation();
                }
                app.toast.message = Some(if applied_count == 0 {
                    format!("MCP sync failed: {}", e)
                } else {
                    format!("MCP sync failed after {applied_count} change(s): {}", e)
                });
                app.toast.expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
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
                app.toast.message = Some(format!("Theme: {}", chosen));
                app.toast.expire =
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::fs;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn mcp_sync_overlay_requires_confirmation_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let claude_path = dir.path().join("claude").join("settings.json");
        let codex_path = dir.path().join("codex").join("config.toml");
        fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
        fs::create_dir_all(codex_path.parent().unwrap()).unwrap();
        fs::write(
            &claude_path,
            r#"{"mcpServers":{"wavecrest":{"command":"uvx","args":["wavecrest-mcp"]}}}"#,
        )
        .unwrap();
        fs::write(&codex_path, r#"model = "gpt-5.5""#).unwrap();
        let paths = crate::core::mcp::McpSyncConfigPaths {
            claude_settings: claude_path,
            codex_config: codex_path.clone(),
        };
        let plan = crate::core::mcp::load_sync_plan_from_paths(&paths).unwrap();
        let mut app = crate::app::App::new(false);
        app.overlay = crate::app::Overlay::McpSync(crate::app::McpSyncForm::new(paths, plan));

        super::handle_mcp_sync_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(
            fs::read_to_string(&codex_path).unwrap(),
            r#"model = "gpt-5.5""#
        );
        assert!(matches!(
            app.overlay,
            crate::app::Overlay::McpSync(crate::app::McpSyncForm {
                confirming: true,
                ..
            })
        ));

        super::handle_mcp_sync_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        assert!(fs::read_to_string(&codex_path)
            .unwrap()
            .contains("[mcp_servers.wavecrest]"));
    }

    #[test]
    fn mcp_sync_overlay_can_apply_all_missing_servers_after_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let claude_path = dir.path().join("claude").join("settings.json");
        let codex_path = dir.path().join("codex").join("config.toml");
        fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
        fs::create_dir_all(codex_path.parent().unwrap()).unwrap();
        fs::write(
            &claude_path,
            r#"{"mcpServers":{"wavecrest":{"command":"uvx","args":["wavecrest-mcp"]}}}"#,
        )
        .unwrap();
        fs::write(
            &codex_path,
            r#"[mcp_servers.GitLabMITRE]
url = "https://gitlab.example.test/api/v4/mcp"
"#,
        )
        .unwrap();
        let paths = crate::core::mcp::McpSyncConfigPaths {
            claude_settings: claude_path.clone(),
            codex_config: codex_path.clone(),
        };
        let plan = crate::core::mcp::load_sync_plan_from_paths(&paths).unwrap();
        assert_eq!(plan.proposals.len(), 2);
        let mut app = crate::app::App::new(false);
        app.overlay = crate::app::Overlay::McpSync(crate::app::McpSyncForm::new(paths, plan));

        super::handle_mcp_sync_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(
            fs::read_to_string(&claude_path).unwrap(),
            r#"{"mcpServers":{"wavecrest":{"command":"uvx","args":["wavecrest-mcp"]}}}"#
        );
        assert!(matches!(
            app.overlay,
            crate::app::Overlay::McpSync(crate::app::McpSyncForm {
                confirming_all: true,
                ..
            })
        ));

        super::handle_mcp_sync_key(&mut app, key(KeyCode::Char('y'))).unwrap();

        let claude_settings = fs::read_to_string(&claude_path).unwrap();
        let codex_config = fs::read_to_string(&codex_path).unwrap();
        assert!(claude_settings.contains("GitLabMITRE"));
        assert!(codex_config.contains("[mcp_servers.wavecrest]"));
        assert!(matches!(app.overlay, crate::app::Overlay::None));
    }
}
