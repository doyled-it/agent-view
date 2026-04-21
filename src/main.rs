mod app;
mod core;
mod event;
mod input;
mod poller;
mod types;
mod ui;

use clap::Parser;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "agent-view", version = VERSION, about = "Terminal UI for managing AI coding agent sessions")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Use light mode theme
    #[arg(long)]
    light: bool,

    /// Attach to session immediately (for notification click-through)
    #[arg(long)]
    attach: Option<String>,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Execute a scheduled routine (called by system scheduler)
    ExecRoutine {
        /// The routine ID to execute
        routine_id: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Handle subcommands that don't need the TUI
    if let Some(Commands::ExecRoutine { routine_id }) = &cli.command {
        return crate::core::routine::exec_routine(routine_id).map_err(|e| e.into());
    }

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
    app.detail_mode = crate::app::DetailPanelMode::from_str(&config.detail_panel_mode);

    // Load sessions from storage
    app.sessions = storage.load_sessions()?;
    app.groups = storage.load_groups().unwrap_or_default();
    app.rebuild_list_rows();

    // Load routines
    app.routines = storage.load_routines().unwrap_or_default();
    for routine in &app.routines {
        if let Ok(runs) = storage.load_routine_runs(&routine.id) {
            app.routine_runs_cache.insert(routine.id.clone(), runs);
        }
    }
    app.rebuild_routine_list_rows();

    // Reconcile scheduler state
    {
        let scheduler = crate::core::scheduler::platform_scheduler();
        let mut stale_count = 0;
        for routine in &app.routines {
            if routine.enabled && !scheduler.is_installed(&routine.id) {
                // Re-install missing job
                let _ = scheduler.install(routine);
            } else if routine.enabled && scheduler.has_stale_binary_path(&routine.id) {
                // Binary moved since job was installed — re-register with current path
                let _ = scheduler.uninstall(&routine.id);
                let _ = scheduler.install(routine);
                stale_count += 1;
            }
            if !routine.enabled && scheduler.is_installed(&routine.id) {
                // Remove orphaned job
                let _ = scheduler.uninstall(&routine.id);
            }
        }
        if stale_count > 0 {
            app.toast_message = Some(format!(
                "Re-registered {} routine(s) with updated binary path",
                stale_count
            ));
            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
        }

        // Mark crashed runs (finished_at IS NULL but tmux session gone)
        for routine in &app.routines {
            if let Ok(runs) = storage.load_routine_runs(&routine.id) {
                for run in &runs {
                    if run.finished_at.is_none() {
                        let tmux_alive = run
                            .tmux_session
                            .as_ref()
                            .map(|t| crate::core::tmux::session_exists(t))
                            .unwrap_or(false);
                        if !tmux_alive {
                            let _ = storage.update_routine_run_status(
                                &run.id,
                                crate::types::RunStatus::Crashed,
                                Some(chrono::Utc::now().timestamp_millis()),
                            );
                        }
                    }
                }
            }
        }
    }

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
    let bg_sound = config.notifications.sound;
    let _bg_handle = crate::poller::spawn(Arc::clone(&attach_state), bg_sound);

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
                        crate::app::Overlay::NewRoutine(_) => {
                            crate::input::routine::handle_new_routine_key(&mut app, key, &storage);
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

                // Reload routines
                app.routines = storage.load_routines().unwrap_or_default();
                for routine in &app.routines {
                    if let Ok(runs) = storage.load_routine_runs(&routine.id) {
                        app.routine_runs_cache.insert(routine.id.clone(), runs);
                    }
                }
                app.rebuild_routine_list_rows();
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
            app.detail_mode = crate::app::DetailPanelMode::from_str(&new_config.detail_panel_mode);
            app.toast_message = Some("Config reloaded".to_string());
            app.toast_expire = Some(Instant::now() + std::time::Duration::from_secs(2));
        }

        // Refresh preview content for the selected item (throttled)
        if app.detail_mode.shows_preview() {
            match app.active_tab {
                crate::app::ActiveTab::Sessions => {
                    let should_capture = if let Some(session) = app.selected_session() {
                        let session_id = session.id.clone();
                        let tmux_empty = session.tmux_session.is_empty();
                        let status = session.status;
                        let session_changed =
                            app.preview_last_session.as_deref() != Some(session_id.as_str());
                        let time_elapsed = app
                            .preview_last_capture
                            .map(|t| t.elapsed() >= std::time::Duration::from_millis(200))
                            .unwrap_or(true);
                        if session_changed {
                            app.preview_last_session = Some(session_id);
                            app.preview_content.clear();
                        }
                        (session_changed || time_elapsed)
                            && !tmux_empty
                            && status != crate::types::SessionStatus::Stopped
                            && status != crate::types::SessionStatus::Crashed
                    } else {
                        false
                    };

                    if should_capture {
                        if let Some(session) = app.selected_session() {
                            let tmux_name = session.tmux_session.clone();
                            match crate::core::tmux::capture_pane(&tmux_name, Some(-50), true) {
                                Ok(content) => {
                                    app.preview_content = content;
                                }
                                Err(_) => {
                                    app.preview_content.clear();
                                }
                            }
                            app.preview_last_capture = Some(std::time::Instant::now());
                        }
                    }
                }
                crate::app::ActiveTab::Routines => {
                    let should_capture = app
                        .preview_last_capture
                        .map(|t| t.elapsed() >= std::time::Duration::from_millis(500))
                        .unwrap_or(true);

                    if should_capture {
                        let content = match app.routine_list_rows.get(app.routine_selected_index) {
                            Some(crate::app::RoutineListRow::Routine(routine)) => app
                                .routine_runs_cache
                                .get(&routine.id)
                                .and_then(|runs| runs.first())
                                .and_then(|run| {
                                    if run.finished_at.is_none() {
                                        if let Some(ref tmux_name) = run.tmux_session {
                                            if crate::core::tmux::session_exists(tmux_name) {
                                                return crate::core::tmux::capture_pane(
                                                    tmux_name,
                                                    Some(-50),
                                                    true,
                                                )
                                                .ok();
                                            }
                                        }
                                    }
                                    run.log_path
                                        .as_ref()
                                        .and_then(|p| std::fs::read_to_string(p).ok())
                                })
                                .unwrap_or_default(),
                            Some(crate::app::RoutineListRow::Run { run, .. }) => {
                                if run.finished_at.is_none() {
                                    if let Some(ref tmux_name) = run.tmux_session {
                                        if crate::core::tmux::session_exists(tmux_name) {
                                            crate::core::tmux::capture_pane(
                                                tmux_name,
                                                Some(-50),
                                                true,
                                            )
                                            .unwrap_or_default()
                                        } else {
                                            run.log_path
                                                .as_ref()
                                                .and_then(|p| std::fs::read_to_string(p).ok())
                                                .unwrap_or_default()
                                        }
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    run.log_path
                                        .as_ref()
                                        .and_then(|p| std::fs::read_to_string(p).ok())
                                        .unwrap_or_default()
                                }
                            }
                            _ => String::new(),
                        };
                        app.preview_content = content;
                        app.preview_last_capture = Some(std::time::Instant::now());
                    }
                }
            }
        } else if !app.preview_content.is_empty() {
            app.preview_content.clear();
            app.preview_last_session = None;
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
