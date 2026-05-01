//! SQLite storage for session/group persistence
//! Compatible with the TypeScript version's schema (v3)

use rusqlite::{Connection, Result as SqlResult};
use std::fs;
use std::path::PathBuf;

mod groups;
mod meta;
mod routines;
mod runs;
mod schema;
mod sessions;

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

    #[allow(dead_code)]
    pub fn close(self) -> SqlResult<()> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn conn(&self) -> &Connection {
        &self.conn
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
pub(crate) mod test_helpers {
    use super::Storage;
    use tempfile::TempDir;

    pub fn test_storage() -> (Storage, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let storage = Storage::open(db_path.to_str().unwrap()).unwrap();
        storage.migrate().unwrap();
        (storage, dir)
    }

    pub fn make_test_session(id: &str) -> crate::types::Session {
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

    pub fn make_test_routine(id: &str) -> crate::types::Routine {
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
}
