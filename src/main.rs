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

    let mut session_manager = crate::core::session::StatusProcessor::new();
    let session_ops = crate::core::session::SessionOps;

    // Shared attached session name — background thread reads this to fire
    // notifications for OTHER sessions while the user is inside one.
    use std::sync::{Arc, Mutex, mpsc};
    let attached_session_shared: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let attached_for_bg = Arc::clone(&attached_session_shared);

    // Background thread for status polling — never blocks the UI
    let (status_tx, status_rx) = mpsc::channel::<Vec<(String, crate::types::SessionStatus)>>();

    let bg_sound = config.notifications.sound;
    let bg_handle = std::thread::spawn(move || {
        let mut cache = crate::core::tmux::SessionCache::new();
        // Track last notified status per session to avoid repeated notifications
        let mut bg_last_notified: std::collections::HashMap<String, crate::types::SessionStatus> =
            std::collections::HashMap::new();
        loop {
            std::thread::sleep(Duration::from_millis(500));

            cache.refresh();
            let mut results = Vec::new();

            // Read fresh session list from storage each tick
            let bg_storage = match crate::core::storage::Storage::open_default() {
                Ok(s) => { let _ = s.migrate(); s }
                Err(_) => continue,
            };
            let sessions = bg_storage.load_sessions().unwrap_or_default();

            // Check if user is currently attached to a session
            let attached = attached_for_bg.lock().ok().and_then(|g| g.clone());

            for session in &sessions {
                if session.tmux_session.is_empty() {
                    continue;
                }

                let exists = cache.session_exists(&session.tmux_session);
                if !exists {
                    if session.status != crate::types::SessionStatus::Stopped {
                        results.push((session.id.clone(), crate::types::SessionStatus::Stopped));
                    }
                    continue;
                }

                let is_active = cache.is_session_active(&session.tmux_session, 2);

                let raw_status = match crate::core::tmux::capture_pane(&session.tmux_session, Some(-100)) {
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
                };

                // Fire notifications directly from the background thread when user
                // is attached to a DIFFERENT session. The main loop is blocked during
                // attach, so it can't fire notifications — we do it here instead.
                if let Some(ref attached_name) = attached {
                    if session.notify
                        && session.tmux_session != *attached_name
                        && raw_status != session.status
                        && bg_last_notified.get(&session.id) != Some(&raw_status)
                    {
                        let fired = match raw_status {
                            crate::types::SessionStatus::Waiting => {
                                crate::core::notify::send_notification(
                                    crate::core::notify::NotificationOptions {
                                        title: format!("\u{1F7E1} {}", session.title),
                                        body: "Needs approval".to_string(),
                                        subtitle: None,
                                        sound: bg_sound,
                                    },
                                );
                                true
                            }
                            crate::types::SessionStatus::Paused => {
                                crate::core::notify::send_notification(
                                    crate::core::notify::NotificationOptions {
                                        title: format!("\u{1F535} {}", session.title),
                                        body: "Asked you a question".to_string(),
                                        subtitle: None,
                                        sound: bg_sound,
                                    },
                                );
                                true
                            }
                            crate::types::SessionStatus::Error => {
                                crate::core::notify::send_notification(
                                    crate::core::notify::NotificationOptions {
                                        title: format!("\u{1F534} {}", session.title),
                                        body: "Was interrupted".to_string(),
                                        subtitle: None,
                                        sound: bg_sound,
                                    },
                                );
                                true
                            }
                            _ => false,
                        };
                        if fired {
                            bg_last_notified.insert(session.id.clone(), raw_status);
                        }
                    }
                }

                results.push((session.id.clone(), raw_status));
            }

            // Clear stale entries from bg_last_notified when not attached
            if attached.is_none() {
                bg_last_notified.clear();
            }

            if status_tx.send(results).is_err() {
                break; // Main thread dropped the receiver — exit
            }
        }
    });

    // Handle --attach: immediately attach to the session
    if let Some(session_id) = app.attach_session.take() {
        if let Some(session) = app.sessions.iter().find(|s| s.id == session_id) {
            if !session.tmux_session.is_empty() {
                let tmux_name = session.tmux_session.clone();
                app.attached_tmux_session = Some(tmux_name.clone());
                if let Ok(mut guard) = attached_session_shared.lock() {
                    *guard = Some(tmux_name.clone());
                }

                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                let _ = crate::core::tmux::attach_session_sync(&tmux_name);
                session_manager.suppress_notification(&tmux_name);

                app.attached_tmux_session = None;
                if let Ok(mut guard) = attached_session_shared.lock() {
                    *guard = None;
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
                                &mut session_manager,
                                &session_ops,
                                &mut terminal,
                                &attached_session_shared,
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

        // After returning from attach, process pending results (don't discard them).
        // The attached session's notifications were already suppressed during the
        // normal processing loop below — we just need to clear the flag.
        if app.returning_from_attach {
            app.returning_from_attach = false;
        }

        // Apply status updates from background thread (non-blocking)
        // Batch all pending results, only reload DB once at the end
        let mut any_changed = false;
        while let Ok(results) = status_rx.try_recv() {
            for (session_id, raw_status) in results {
                if let Some(session) = app.sessions.iter().find(|s| s.id == session_id) {
                    let previous = session.status;
                    let resolved = session_manager.resolve_status(&session_id, raw_status, previous);

                    if resolved != previous {
                        let _ = storage.write_status(&session_id, resolved, session.tool);
                        any_changed = true;
                    }

                    session_manager.track_durations(&session_id, resolved);

                    let sound = config.notifications.sound;
                    // Skip the expensive get_attached_sessions() subprocess call here;
                    // treat sessions as not attached (slight over-notification is better than lag)
                    session_manager.maybe_notify(
                        session,
                        resolved,
                        app.attached_tmux_session.as_deref(),
                        sound,
                    );
                }
            }
        }
        if any_changed {
            let _ = storage.touch();
            app.sessions = storage.load_sessions().unwrap_or_default();
            app.groups = storage.load_groups().unwrap_or_default();
            app.rebuild_list_rows();
        }

        // Clear expired toasts
        if let Some(expire) = app.toast_expire {
            if expire < Instant::now() {
                app.toast_message = None;
                app.toast_expire = None;
            }
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
    session_manager: &mut crate::core::session::StatusProcessor,
    session_ops: &crate::core::session::SessionOps,
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
    attached_session_shared: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    use crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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
                    app.attached_tmux_session = Some(tmux_name.clone());
                    // Tell the background thread which session we're in
                    if let Ok(mut guard) = attached_session_shared.lock() {
                        *guard = Some(tmux_name.clone());
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
                    session_manager.suppress_notification(&tmux_name);

                    // Signal main loop to process pending results (not drain them)
                    app.returning_from_attach = true;
                    app.attached_tmux_session = None;
                    // Tell the background thread we're no longer attached
                    if let Ok(mut guard) = attached_session_shared.lock() {
                        *guard = None;
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
            // Stop selected session
            if let Some(session) = app.selected_session() {
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
            // Delete selected session
            if let Some(session) = app.selected_session() {
                let msg = format!("Delete session \"{}\"?", session.title);
                app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::DeleteSession(session.id.clone()),
                });
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
                    match export_session_log(&tmux_name, &title) {
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
        _ => {}
    }

    Ok(())
}

fn export_session_log(tmux_session: &str, title: &str) -> Result<String, String> {
    let output = crate::core::tmux::capture_pane(tmux_session, Some(-10000))
        .map_err(|e| format!("Capture failed: {}", e))?;

    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let logs_dir = home.join(".agent-view").join("logs");
    std::fs::create_dir_all(&logs_dir).map_err(|e| format!("Cannot create logs dir: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe_name: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .take(30)
        .collect();
    let filename = format!("{}-{}.log", safe_name, timestamp);
    let filepath = logs_dir.join(&filename);

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
                    match export_session_log(&tmux_name, &title) {
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
    }
    Ok(())
}

// refresh_statuses is now handled by the background thread in run_tui()
