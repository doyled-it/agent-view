mod app;
mod core;
mod event;
mod types;
mod ui;

use clap::Parser;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "agent-view", version = VERSION, about = "Terminal UI for managing AI coding agent sessions")]
struct Cli {
    /// Use light mode theme
    #[arg(long)]
    light: bool,

    /// Attach to session immediately (for notification click-through)
    #[arg(long)]
    attach: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Verify tmux is available
    if !crate::core::tmux::is_tmux_available() {
        eprintln!("Error: tmux is not installed or not in PATH.");
        eprintln!("Install with: brew install tmux");
        std::process::exit(1);
    }

    // Open storage and run migrations
    let storage = crate::core::storage::Storage::open_default()?;
    storage.migrate()?;

    // Load config
    let config = crate::core::config::load_config();

    // Initialize app state
    let mut app = crate::app::App::new(cli.light);

    // Load sessions from storage
    app.sessions = storage.load_sessions()?;
    app.groups = storage.load_groups().unwrap_or_default();
    app.rebuild_list_rows();

    // If --attach was passed, store for immediate attach after TUI starts
    if let Some(ref session_id) = cli.attach {
        app.attach_session = Some(session_id.clone());
    }

    // Spawn config file watcher — watches the config directory for changes
    let config_changed = app.config_changed.clone();
    let _config_watcher = {
        use notify::{Event, EventKind, RecursiveMode, Watcher};
        let config_path_clone = crate::core::config::config_dir();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    config_changed.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        })
        .ok();
        if let Some(ref mut w) = watcher {
            let _ = w.watch(&config_path_clone, RecursiveMode::NonRecursive);
        }
        watcher
    };

    // Run the TUI event loop
    run_tui(app, storage, config)?;

    Ok(())
}

