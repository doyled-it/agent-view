//! Background status polling thread

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Spawn the background status polling thread.
pub fn spawn(
    attach_state: Arc<Mutex<crate::core::attach_state::AttachState>>,
    event_state: crate::core::runner::event_watcher::EventStateHandle,
    sound: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
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
            let (attached, suppress_queue) = if let Ok(mut guard) = attach_state.lock() {
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

                // Skip meta monitoring sessions
                if session
                    .tmux_session
                    .starts_with(crate::core::usage::META_SESSION_PREFIX)
                {
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
                    let runner = crate::core::runner::runner_for(session.tool);

                    let hook = if let Ok(s) = event_state.lock() {
                        s.hook_status.get(&session.id).cloned()
                    } else {
                        None
                    };

                    let pane_title_status = if hook.is_none() {
                        crate::core::runner::osc_title::check_pane_title(&session.tmux_session)
                    } else {
                        None
                    };

                    match crate::core::tmux::capture_pane(&session.tmux_session, Some(-100), false)
                    {
                        Ok(output) => {
                            // Prefer tool_session_id from hook; fall back to regex extraction.
                            let session_id_opt = hook
                                .as_ref()
                                .and_then(|h| h.tool_session_id.clone())
                                .or_else(|| runner.extract_session_id(&output));
                            if let Some(session_id) = session_id_opt {
                                let key = runner.tool_data_session_id_key();
                                if !key.is_empty() {
                                    let mut data: serde_json::Value =
                                        serde_json::from_str(&session.tool_data)
                                            .unwrap_or_else(|_| serde_json::json!({}));
                                    if data.get(key).and_then(|v| v.as_str()) != Some(&session_id) {
                                        data[key] = serde_json::Value::String(session_id);
                                        let _ = bg_storage
                                            .update_tool_data(&session.id, &data.to_string());
                                    }
                                }
                            }

                            crate::core::runner::compose_status(
                                hook.as_ref(),
                                pane_title_status,
                                &output,
                                runner,
                                is_active,
                                std::time::SystemTime::now(),
                            )
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
                processor.maybe_notify(session, resolved, attached.as_deref(), sound);
            }

            if any_changed {
                let _ = bg_storage.touch();
            }

            // Detect live routine runs for UI status
            // No heavy processing needed — exec-routine manages its own lifecycle
            // The main loop reloads routines on storage mtime change

            // Log capture every 10 ticks (5s at 500ms interval)
            log_tick += 1;
            if log_tick >= 10 {
                log_tick = 0;
                for session in &sessions {
                    if !session.tmux_session.is_empty()
                        && session.status != crate::types::SessionStatus::Stopped
                        && !session
                            .tmux_session
                            .starts_with(crate::core::usage::META_SESSION_PREFIX)
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
                        && !session
                            .tmux_session
                            .starts_with(crate::core::usage::META_SESSION_PREFIX)
                    {
                        if let Ok(output) =
                            crate::core::tmux::capture_pane(&session.tmux_session, Some(-50), false)
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
    })
}
