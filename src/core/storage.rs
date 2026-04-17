//! SQLite storage for session/group persistence
//! Compatible with the TypeScript version's schema (v3)

use rusqlite::{params, Connection, Result as SqlResult};
use std::fs;
use std::path::PathBuf;

const SCHEMA_VERSION: i32 = 7;

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(db_path: &str) -> SqlResult<Self> {
        let path = PathBuf::from(db_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(Self { conn })
    }

    pub fn open_default() -> SqlResult<Self> {
        let home = dirs::home_dir().expect("Cannot determine home directory");
        let db_path = home.join(".agent-orchestrator").join("state.db");
        Self::open(db_path.to_str().unwrap())
    }

    pub fn migrate(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )?;

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                project_path TEXT NOT NULL,
                group_path TEXT NOT NULL DEFAULT 'my-sessions',
                sort_order INTEGER NOT NULL DEFAULT 0,
                command TEXT NOT NULL DEFAULT '',
                wrapper TEXT NOT NULL DEFAULT '',
                tool TEXT NOT NULL DEFAULT 'shell',
                status TEXT NOT NULL DEFAULT 'idle',
                tmux_session TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                last_accessed INTEGER NOT NULL DEFAULT 0,
                parent_session_id TEXT NOT NULL DEFAULT '',
                worktree_path TEXT NOT NULL DEFAULT '',
                worktree_repo TEXT NOT NULL DEFAULT '',
                worktree_branch TEXT NOT NULL DEFAULT '',
                tool_data TEXT NOT NULL DEFAULT '{}',
                acknowledged INTEGER NOT NULL DEFAULT 0
            )",
        )?;

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS groups (
                path TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                expanded INTEGER NOT NULL DEFAULT 1,
                sort_order INTEGER NOT NULL DEFAULT 0,
                default_path TEXT NOT NULL DEFAULT ''
            )",
        )?;

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS heartbeats (
                pid INTEGER PRIMARY KEY,
                started INTEGER NOT NULL,
                heartbeat INTEGER NOT NULL,
                is_primary INTEGER NOT NULL DEFAULT 0
            )",
        )?;

        let current_version: Option<i32> = self
            .conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| {
                    let val: String = row.get(0)?;
                    Ok(val.parse::<i32>().unwrap_or(0))
                },
            )
            .ok();

        let version = current_version.unwrap_or(0);

        // v1 -> v2
        if version < 2 {
            let columns = [
                "ALTER TABLE sessions ADD COLUMN notify INTEGER NOT NULL DEFAULT 0",
                "ALTER TABLE sessions ADD COLUMN status_changed_at INTEGER NOT NULL DEFAULT 0",
                "ALTER TABLE sessions ADD COLUMN restart_count INTEGER NOT NULL DEFAULT 0",
                "ALTER TABLE sessions ADD COLUMN status_history TEXT NOT NULL DEFAULT '[]'",
            ];
            for sql in &columns {
                let _ = self.conn.execute(sql, []);
            }
        }

        // v2 -> v3
        if version < 3 {
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN follow_up INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        // v3 -> v4
        if version < 4 {
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
                [],
            );
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN tokens_used INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        // v4 -> v5
        if version < 5 {
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN last_started_at INTEGER NOT NULL DEFAULT 0",
                [],
            );
            // Backfill: set last_started_at = created_at for existing sessions
            let _ = self.conn.execute(
                "UPDATE sessions SET last_started_at = created_at WHERE last_started_at = 0",
                [],
            );
        }

        // v5 -> v6
        if version < 6 {
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN notes TEXT NOT NULL DEFAULT '[]'",
                [],
            );
        }

        // v6 -> v7: Add routines and routine_runs tables
        if version < 7 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS routines (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    group_path TEXT NOT NULL DEFAULT 'my-routines',
                    sort_order INTEGER NOT NULL DEFAULT 0,
                    working_dir TEXT NOT NULL,
                    default_tool TEXT NOT NULL DEFAULT 'claude',
                    schedule TEXT NOT NULL,
                    steps TEXT NOT NULL DEFAULT '[]',
                    enabled INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    last_run_at INTEGER,
                    next_run_at INTEGER,
                    run_count INTEGER NOT NULL DEFAULT 0,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    notify INTEGER NOT NULL DEFAULT 1,
                    step_timeout_secs INTEGER NOT NULL DEFAULT 1800
                )",
            )?;

            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS routine_runs (
                    id TEXT PRIMARY KEY,
                    routine_id TEXT NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
                    started_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    status TEXT NOT NULL DEFAULT 'running',
                    steps_completed INTEGER NOT NULL DEFAULT 0,
                    steps_total INTEGER NOT NULL,
                    log_path TEXT,
                    tmux_session TEXT,
                    tool_data TEXT NOT NULL DEFAULT '{}',
                    promoted_session_id TEXT
                )",
            )?;

            self.conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_routine_runs_routine_id ON routine_runs(routine_id)",
            )?;
        }

        // Set schema version
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;

        Ok(())
    }

    /// Save a session (insert or replace)
    pub fn save_session(&self, session: &crate::types::Session) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (
                id, title, project_path, group_path, sort_order,
                command, wrapper, tool, status, tmux_session,
                created_at, last_accessed,
                parent_session_id, worktree_path, worktree_repo, worktree_branch,
                tool_data, acknowledged,
                notify, follow_up, status_changed_at, restart_count, status_history,
                pinned, tokens_used, last_started_at, notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
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
    pub fn load_sessions(&self) -> SqlResult<Vec<crate::types::Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, project_path, group_path, sort_order,
                    command, wrapper, tool, status, tmux_session,
                    created_at, last_accessed,
                    parent_session_id, worktree_path, worktree_repo, worktree_branch,
                    tool_data, acknowledged,
                    notify, follow_up, status_changed_at, restart_count, status_history,
                    pinned, tokens_used, last_started_at, notes
             FROM sessions ORDER BY sort_order",
        )?;

        let rows = stmt.query_map([], |row| {
            let tool_str: String = row.get(7)?;
            let status_str: String = row.get(8)?;
            let history_json: String = row.get(22)?;
            let status_changed_at: i64 = row.get(20)?;
            let created_at: i64 = row.get(10)?;

            Ok(crate::types::Session {
                id: row.get(0)?,
                title: row.get(1)?,
                project_path: row.get(2)?,
                group_path: row.get(3)?,
                order: row.get(4)?,
                command: row.get(5)?,
                wrapper: row.get(6)?,
                tool: crate::types::Tool::from_str(&tool_str),
                status: crate::types::SessionStatus::from_str(&status_str),
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
                status_changed_at: if status_changed_at > 0 {
                    status_changed_at
                } else {
                    created_at
                },
                restart_count: row.get(21)?,
                last_started_at: {
                    let v: i64 = row.get(25).unwrap_or(0);
                    if v > 0 {
                        v
                    } else {
                        created_at
                    }
                },
                notes: {
                    let json: String = row.get(26).unwrap_or_else(|_| "[]".to_string());
                    serde_json::from_str(&json).unwrap_or_default()
                },
                status_history: serde_json::from_str(&history_json).unwrap_or_default(),
                pinned: row.get::<_, i32>(23)? == 1,
                tokens_used: row.get(24)?,
            })
        })?;

        rows.collect()
    }

    /// Get a single session by ID
    pub fn get_session(&self, id: &str) -> SqlResult<Option<crate::types::Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, project_path, group_path, sort_order,
                    command, wrapper, tool, status, tmux_session,
                    created_at, last_accessed,
                    parent_session_id, worktree_path, worktree_repo, worktree_branch,
                    tool_data, acknowledged,
                    notify, follow_up, status_changed_at, restart_count, status_history,
                    pinned, tokens_used, last_started_at, notes
             FROM sessions WHERE id = ?1",
        )?;

        let result = stmt.query_row(params![id], |row| {
            let tool_str: String = row.get(7)?;
            let status_str: String = row.get(8)?;
            let history_json: String = row.get(22)?;
            let status_changed_at: i64 = row.get(20)?;
            let created_at: i64 = row.get(10)?;

            Ok(crate::types::Session {
                id: row.get(0)?,
                title: row.get(1)?,
                project_path: row.get(2)?,
                group_path: row.get(3)?,
                order: row.get(4)?,
                command: row.get(5)?,
                wrapper: row.get(6)?,
                tool: crate::types::Tool::from_str(&tool_str),
                status: crate::types::SessionStatus::from_str(&status_str),
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
                status_changed_at: if status_changed_at > 0 {
                    status_changed_at
                } else {
                    created_at
                },
                restart_count: row.get(21)?,
                last_started_at: {
                    let v: i64 = row.get(25).unwrap_or(0);
                    if v > 0 {
                        v
                    } else {
                        created_at
                    }
                },
                notes: {
                    let json: String = row.get(26).unwrap_or_else(|_| "[]".to_string());
                    serde_json::from_str(&json).unwrap_or_default()
                },
                status_history: serde_json::from_str(&history_json).unwrap_or_default(),
                pinned: row.get::<_, i32>(23)? == 1,
                tokens_used: row.get(24)?,
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
    pub fn write_status(
        &self,
        id: &str,
        status: crate::types::SessionStatus,
        tool: crate::types::Tool,
    ) -> SqlResult<()> {
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

    /// Set the pinned flag
    pub fn set_pinned(&self, id: &str, pinned: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET pinned = ?1 WHERE id = ?2",
            params![pinned as i32, id],
        )?;
        Ok(())
    }

    /// Add tokens to a session's token count
    pub fn add_tokens(&self, id: &str, tokens: i64) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET tokens_used = tokens_used + ?1 WHERE id = ?2",
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

    /// Set the acknowledged flag
    #[allow(dead_code)]
    pub fn set_acknowledged(&self, id: &str, ack: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE sessions SET acknowledged = ?1 WHERE id = ?2",
            params![ack as i32, id],
        )?;
        Ok(())
    }

    /// Append a status entry to status_history (capped at 50 entries)
    pub fn update_status_history(
        &self,
        id: &str,
        status: crate::types::SessionStatus,
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

        let mut history: Vec<crate::types::StatusHistoryEntry> =
            serde_json::from_str(&history_json).unwrap_or_default();

        history.push(crate::types::StatusHistoryEntry {
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

    /// Load all groups ordered by sort_order
    pub fn load_groups(&self) -> SqlResult<Vec<crate::types::Group>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, name, expanded, sort_order, default_path
             FROM groups ORDER BY sort_order",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(crate::types::Group {
                path: row.get(0)?,
                name: row.get(1)?,
                expanded: row.get::<_, i32>(2)? == 1,
                order: row.get(3)?,
                default_path: row.get(4)?,
            })
        })?;

        rows.collect()
    }

    /// Save a group (insert or replace)
    pub fn save_group(&self, group: &crate::types::Group) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO groups (path, name, expanded, sort_order, default_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                group.path,
                group.name,
                group.expanded as i32,
                group.order,
                group.default_path,
            ],
        )?;
        Ok(())
    }

    /// Delete a group by path
    #[allow(dead_code)]
    pub fn delete_group(&self, path: &str) -> SqlResult<()> {
        self.conn
            .execute("DELETE FROM groups WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Swap the sort_order of two groups by path
    pub fn swap_group_order(&self, path_a: &str, path_b: &str) -> SqlResult<()> {
        let order_a: i32 = self.conn.query_row(
            "SELECT sort_order FROM groups WHERE path = ?1",
            params![path_a],
            |row| row.get(0),
        )?;
        let order_b: i32 = self.conn.query_row(
            "SELECT sort_order FROM groups WHERE path = ?1",
            params![path_b],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "UPDATE groups SET sort_order = ?1 WHERE path = ?2",
            params![order_b, path_a],
        )?;
        self.conn.execute(
            "UPDATE groups SET sort_order = ?1 WHERE path = ?2",
            params![order_a, path_b],
        )?;
        Ok(())
    }

    /// Toggle the expanded state of a group
    pub fn toggle_group_expanded(&self, path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE groups SET expanded = CASE WHEN expanded = 1 THEN 0 ELSE 1 END WHERE path = ?1",
            params![path],
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

    // --- Routine methods ---

    #[allow(dead_code)]
    pub fn save_routine(&self, routine: &crate::types::Routine) -> SqlResult<()> {
        let steps_json = serde_json::to_string(&routine.steps).unwrap_or_else(|_| "[]".to_string());
        self.conn.execute(
            "INSERT OR REPLACE INTO routines (
                id, name, group_path, sort_order, working_dir, default_tool,
                schedule, steps, enabled, created_at, last_run_at, next_run_at,
                run_count, pinned, notify, step_timeout_secs
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                routine.id, routine.name, routine.group_path, routine.sort_order,
                routine.working_dir, routine.default_tool, routine.schedule, steps_json,
                routine.enabled as i32, routine.created_at, routine.last_run_at,
                routine.next_run_at, routine.run_count, routine.pinned as i32,
                routine.notify as i32, routine.step_timeout_secs,
            ],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn load_routines(&self) -> SqlResult<Vec<crate::types::Routine>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, group_path, sort_order, working_dir, default_tool,
                    schedule, steps, enabled, created_at, last_run_at, next_run_at,
                    run_count, pinned, notify, step_timeout_secs
             FROM routines ORDER BY sort_order",
        )?;
        let rows = stmt.query_map([], |row| {
            let steps_json: String = row.get(7)?;
            Ok(crate::types::Routine {
                id: row.get(0)?,
                name: row.get(1)?,
                group_path: row.get(2)?,
                sort_order: row.get(3)?,
                working_dir: row.get(4)?,
                default_tool: row.get(5)?,
                schedule: row.get(6)?,
                steps: serde_json::from_str(&steps_json).unwrap_or_default(),
                enabled: row.get::<_, i32>(8)? == 1,
                created_at: row.get(9)?,
                last_run_at: row.get(10)?,
                next_run_at: row.get(11)?,
                run_count: row.get(12)?,
                pinned: row.get::<_, i32>(13)? == 1,
                notify: row.get::<_, i32>(14)? == 1,
                step_timeout_secs: row.get(15)?,
                expanded: false,
            })
        })?;
        rows.collect()
    }

    #[allow(dead_code)]
    pub fn get_routine(&self, id: &str) -> SqlResult<Option<crate::types::Routine>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, group_path, sort_order, working_dir, default_tool,
                    schedule, steps, enabled, created_at, last_run_at, next_run_at,
                    run_count, pinned, notify, step_timeout_secs
             FROM routines WHERE id = ?1",
        )?;
        let result = stmt.query_row(params![id], |row| {
            let steps_json: String = row.get(7)?;
            Ok(crate::types::Routine {
                id: row.get(0)?,
                name: row.get(1)?,
                group_path: row.get(2)?,
                sort_order: row.get(3)?,
                working_dir: row.get(4)?,
                default_tool: row.get(5)?,
                schedule: row.get(6)?,
                steps: serde_json::from_str(&steps_json).unwrap_or_default(),
                enabled: row.get::<_, i32>(8)? == 1,
                created_at: row.get(9)?,
                last_run_at: row.get(10)?,
                next_run_at: row.get(11)?,
                run_count: row.get(12)?,
                pinned: row.get::<_, i32>(13)? == 1,
                notify: row.get::<_, i32>(14)? == 1,
                step_timeout_secs: row.get(15)?,
                expanded: false,
            })
        });
        match result {
            Ok(routine) => Ok(Some(routine)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    #[allow(dead_code)]
    pub fn delete_routine(&self, id: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM routines WHERE id = ?1", params![id])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn set_routine_enabled(&self, id: &str, enabled: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET enabled = ?1 WHERE id = ?2",
            params![enabled as i32, id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn set_routine_pinned(&self, id: &str, pinned: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET pinned = ?1 WHERE id = ?2",
            params![pinned as i32, id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn rename_routine(&self, id: &str, new_name: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn move_routine_to_group(&self, id: &str, group_path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET group_path = ?1 WHERE id = ?2",
            params![group_path, id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn record_routine_execution(
        &self,
        id: &str,
        last_run_at: i64,
        next_run_at: Option<i64>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET last_run_at = ?1, next_run_at = ?2, run_count = run_count + 1 WHERE id = ?3",
            params![last_run_at, next_run_at, id],
        )?;
        Ok(())
    }

    // --- Routine run methods ---

    #[allow(dead_code)]
    pub fn save_routine_run(&self, run: &crate::types::RoutineRun) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO routine_runs (
                id, routine_id, started_at, finished_at, status,
                steps_completed, steps_total, log_path, tmux_session,
                tool_data, promoted_session_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                run.id, run.routine_id, run.started_at, run.finished_at,
                run.status.as_str(), run.steps_completed, run.steps_total,
                run.log_path, run.tmux_session, run.tool_data, run.promoted_session_id,
            ],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn load_routine_runs(&self, routine_id: &str) -> SqlResult<Vec<crate::types::RoutineRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, routine_id, started_at, finished_at, status,
                    steps_completed, steps_total, log_path, tmux_session,
                    tool_data, promoted_session_id
             FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![routine_id], |row| {
            let status_str: String = row.get(4)?;
            Ok(crate::types::RoutineRun {
                id: row.get(0)?,
                routine_id: row.get(1)?,
                started_at: row.get(2)?,
                finished_at: row.get(3)?,
                status: crate::types::RunStatus::from_str(&status_str),
                steps_completed: row.get(5)?,
                steps_total: row.get(6)?,
                log_path: row.get(7)?,
                tmux_session: row.get(8)?,
                tool_data: row.get(9)?,
                promoted_session_id: row.get(10)?,
            })
        })?;
        rows.collect()
    }

    #[allow(dead_code)]
    pub fn update_routine_run_status(
        &self,
        run_id: &str,
        status: crate::types::RunStatus,
        finished_at: Option<i64>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
            params![status.as_str(), finished_at, run_id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn increment_run_steps_completed(&self, run_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET steps_completed = steps_completed + 1 WHERE id = ?1",
            params![run_id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn has_active_run(&self, routine_id: &str) -> SqlResult<bool> {
        let count: i32 = self.conn.query_row(
            "SELECT COUNT(*) FROM routine_runs WHERE routine_id = ?1 AND finished_at IS NULL",
            params![routine_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    #[allow(dead_code)]
    pub fn get_latest_run(
        &self,
        routine_id: &str,
    ) -> SqlResult<Option<crate::types::RoutineRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, routine_id, started_at, finished_at, status,
                    steps_completed, steps_total, log_path, tmux_session,
                    tool_data, promoted_session_id
             FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC LIMIT 1",
        )?;
        let result = stmt.query_row(params![routine_id], |row| {
            let status_str: String = row.get(4)?;
            Ok(crate::types::RoutineRun {
                id: row.get(0)?,
                routine_id: row.get(1)?,
                started_at: row.get(2)?,
                finished_at: row.get(3)?,
                status: crate::types::RunStatus::from_str(&status_str),
                steps_completed: row.get(5)?,
                steps_total: row.get(6)?,
                log_path: row.get(7)?,
                tmux_session: row.get(8)?,
                tool_data: row.get(9)?,
                promoted_session_id: row.get(10)?,
            })
        });
        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    #[allow(dead_code)]
    pub fn delete_routine_run(&self, run_id: &str) -> SqlResult<()> {
        self.conn.execute("DELETE FROM routine_runs WHERE id = ?1", params![run_id])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn set_run_promoted(&self, run_id: &str, session_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET promoted_session_id = ?1 WHERE id = ?2",
            params![session_id, run_id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn update_run_tool_data(&self, run_id: &str, tool_data: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET tool_data = ?1 WHERE id = ?2",
            params![tool_data, run_id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn close(self) -> SqlResult<()> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn set_meta(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> SqlResult<Option<String>> {
        let result = self.conn.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );
        match result {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn touch(&self) -> SqlResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        self.set_meta("last_modified", &now.to_string())
    }

    /// Read the last_modified timestamp from metadata.
    /// Returns 0 if not set.
    pub fn last_modified(&self) -> i64 {
        self.get_meta("last_modified")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_storage() -> (Storage, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let storage = Storage::open(db_path.to_str().unwrap()).unwrap();
        storage.migrate().unwrap();
        (storage, dir)
    }

    #[test]
    fn test_migrate_creates_tables() {
        let (storage, _dir) = test_storage();
        let count: i32 = storage
            .conn()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i32 = storage
            .conn()
            .query_row("SELECT COUNT(*) FROM groups", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_migrate_sets_schema_version() {
        let (storage, _dir) = test_storage();
        let version = storage.get_meta("schema_version").unwrap();
        assert_eq!(version, Some("7".to_string()));
    }

    #[test]
    fn test_migrate_is_idempotent() {
        let (storage, _dir) = test_storage();
        storage.migrate().unwrap();
        let version = storage.get_meta("schema_version").unwrap();
        assert_eq!(version, Some("7".to_string()));
    }

    #[test]
    fn test_metadata_crud() {
        let (storage, _dir) = test_storage();
        storage.set_meta("test_key", "test_value").unwrap();
        let val = storage.get_meta("test_key").unwrap();
        assert_eq!(val, Some("test_value".to_string()));

        let missing = storage.get_meta("nonexistent").unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn test_v2_columns_exist() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO sessions (id, title, project_path, created_at, notify, status_changed_at, restart_count, status_history)
                 VALUES ('test', 'Test', '/tmp', 0, 1, 12345, 3, '[]')",
                [],
            )
            .unwrap();

        let (notify, status_changed_at, restart_count): (i32, i64, i32) = storage
            .conn()
            .query_row(
                "SELECT notify, status_changed_at, restart_count FROM sessions WHERE id = 'test'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(notify, 1);
        assert_eq!(status_changed_at, 12345);
        assert_eq!(restart_count, 3);
    }

    fn make_test_session(id: &str) -> crate::types::Session {
        crate::types::Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp/test".to_string(),
            group_path: "my-sessions".to_string(),
            order: 0,
            command: "claude".to_string(),
            wrapper: String::new(),
            tool: crate::types::Tool::Claude,
            status: crate::types::SessionStatus::Idle,
            tmux_session: format!("agentorch_{}", id),
            created_at: 1700000000000,
            last_accessed: 1700000000000,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: 1700000000000,
            restart_count: 0,
            last_started_at: 1700000000000,
            notes: vec![],
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        }
    }

    #[test]
    fn test_save_and_load_session() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        let loaded = storage.load_sessions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "s1");
        assert_eq!(loaded[0].title, "Session s1");
        assert_eq!(loaded[0].tool, crate::types::Tool::Claude);
        assert_eq!(loaded[0].status, crate::types::SessionStatus::Idle);
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
            .write_status(
                "s1",
                crate::types::SessionStatus::Running,
                crate::types::Tool::Claude,
            )
            .unwrap();

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.status, crate::types::SessionStatus::Running);
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
    fn test_update_status_history() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage
            .update_status_history("s1", crate::types::SessionStatus::Running, 1700000001000)
            .unwrap();
        storage
            .update_status_history("s1", crate::types::SessionStatus::Waiting, 1700000002000)
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
                .update_status_history(
                    "s1",
                    crate::types::SessionStatus::Running,
                    1700000000000 + i,
                )
                .unwrap();
        }

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.status_history.len(), 50);
    }

    #[test]
    fn test_v3_follow_up_column_exists() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO sessions (id, title, project_path, created_at, follow_up)
                 VALUES ('test', 'Test', '/tmp', 0, 1)",
                [],
            )
            .unwrap();

        let follow_up: i32 = storage
            .conn()
            .query_row(
                "SELECT follow_up FROM sessions WHERE id = 'test'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(follow_up, 1);
    }

    #[test]
    fn test_v4_columns_exist() {
        let (storage, _dir) = test_storage();
        let mut session = make_test_session("s1");
        session.pinned = true;
        session.tokens_used = 5000;
        storage.save_session(&session).unwrap();

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert!(loaded.pinned);
        assert_eq!(loaded.tokens_used, 5000);
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
    fn test_add_tokens() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();

        storage.add_tokens("s1", 1000).unwrap();
        storage.add_tokens("s1", 2500).unwrap();

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.tokens_used, 3500);
    }

    #[test]
    fn test_save_and_load_groups() {
        let (storage, _dir) = test_storage();
        let group = crate::types::Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&group).unwrap();

        let groups = storage.load_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Work");
        assert!(groups[0].expanded);
    }

    #[test]
    fn test_delete_group() {
        let (storage, _dir) = test_storage();
        let group = crate::types::Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&group).unwrap();
        storage.delete_group("work").unwrap();
        let groups = storage.load_groups().unwrap();
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_swap_group_order() {
        let (storage, _dir) = test_storage();
        let g1 = crate::types::Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 0,
            default_path: String::new(),
        };
        let g2 = crate::types::Group {
            path: "personal".to_string(),
            name: "Personal".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&g1).unwrap();
        storage.save_group(&g2).unwrap();

        storage.swap_group_order("work", "personal").unwrap();

        let groups = storage.load_groups().unwrap();
        assert_eq!(groups[0].path, "personal");
        assert_eq!(groups[1].path, "work");
    }

    #[test]
    fn test_toggle_group_expanded() {
        let (storage, _dir) = test_storage();
        let group = crate::types::Group {
            path: "work".to_string(),
            name: "Work".to_string(),
            expanded: true,
            order: 1,
            default_path: String::new(),
        };
        storage.save_group(&group).unwrap();
        storage.toggle_group_expanded("work").unwrap();
        let groups = storage.load_groups().unwrap();
        assert!(!groups[0].expanded);
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

    #[test]
    fn test_v7_routines_table_exists() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO routines (id, name, working_dir, schedule, steps, created_at)
                 VALUES ('r1', 'Test', '/tmp', '0 9 * * *', '[]', 0)",
                [],
            )
            .unwrap();

        let name: String = storage
            .conn()
            .query_row(
                "SELECT name FROM routines WHERE id = 'r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Test");
    }

    #[test]
    fn test_v7_routine_runs_table_exists() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO routines (id, name, working_dir, schedule, steps, created_at)
                 VALUES ('r1', 'Test', '/tmp', '0 9 * * *', '[]', 0)",
                [],
            )
            .unwrap();
        storage
            .conn()
            .execute(
                "INSERT INTO routine_runs (id, routine_id, started_at, status, steps_total)
                 VALUES ('run1', 'r1', 0, 'running', 2)",
                [],
            )
            .unwrap();

        let status: String = storage
            .conn()
            .query_row(
                "SELECT status FROM routine_runs WHERE id = 'run1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "running");
    }

    #[test]
    fn test_v7_schema_version() {
        let (storage, _dir) = test_storage();
        let version = storage.get_meta("schema_version").unwrap();
        assert_eq!(version, Some("7".to_string()));
    }

    fn make_test_routine(id: &str) -> crate::types::Routine {
        crate::types::Routine {
            id: id.to_string(),
            name: format!("Routine {}", id),
            group_path: "my-routines".to_string(),
            sort_order: 0,
            working_dir: "/tmp/test".to_string(),
            default_tool: "claude".to_string(),
            schedule: "0 9 * * *".to_string(),
            steps: vec![crate::types::RoutineStep::Claude {
                prompt: "Do something".to_string(),
            }],
            enabled: false,
            created_at: 1700000000000,
            last_run_at: None,
            next_run_at: None,
            run_count: 0,
            pinned: false,
            notify: true,
            step_timeout_secs: 1800,
            expanded: false,
        }
    }

    #[test]
    fn test_save_and_load_routine() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let loaded = storage.load_routines().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "r1");
        assert_eq!(loaded[0].name, "Routine r1");
        assert_eq!(loaded[0].schedule, "0 9 * * *");
        assert_eq!(loaded[0].steps.len(), 1);
    }

    #[test]
    fn test_get_routine_by_id() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let found = storage.get_routine("r1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Routine r1");

        let missing = storage.get_routine("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_routine() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();
        storage.delete_routine("r1").unwrap();

        let loaded = storage.load_routines().unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_delete_routine_cascades_runs() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: crate::types::RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        storage.delete_routine("r1").unwrap();
        let runs = storage.load_routine_runs("r1").unwrap();
        assert_eq!(runs.len(), 0);
    }

    #[test]
    fn test_save_and_load_routine_run() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: crate::types::RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: Some("/tmp/log".to_string()),
            tmux_session: Some("agentorch_routine_test".to_string()),
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        let runs = storage.load_routine_runs("r1").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run1");
        assert_eq!(runs[0].status, crate::types::RunStatus::Completed);
        assert_eq!(runs[0].log_path, Some("/tmp/log".to_string()));
    }

    #[test]
    fn test_update_routine_run_status() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: None,
            status: crate::types::RunStatus::Running,
            steps_completed: 0,
            steps_total: 2,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        storage
            .update_routine_run_status("run1", crate::types::RunStatus::Failed, Some(1700000002000))
            .unwrap();

        let runs = storage.load_routine_runs("r1").unwrap();
        assert_eq!(runs[0].status, crate::types::RunStatus::Failed);
        assert_eq!(runs[0].finished_at, Some(1700000002000));
    }

    #[test]
    fn test_routine_set_enabled() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        storage.set_routine_enabled("r1", true).unwrap();
        let loaded = storage.get_routine("r1").unwrap().unwrap();
        assert!(loaded.enabled);

        storage.set_routine_enabled("r1", false).unwrap();
        let loaded = storage.get_routine("r1").unwrap().unwrap();
        assert!(!loaded.enabled);
    }

    #[test]
    fn test_routine_set_pinned() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        storage.set_routine_pinned("r1", true).unwrap();
        let loaded = storage.get_routine("r1").unwrap().unwrap();
        assert!(loaded.pinned);
    }

    #[test]
    fn test_has_active_run() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        assert!(!storage.has_active_run("r1").unwrap());

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: None,
            status: crate::types::RunStatus::Running,
            steps_completed: 0,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        assert!(storage.has_active_run("r1").unwrap());
    }

    #[test]
    fn test_get_latest_run() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        assert!(storage.get_latest_run("r1").unwrap().is_none());

        let run1 = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: crate::types::RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        let run2 = crate::types::RoutineRun {
            id: "run2".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000002000,
            finished_at: Some(1700000003000),
            status: crate::types::RunStatus::Failed,
            steps_completed: 0,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run1).unwrap();
        storage.save_routine_run(&run2).unwrap();

        let latest = storage.get_latest_run("r1").unwrap().unwrap();
        assert_eq!(latest.id, "run2");
    }
}
