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
    let mut app = crate::app::App::new();

    // Load sessions from storage
    app.sessions = storage.load_sessions()?;
    app.clamp_selection();

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

    let mut session_manager = crate::core::session::SessionManager::new();

    // Background thread for status polling — never blocks the UI
    use std::sync::mpsc;
    let (status_tx, status_rx) = mpsc::channel::<Vec<(String, crate::types::SessionStatus)>>();

    // Collect session info for the background thread
    let sessions_for_bg: Vec<(String, String, crate::types::Tool)> = app
        .sessions
        .iter()
        .map(|s| (s.id.clone(), s.tmux_session.clone(), s.tool))
        .collect();

    let bg_handle = std::thread::spawn(move || {
        let mut cache = crate::core::tmux::SessionCache::new();
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

                results.push((session.id.clone(), raw_status));
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
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                let tmux_name = session.tmux_session.clone();
                let _ = crate::core::tmux::attach_session_sync(&tmux_name);

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
                match app.overlay {
                    crate::app::Overlay::None => {
                        handle_main_key(
                            &mut app,
                            key,
                            &storage,
                            &mut session_manager,
                            &mut terminal,
                        )?;
                    }
                    crate::app::Overlay::NewSession(_) => {
                        handle_new_session_key(
                            &mut app,
                            key,
                            &storage,
                            &session_manager,
                        )?;
                    }
                    crate::app::Overlay::Confirm(_) => {
                        handle_confirm_key(
                            &mut app,
                            key,
                            &storage,
                            &session_manager,
                        )?;
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

        // After returning from attach, discard stale status results
        if app.returning_from_attach {
            while status_rx.try_recv().is_ok() {}
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
                    session_manager.maybe_notify(session, resolved, false, sound);
                }
            }
        }
        if any_changed {
            let _ = storage.touch();
            app.sessions = storage.load_sessions().unwrap_or_default();
            app.clamp_selection();
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
    session_manager: &mut crate::core::session::SessionManager,
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
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
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Attach to selected session
            if let Some(session) = app.selected_session() {
                if !session.tmux_session.is_empty()
                    && session.status != crate::types::SessionStatus::Stopped
                {
                    let tmux_name = session.tmux_session.clone();

                    // Leave TUI for attach
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                    let _ = crate::core::tmux::attach_session_sync(&tmux_name);
                    session_manager.suppress_notification(&tmux_name);

                    // Signal main loop to drain stale status results
                    app.returning_from_attach = true;

                    // Re-enter TUI
                    enable_raw_mode()?;
                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                    terminal.clear()?;

                    // Fresh reload after returning
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                        app.clamp_selection();
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
                let _ = session_manager.restart_session(storage, &mut cache, &id);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.clamp_selection();
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('!')) => {
            // Toggle notifications for selected session
            if let Some(session) = app.selected_session() {
                let new_val = !session.notify;
                let id = session.id.clone();
                let _ = storage.set_notify(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.clamp_selection();
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_new_session_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_manager: &crate::core::session::SessionManager,
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
                match session_manager.create_session(storage, &mut cache, options) {
                    Ok(_) => {
                        if let Ok(sessions) = storage.load_sessions() {
                            app.sessions = sessions;
                            // Select the newly created session (last one)
                            if !app.sessions.is_empty() {
                                app.selected_index = app.sessions.len() - 1;
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
    session_manager: &crate::core::session::SessionManager,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Confirm(ref dialog) = app.overlay.clone() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match &dialog.action {
                    crate::app::ConfirmAction::DeleteSession(id) => {
                        let mut cache = crate::core::tmux::SessionCache::new();
                        let _ = session_manager.delete_session(storage, &mut cache, id);
                    }
                    crate::app::ConfirmAction::StopSession(id) => {
                        let _ = session_manager.stop_session(storage, id);
                    }
                }
                // Refresh sessions
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.clamp_selection();
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

// refresh_statuses is now handled by the background thread in run_tui()
