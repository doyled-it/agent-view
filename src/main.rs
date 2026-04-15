mod app;
mod core;
mod event;
mod input;
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

    // Initialize app state — CLI --light flag overrides config
    let theme_name = if cli.light {
        "light".to_string()
    } else {
        config.theme.clone()
    };
    let mut app = crate::app::App::new(false); // we set theme manually below
    app.theme = crate::ui::theme::Theme::from_name(&theme_name);
    app.theme_name = theme_name;

    // Load sessions from storage
    app.sessions = storage.load_sessions()?;
    app.groups = storage.load_groups().unwrap_or_default();
    app.rebuild_list_rows();

    // Detect crashed sessions (tmux died since last run)
    let crashed_ids = crate::core::session::detect_crashed_statuses(&app.sessions);
    for id in &crashed_ids {
        let tool = app
            .sessions
            .iter()
            .find(|s| s.id == *id)
            .map(|s| s.tool)
            .unwrap_or(crate::types::Tool::Claude);
        let _ = storage.write_status(id, crate::types::SessionStatus::Crashed, tool);
    }
    if !crashed_ids.is_empty() {
        app.sessions = storage.load_sessions().unwrap_or_default();
        app.rebuild_list_rows();
    }

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
                Ok(s) => {
                    let _ = s.migrate();
                    s
                }
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
                    if session.status != crate::types::SessionStatus::Stopped
                        && session.status != crate::types::SessionStatus::Crashed
                    {
                        crate::types::SessionStatus::Crashed
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

                            // Capture Claude session ID if present
                            if tool_str == Some("claude") {
                                if let Some(session_id) =
                                    crate::core::status::extract_claude_session_id(&output)
                                {
                                    let mut data: serde_json::Value =
                                        serde_json::from_str(&session.tool_data)
                                            .unwrap_or_else(|_| serde_json::json!({}));
                                    if data.get("claude_session_id").and_then(|v| v.as_str())
                                        != Some(&session_id)
                                    {
                                        data["claude_session_id"] =
                                            serde_json::Value::String(session_id);
                                        let _ = bg_storage
                                            .update_tool_data(&session.id, &data.to_string());
                                    }
                                }
                            }

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
                    guard.suppress_queue.push(tmux_name.clone());
                    guard.attached_session = None;
                }

                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;

                // Fresh reload after returning; restore cursor to attached session
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                    if let Some(pos) = app.list_rows.iter().position(|row| {
                        matches!(row, crate::core::groups::ListRow::Session(s) if s.tmux_session == tmux_name)
                    }) {
                        app.selected_index = pos;
                    }
                }
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
                            crate::input::handle_main_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                                &mut terminal,
                                &attach_state,
                            )?;
                        }
                        crate::app::Overlay::NewSession(_) => {
                            crate::input::session::handle_new_session_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                            )?;
                        }
                        crate::app::Overlay::Confirm(_) => {
                            crate::input::session::handle_confirm_key(
                                &mut app,
                                key,
                                &storage,
                                &session_ops,
                            )?;
                        }
                        crate::app::Overlay::Rename(_) => {
                            crate::input::session::handle_rename_key(&mut app, key, &storage)?;
                        }
                        crate::app::Overlay::Move(_) => {
                            crate::input::session::handle_move_key(&mut app, key, &storage)?;
                        }
                        crate::app::Overlay::GroupManage(_) => {
                            crate::input::overlay::handle_group_key(&mut app, key, &storage)?;
                        }
                        crate::app::Overlay::CommandPalette(_) => {
                            crate::input::overlay::handle_palette_key(
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
                            crate::input::overlay::handle_theme_select_key(&mut app, key)?;
                        }
                        crate::app::Overlay::AddNote(_) => {
                            crate::input::overlay::handle_add_note_key(&mut app, key, &storage)?;
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
        if app
            .config_changed
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            app.config_changed
                .store(false, std::sync::atomic::Ordering::Relaxed);
            let new_config = crate::core::config::load_config();
            app.theme = crate::ui::theme::Theme::from_name(&new_config.theme);
            app.theme_name = new_config.theme;
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
