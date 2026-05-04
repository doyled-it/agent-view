use std::collections::HashMap;

use crate::core::storage::Storage;
use crate::core::tmux;
use crate::core::tmux::SessionCache;
use crate::types::{Session, SessionCreateOptions, SessionStatus, StatusHistoryEntry, Tool};

use super::{build_restart_command, generate_title};

/// Session lifecycle operations (create, stop, delete, restart).
/// Stateless — lives on the main thread.
pub struct SessionOps;

impl SessionOps {
    /// Create a new session (creates tmux session and saves to storage)
    pub fn create_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        options: SessionCreateOptions,
    ) -> Result<Session, String> {
        let title = options.title.unwrap_or_else(generate_title);
        let id = uuid::Uuid::new_v4().to_string();
        let tmux_name = tmux::generate_session_name(&title);
        let command = options
            .command
            .unwrap_or_else(|| options.tool.command().to_string());

        let now = chrono::Utc::now().timestamp_millis();

        let mut env = HashMap::new();
        env.insert("AGENT_ORCHESTRATOR_SESSION".to_string(), id.clone());

        tmux::create_session(
            &tmux_name,
            Some(&command),
            Some(&options.project_path),
            Some(&env),
        )?;

        cache.register(&tmux_name);

        let session = Session {
            id: id.clone(),
            title,
            project_path: options.project_path,
            group_path: options
                .group_path
                .unwrap_or_else(|| "my-sessions".to_string()),
            order: storage.load_sessions().unwrap_or_default().len() as i32,
            command,
            wrapper: String::new(),
            tool: options.tool,
            status: SessionStatus::Running,
            tmux_session: tmux_name,
            created_at: now,
            last_accessed: now,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: now,
            restart_count: 0,
            last_started_at: now,
            notes: vec![],
            status_history: vec![StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: now,
            }],
            pinned: false,
            tokens_used: 0,
        };

        storage
            .save_session(&session)
            .map_err(|e| format!("Failed to save session: {}", e))?;
        storage.touch().ok();

        Ok(session)
    }

    /// Stop a session (kill tmux but keep the record)
    pub fn stop_session(&self, storage: &Storage, session_id: &str) -> Result<(), String> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?
            .ok_or_else(|| "Session not found".to_string())?;

        if !session.tmux_session.is_empty() {
            tmux::kill_session(&session.tmux_session)?;
        }

        storage
            .write_status(session_id, SessionStatus::Stopped, session.tool)
            .map_err(|e| format!("DB error: {}", e))?;
        storage.touch().ok();

        Ok(())
    }

    /// Delete a session (kill tmux and remove from storage)
    pub fn delete_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
    ) -> Result<(), String> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?;

        if let Some(session) = session {
            if !session.tmux_session.is_empty() {
                tmux::kill_session(&session.tmux_session)?;
                cache.remove(&session.tmux_session);
            }
        }

        storage
            .delete_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?;
        storage.touch().ok();

        Ok(())
    }

    /// Restart a session (kill and recreate tmux session)
    pub fn restart_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
    ) -> Result<Session, String> {
        let mut session = storage
            .get_session(session_id)
            .map_err(|e| format!("DB error: {}", e))?
            .ok_or_else(|| "Session not found".to_string())?;

        if !session.tmux_session.is_empty() {
            if tmux::session_exists(&session.tmux_session) {
                tmux::kill_session(&session.tmux_session)?;
            }
            cache.remove(&session.tmux_session);
        }

        let new_tmux_name = tmux::generate_session_name(&session.title);
        let mut env = HashMap::new();
        env.insert("AGENT_ORCHESTRATOR_SESSION".to_string(), session.id.clone());

        let restart_cmd = build_restart_command(session.tool, &session.command, &session.tool_data);
        tmux::create_session(
            &new_tmux_name,
            Some(&restart_cmd),
            Some(&session.project_path),
            Some(&env),
        )?;

        cache.register(&new_tmux_name);

        session.tmux_session = new_tmux_name;
        session.status = SessionStatus::Running;
        let now = chrono::Utc::now().timestamp_millis();
        session.last_accessed = now;
        session.last_started_at = now;

        // Clear old Claude session ID — new session will get a new one
        if session.tool == Tool::Claude {
            if let Ok(mut data) = serde_json::from_str::<serde_json::Value>(&session.tool_data) {
                data.as_object_mut().map(|o| o.remove("claude_session_id"));
                session.tool_data = data.to_string();
            }
        }

        storage
            .save_session(&session)
            .map_err(|e| format!("DB error: {}", e))?;
        storage
            .increment_restart_count(session_id)
            .map_err(|e| format!("DB error: {}", e))?;
        storage.touch().ok();

        Ok(session)
    }
}
