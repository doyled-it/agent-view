//! Background status polling thread

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Live context-size resolver. For each tool with an authoritative source
/// (Claude transcript JSONL, Codex rollout JSONL), returns the current
/// context-token count. `None` for Shell or when the necessary metadata
/// (transcript path, cached rollout path) isn't available yet.
///
/// Takes `&mut EventState` because the Codex branch consults — and may
/// refresh — the mtime-gated rollout snapshot cache. The filesystem walk
/// to discover a rollout path runs only on the watcher thread; this
/// function never walks.
fn live_context_tokens(
    session: &crate::types::Session,
    state: &mut crate::core::runner::event_watcher::EventState,
) -> Option<i64> {
    use crate::types::Tool;
    match session.tool {
        Tool::Claude => {
            let entry = state.hook_status.get(&session.id)?;
            let transcript_path: std::path::PathBuf = entry.transcript_path.clone()?.into();
            crate::core::runner::claude::hook_handler::current_context_tokens(&transcript_path)
        }
        Tool::Codex => {
            let entry = state.hook_status.get(&session.id)?;
            let thread_id = entry.tool_session_id.clone()?;
            let path = state.cached_rollout_path(&thread_id)?.to_path_buf();
            state.rollout_snapshot(&path).context_tokens
        }
        _ => None,
    }
}

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

                    match crate::core::tmux::capture_pane(
                        &session.tmux_session,
                        Some(-100),
                        runner.wants_ansi_escapes(),
                    ) {
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

                // Live context-size update — runs for every tool that exposes
                // an authoritative source (Claude transcript JSONL, Codex
                // rollout JSONL). Falls back to no-op for Shell sessions.
                let mut tokens_changed = false;
                for session in &sessions {
                    if session.tmux_session.is_empty()
                        || session.status == crate::types::SessionStatus::Stopped
                        || session
                            .tmux_session
                            .starts_with(crate::core::usage::META_SESSION_PREFIX)
                    {
                        continue;
                    }
                    let Ok(mut state_guard) = event_state.lock() else {
                        continue;
                    };
                    if let Some(tokens) = live_context_tokens(session, &mut state_guard) {
                        drop(state_guard);
                        if tokens != session.tokens_used {
                            // Overwrite rather than incremental add: the new
                            // source is the absolute context size, not a
                            // monotonic counter.
                            let _ = bg_storage.set_tokens(&session.id, tokens);
                            tokens_changed = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runner::event_watcher::EventState;
    use crate::types::{Session, SessionStatus, Tool};

    fn make_session(id: &str, tool: Tool) -> Session {
        Session {
            id: id.to_string(),
            title: String::new(),
            project_path: String::new(),
            group_path: String::new(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool,
            status: SessionStatus::Idle,
            tmux_session: String::new(),
            created_at: 0,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            mcp_selection: crate::core::mcp::McpSelection::default(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            user_waiting: false,
            status_changed_at: 0,
            restart_count: 0,
            last_started_at: 0,
            notes: vec![],
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        }
    }

    #[test]
    fn live_tokens_returns_none_for_shell_session() {
        let session = make_session("s1", Tool::Shell);
        let mut state = EventState::default();
        assert!(live_context_tokens(&session, &mut state).is_none());
    }

    #[test]
    fn live_tokens_returns_none_when_no_hook_status() {
        let session = make_session("no-hooks-yet", Tool::Claude);
        let mut state = EventState::default();
        assert!(live_context_tokens(&session, &mut state).is_none());
    }

    #[test]
    fn live_tokens_codex_reads_from_cached_rollout_snapshot() {
        // Codex success path: a rollout path is already in the cache (the
        // watcher thread would have populated this via process_hook_file
        // bootstrap). `live_context_tokens` must NOT walk the filesystem
        // — it reads via `EventState::rollout_snapshot`.
        use crate::core::runner::event_watcher::HookStatus;
        use std::time::SystemTime;

        let dir = tempfile::tempdir().unwrap();
        let rollout = dir.path().join("rollout.jsonl");
        let body = concat!(
            r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150},"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0,"total_tokens":150}}}}"#,
            "\n",
        );
        std::fs::write(&rollout, body).unwrap();

        let thread = "019e289a-0f2d-73f1-94d3-d15182ff1741".to_string();
        let mut state = EventState::default();
        state.hook_status.insert(
            "av-sess".to_string(),
            HookStatus {
                status: SessionStatus::Idle,
                tool_session_id: Some(thread.clone()),
                received_at: SystemTime::now(),
                transcript_path: None,
                claude_context_window: None,
            },
        );
        state.record_rollout_path(&thread, rollout);

        let session = make_session("av-sess", Tool::Codex);
        assert_eq!(live_context_tokens(&session, &mut state), Some(150));
    }
}