fn run_tui(
    mut app: crate::app::App,
    storage: crate::core::storage::Storage,
    config: crate::core::config::AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::{
        event::{self, Event},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::prelude::*;
    use std::io;
    use std::time::{Duration, Instant};

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let session_ops = crate::core::session::SessionOps;

    // Shared attach state — background thread reads this for notification suppression
    use std::sync::{Arc, Mutex};
    let attach_state: Arc<Mutex<crate::core::attach_state::AttachState>> =
        Arc::new(Mutex::new(crate::core::attach_state::AttachState::new()));
    let attach_state_for_bg = Arc::clone(&attach_state);

    let bg_sound = config.notifications.sound;
    let _bg_handle = std::thread::spawn(move || {
        let mut cache = crate::core::tmux::SessionCache::new();
        let mut processor = crate::core::session::StatusProcessor::new();
        let mut logger = crate::core::logger::SessionLogger::new();
        let mut log_tick: u32 = 0;

        loop {
            std::thread::sleep(Duration::from_millis(500));

            // Open fresh storage connection each tick
            let bg_storage = match crate::core::storage::Storage::open_default() {
                Ok(s) => { let _ = s.migrate(); s }
                Err(_) => continue,
            };
            let sessions = bg_storage.load_sessions().unwrap_or_default();

            // Read attach state from main thread
            let (attached, suppress_queue) = if let Ok(mut guard) = attach_state_for_bg.lock() {
                let attached = guard.attached_session.clone();
                let queue = std::mem::take(&mut guard.suppress_queue);
                (attached, queue)
            } else {
                (None, vec![])
            };

            // Process suppress queue from main thread
            for tmux_name in suppress_queue {
                processor.suppress_notification(&tmux_name);
            }

            cache.refresh();
            let mut any_changed = false;

            for session in &sessions {
                if session.tmux_session.is_empty() {
                    continue;
                }

                // Detect raw status
                let raw_status = if !cache.session_exists(&session.tmux_session) {
                    if session.status != crate::types::SessionStatus::Stopped {
                        crate::types::SessionStatus::Stopped
                    } else {
                        continue;
                    }
                } else {
                    let is_active = cache.is_session_active(&session.tmux_session, 2);
                    match crate::core::tmux::capture_pane(&session.tmux_session, Some(-100)) {
                        Ok(output) => {
                            let tool_str = if session.tool == crate::types::Tool::Claude {
                                Some("claude")
                            } else {
                                None
                            };
                            let parsed = crate::core::status::parse_tool_status(&output, tool_str);

                            if parsed.is_waiting {
                                crate::types::SessionStatus::Waiting
                            } else if parsed.is_compacting {
                                crate::types::SessionStatus::Compacting
                            } else if parsed.has_exited {
                                crate::types::SessionStatus::Idle
                            } else if parsed.has_error {
                                crate::types::SessionStatus::Error
                            } else if parsed.has_idle_prompt && parsed.has_question {
                                crate::types::SessionStatus::Paused
                            } else if parsed.has_idle_prompt {
                                crate::types::SessionStatus::Idle
                            } else if parsed.is_busy || is_active {
                                crate::types::SessionStatus::Running
                            } else {
                                crate::types::SessionStatus::Idle
                            }
                        }
                        Err(_) => {
                            if is_active {
                                crate::types::SessionStatus::Running
                            } else {
                                crate::types::SessionStatus::Idle
                            }
                        }
                    }
                };

                // Resolve status (debouncing + hysteresis)
                let previous = session.status;
                let resolved = processor.resolve_status(&session.id, raw_status, previous);

                // Write to DB if changed
                if resolved != previous {
                    let _ = bg_storage.write_status(&session.id, resolved, session.tool);
                    any_changed = true;
                }

                // Track durations and fire notifications
                processor.track_durations(&session.id, resolved);
                processor.maybe_notify(session, resolved, attached.as_deref(), bg_sound);
            }

            if any_changed {
                let _ = bg_storage.touch();
            }

            // Log capture every 10 ticks (5s at 500ms interval)
            log_tick += 1;
            if log_tick >= 10 {
                log_tick = 0;
                for session in &sessions {
                    if !session.tmux_session.is_empty()
                        && session.status != crate::types::SessionStatus::Stopped
                    {
                        logger.capture_and_log(&session.tmux_session, &session.id);
                    }
                }

                // Parse tokens from Claude sessions
                let mut tokens_changed = false;
                for session in &sessions {
                    if session.tool == crate::types::Tool::Claude
                        && !session.tmux_session.is_empty()
                        && session.status != crate::types::SessionStatus::Stopped
                    {
                        if let Ok(output) =
                            crate::core::tmux::capture_pane(&session.tmux_session, Some(-50))
                        {
                            if let Some(tokens) =
                                crate::core::tokens::extract_latest_tokens(&output)
                            {
                                if tokens > session.tokens_used {
                                    let diff = tokens - session.tokens_used;
                                    if diff > 0 {
                                        let _ = bg_storage.add_tokens(&session.id, diff);
                                        tokens_changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
                if tokens_changed {
                    let _ = bg_storage.touch();
                }
            }
        }
    });

    // Handle --attach: immediately attach to the session
    if let Some(session_id) = app.attach_session.take() {
        if let Some(session) = app.sessions.iter().find(|s| s.id == session_id) {
            if !session.tmux_session.is_empty() {
                let tmux_name = session.tmux_session.clone();
                if let Ok(mut guard) = attach_state.lock() {
                    guard.attached_session = Some(tmux_name.clone());
                }

                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                let _ = crate::core::tmux::attach_session_sync(&tmux_name);

                if let Ok(mut guard) = attach_state.lock() {
                    guard.suppress_queue.push(tmux_name);
                    guard.attached_session = None;
                }

                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;
            }
        }
    }

    loop {
        // Render
        terminal.draw(|frame| {
            crate::ui::home::render(frame, &app);
        })?;

        // Drain ALL pending keyboard input in one go (not one key per frame)
        while event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                use crossterm::event::KeyCode as KC;
                if app.search_query.is_some() {
                    match key.code {
                        KC::Esc => {
                            app.search_query = None;
                        }
                        KC::Enter => {
                            // Jump to first match then close search
                            let matches = app.search_matches();
                            if let Some(&idx) = matches.first() {
                                app.selected_index = idx;
                            }
                            app.search_query = None;
                        }
                        KC::Backspace => {
                            if let Some(ref mut q) = app.search_query {
                                q.pop();
                            }
                        }
                        KC::Char(c) => {
                            if let Some(ref mut q) = app.search_query {
                                q.push(c);
                                // Auto-jump to first match as user types
                                let query = q.to_lowercase();
                                for (i, row) in app.list_rows.iter().enumerate() {
                                    if let crate::core::groups::ListRow::Session(s) = row {
                                        if s.title.to_lowercase().contains(&query) {
                                            app.selected_index = i;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    match app.overlay {
                        crate::app::Overlay::None => {
                            handle_main_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                                &mut terminal,
                                &attach_state,
                            )?;
                        }
                        crate::app::Overlay::NewSession(_) => {
                            handle_new_session_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                            )?;
                        }
                        crate::app::Overlay::Confirm(_) => {
                            handle_confirm_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                            )?;
                        }
                        crate::app::Overlay::Rename(_) => {
                            handle_rename_key(&mut app, key, &storage)?;
                        }
                        crate::app::Overlay::Move(_) => {
                            handle_move_key(&mut app, key, &storage)?;
                        }
                        crate::app::Overlay::GroupManage(_) => {
                            handle_group_key(&mut app, key, &storage)?;
                        }
                        crate::app::Overlay::CommandPalette(_) => {
                            handle_palette_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                            )?;
                        }
                        crate::app::Overlay::Help => {
                            if key.code == crossterm::event::KeyCode::Esc
                                || key.code == crossterm::event::KeyCode::Char('?')
                                || key.code == crossterm::event::KeyCode::Char('q')
                            {
                                app.overlay = crate::app::Overlay::None;
                            }
                        }
                        crate::app::Overlay::ThemeSelect(_) => {
                            handle_theme_select_key(&mut app, key)?;
                        }
                    }
                }
                if app.should_quit {
                    break;
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Poll storage for changes from the background thread
        {
            let current_mtime = storage.last_modified();
            if current_mtime != app.last_storage_mtime {
                app.last_storage_mtime = current_mtime;
                let new_sessions = storage.load_sessions().unwrap_or_default();

                // Diff statuses for activity feed
                for new_s in &new_sessions {
                    if let Some(old_s) = app.sessions.iter().find(|s| s.id == new_s.id) {
                        if old_s.status != new_s.status {
                            app.push_activity(crate::types::ActivityEvent {
                                session_title: new_s.title.clone(),
                                old_status: old_s.status,
                                new_status: new_s.status,
                                timestamp: chrono::Utc::now().timestamp_millis(),
                                message: None,
                            });
                        }
                    }
                }

                app.sessions = new_sessions;
                app.groups = storage.load_groups().unwrap_or_default();
                app.rebuild_list_rows();
            }
        }

        // Clear expired toasts
        if let Some(expire) = app.toast_expire {
            if expire < Instant::now() {
                app.toast_message = None;
                app.toast_expire = None;
            }
        }

        // Check for config hot-reload
        if app.config_changed.load(std::sync::atomic::Ordering::Relaxed) {
            app.config_changed.store(false, std::sync::atomic::Ordering::Relaxed);
            let new_config = crate::core::config::load_config();
            if new_config.theme == "light" {
                app.theme = crate::ui::theme::Theme::light();
            } else {
                app.theme = crate::ui::theme::Theme::dark();
            }
            app.toast_message = Some("Config reloaded".to_string());
            app.toast_expire = Some(Instant::now() + std::time::Duration::from_secs(2));
        }

        // Sleep briefly to avoid busy-spinning when idle
        if !event::poll(Duration::from_millis(16))? {
            // No input arrived in 16ms — just loop back to render
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}

fn handle_main_key(
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
        (KeyModifiers::NONE, KeyCode::Char('q'))
        | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.should_quit = true;
        }
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            app.move_selection_up();
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            app.move_selection_down();
        }
        (KeyModifiers::NONE, KeyCode::Char('n')) => {
            app.overlay =
                crate::app::Overlay::NewSession(crate::app::NewSessionForm::new());
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
                    let _ = std::io::Write::write_all(
                        &mut std::io::stdout(),
                        b"\x1bc",
                    );
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
                app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
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
                    match export_session_log(&tmux_name, &title, &id) {
                        Ok(path) => {
                            app.toast_message = Some(format!("Exported to {}", path));
                        }
                        Err(e) => {
                            app.toast_message = Some(format!("Export failed: {}", e));
                        }
                    }
                    app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.search_query = Some(String::new());
        }
        (KeyModifiers::NONE, KeyCode::Char('m')) => {
            if let Some(session) = app.selected_session() {
                let groups: Vec<(String, String)> = app.groups.iter()
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
                app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
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
            let current = if app.theme.background == crate::ui::theme::Theme::light().background {
                "light"
            } else {
                "dark"
            };
            app.overlay = crate::app::Overlay::ThemeSelect(crate::app::ThemeSelectForm::new(current));
        }
        _ => {}
    }

    Ok(())
}

fn export_session_log(tmux_session: &str, title: &str, session_id: &str) -> Result<String, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let export_dir = home.join(".agent-view").join("exports");
    std::fs::create_dir_all(&export_dir).map_err(|e| format!("Cannot create exports dir: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe_name: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .take(30)
        .collect();
    let filename = format!("{}-{}.log", safe_name, timestamp);
    let filepath = export_dir.join(&filename);

    // Try continuous log file first
    let log_path = crate::core::logger::session_log_path(session_id);
    if log_path.exists() {
        std::fs::copy(&log_path, &filepath)
            .map_err(|e| format!("Copy failed: {}", e))?;
        return Ok(filepath.to_string_lossy().to_string());
    }

    // Fallback to live capture
    let output = crate::core::tmux::capture_pane(tmux_session, Some(-10000))
        .map_err(|e| format!("Capture failed: {}", e))?;
    std::fs::write(&filepath, &output)
        .map_err(|e| format!("Write failed: {}", e))?;

    Ok(filepath.to_string_lossy().to_string())
}

fn handle_new_session_key(
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
                form.focused_field = (form.focused_field + 1) % 2;
            }
            KeyCode::BackTab => {
                form.focused_field = if form.focused_field == 0 { 1 } else { 0 };
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
                1 => form.project_path.push(c),
                _ => {}
            },
            KeyCode::Backspace => match form.focused_field {
                0 => {
                    form.title.pop();
                }
                1 => {
                    form.project_path.pop();
                }
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}

fn handle_confirm_key(
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

fn handle_rename_key(
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
                                if let Some(mut group) = groups.into_iter().find(|g| g.path == form.target_id) {
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

fn handle_move_key(
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
                    let _ = storage.move_session_to_group(&form.session_id.clone(), &path);
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                    }
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                    app.toast_message = Some(format!("Moved to {}", name));
                    app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                app.overlay = crate::app::Overlay::None;
            }
            _ => {}
        }
    }
    Ok(())
}

fn handle_group_key(
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

fn handle_palette_key(
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
            KeyCode::Up | KeyCode::BackTab => {
                if palette.selected > 0 {
                    palette.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if palette.selected < palette.filtered.len().saturating_sub(1) {
                    palette.selected += 1;
                }
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

fn execute_command_action(
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
            app.overlay = Overlay::GroupManage(crate::app::GroupForm { name: String::new() });
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
                let groups: Vec<(String, String)> = app.groups.iter()
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
                    match export_session_log(&tmux_name, &title, &id) {
                        Ok(path) => {
                            app.toast_message = Some(format!("Exported to {}", path));
                            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                        }
                        Err(e) => {
                            app.toast_message = Some(format!("Export failed: {}", e));
                            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
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
                app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
        }
        CommandAction::ShowHelp => {
            app.overlay = Overlay::Help;
        }
        CommandAction::SelectTheme => {
            let current = if app.theme.background == crate::ui::theme::Theme::light().background {
                "light"
            } else {
                "dark"
            };
            app.overlay = Overlay::ThemeSelect(crate::app::ThemeSelectForm::new(current));
        }
    }
    Ok(())
}

fn handle_theme_select_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::ThemeSelect(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                let original = form.original_theme_name.clone();
                if original == "light" {
                    app.theme = crate::ui::theme::Theme::light();
                } else {
                    app.theme = crate::ui::theme::Theme::dark();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Enter => {
                let chosen = form.options[form.selected].clone();
                let mut config = crate::core::config::load_config();
                config.theme = chosen;
                let _ = crate::core::config::save_config(&config);
                // Suppress the watcher-triggered config reload toast
                app.config_changed.store(false, std::sync::atomic::Ordering::Relaxed);
                app.overlay = crate::app::Overlay::None;
                app.toast_message = Some("Theme saved".to_string());
                app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if form.selected > 0 {
                    form.selected -= 1;
                }
                let theme_name = form.options[form.selected].clone();
                if theme_name == "light" {
                    app.theme = crate::ui::theme::Theme::light();
                } else {
                    app.theme = crate::ui::theme::Theme::dark();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if form.selected < form.options.len() - 1 {
                    form.selected += 1;
                }
                let theme_name = form.options[form.selected].clone();
                if theme_name == "light" {
                    app.theme = crate::ui::theme::Theme::light();
                } else {
                    app.theme = crate::ui::theme::Theme::dark();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// refresh_statuses is now handled by the background thread in run_tui()
