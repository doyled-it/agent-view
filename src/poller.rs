//! Background status polling thread

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Spawn the background status polling thread.
pub fn spawn(
    attach_state: Arc<Mutex<crate::core::attach_state::AttachState>>,
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
                    match crate::core::tmux::capture_pane(&session.tmux_session, Some(-100), false)
                    {
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
                            } else if parsed.has_draft {
                                // User has typed text at the prompt — draft overrides paused
                                crate::types::SessionStatus::Draft
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
