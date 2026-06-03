use rusqlite::params;
use rusqlite::Result as SqlResult;

use super::Storage;
use crate::types::{Session, SessionStatus, StatusHistoryEntry, Tool};

impl Storage {
    /// Save a session (insert or replace)
    pub fn save_session(&self, session: &Session) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (
                id, title, project_path, group_path, sort_order,
                command, wrapper, tool, status, tmux_session,
                created_at, last_accessed,
                parent_session_id, worktree_path, worktree_repo, worktree_branch,
                tool_data, acknowledged,
                notify, follow_up, user_waiting, status_changed_at, restart_count, status_history,
                pinned, tokens_used, last_started_at, notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)",
            params![
                session.id,
                session.title,
                session.project_path,
                session.group_path,
                session.order,
                session.command,
                session.wrapper,
                session.tool.as_str(),
                session.status.as_str(),
                session.tmux_session,
                session.created_at,
                session.last_accessed,
                session.parent_session_id,
                session.worktree_path,
                session.worktree_repo,
                session.worktree_branch,
                session.tool_data,
                session.acknowledged as i32,
                session.notify as i32,
                session.follow_up as i32,
                session.user_waiting as i32,
                session.status_changed_at,
                session.restart_count,
                session.status_history_json(),
                session.pinned as i32,
                session.tokens_used,
                session.last_started_at,
                session.notes_json(),
            ],
        )?;
        Ok(())
    }

    /// Load all sessions ordered by sort_order
    pub fn load_sessions(&self) -> SqlResult<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, project_path, group_path, sort_order,
                    command, wrapper, tool, status, tmux_session,
                    created_at, last_accessed,
                    parent_session_id, worktree_path, worktree_repo, worktree_branch,
                    tool_data, acknowledged,
                    notify, follow_up, user_waiting, status_changed_at, restart_count, status_history,
                    pinned, tokens_used, last_started_at, notes
             FROM sessions ORDER BY sort_order",
        )?;

        let rows = stmt.query_map([], |row| {
            let tool_str: String = row.get(7)?;
            let status_str: String = row.get(8)?;
            let history_json: String = row.get(23)?;
            let status_changed_at: i64 = row.get(21)?;
            let created_at: i64 = row.get(10)?;

            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                project_path: row.get(2)?,
                group_path: row.get(3)?,
                order: row.get(4)?,
                command: row.get(5)?,
                wrapper: row.get(6)?,
                tool: Tool::from_str(&tool_str),
                status: SessionStatus::from_str(&status_str),
                tmux_session: row.get(9)?,
                created_at,
                last_accessed: row.get(11)?,
                parent_session_id: row.get(12)?,
                worktree_path: row.get(13)?,
                worktree_repo: row.get(14)?,
                worktree_branch: row.get(15)?,
                tool_data: row.get(16)?,
                acknowledged: row.get::<_, i32>(17)? == 1,
                notify: row.get::<_, i32>(18)? == 1,
                follow_up: row.get::<_, i32>(19)? == 1,
                user_waiting: row.get::<_, i32>(20)? == 1,
                status_changed_at: if status_changed_at > 0 {
                    status_changed_at
                } else {
                    created_at
                },
                restart_count: row.get(22)?,
                last_started_at: {
                    let v: i64 = row.get(26).unwrap_or(0);
                    if v > 0 {
                        v
                    } else {
                        created_at
                    }
                },
                notes: {
                    let json: String = row.get(27).unwrap_or_else(|_| "[]".to_string());
                    serde_json::from_str(&json).unwrap_or_default()
                },
                status_history: serde_json::from_str(&history_json).unwrap_or_default(),
                pinned: row.get::<_, i32>(24)? == 1,
                tokens_used: row.get(25)?,
            })
        })?;

        rows.collect()
    }

    /// Get a single session by ID
    pub fn get_session(&self, id: &str) -> SqlResult<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, project_path, group_path, sort_order,
                    command, wrapper, tool, status, tmux_session,
                    created_at, last_accessed,
                    parent_session_id, worktree_path, worktree_repo, worktree_branch,
                    tool_data, acknowledged,
                    notify, follow_up, user_waiting, status_changed_at, restart_count, status_history,
                    pinned, tokens_used, last_started_at, notes
             FROM sessions WHERE id = ?1",
        )?;

        let result = stmt.query_row(params![id], |row| {
            let tool_str: String = row.get(7)?;
            let status_str: String = row.get(8)?;
            let history_json: String = row.get(23)?;
            let status_changed_at: i64 = row.get(21)?;
            let created_at: i64 = row.get(10)?;

            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                project_path: row.get(2)?,
                group_path: row.get(3)?,
                order: row.get(4)?,
                command: row.get(5)?,
                wrapper: row.get(6)?,
                tool: Tool::from_str(&tool_str),
                status: SessionStatus::from_str(&status_str),
                tmux_session: row.get(9)?,
                created_at,
                last_accessed: row.get(11)?,
                parent_session_id: row.get(12)?,
                worktree_path: row.get(13)?,
                worktree_repo: row.get(14)?,
                worktree_branch: row.get(15)?,
                tool_data: row.get(16)?,
                acknowledged: row.get::<_, i32>(17)? == 1,
                notify: row.get::<_, i32>(18)? == 1,
                follow_up: row.get::<_, i32>(19)? == 1,
                user_waiting: row.get::<_, i32>(20)? == 1,
                status_changed_at: if status_changed_at > 0 {
                    status_changed_at
                } else {
                    created_at
                },
                restart_count: row.get(22)?,
                last_started_at: {
                    let v: i64 = row.get(26).unwrap_or(0);
                    if v > 0 {
                        v
                    } else {
                        created_at
                    }
                },
                notes: {
                    let json: String = row.get(27).unwrap_or_else(|_| "[]".to_string());
                    serde_json::from_str(&json).unwrap_or_default()
                },
                status_history: serde_json::from_str(&history_json).unwrap_or_default(),
                pinned: row.get::<_, i32>(24)? == 1,
                tokens_used: row.get(25)?,
            })
        });

        match result {
            Ok(session) => Ok(Some(session)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Delete a session by ID
    pub fn delete_session(&self, id: &str) -> SqlResult<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Update status and tool for a session
    pub fn write_status(&self, id: &str, status: SessionStatus, tool: Tool) -> SqlResult<()> {
        // Check if status actually changed (to append to history)
        let current: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();

        if let Some(current_status) = current {
            if current_status != status.as_str() {
                let now = chrono::Utc::now().timestamp_millis();
                self.update_status_history(id, status, now)?;
            }
        }

        self.conn.execute(
            "UPDATE sessions SET status = ?1, tool = ?2 WHERE id = ?3",
            params![status.as_str(), tool.as_str(), id],
        )?;
        Ok(())
    }

    /// Toggle or set the notify flag
    pub fn set_notify(&self, id: &str, notify: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET notify = ?1 WHERE id = ?2",
            params![notify as i32, id],
        )?;
        Ok(())
    }

    /// Toggle or set the follow_up flag
    pub fn set_follow_up(&self, id: &str, follow_up: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET follow_up = ?1 WHERE id = ?2",
            params![follow_up as i32, id],
        )?;
        Ok(())
    }

    /// Toggle or set the operator waiting marker.
    pub fn set_user_waiting(&self, id: &str, user_waiting: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET user_waiting = ?1 WHERE id = ?2",
            params![user_waiting as i32, id],
        )?;
        Ok(())
    }

    /// Set the pinned flag
    pub fn set_pinned(&self, id: &str, pinned: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET pinned = ?1 WHERE id = ?2",
            params![pinned as i32, id],
        )?;
        Ok(())
    }

    /// Replace the token count (used by the poller's live context-size update).
    pub fn set_tokens(&self, id: &str, tokens: i64) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET tokens_used = ?1 WHERE id = ?2",
            params![tokens, id],
        )?;
        Ok(())
    }

    /// Update only the tool_data field for a session
    pub fn update_tool_data(&self, session_id: &str, tool_data: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET tool_data = ?1 WHERE id = ?2",
            params![tool_data, session_id],
        )?;
        Ok(())
    }

    /// Append a status entry to status_history (capped at 50 entries)
    pub fn update_status_history(
        &self,
        id: &str,
        status: SessionStatus,
        timestamp: i64,
    ) -> SqlResult<()> {
        let history_json: String = self
            .conn
            .query_row(
                "SELECT status_history FROM sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "[]".to_string());

        let mut history: Vec<StatusHistoryEntry> =
            serde_json::from_str(&history_json).unwrap_or_default();

        history.push(StatusHistoryEntry {
            status: status.as_str().to_string(),
            timestamp,
        });

        // Cap at 50 entries
        if history.len() > 50 {
            let start = history.len() - 50;
            history = history[start..].to_vec();
        }

        let new_json = serde_json::to_string(&history).unwrap_or_else(|_| "[]".to_string());

        self.conn.execute(
            "UPDATE sessions SET status_history = ?1, status_changed_at = ?2 WHERE id = ?3",
            params![new_json, timestamp, id],
        )?;

        Ok(())
    }

    /// Increment the restart count for a session
    pub fn increment_restart_count(&self, id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET restart_count = restart_count + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Rename a session
    pub fn rename_session(&self, id: &str, new_title: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?1 WHERE id = ?2",
            params![new_title, id],
        )?;
        Ok(())
    }

    /// Move a session to a different group
    pub fn move_session_to_group(&self, id: &str, group_path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET group_path = ?1 WHERE id = ?2",
            params![group_path, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use crate::types::{SessionStatus, Tool};

    #[test]
    fn test_save_and_load_session() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        let loaded = storage.load_sessions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "s1");
        assert_eq!(loaded[0].title, "Session s1");
        assert_eq!(loaded[0].tool, Tool::Claude);
        assert_eq!(loaded[0].status, SessionStatus::Idle);
    }

    #[test]
    fn test_get_session_by_id() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        let found = storage.get_session("s1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Session s1");

        let missing = storage.get_session("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_session() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        storage.delete_session("s1").unwrap();

        let loaded = storage.load_sessions().unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_write_status() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage
            .write_status("s1", SessionStatus::Running, Tool::Claude)
            .unwrap();

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.status, SessionStatus::Running);
    }

    #[test]
    fn test_set_notify() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage.set_notify("s1", true).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(loaded.notify);

        storage.set_notify("s1", false).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(!loaded.notify);
    }

    #[test]
    fn test_set_user_waiting() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage.set_user_waiting("s1", true).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(loaded.user_waiting);

        storage.set_user_waiting("s1", false).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(!loaded.user_waiting);
    }

    #[test]
    fn test_update_status_history() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage
            .update_status_history("s1", SessionStatus::Running, 1700000001000)
            .unwrap();
        storage
            .update_status_history("s1", SessionStatus::Waiting, 1700000002000)
            .unwrap();

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.status_history.len(), 2);
        assert_eq!(loaded.status_history[0].status, "running");
        assert_eq!(loaded.status_history[1].status, "waiting");
        assert_eq!(loaded.status_changed_at, 1700000002000);
    }

    #[test]
    fn test_increment_restart_count() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage.increment_restart_count("s1").unwrap();
        storage.increment_restart_count("s1").unwrap();

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.restart_count, 2);
    }

    #[test]
    fn test_status_history_caps_at_50() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        for i in 0..60 {
            storage
                .update_status_history("s1", SessionStatus::Running, 1700000000000 + i)
                .unwrap();
        }

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.status_history.len(), 50);
    }

    #[test]
    fn test_set_pinned() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage.set_pinned("s1", true).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(loaded.pinned);

        storage.set_pinned("s1", false).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(!loaded.pinned);
    }

    #[test]
    fn test_set_tokens_overwrites() {
        let (storage, _dir) = test_storage();
        let mut session = make_test_session("s1");
        session.tokens_used = 999;
        storage.save_session(&session).unwrap();
        storage.set_tokens("s1", 5000).unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.tokens_used, 5000);
    }

    #[test]
    fn test_rename_session() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        storage.rename_session("s1", "New Name").unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.title, "New Name");
    }

    #[test]
    fn test_move_session_to_group() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        storage.move_session_to_group("s1", "work").unwrap();
        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.group_path, "work");
    }
}
