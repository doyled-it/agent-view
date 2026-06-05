use rusqlite::params;
use rusqlite::Result as SqlResult;

use super::Storage;
use crate::core::cost::Pricer;

const SCHEMA_VERSION: i32 = 11;

impl Storage {
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

        // v7 -> v8: cost_events table for hook-driven token/cost tracking
        if version < 8 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS cost_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    model TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL DEFAULT 0,
                    output_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                    ts INTEGER NOT NULL,
                    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
                )",
            )?;
            self.conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_cost_events_session ON cost_events(session_id, ts)",
            )?;
        }

        // v8 -> v9: add cost_microdollars column to cost_events and backfill
        // existing rows using the built-in default rate table. The watcher
        // re-runs this with the user's config-aware Pricer at startup so any
        // active overrides take effect on historical rows.
        if version < 9 {
            let _ = self.conn.execute(
                "ALTER TABLE cost_events ADD COLUMN cost_microdollars INTEGER NOT NULL DEFAULT 0",
                [],
            );
            self.recompute_cost_microdollars(&Pricer::with_defaults())?;
        }

        // v9 -> v10: user marker for sessions the operator is waiting on.
        if version < 10 {
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN user_waiting INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        // v10 -> v11: resolved MCP tool/server selection persisted per session.
        if version < 11 {
            let _ = self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN mcp_selection TEXT NOT NULL DEFAULT '{}'",
                [],
            );
        }

        // Set schema version
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;

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
        assert_eq!(version, Some("11".to_string()));
    }

    #[test]
    fn test_migrate_is_idempotent() {
        let (storage, _dir) = test_storage();
        storage.migrate().unwrap();
        let version = storage.get_meta("schema_version").unwrap();
        assert_eq!(version, Some("11".to_string()));
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
            .query_row("SELECT name FROM routines WHERE id = 'r1'", [], |row| {
                row.get(0)
            })
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
    fn test_current_schema_version() {
        let (storage, _dir) = test_storage();
        let version = storage.get_meta("schema_version").unwrap();
        assert_eq!(version, Some("11".to_string()));
    }

    #[test]
    fn test_v9_cost_microdollars_column_exists() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO sessions (id, title, project_path, created_at)
                 VALUES ('s1', 't', '/tmp', 0)",
                [],
            )
            .unwrap();
        // Inserting with explicit cost_microdollars value should succeed,
        // proving the column is present after migration.
        storage
            .conn()
            .execute(
                "INSERT INTO cost_events
                    (session_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, ts, cost_microdollars)
                 VALUES ('s1', 'claude-opus-4-7', 1, 2, 0, 0, 0, 12345)",
                [],
            )
            .unwrap();
        let micros: i64 = storage
            .conn()
            .query_row(
                "SELECT cost_microdollars FROM cost_events WHERE session_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(micros, 12345);
    }

    #[test]
    fn test_v9_backfills_existing_rows() {
        // Simulate the pre-v9 state: a cost_events row that lacks
        // cost_microdollars. We do this by manually downgrading the schema
        // version after migrate() and re-running it, but the cleaner approach
        // is to insert with cost_microdollars=0 (the default) and confirm
        // a fresh `migrate()` call recomputes it.
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO sessions (id, title, project_path, created_at)
                 VALUES ('s1', 't', '/tmp', 0)",
                [],
            )
            .unwrap();
        storage
            .conn()
            .execute(
                "INSERT INTO cost_events
                    (session_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, ts, cost_microdollars)
                 VALUES ('s1', 'claude-opus-4-7', 1000000, 1000000, 0, 0, 0, 0)",
                [],
            )
            .unwrap();
        // Force a re-run of the backfill by rolling schema_version back and
        // re-migrating. The backfill is idempotent — calling it again
        // recomputes microdollars on the existing rows.
        storage
            .conn()
            .execute(
                "UPDATE metadata SET value = '8' WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
        storage.migrate().unwrap();
        let micros: i64 = storage
            .conn()
            .query_row(
                "SELECT cost_microdollars FROM cost_events WHERE session_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // 1M input @ $15 + 1M output @ $75 = $90 = 90_000_000 microdollars
        assert_eq!(micros, 90_000_000);
    }

    #[test]
    fn test_v10_user_waiting_column_exists() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO sessions (id, title, project_path, created_at, user_waiting)
                 VALUES ('test', 'Test', '/tmp', 0, 1)",
                [],
            )
            .unwrap();

        let user_waiting: i32 = storage
            .conn()
            .query_row(
                "SELECT user_waiting FROM sessions WHERE id = 'test'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(user_waiting, 1);
    }

    #[test]
    fn test_v11_mcp_selection_column_exists() {
        let (storage, _dir) = test_storage();
        storage
            .conn()
            .execute(
                "INSERT INTO sessions (id, title, project_path, created_at, mcp_selection)
                 VALUES ('test', 'Test', '/tmp', 0, '{\"profile_id\":\"dev\"}')",
                [],
            )
            .unwrap();

        let selection: String = storage
            .conn()
            .query_row(
                "SELECT mcp_selection FROM sessions WHERE id = 'test'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(selection, "{\"profile_id\":\"dev\"}");
    }
}
