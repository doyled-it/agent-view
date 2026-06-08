//! Session overlay keyboard handlers

const FIELD_COUNT: usize = 6;
const MCP_FIELD: usize = 5;

fn build_session_create_options_from_form(
    form: &crate::app::NewSessionForm,
    group_path: Option<String>,
) -> Result<crate::types::SessionCreateOptions, String> {
    let wt_branch_trimmed = form.worktree_branch.trim().to_string();
    let worktree = if wt_branch_trimmed.is_empty() {
        None
    } else {
        if let Some(err) = crate::core::git::validate_branch_name(&wt_branch_trimmed) {
            return Err(err);
        }
        if !crate::core::git::is_git_repo(&form.project_path) {
            return Err("Project path is not a git repository".to_string());
        }
        let exists = crate::core::git::branch_exists(&form.project_path, &wt_branch_trimmed);
        if form.worktree_new_branch && exists {
            return Err(format!(
                "Branch '{}' already exists — toggle to attach (^t)",
                wt_branch_trimmed
            ));
        }
        if !form.worktree_new_branch && !exists {
            return Err(format!(
                "Branch '{}' does not exist — toggle to create (^t)",
                wt_branch_trimmed
            ));
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

    Ok(crate::types::SessionCreateOptions {
        title,
        project_path,
        group_path,
        tool: form.runner,
        command: None,
        mcp_selection: Some(form.mcp_selection.clone()),
        role: crate::types::SessionRole::Normal,
        parent_session_id: None,
        conductor_config: None,
        worktree,
    })
}

pub fn handle_new_session_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_ops: &crate::core::session::SessionOps,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let mut mcp_profiles_to_persist: Option<Vec<crate::core::mcp::McpProfile>> = None;
    let mut toast_message: Option<String> = None;

    if let crate::app::Overlay::NewSession(ref mut form) = app.overlay {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) if form.mcp_profile_save_name.is_some() => {
                form.cancel_save_mcp_profile();
            }
            (_, KeyCode::Esc) => {
                app.overlay = crate::app::Overlay::None;
            }
            (_, KeyCode::Enter) if form.mcp_profile_save_name.is_some() => {
                match form.save_mcp_profile_from_prompt() {
                    Ok(profile) => {
                        mcp_profiles_to_persist = Some(form.mcp_profiles.clone());
                        toast_message = Some(format!("Saved MCP profile: {}", profile.name));
                    }
                    Err(err) => {
                        form.error = Some(err);
                    }
                }
            }
            (_, KeyCode::Backspace) if form.mcp_profile_save_name.is_some() => {
                if let Some(name) = &mut form.mcp_profile_save_name {
                    name.pop();
                }
            }
            (m, KeyCode::Char(c))
                if form.mcp_profile_save_name.is_some()
                    && !m.contains(KeyModifiers::CONTROL)
                    && !m.contains(KeyModifiers::SUPER) =>
            {
                if let Some(name) = &mut form.mcp_profile_save_name {
                    name.push(c);
                    form.error = None;
                }
            }
            // Ctrl+T must precede the generic Char arm so it doesn't append 't'
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                form.worktree_new_branch = !form.worktree_new_branch;
                form.error = None;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                form.begin_save_mcp_profile();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => match form.update_active_mcp_profile() {
                Ok(profile) => {
                    mcp_profiles_to_persist = Some(form.mcp_profiles.clone());
                    toast_message = Some(format!("Updated MCP profile: {}", profile.name));
                }
                Err(err) => {
                    form.error = Some(err);
                }
            },
            // Ctrl+S or Super+S — submit
            (m, KeyCode::Char('s'))
                if m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::SUPER) =>
            {
                let options = match build_session_create_options_from_form(form, None) {
                    Ok(options) => options,
                    Err(err) => {
                        form.error = Some(err);
                        return Ok(());
                    }
                };

                let mut cache = crate::core::tmux::SessionCache::new();
                match session_ops.create_session(storage, &mut cache, options) {
                    Ok((_, warn)) => {
                        if let Some(msg) = warn {
                            app.toast.message = Some(msg);
                            app.toast.expire =
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
                        crate::core::logger::log_diagnostic(&format!(
                            "Failed to create session: {}",
                            e
                        ));
                        form.error = Some(e.to_string());
                    }
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) if form.focused_field == 0 => {
                form.cycle_runner_prev();
            }
            (KeyModifiers::NONE, KeyCode::Right) if form.focused_field == 0 => {
                form.cycle_runner_next();
            }
            (KeyModifiers::NONE, KeyCode::Char(' '))
                if form.focused_field == MCP_FIELD && form.mcp_expanded =>
            {
                if let Err(err) = form.activate_selected_mcp_row() {
                    form.error = Some(err);
                }
            }
            (m, KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete | KeyCode::Backspace)
                if form.focused_field == MCP_FIELD
                    && form.mcp_expanded
                    && !m.contains(KeyModifiers::CONTROL)
                    && !m.contains(KeyModifiers::SUPER) =>
            {
                if let Some(profile) = form.delete_selected_mcp_profile() {
                    mcp_profiles_to_persist = Some(form.mcp_profiles.clone());
                    toast_message = Some(format!("Deleted MCP profile: {}", profile.name));
                }
            }
            (_, KeyCode::Tab) => {
                match form.focused_field {
                    2 => {
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
                    3 => {
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
                    4 if form.worktree_new_branch => {
                        // Base ref field: local-branch completion (only when
                        // creating a new branch — no-op in attach mode).
                        if !form.completions.is_empty() && form.completions.len() > 1 {
                            let idx = match form.completion_index {
                                Some(i) => (i + 1) % form.completions.len(),
                                None => 0,
                            };
                            form.completion_index = Some(idx);
                            form.worktree_base = form.completions[idx].clone();
                        } else {
                            let all = crate::core::git::list_local_branches(&form.project_path)
                                .unwrap_or_default();
                            let prefix = form.worktree_base.clone();
                            let candidates: Vec<String> =
                                all.into_iter().filter(|b| b.starts_with(&prefix)).collect();
                            match candidates.len() {
                                0 => {}
                                1 => {
                                    form.worktree_base = candidates.into_iter().next().unwrap();
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
                        // Fields without completion behavior: advance focus.
                        form.focused_field = (form.focused_field + 1) % FIELD_COUNT;
                        form.clear_completions();
                    }
                }
            }
            (_, KeyCode::BackTab) => {
                form.focused_field = (form.focused_field + FIELD_COUNT - 1) % FIELD_COUNT;
                form.clear_completions();
            }
            (_, KeyCode::Down | KeyCode::Char('j'))
                if form.focused_field == MCP_FIELD && form.mcp_expanded =>
            {
                let row_count = form.mcp_row_count();
                if row_count == 0 {
                    form.mcp_selected_row = 0;
                } else {
                    form.mcp_selected_row = (form.mcp_selected_row + 1).min(row_count - 1);
                }
            }
            (_, KeyCode::Up | KeyCode::Char('k'))
                if form.focused_field == MCP_FIELD && form.mcp_expanded =>
            {
                form.mcp_selected_row = form.mcp_selected_row.saturating_sub(1);
            }
            (_, KeyCode::Down) => {
                form.focused_field = (form.focused_field + 1) % FIELD_COUNT;
                form.clear_completions();
            }
            (_, KeyCode::Up) => {
                form.focused_field = (form.focused_field + FIELD_COUNT - 1) % FIELD_COUNT;
                form.clear_completions();
            }
            (_, KeyCode::Enter) if form.focused_field == MCP_FIELD => {
                form.mcp_expanded = !form.mcp_expanded;
                form.clear_completions();
            }
            (_, KeyCode::Enter) => {
                // Advance focus forward — NEVER submits
                form.focused_field = (form.focused_field + 1) % FIELD_COUNT;
                form.clear_completions();
            }
            // Generic Char arm — guard excludes Ctrl and Super so those don't append
            (m, KeyCode::Char(c))
                if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::SUPER) =>
            {
                match form.focused_field {
                    0 => {} // runner field — text input ignored
                    1 => form.title.push(c),
                    2 => {
                        form.project_path.push(c);
                        form.clear_completions();
                    }
                    3 => {
                        form.worktree_branch.push(c);
                        form.error = None;
                    }
                    4 => {
                        form.worktree_base.push(c);
                        form.error = None;
                    }
                    _ => {}
                }
            }
            (_, KeyCode::Backspace) => match form.focused_field {
                0 => {} // runner field — text input ignored
                1 => {
                    form.title.pop();
                }
                2 => {
                    form.project_path.pop();
                    form.clear_completions();
                }
                3 => {
                    form.worktree_branch.pop();
                    form.error = None;
                }
                4 => {
                    form.worktree_base.pop();
                    form.error = None;
                }
                _ => {}
            },
            _ => {}
        }
    }

    if let Some(profiles) = mcp_profiles_to_persist {
        app.config.mcp_profiles = profiles;
        match crate::core::config::save_config(&app.config) {
            Ok(()) => {
                app.toast.message = toast_message;
                app.toast.expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            Err(e) => {
                app.toast.message = Some(format!("MCP profile save failed: {}", e));
                app.toast.expire =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
            }
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
                        let ids: Vec<String> = app.bulk.selected.iter().cloned().collect();
                        let mut cache = crate::core::tmux::SessionCache::new();
                        for id in &ids {
                            let _ = session_ops.delete_session(storage, &mut cache, id);
                        }
                        app.clear_bulk_selection();
                    }
                    crate::app::ConfirmAction::BulkStop => {
                        let ids: Vec<String> = app.bulk.selected.iter().cloned().collect();
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
                        app.routine_state.routines = storage.load_routines().unwrap_or_default();
                        app.routine_state.runs_cache.remove(id);
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
                                app.toast.message = Some(msg.to_string());
                                app.toast.expire = Some(
                                    std::time::Instant::now() + std::time::Duration::from_secs(5),
                                );
                            }
                            Err(e) => {
                                app.toast.message = Some(format!("Finish failed: {}", e));
                                app.toast.expire = Some(
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
                            app.routine_state.routines =
                                storage.load_routines().unwrap_or_default();
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
                            app.routine_state.routines =
                                storage.load_routines().unwrap_or_default();
                            app.rebuild_routine_list_rows();
                        }
                        crate::app::ActiveTab::Costs => {} // TODO: handled in Task 8/15
                    }
                    app.toast.message = Some(format!("Moved to {}", name));
                    app.toast.expire =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                app.overlay = crate::app::Overlay::None;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Overlay};
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use crate::core::session::SessionOps;
    use crate::types::Tool;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    struct HomeRestore {
        home: Option<std::ffi::OsString>,
    }

    impl HomeRestore {
        fn capture() -> Self {
            Self {
                home: std::env::var_os("HOME"),
            }
        }
    }

    impl Drop for HomeRestore {
        fn drop(&mut self) {
            if let Some(value) = &self.home {
                std::env::set_var("HOME", value);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    fn form_in_overlay(app: &App) -> &crate::app::NewSessionForm {
        match &app.overlay {
            Overlay::NewSession(form) => form,
            other => panic!("expected new session overlay, got {other:?}"),
        }
    }

    #[test]
    fn test_build_session_create_options_includes_mcp_selection() {
        let mut form = crate::app::NewSessionForm::new();
        form.title = "MCP session".into();
        form.project_path = "/tmp/project".into();
        form.runner = Tool::Codex;
        form.mcp_selection = McpSelection {
            profile_id: Some("minimal".into()),
            servers: vec![McpServerSelection {
                id: "browser".into(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let options =
            build_session_create_options_from_form(&form, Some("work/tools".into())).unwrap();

        assert_eq!(options.title.as_deref(), Some("MCP session"));
        assert_eq!(options.project_path, "/tmp/project");
        assert_eq!(options.group_path.as_deref(), Some("work/tools"));
        assert_eq!(options.tool, Tool::Codex);
        assert_eq!(options.mcp_selection, Some(form.mcp_selection.clone()));
        assert!(options.worktree.is_none());
    }

    #[test]
    fn test_mcp_field_enter_toggles_expanded() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut form = crate::app::NewSessionForm::new();
        form.focused_field = 5;
        let mut app = App::new(false);
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Enter), &storage, &session_ops).unwrap();
        assert!(form_in_overlay(&app).mcp_expanded);

        handle_new_session_key(&mut app, key(KeyCode::Enter), &storage, &session_ops).unwrap();
        assert!(!form_in_overlay(&app).mcp_expanded);
    }

    #[test]
    fn test_mcp_field_space_toggles_selected_server_when_expanded() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut form = crate::app::NewSessionForm::new();
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);
        form.focused_field = 5;
        form.mcp_expanded = true;
        form.mcp_selected_row = 1;
        let mut app = App::new(false);
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Char(' ')), &storage, &session_ops).unwrap();

        let form = form_in_overlay(&app);
        let browser = form
            .mcp_selection
            .servers
            .iter()
            .find(|server| server.id == "browser")
            .unwrap();
        assert!(!browser.enabled);
    }

    #[test]
    fn test_mcp_field_space_applies_selected_profile_when_expanded() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut form = crate::app::NewSessionForm::new();
        form.mcp_profiles = vec![crate::core::mcp::McpProfile {
            id: "rust".into(),
            name: "Rust".into(),
            selection: McpSelection {
                profile_id: None,
                servers: vec![McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                }],
            },
        }];
        form.focused_field = 5;
        form.mcp_expanded = true;
        form.mcp_selected_row = 0;
        let mut app = App::new(false);
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Char(' ')), &storage, &session_ops).unwrap();

        let form = form_in_overlay(&app);
        assert_eq!(form.mcp_selection.profile_id.as_deref(), Some("rust"));
        assert_eq!(form.mcp_selection.servers.len(), 1);
        assert_eq!(form.mcp_selection.servers[0].id, "GitLabMITRE");
    }

    #[test]
    fn test_mcp_field_space_clears_selected_profile_when_already_active() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut form = crate::app::NewSessionForm::new();
        form.mcp_profiles = vec![crate::core::mcp::McpProfile {
            id: "rust".into(),
            name: "Rust".into(),
            selection: McpSelection {
                profile_id: None,
                servers: vec![McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                }],
            },
        }];
        form.focused_field = 5;
        form.mcp_expanded = true;
        form.mcp_selected_row = 0;
        form.apply_mcp_profile("rust").unwrap();
        let mut app = App::new(false);
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Char(' ')), &storage, &session_ops).unwrap();

        let form = form_in_overlay(&app);
        assert_eq!(form.mcp_selection, McpSelection::default());
    }

    #[test]
    fn test_new_session_delete_key_removes_selected_mcp_profile() {
        let _env_lock = crate::core::runner::hook_io::lock_env();
        let _home_restore = HomeRestore::capture();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut app = App::new(false);
        app.config.mcp_profiles = vec![
            crate::core::mcp::McpProfile {
                id: "rust".into(),
                name: "Rust".into(),
                selection: McpSelection {
                    profile_id: None,
                    servers: vec![McpServerSelection {
                        id: "GitLabMITRE".into(),
                        enabled: true,
                        selected_tools: None,
                    }],
                },
            },
            crate::core::mcp::McpProfile {
                id: "docs".into(),
                name: "Docs".into(),
                selection: McpSelection::default(),
            },
        ];
        let mut form = crate::app::NewSessionForm::new();
        form.focused_field = MCP_FIELD;
        form.mcp_expanded = true;
        form.mcp_selected_row = 0;
        form.mcp_profiles = app.config.mcp_profiles.clone();
        form.apply_mcp_profile("rust").unwrap();
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Delete), &storage, &session_ops).unwrap();

        assert_eq!(app.config.mcp_profiles.len(), 1);
        assert_eq!(app.config.mcp_profiles[0].id, "docs");
        let form = form_in_overlay(&app);
        assert_eq!(form.mcp_profiles.len(), 1);
        assert_eq!(form.mcp_profiles[0].id, "docs");
        assert_eq!(form.mcp_selection, McpSelection::default());
        let loaded = crate::core::config::load_config_from_path(
            &home.path().join(".agent-view/config.json"),
        );
        assert_eq!(loaded.mcp_profiles.len(), 1);
        assert_eq!(loaded.mcp_profiles[0].id, "docs");
    }

    #[test]
    fn test_new_session_ctrl_p_saves_current_mcp_selection_as_profile() {
        let _env_lock = crate::core::runner::hook_io::lock_env();
        let _home_restore = HomeRestore::capture();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut app = App::new(false);
        let mut form = crate::app::NewSessionForm::new();
        form.focused_field = MCP_FIELD;
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "wavecrest".into()]);
        form.toggle_mcp_server("GitLabMITRE");
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, ctrl_key('p'), &storage, &session_ops).unwrap();
        for c in "Rust".chars() {
            handle_new_session_key(&mut app, key(KeyCode::Char(c)), &storage, &session_ops)
                .unwrap();
        }
        handle_new_session_key(&mut app, key(KeyCode::Enter), &storage, &session_ops).unwrap();

        assert_eq!(app.config.mcp_profiles.len(), 1);
        assert_eq!(app.config.mcp_profiles[0].id, "rust");
        assert_eq!(app.config.mcp_profiles[0].name, "Rust");
        assert!(app.config.mcp_profiles[0]
            .selection
            .servers
            .iter()
            .any(|server| server.id == "GitLabMITRE" && !server.enabled));
        assert_eq!(
            form_in_overlay(&app).mcp_selection.profile_id.as_deref(),
            Some("rust")
        );
        let loaded = crate::core::config::load_config_from_path(
            &home.path().join(".agent-view/config.json"),
        );
        assert_eq!(loaded.mcp_profiles.len(), 1);
        assert_eq!(loaded.mcp_profiles[0].id, "rust");
    }

    #[test]
    fn test_new_session_ctrl_u_updates_active_mcp_profile() {
        let _env_lock = crate::core::runner::hook_io::lock_env();
        let _home_restore = HomeRestore::capture();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut app = App::new(false);
        app.config.mcp_profiles = vec![crate::core::mcp::McpProfile {
            id: "rust".into(),
            name: "Rust".into(),
            selection: McpSelection::default(),
        }];
        let mut form = crate::app::NewSessionForm::new();
        form.focused_field = MCP_FIELD;
        form.mcp_profiles = app.config.mcp_profiles.clone();
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "wavecrest".into()]);
        form.apply_mcp_profile("rust").unwrap();
        form.toggle_mcp_server("wavecrest");
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, ctrl_key('u'), &storage, &session_ops).unwrap();

        assert_eq!(app.config.mcp_profiles.len(), 1);
        let profile = &app.config.mcp_profiles[0];
        assert_eq!(profile.id, "rust");
        assert!(profile
            .selection
            .servers
            .iter()
            .any(|server| server.id == "wavecrest" && !server.enabled));
        let loaded = crate::core::config::load_config_from_path(
            &home.path().join(".agent-view/config.json"),
        );
        assert!(loaded.mcp_profiles[0]
            .selection
            .servers
            .iter()
            .any(|server| server.id == "wavecrest" && !server.enabled));
    }

    #[test]
    fn test_mcp_field_up_down_move_selected_server_row_when_expanded() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut form = crate::app::NewSessionForm::new();
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);
        form.focused_field = 5;
        form.mcp_expanded = true;
        let mut app = App::new(false);
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Down), &storage, &session_ops).unwrap();
        assert_eq!(form_in_overlay(&app).focused_field, 5);
        assert_eq!(form_in_overlay(&app).mcp_selected_row, 1);

        handle_new_session_key(&mut app, key(KeyCode::Down), &storage, &session_ops).unwrap();
        assert_eq!(form_in_overlay(&app).mcp_selected_row, 1);

        handle_new_session_key(&mut app, key(KeyCode::Up), &storage, &session_ops).unwrap();
        assert_eq!(form_in_overlay(&app).focused_field, 5);
        assert_eq!(form_in_overlay(&app).mcp_selected_row, 0);
    }

    #[test]
    fn test_mcp_field_j_k_move_selected_server_row_when_expanded() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut form = crate::app::NewSessionForm::new();
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);
        form.focused_field = 5;
        form.mcp_expanded = true;
        let mut app = App::new(false);
        app.overlay = Overlay::NewSession(form);

        handle_new_session_key(&mut app, key(KeyCode::Char('j')), &storage, &session_ops).unwrap();
        assert_eq!(form_in_overlay(&app).focused_field, 5);
        assert_eq!(form_in_overlay(&app).mcp_selected_row, 1);

        handle_new_session_key(&mut app, key(KeyCode::Char('j')), &storage, &session_ops).unwrap();
        assert_eq!(form_in_overlay(&app).mcp_selected_row, 1);

        handle_new_session_key(&mut app, key(KeyCode::Char('k')), &storage, &session_ops).unwrap();
        assert_eq!(form_in_overlay(&app).focused_field, 5);
        assert_eq!(form_in_overlay(&app).mcp_selected_row, 0);
    }

    #[test]
    fn test_new_session_command_uses_app_config_profiles() {
        let (storage, _dir) = crate::core::storage::test_helpers::test_storage();
        let session_ops = SessionOps;
        let mut app = App::new(false);
        app.config.mcp_profiles = vec![crate::core::mcp::McpProfile {
            id: "rust".into(),
            name: "Rust".into(),
            selection: McpSelection {
                profile_id: None,
                servers: vec![McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                }],
            },
        }];

        crate::input::overlay::execute_command_action(
            &mut app,
            crate::app::CommandAction::NewSession,
            &storage,
            &session_ops,
        )
        .unwrap();

        let form = form_in_overlay(&app);
        assert_eq!(form.mcp_profiles.len(), 1);
        assert_eq!(form.mcp_profiles[0].id, "rust");
    }

    #[test]
    fn test_new_session_command_auto_syncs_mcp_servers_before_loading_catalog() {
        let _guard = crate::core::runner::hook_io::lock_env();
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
        let session_ops = SessionOps;
        let mut app = App::new(false);

        crate::input::overlay::execute_command_action(
            &mut app,
            crate::app::CommandAction::NewSession,
            &storage,
            &session_ops,
        )
        .unwrap();

        let form = form_in_overlay(&app);
        assert_eq!(form.runner, Tool::Claude);
        assert_server_ids(&form.mcp_servers, &["GitLabMITRE", "wavecrest"]);

        let mut form = form.clone();
        while form.runner != Tool::Codex {
            form.cycle_runner_next();
        }
        assert_server_ids(&form.mcp_servers, &["GitLabMITRE", "wavecrest"]);
        assert!(std::fs::read_to_string(claude_dir.join("settings.json"))
            .unwrap()
            .contains("GitLabMITRE"));
        assert!(std::fs::read_to_string(codex_dir.join("config.toml"))
            .unwrap()
            .contains("[mcp_servers.wavecrest]"));

        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::remove_var("CODEX_HOME");
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
}
