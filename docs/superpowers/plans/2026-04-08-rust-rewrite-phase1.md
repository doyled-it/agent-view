# Rust Rewrite Phase 1: Core + Minimal TUI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a working Rust binary that can list sessions with live status, create new sessions (claude tool only), attach/detach with Ctrl+K/Ctrl+Q keybinds, stop and delete sessions, detect Claude Code statuses (running, waiting, paused, idle, error, stopped, compacting) with debouncing, and send desktop notifications on status changes.

**Architecture:** Single `App` struct holding all state, tick-based event loop (16ms input poll, 500ms status refresh), ratatui for rendering, crossterm for terminal control, rusqlite for SQLite, `std::process::Command` for tmux and notification subprocesses. The SQLite schema is identical to the existing TypeScript v3 schema so the same `~/.agent-orchestrator/state.db` works with both versions.

**Tech Stack:** Rust, ratatui, crossterm, rusqlite (bundled), tokio, regex, serde, serde_json, chrono, dirs, clap

---

### Task 1: Scaffold the Cargo project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/app.rs`
- Create: `src/event.rs`
- Create: `src/types.rs`
- Create: `src/core/mod.rs`
- Create: `src/ui/mod.rs`

- [ ] **Step 1: Initialize the Cargo project**

Run from the repo root (the Rust project lives at the repo root alongside the existing TypeScript code):

```bash
cargo init --name agent-view
```

- [ ] **Step 2: Set up Cargo.toml with all Phase 1 dependencies**

Replace `Cargo.toml`:

```toml
[package]
name = "agent-view"
version = "0.1.2"
edition = "2021"
description = "Terminal UI for managing AI coding agent sessions"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
rusqlite = { version = "0.32", features = ["bundled"] }
tokio = { version = "1", features = ["full"] }
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
dirs = "6"
clap = { version = "4", features = ["derive"] }
uuid = { version = "1", features = ["v4"] }
lazy_static = "1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Create the module structure**

Create `src/types.rs`:

```rust
//! Core types for Agent View

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Waiting,
    Paused,
    Compacting,
    Idle,
    Error,
    Stopped,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Compacting => "compacting",
            Self::Idle => "idle",
            Self::Error => "error",
            Self::Stopped => "stopped",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "paused" => Self::Paused,
            "compacting" => Self::Compacting,
            "idle" => Self::Idle,
            "error" => Self::Error,
            "stopped" => Self::Stopped,
            _ => Self::Idle,
        }
    }

    /// Icon character for display in session list
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Running => "◆",
            Self::Waiting => "⚑",
            Self::Paused => "⍾",
            Self::Compacting => "⟳",
            Self::Idle => "○",
            Self::Error => "✗",
            Self::Stopped => "■",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Opencode,
    Gemini,
    Codex,
    Custom,
    Shell,
}

impl Tool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Custom => "custom",
            Self::Shell => "shell",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "claude" => Self::Claude,
            "opencode" => Self::Opencode,
            "gemini" => Self::Gemini,
            "codex" => Self::Codex,
            "custom" => Self::Custom,
            "shell" => Self::Shell,
            _ => Self::Shell,
        }
    }

    pub fn command(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::Custom => "bash",
            Self::Shell => "bash",
        }
    }
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusHistoryEntry {
    pub status: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub project_path: String,
    pub group_path: String,
    pub order: i32,
    pub command: String,
    pub wrapper: String,
    pub tool: Tool,
    pub status: SessionStatus,
    pub tmux_session: String,
    pub created_at: i64,
    pub last_accessed: i64,
    pub parent_session_id: String,
    pub worktree_path: String,
    pub worktree_repo: String,
    pub worktree_branch: String,
    pub tool_data: String,
    pub acknowledged: bool,
    pub notify: bool,
    pub follow_up: bool,
    pub status_changed_at: i64,
    pub restart_count: i32,
    pub status_history: Vec<StatusHistoryEntry>,
}

impl Session {
    pub fn status_history_json(&self) -> String {
        serde_json::to_string(&self.status_history).unwrap_or_else(|_| "[]".to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub path: String,
    pub name: String,
    pub expanded: bool,
    pub order: i32,
    pub default_path: String,
}

/// Options for creating a new session
pub struct SessionCreateOptions {
    pub title: Option<String>,
    pub project_path: String,
    pub group_path: Option<String>,
    pub tool: Tool,
    pub command: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_status_roundtrip() {
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Compacting,
            SessionStatus::Idle,
            SessionStatus::Error,
            SessionStatus::Stopped,
        ];
        for s in statuses {
            assert_eq!(SessionStatus::from_str(s.as_str()), s);
        }
    }

    #[test]
    fn test_tool_roundtrip() {
        let tools = [
            Tool::Claude,
            Tool::Opencode,
            Tool::Gemini,
            Tool::Codex,
            Tool::Custom,
            Tool::Shell,
        ];
        for t in tools {
            assert_eq!(Tool::from_str(t.as_str()), t);
        }
    }

    #[test]
    fn test_session_status_unknown_defaults_to_idle() {
        assert_eq!(SessionStatus::from_str("unknown"), SessionStatus::Idle);
    }

    #[test]
    fn test_tool_unknown_defaults_to_shell() {
        assert_eq!(Tool::from_str("unknown"), Tool::Shell);
    }
}
```

Create `src/event.rs`:

```rust
//! Event types for the application event loop

use crossterm::event::KeyEvent;

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    StatusRefresh,
}
```

Create `src/app.rs`:

```rust
//! Application state and event dispatch

use crate::types::{Session, SessionStatus};

/// Which overlay is currently shown
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NewSession(NewSessionForm),
    Confirm(ConfirmDialog),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    pub title: String,
    pub project_path: String,
    /// Which field is focused: 0 = title, 1 = project_path
    pub focused_field: usize,
}

impl NewSessionForm {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            title: String::new(),
            project_path: home,
            focused_field: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialog {
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    DeleteSession(String),
    StopSession(String),
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected_index: usize,
    pub overlay: Overlay,
    pub should_quit: bool,
    pub last_status_refresh: std::time::Instant,
    pub attach_session: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            selected_index: 0,
            overlay: Overlay::None,
            should_quit: false,
            last_status_refresh: std::time::Instant::now(),
            attach_session: None,
        }
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions.get(self.selected_index)
    }

    pub fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        if !self.sessions.is_empty() && self.selected_index < self.sessions.len() - 1 {
            self.selected_index += 1;
        }
    }

    pub fn clamp_selection(&mut self) {
        if self.sessions.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.sessions.len() {
            self.selected_index = self.sessions.len() - 1;
        }
    }
}
```

Create `src/core/mod.rs`:

```rust
pub mod config;
pub mod notify;
pub mod session;
pub mod status;
pub mod storage;
pub mod tmux;
```

Create `src/ui/mod.rs`:

```rust
pub mod footer;
pub mod home;
pub mod overlay;
```

Create `src/main.rs`:

```rust
mod app;
mod core;
mod event;
mod types;
mod ui;

fn main() {
    println!("agent-view scaffold OK");
}
```

- [ ] **Step 4: Verify it compiles**

```bash
cargo build
```

- [ ] **Step 5: Run the type tests**

```bash
cargo test types::tests
```

- [ ] **Step 6: Commit**

```
feat: scaffold Rust project with types, app state, and module structure
```

---

### Task 2: SQLite storage — schema and migrations

**Files:**
- Create: `src/core/storage.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/core/storage.rs`:

```rust
//! SQLite storage for session/group persistence
//! Compatible with the TypeScript version's schema (v3)

use rusqlite::{params, Connection, Result as SqlResult};
use std::fs;
use std::path::PathBuf;

const SCHEMA_VERSION: i32 = 3;

pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open a database at the given path (creates parent dirs if needed)
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

    /// Open the default database at ~/.agent-orchestrator/state.db
    pub fn open_default() -> SqlResult<Self> {
        let home = dirs::home_dir().expect("Cannot determine home directory");
        let db_path = home.join(".agent-orchestrator").join("state.db");
        Self::open(db_path.to_str().unwrap())
    }

    /// Run all schema migrations (idempotent)
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

        // Check current schema version
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
                // Column may already exist — ignore the error
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

        // Set schema version
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;

        Ok(())
    }

    /// Close the database cleanly
    pub fn close(self) -> SqlResult<()> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        // Connection drops automatically
        Ok(())
    }

    /// Get a reference to the underlying connection (for CRUD methods)
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Set a metadata key-value pair
    pub fn set_meta(&self, key: &str, value: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Get a metadata value by key
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

    /// Touch the last_modified timestamp
    pub fn touch(&self) -> SqlResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        self.set_meta("last_modified", &now.to_string())
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
        // Verify tables exist by querying them
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
        assert_eq!(version, Some("3".to_string()));
    }

    #[test]
    fn test_migrate_is_idempotent() {
        let (storage, _dir) = test_storage();
        // Run migrate again — should not error
        storage.migrate().unwrap();
        let version = storage.get_meta("schema_version").unwrap();
        assert_eq!(version, Some("3".to_string()));
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
        // Insert a row and verify v2 columns (notify, status_changed_at, restart_count, status_history)
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
}
```

- [ ] **Step 2: Verify tests pass**

```bash
cargo test core::storage::tests
```

- [ ] **Step 3: Commit**

```
feat(storage): add SQLite schema and migrations compatible with TypeScript v3
```

---

### Task 3: Storage CRUD — session save, load, delete

**Files:**
- Modify: `src/core/storage.rs`

- [ ] **Step 1: Write failing tests for session CRUD**

Add these tests to the `mod tests` block in `src/core/storage.rs`:

```rust
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
            status_history: vec![],
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
            .write_status("s1", crate::types::SessionStatus::Running, crate::types::Tool::Claude)
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

        storage.update_status_history("s1", crate::types::SessionStatus::Running, 1700000001000).unwrap();
        storage.update_status_history("s1", crate::types::SessionStatus::Waiting, 1700000002000).unwrap();

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
                .update_status_history("s1", crate::types::SessionStatus::Running, 1700000000000 + i)
                .unwrap();
        }

        let loaded = storage.get_session("s1").unwrap().unwrap();
        assert_eq!(loaded.status_history.len(), 50);
    }
```

- [ ] **Step 2: Implement session CRUD methods**

Add these methods to the `impl Storage` block in `src/core/storage.rs`:

```rust
    /// Save a session (insert or replace)
    pub fn save_session(&self, session: &crate::types::Session) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (
                id, title, project_path, group_path, sort_order,
                command, wrapper, tool, status, tmux_session,
                created_at, last_accessed,
                parent_session_id, worktree_path, worktree_repo, worktree_branch,
                tool_data, acknowledged,
                notify, follow_up, status_changed_at, restart_count, status_history
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
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
                    notify, follow_up, status_changed_at, restart_count, status_history
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
                status_history: serde_json::from_str(&history_json).unwrap_or_default(),
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
                    notify, follow_up, status_changed_at, restart_count, status_history
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
                status_history: serde_json::from_str(&history_json).unwrap_or_default(),
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

    /// Set the acknowledged flag
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
```

- [ ] **Step 3: Verify all tests pass**

```bash
cargo test core::storage::tests
```

- [ ] **Step 4: Commit**

```
feat(storage): add session CRUD, status writes, and history tracking
```

---

### Task 4: Tmux wrapper — session lifecycle and capture

**Files:**
- Create: `src/core/tmux.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/core/tmux.rs`:

```rust
//! Tmux subprocess wrapper for session management

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

pub const SESSION_PREFIX: &str = "agentorch_";

/// Cache of tmux session activity timestamps
pub struct SessionCache {
    data: HashMap<String, i64>,
    last_refresh: Instant,
}

impl SessionCache {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            last_refresh: Instant::now(),
        }
    }

    /// Refresh cache by querying tmux for all windows
    pub fn refresh(&mut self) {
        let output = Command::new("tmux")
            .args(["list-windows", "-a", "-F", "#{session_name}\t#{window_activity}"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let mut new_data = HashMap::new();

                for line in stdout.trim().lines() {
                    if line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.splitn(2, '\t').collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let name = parts[0];
                    let activity: i64 = parts[1].parse().unwrap_or(0);
                    let existing = new_data.get(name).copied().unwrap_or(0);
                    if activity > existing {
                        new_data.insert(name.to_string(), activity);
                    }
                }

                self.data = new_data;
                self.last_refresh = Instant::now();
            }
            _ => {
                self.data.clear();
                self.last_refresh = Instant::now();
            }
        }
    }

    /// Check if a session exists in the cache
    pub fn session_exists(&self, name: &str) -> bool {
        self.data.contains_key(name)
    }

    /// Check if a session has recent activity
    pub fn is_session_active(&self, name: &str, threshold_seconds: i64) -> bool {
        if let Some(&activity) = self.data.get(name) {
            if activity == 0 {
                return false;
            }
            let now = chrono::Utc::now().timestamp();
            now - activity < threshold_seconds
        } else {
            false
        }
    }

    /// Register a newly created session in cache to prevent race conditions
    pub fn register(&mut self, name: &str) {
        let now = chrono::Utc::now().timestamp();
        self.data.insert(name.to_string(), now);
    }

    /// Remove a session from cache
    pub fn remove(&mut self, name: &str) {
        self.data.remove(name);
    }
}

/// Check if tmux is available on the system
pub fn is_tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a unique tmux session name from a title
pub fn generate_session_name(title: &str) -> String {
    let safe: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let safe = safe.trim_matches('-');
    let safe = if safe.len() > 20 { &safe[..20] } else { safe };

    let timestamp = chrono::Utc::now().timestamp_millis();
    let ts_base36 = radix_string(timestamp as u64, 36);
    format!("{}{}-{}", SESSION_PREFIX, safe, ts_base36)
}

/// Convert a u64 to a base-36 string (matches JS Date.now().toString(36))
fn radix_string(mut n: u64, radix: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let chars: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
    let mut result = Vec::new();
    while n > 0 {
        result.push(chars[(n % radix) as usize]);
        n /= radix;
    }
    result.reverse();
    result.into_iter().collect()
}

/// Create a new tmux session
pub fn create_session(
    name: &str,
    command: Option<&str>,
    cwd: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> Result<(), String> {
    let cwd = cwd.unwrap_or("/tmp");

    // Step 1: Create detached session
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-c", cwd])
        .status()
        .map_err(|e| format!("Failed to spawn tmux: {}", e))?;

    if !status.success() {
        return Err(format!("tmux new-session failed with status {}", status));
    }

    // Step 2: Set environment variables
    if let Some(env_vars) = env {
        for (key, value) in env_vars {
            let _ = Command::new("tmux")
                .args(["set-environment", "-t", name, key, value])
                .status();
        }
    }

    // Step 3: Send command via send-keys
    if let Some(cmd) = command {
        let cmd_to_send = if cmd.contains("$(") || cmd.contains("session_id=") {
            let escaped = cmd.replace('\'', "'\"'\"'");
            format!("bash -c '{}'", escaped)
        } else {
            cmd.to_string()
        };

        send_keys(name, &cmd_to_send)?;
    }

    Ok(())
}

/// Kill a tmux session
pub fn kill_session(name: &str) -> Result<(), String> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output();
    Ok(())
}

/// Send keys to a tmux session (followed by Enter)
pub fn send_keys(name: &str, keys: &str) -> Result<(), String> {
    let escaped = keys
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");

    let status = Command::new("tmux")
        .args(["send-keys", "-t", name, &escaped, "Enter"])
        .status()
        .map_err(|e| format!("Failed to send keys: {}", e))?;

    if !status.success() {
        return Err(format!("tmux send-keys failed with status {}", status));
    }
    Ok(())
}

/// Capture pane content from a tmux session
pub fn capture_pane(name: &str, start_line: Option<i32>) -> Result<String, String> {
    let mut args = vec!["capture-pane", "-t", name, "-p"];
    let start_str;

    if let Some(start) = start_line {
        start_str = start.to_string();
        args.push("-S");
        args.push(&start_str);
    }

    let output = Command::new("tmux")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to capture pane: {}", e))?;

    if !output.status.success() {
        return Err("capture-pane failed".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get sessions that currently have an attached client
pub fn get_attached_sessions() -> std::collections::HashSet<String> {
    let output = Command::new("tmux")
        .args(["list-clients", "-F", "#{client_session}"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.trim().lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect()
        }
        _ => std::collections::HashSet::new(),
    }
}

/// Attach to a tmux session synchronously (blocks until detach).
/// Sets up Ctrl+Q to detach, Ctrl+K for command palette signal, Ctrl+T for terminal split.
/// Returns true if command palette was requested.
pub fn attach_session_sync(session_name: &str) -> Result<bool, String> {
    use std::io::Write;

    let signal_file = get_signal_file_path();

    // Clear any existing signal
    let _ = std::fs::remove_file(&signal_file);

    // Clear screen, show cursor, stay in alternate buffer
    let _ = std::io::stdout().write_all(b"\x1b[2J\x1b[H\x1b[?25h");
    let _ = std::io::stdout().flush();

    // Cancel copy-mode (non-fatal)
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", session_name, "-X", "cancel"])
        .output();

    // Batch pre-attach setup
    let status_right = "#[fg=#89b4fa]Ctrl+K#[fg=#6c7086] cmd  #[fg=#89b4fa]Ctrl+T#[fg=#6c7086] terminal  #[fg=#89b4fa]Ctrl+Q#[fg=#6c7086] detach  #[fg=#89b4fa]Ctrl+C#[fg=#6c7086] cancel";

    let _ = Command::new("tmux")
        .args([
            "bind-key", "-n", "C-q", "detach-client", ";",
            "bind-key", "-n", "C-k", "run-shell",
            &format!("touch {} && tmux detach-client", signal_file),
            ";",
            "bind-key", "-n", "C-t", "split-window", "-v", "-c", "#{pane_current_path}", ";",
            "set-option", "-t", session_name, "status", "on", ";",
            "set-option", "-t", session_name, "status-position", "bottom", ";",
            "set-option", "-t", session_name, "status-style", "bg=#1e1e2e,fg=#cdd6f4", ";",
            "set-option", "-t", session_name, "status-left", "", ";",
            "set-option", "-t", session_name, "status-right-length", "120", ";",
            "set-option", "-t", session_name, "status-right", status_right,
        ])
        .output();

    // Attach — blocks until detach
    let result = Command::new("tmux")
        .args(["attach-session", "-t", session_name])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::piped())
        .status();

    // Unbind keys
    let _ = Command::new("tmux")
        .args([
            "unbind-key", "-n", "C-q", ";",
            "unbind-key", "-n", "C-k", ";",
            "unbind-key", "-n", "C-t",
        ])
        .output();

    // Clear screen for TUI return
    let _ = std::io::stdout().write_all(b"\x1b[2J\x1b[H");
    let _ = std::io::stdout().flush();

    match result {
        Ok(status) if !status.success() => {
            Err("tmux attach failed: this is usually caused by a tmux version mismatch. \
                 Run 'tmux kill-server' in a terminal to fix this.".to_string())
        }
        Err(e) => Err(format!("Failed to attach: {}", e)),
        Ok(_) => {
            // Check if command palette was requested
            let was_requested = std::fs::metadata(&signal_file).is_ok();
            let _ = std::fs::remove_file(&signal_file);
            Ok(was_requested)
        }
    }
}

/// Get the path to the signal file for command palette requests
fn get_signal_file_path() -> String {
    let uid = unsafe { libc::getuid() };
    format!("/tmp/agent-view-cmd-palette-{}", uid)
}

/// Strip ANSI escape sequences from terminal output
pub fn strip_ansi(text: &str) -> String {
    lazy_static::lazy_static! {
        static ref ANSI_RE: regex::Regex = regex::Regex::new(
            r"(\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[PX^_][^\x1b]*\x1b\\)"
        ).unwrap();
    }
    ANSI_RE.replace_all(text, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_name_format() {
        let name = generate_session_name("My Test Session");
        assert!(name.starts_with("agentorch_"));
        assert!(name.contains("my-test-session"));
    }

    #[test]
    fn test_generate_session_name_truncates_long_titles() {
        let name = generate_session_name("this is a very long title that should be truncated");
        // The safe part should be at most 20 chars
        let after_prefix = &name["agentorch_".len()..];
        let parts: Vec<&str> = after_prefix.rsplitn(2, '-').collect();
        // parts[1] is the safe title part, parts[0] is the timestamp
        assert!(parts.len() == 2);
        assert!(parts[1].len() <= 20);
    }

    #[test]
    fn test_generate_session_name_sanitizes_special_chars() {
        let name = generate_session_name("hello@world!#$%");
        assert!(name.starts_with("agentorch_"));
        // Should not contain special characters
        let after_prefix = &name["agentorch_".len()..];
        assert!(!after_prefix.contains('@'));
        assert!(!after_prefix.contains('!'));
    }

    #[test]
    fn test_strip_ansi_removes_color_codes() {
        let input = "\x1b[31mHello\x1b[0m World";
        assert_eq!(strip_ansi(input), "Hello World");
    }

    #[test]
    fn test_strip_ansi_removes_osc_sequences() {
        let input = "Hello\x1b]0;title\x07World";
        assert_eq!(strip_ansi(input), "HelloWorld");
    }

    #[test]
    fn test_strip_ansi_preserves_normal_text() {
        let input = "Hello World";
        assert_eq!(strip_ansi(input), "Hello World");
    }

    #[test]
    fn test_radix_string_base36() {
        assert_eq!(radix_string(0, 36), "0");
        assert_eq!(radix_string(35, 36), "z");
        assert_eq!(radix_string(36, 36), "10");
    }

    #[test]
    fn test_session_cache_register_and_exists() {
        let mut cache = SessionCache::new();
        assert!(!cache.session_exists("test"));
        cache.register("test");
        assert!(cache.session_exists("test"));
    }

    #[test]
    fn test_session_cache_remove() {
        let mut cache = SessionCache::new();
        cache.register("test");
        cache.remove("test");
        assert!(!cache.session_exists("test"));
    }
}
```

- [ ] **Step 2: Add `libc` dependency to Cargo.toml**

Add under `[dependencies]`:

```toml
libc = "0.2"
```

- [ ] **Step 3: Verify tests pass**

```bash
cargo test core::tmux::tests
```

- [ ] **Step 4: Commit**

```
feat(tmux): add tmux session management, capture, and attach with keybinds
```

---

### Task 5: Status detection — Claude Code pattern matching

**Files:**
- Create: `src/core/status.rs`

- [ ] **Step 1: Write failing tests with exact patterns from TypeScript**

Add to `src/core/status.rs`:

```rust
//! Claude Code status detection via regex pattern matching
//! Ports the exact patterns from the TypeScript tmux.ts

use lazy_static::lazy_static;
use regex::Regex;

/// Result of parsing tmux pane output for tool status
#[derive(Debug, Clone, Default)]
pub struct ToolStatus {
    pub is_active: bool,
    pub is_waiting: bool,
    pub is_compacting: bool,
    pub is_busy: bool,
    pub has_error: bool,
    pub has_exited: bool,
    pub has_idle_prompt: bool,
    pub has_question: bool,
}

/// Spinner characters used by Claude Code when processing
const SPINNER_CHARS: &[&str] = &[
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}",
    "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}", "\u{2733}", "\u{273d}",
    "\u{2736}", "\u{2722}",
];

lazy_static! {
    // Claude Code busy indicators — agent is actively working
    static ref CLAUDE_BUSY_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)ctrl\+c to interrupt").unwrap(),
        Regex::new(r"(?i)esc to interrupt").unwrap(),
        Regex::new(r"(?i)\u{2026}.*tokens").unwrap(),
    ];

    // Claude Code waiting indicators — needs user input
    static ref CLAUDE_WAITING_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)Do you want to proceed\?").unwrap(),
        Regex::new(r"(?i)\d\.\s*Yes\b").unwrap(),
        Regex::new(r"(?i)Esc to cancel.*Tab to amend").unwrap(),
        Regex::new(r"(?i)Enter to select.*to navigate").unwrap(),
        Regex::new(r"(?i)\(Y/n\)").unwrap(),
        Regex::new(r"(?i)Continue\?").unwrap(),
        Regex::new(r"(?i)Approve this plan\?").unwrap(),
        Regex::new(r"(?i)\[Y/n\]").unwrap(),
        Regex::new(r"(?i)\[y/N\]").unwrap(),
        Regex::new(r"(?i)Yes,? allow once").unwrap(),
        Regex::new(r"(?i)Allow always").unwrap(),
        Regex::new(r"(?i)No,? and tell Claude").unwrap(),
    ];

    // Claude exited patterns (shell returned)
    static ref CLAUDE_EXITED_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)Resume this session with:").unwrap(),
        Regex::new(r"(?i)claude --resume").unwrap(),
        Regex::new(r"(?i)Press Ctrl-C again to exit").unwrap(),
    ];

    // Claude compacting patterns
    static ref CLAUDE_COMPACTING_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)compacting conversation").unwrap(),
        Regex::new(r"(?i)summarizing conversation").unwrap(),
        Regex::new(r"(?i)context window.*(compact|compress)").unwrap(),
    ];

    // Error patterns
    static ref ERROR_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)error:").unwrap(),
        Regex::new(r"(?i)failed:").unwrap(),
        Regex::new(r"(?i)exception:").unwrap(),
        Regex::new(r"(?i)traceback").unwrap(),
        Regex::new(r"(?i)panic:").unwrap(),
    ];

    // Idle prompt pattern
    static ref IDLE_PROMPT_RE: Regex = Regex::new(r"(?m)^\u{276f}\s").unwrap();

    // Question detection
    static ref QUESTION_RE: Regex = Regex::new(r"\?\s*$").unwrap();

    // Non-content line patterns (for question scanning)
    static ref SEPARATOR_RE: Regex = Regex::new(r"^[\u{2500}\u{2501}\u{2550}]{10,}").unwrap();
    static ref COMPANION_RE: Regex = Regex::new(r"Thistle").unwrap();
    static ref ART_LINE_RE: Regex = Regex::new(r"^\.\-\-\.$|^\\|^\\_|^~+$").unwrap();
    static ref SPINNER_LINE_RE: Regex = Regex::new(
        r"^[\u{273b}\u{273d}\u{2736}\u{2722}\u{280b}\u{2819}\u{2839}\u{2838}\u{283c}\u{2834}\u{2826}\u{2827}\u{2807}\u{280f}\u{00b7}]"
    ).unwrap();
    static ref USER_INPUT_RE: Regex = Regex::new(r"^\u{276f}").unwrap();
    static ref SHORTCUTS_RE: Regex = Regex::new(r"^\u{23f5}\u{23f5}|^\? for shortcuts").unwrap();
}

/// Check if text contains spinner characters
fn has_spinner(text: &str) -> bool {
    SPINNER_CHARS.iter().any(|c| text.contains(c))
}

/// Parse tmux pane output to detect Claude Code tool status.
/// The `tool` argument should be "claude" for Claude-specific detection.
pub fn parse_tool_status(output: &str, tool: Option<&str>) -> ToolStatus {
    let cleaned = crate::core::tmux::strip_ansi(output);

    // Filter out trailing empty lines
    let all_lines: Vec<&str> = cleaned.split('\n').collect();
    let mut last_non_empty = all_lines.len();
    while last_non_empty > 0 && all_lines[last_non_empty - 1].trim().is_empty() {
        last_non_empty -= 1;
    }
    let trimmed_lines: Vec<&str> = all_lines[..last_non_empty].to_vec();
    let last_30_start = if trimmed_lines.len() > 30 {
        trimmed_lines.len() - 30
    } else {
        0
    };
    let last_lines = trimmed_lines[last_30_start..].join("\n");
    let last_10_start = if trimmed_lines.len() > 10 {
        trimmed_lines.len() - 10
    } else {
        0
    };
    let last_few_lines = trimmed_lines[last_10_start..].join("\n");

    let mut status = ToolStatus::default();

    if tool == Some("claude") {
        // Check if Claude has exited
        status.has_exited = CLAUDE_EXITED_PATTERNS.iter().any(|p| p.is_match(&last_lines));

        if !status.has_exited {
            // Compacting
            status.is_compacting =
                CLAUDE_COMPACTING_PATTERNS.iter().any(|p| p.is_match(&last_lines));

            // Busy (actively working)
            status.is_busy = CLAUDE_BUSY_PATTERNS.iter().any(|p| p.is_match(&last_lines))
                || has_spinner(&last_few_lines);

            // Idle prompt detection — BEFORE waiting patterns
            if !status.is_busy && !status.is_compacting {
                status.has_idle_prompt = IDLE_PROMPT_RE.is_match(&last_few_lines);
            }

            // Waiting — only when there's NO idle prompt
            if !status.has_idle_prompt {
                status.is_waiting =
                    CLAUDE_WAITING_PATTERNS.iter().any(|p| p.is_match(&last_few_lines));
            }

            // Question detection when at idle prompt
            if status.has_idle_prompt && !status.is_busy && !status.is_compacting {
                // Find the prompt line index and scan lines above it
                if let Some(prompt_idx) = trimmed_lines
                    .iter()
                    .rposition(|l| IDLE_PROMPT_RE.is_match(l))
                {
                    let scan_start = if prompt_idx > 20 {
                        prompt_idx - 20
                    } else {
                        0
                    };
                    let lines_above = &trimmed_lines[scan_start..prompt_idx];
                    let mut content_checked = 0;

                    for line in lines_above.iter().rev() {
                        if content_checked >= 8 {
                            break;
                        }
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if SEPARATOR_RE.is_match(trimmed) || COMPANION_RE.is_match(trimmed) {
                            continue;
                        }
                        if ART_LINE_RE.is_match(trimmed) {
                            continue;
                        }
                        if SPINNER_LINE_RE.is_match(trimmed) {
                            continue;
                        }
                        if USER_INPUT_RE.is_match(trimmed) {
                            continue;
                        }
                        if SHORTCUTS_RE.is_match(trimmed) {
                            continue;
                        }
                        content_checked += 1;
                        if QUESTION_RE.is_match(trimmed) {
                            status.has_question = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    // Error detection — only when not busy and not at idle prompt
    if !status.is_busy && !status.has_idle_prompt {
        status.has_error = ERROR_PATTERNS.iter().any(|p| p.is_match(&last_lines));
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_running_ctrl_c_to_interrupt() {
        let output = "Some output\nctrl+c to interrupt\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_running_esc_to_interrupt() {
        let output = "Working...\nesc to interrupt\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
    }

    #[test]
    fn test_running_spinner_characters() {
        let output = "Processing \u{280b} loading...\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
    }

    #[test]
    fn test_running_tokens_indicator() {
        let output = "Processing\n\u{2026} 20.4k tokens\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
    }

    #[test]
    fn test_waiting_yn_prompt() {
        let output = "Do something? (Y/n)\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_waiting_proceed_prompt() {
        let output = "Do you want to proceed?\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_numbered_yes() {
        let output = "Choose an option:\n1. Yes\n2. No\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_allow_once() {
        let output = "Permission needed:\nYes, allow once\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_approve_plan() {
        let output = "Here's the plan:\nApprove this plan?\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_continue() {
        let output = "Continue?\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_esc_tab_footer() {
        let output = "Permission prompt\nEsc to cancel  Tab to amend\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_enter_to_select() {
        let output = "Select option:\nEnter to select, arrows to navigate\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_idle_prompt_overrides_waiting_patterns() {
        // If the idle prompt is visible, waiting patterns should NOT match
        // because they'd be from historical conversational output
        let output = "Earlier output with (Y/n) text\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_idle_prompt_detected() {
        let output = "Claude finished.\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_paused_question_at_prompt() {
        let output = "What file should I edit?\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(status.has_question);
    }

    #[test]
    fn test_no_question_when_no_question_mark() {
        let output = "I have completed the task.\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.has_question);
    }

    #[test]
    fn test_exited_resume_session() {
        let output = "Session ended.\nResume this session with:\nclaude --resume abc123\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_exited);
        assert!(!status.is_busy);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_exited_claude_resume() {
        let output = "Done.\nclaude --resume session-id\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_exited);
    }

    #[test]
    fn test_exited_ctrl_c_exit() {
        let output = "Shutting down...\nPress Ctrl-C again to exit\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_exited);
    }

    #[test]
    fn test_compacting_conversation() {
        let output = "Context getting large...\ncompacting conversation\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_compacting);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_compacting_summarizing() {
        let output = "summarizing conversation to save space\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_compacting);
    }

    #[test]
    fn test_error_not_detected_when_busy() {
        let output = "error: something failed\nctrl+c to interrupt\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
        assert!(!status.has_error);
    }

    #[test]
    fn test_error_not_detected_at_idle_prompt() {
        let output = "error: something failed earlier\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.has_error);
    }

    #[test]
    fn test_error_detected_when_not_busy() {
        let output = "Running task...\nerror: compilation failed\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_error);
    }

    #[test]
    fn test_error_failed_pattern() {
        let output = "Trying something...\nfailed: connection refused\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_error);
    }

    #[test]
    fn test_error_traceback() {
        let output = "Running script...\nTraceback (most recent call last):\n  File...\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_error);
    }

    #[test]
    fn test_empty_output_is_not_busy() {
        let output = "\n\n\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(!status.is_busy);
        assert!(!status.is_waiting);
        assert!(!status.has_error);
    }

    #[test]
    fn test_non_claude_tool_no_claude_patterns() {
        // Non-Claude tools should not trigger Claude-specific patterns
        let output = "ctrl+c to interrupt\n";
        let status = parse_tool_status(output, Some("shell"));
        assert!(!status.is_busy); // Claude busy patterns don't apply
    }

    #[test]
    fn test_question_several_lines_above_prompt() {
        let output = "Would you like me to proceed with this approach?\n\nSome blank lines\n\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(status.has_question);
    }

    #[test]
    fn test_separator_lines_skipped_in_question_scan() {
        let output = "Done with that.\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.has_question);
    }
}
```

- [ ] **Step 2: Verify all tests pass**

```bash
cargo test core::status::tests
```

- [ ] **Step 3: Commit**

```
feat(status): port Claude Code status detection with exact regex patterns
```

---

### Task 6: Config loading

**Files:**
- Create: `src/core/config.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/core/config.rs`:

```rust
//! Configuration loading from ~/.agent-view/config.json

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default)]
    pub sound: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { sound: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_tool")]
    pub default_tool: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_group")]
    pub default_group: String,
    #[serde(default)]
    pub notifications: NotificationConfig,
}

fn default_tool() -> String {
    "claude".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_group() -> String {
    "default".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_tool: default_tool(),
            theme: default_theme(),
            default_group: default_group(),
            notifications: NotificationConfig::default(),
        }
    }
}

/// Get the config directory path (~/.agent-view)
pub fn config_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    home.join(".agent-view")
}

/// Get the config file path (~/.agent-view/config.json)
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Load config from disk, merging with defaults.
/// Returns defaults if file doesn't exist or fails to parse.
pub fn load_config() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => config,
            Err(_) => {
                eprintln!(
                    "Warning: Failed to parse config from {}",
                    path.display()
                );
                AppConfig::default()
            }
        },
        Err(_) => AppConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.default_tool, "claude");
        assert_eq!(config.theme, "dark");
        assert_eq!(config.default_group, "default");
        assert!(!config.notifications.sound);
    }

    #[test]
    fn test_parse_full_config() {
        let json = r#"{
            "default_tool": "gemini",
            "theme": "light",
            "default_group": "work",
            "notifications": { "sound": true }
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_tool, "gemini");
        assert_eq!(config.theme, "light");
        assert_eq!(config.default_group, "work");
        assert!(config.notifications.sound);
    }

    #[test]
    fn test_parse_partial_config_uses_defaults() {
        let json = r#"{ "theme": "light" }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.theme, "light");
        assert_eq!(config.default_tool, "claude"); // default
        assert!(!config.notifications.sound); // default
    }

    #[test]
    fn test_parse_empty_object_uses_defaults() {
        let json = "{}";
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_tool, "claude");
        assert_eq!(config.theme, "dark");
    }

    #[test]
    fn test_invalid_json_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "not valid json!!!").unwrap();

        // We can't easily test load_config() with a custom path,
        // but we test the parsing logic
        let result: Result<AppConfig, _> = serde_json::from_str("not valid json!!!");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Verify tests pass**

```bash
cargo test core::config::tests
```

- [ ] **Step 3: Commit**

```
feat(config): add config loading with defaults from ~/.agent-view/config.json
```

---

### Task 7: Desktop notifications

**Files:**
- Create: `src/core/notify.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/core/notify.rs`:

```rust
//! Desktop notifications via terminal-notifier (macOS) or osascript fallback
//! Uses std::process::Command directly — no external crate needed.

use std::process::Command;
use std::sync::OnceLock;

static HAS_TERMINAL_NOTIFIER: OnceLock<bool> = OnceLock::new();

/// Check if terminal-notifier is available (cached)
fn check_terminal_notifier() -> bool {
    *HAS_TERMINAL_NOTIFIER.get_or_init(|| {
        Command::new("which")
            .arg("terminal-notifier")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

pub struct NotificationOptions {
    pub title: String,
    pub body: String,
    pub subtitle: Option<String>,
    pub sound: bool,
}

/// Build the notification command string for macOS.
/// Returns the command and args as a Vec for use with Command.
pub fn build_notification_command(options: &NotificationOptions) -> (String, Vec<String>) {
    let safe_title = options.title.replace('"', "\\\"");
    let safe_body = options.body.replace('"', "\\\"");

    if cfg!(target_os = "macos") {
        if check_terminal_notifier() {
            let mut args = vec![
                "-title".to_string(),
                safe_title,
                "-message".to_string(),
                safe_body,
                "-timeout".to_string(),
                "30".to_string(),
            ];
            if let Some(ref subtitle) = options.subtitle {
                args.push("-subtitle".to_string());
                args.push(subtitle.replace('"', "\\\""));
            }
            if options.sound {
                args.push("-sound".to_string());
                args.push("default".to_string());
            }
            ("terminal-notifier".to_string(), args)
        } else {
            let sound_clause = if options.sound {
                " sound name \"default\""
            } else {
                ""
            };
            let subtitle_clause = options
                .subtitle
                .as_ref()
                .map(|s| format!(" subtitle \"{}\"", s.replace('"', "\\\"")))
                .unwrap_or_default();

            let script = format!(
                "display notification \"{}\" with title \"{}\"{}{}",
                safe_body, safe_title, subtitle_clause, sound_clause
            );
            ("osascript".to_string(), vec!["-e".to_string(), script])
        }
    } else {
        // Linux: notify-send
        (
            "notify-send".to_string(),
            vec!["-u".to_string(), "critical".to_string(), safe_title, safe_body],
        )
    }
}

/// Build an osascript fallback command (used when terminal-notifier fails)
pub fn build_osascript_fallback(options: &NotificationOptions) -> (String, Vec<String>) {
    let safe_title = options.title.replace('"', "\\\"");
    let safe_body = options.body.replace('"', "\\\"");
    let sound_clause = if options.sound {
        " sound name \"default\""
    } else {
        ""
    };
    let subtitle_clause = options
        .subtitle
        .as_ref()
        .map(|s| format!(" subtitle \"{}\"", s.replace('"', "\\\"")))
        .unwrap_or_default();

    let script = format!(
        "display notification \"{}\" with title \"{}\"{}{}",
        safe_body, safe_title, subtitle_clause, sound_clause
    );
    ("osascript".to_string(), vec!["-e".to_string(), script])
}

/// Send a desktop notification (non-blocking, spawns subprocess)
pub fn send_notification(options: NotificationOptions) {
    let (cmd, args) = build_notification_command(&options);

    let result = Command::new(&cmd).args(&args).output();

    match result {
        Ok(output) if !output.status.success() => {
            // terminal-notifier failed, try osascript fallback on macOS
            if cfg!(target_os = "macos") && check_terminal_notifier() {
                let (fallback_cmd, fallback_args) = build_osascript_fallback(&options);
                let _ = Command::new(&fallback_cmd)
                    .args(&fallback_args)
                    .output();
            }
            if options.sound {
                // Bell fallback
                print!("\x07");
            }
        }
        Err(_) => {
            if options.sound {
                print!("\x07");
            }
        }
        _ => {}
    }

    // Linux sound fallback
    if cfg!(target_os = "linux") && options.sound {
        print!("\x07");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_osascript_fallback_basic() {
        let options = NotificationOptions {
            title: "Test Title".to_string(),
            body: "Test Body".to_string(),
            subtitle: None,
            sound: false,
        };
        let (cmd, args) = build_osascript_fallback(&options);
        assert_eq!(cmd, "osascript");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "-e");
        assert!(args[1].contains("Test Title"));
        assert!(args[1].contains("Test Body"));
    }

    #[test]
    fn test_build_osascript_fallback_with_sound() {
        let options = NotificationOptions {
            title: "Title".to_string(),
            body: "Body".to_string(),
            subtitle: None,
            sound: true,
        };
        let (_, args) = build_osascript_fallback(&options);
        assert!(args[1].contains("sound name"));
    }

    #[test]
    fn test_build_osascript_fallback_with_subtitle() {
        let options = NotificationOptions {
            title: "Title".to_string(),
            body: "Body".to_string(),
            subtitle: Some("Sub".to_string()),
            sound: false,
        };
        let (_, args) = build_osascript_fallback(&options);
        assert!(args[1].contains("subtitle"));
        assert!(args[1].contains("Sub"));
    }

    #[test]
    fn test_build_osascript_escapes_quotes() {
        let options = NotificationOptions {
            title: "Title with \"quotes\"".to_string(),
            body: "Body with \"quotes\"".to_string(),
            subtitle: None,
            sound: false,
        };
        let (_, args) = build_osascript_fallback(&options);
        assert!(args[1].contains("\\\""));
    }

    #[test]
    fn test_notification_options_struct() {
        let options = NotificationOptions {
            title: "\u{1F7E1} BIS".to_string(),
            body: "Needs approval".to_string(),
            subtitle: None,
            sound: false,
        };
        assert_eq!(options.title, "\u{1F7E1} BIS");
        assert_eq!(options.body, "Needs approval");
    }
}
```

- [ ] **Step 2: Verify tests pass**

```bash
cargo test core::notify::tests
```

- [ ] **Step 3: Commit**

```
feat(notify): add desktop notifications via terminal-notifier and osascript
```

---

### Task 8: Session lifecycle manager with debouncing

**Files:**
- Create: `src/core/session.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/core/session.rs`:

```rust
//! Session lifecycle management with status debouncing and notification logic

use crate::core::notify::{send_notification, NotificationOptions};
use crate::core::storage::Storage;
use crate::core::tmux::SessionCache;
use crate::types::{Session, SessionCreateOptions, SessionStatus, Tool};
use std::collections::HashMap;
use std::time::Instant;

// Name generation word lists
const ADJECTIVES: &[&str] = &[
    "swift", "bright", "calm", "deep", "eager", "fair", "gentle", "happy",
    "keen", "light", "mild", "noble", "proud", "quick", "rich", "safe",
    "true", "vivid", "warm", "wise", "bold", "cool", "dark", "fast",
];

const NOUNS: &[&str] = &[
    "fox", "owl", "wolf", "bear", "hawk", "lion", "deer", "crow",
    "dove", "seal", "swan", "hare", "lynx", "moth", "newt", "orca",
    "pike", "rook", "toad", "vole", "wren", "yak", "bass", "crab",
];

fn generate_title() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    let adj = ADJECTIVES[nanos % ADJECTIVES.len()];
    let noun = NOUNS[(nanos / ADJECTIVES.len()) % NOUNS.len()];
    format!("{}-{}", adj, noun)
}

/// Tracks debounce and notification state for status management
pub struct SessionManager {
    /// Last status we notified about per session (prevents repeated notifications)
    last_notified_status: HashMap<String, SessionStatus>,
    /// When a session entered "running" state
    running_start_time: HashMap<String, Instant>,
    /// Last sustained running duration per session
    last_sustained_running: HashMap<String, u128>,
    /// When a session first entered idle
    idle_start_time: HashMap<String, Instant>,
    /// Recently detached sessions (suppress notifications briefly)
    recently_detached: HashMap<String, Instant>,
    /// When a session first showed error patterns
    error_start_time: HashMap<String, Instant>,
    /// Pending status transitions for debouncing
    pending_status: HashMap<String, (SessionStatus, Instant)>,
}

/// Minimum time (ms) a session must be "running" before idle triggers "completed" notification
const MIN_RUNNING_DURATION_MS: u128 = 10_000;
/// Minimum time (ms) a session must be idle before we consider it "completed"
const MIN_IDLE_DURATION_MS: u128 = 8_000;
/// Minimum time (ms) error patterns must persist before showing error status
const MIN_ERROR_DURATION_MS: u128 = 5_000;
/// Minimum time (ms) a new status must persist before the UI updates
const STATUS_DEBOUNCE_MS: u128 = 2_000;

impl SessionManager {
    pub fn new() -> Self {
        Self {
            last_notified_status: HashMap::new(),
            running_start_time: HashMap::new(),
            last_sustained_running: HashMap::new(),
            idle_start_time: HashMap::new(),
            recently_detached: HashMap::new(),
            error_start_time: HashMap::new(),
            pending_status: HashMap::new(),
        }
    }

    /// Mark a session as recently detached to suppress notifications
    pub fn suppress_notification(&mut self, tmux_session: &str) {
        self.recently_detached
            .insert(tmux_session.to_string(), Instant::now());
    }

    /// Determine the resolved status for a session given the raw detected status.
    /// Applies error hysteresis and status debouncing.
    /// Returns the status to display (may be the previous status if still debouncing).
    pub fn resolve_status(
        &mut self,
        session_id: &str,
        raw_status: SessionStatus,
        previous_status: SessionStatus,
    ) -> SessionStatus {
        // Error hysteresis: require sustained error before showing
        if raw_status == SessionStatus::Error {
            if !self.error_start_time.contains_key(session_id) {
                self.error_start_time
                    .insert(session_id.to_string(), Instant::now());
            }
            let error_duration = self
                .error_start_time
                .get(session_id)
                .unwrap()
                .elapsed()
                .as_millis();
            if error_duration < MIN_ERROR_DURATION_MS {
                return if previous_status == SessionStatus::Error {
                    SessionStatus::Idle
                } else {
                    previous_status
                };
            }
        } else {
            self.error_start_time.remove(session_id);
        }

        // Debounce: "waiting" bypasses debounce (immediate)
        if raw_status != previous_status {
            if raw_status == SessionStatus::Waiting {
                self.pending_status.remove(session_id);
                return raw_status;
            }

            if let Some((pending_st, pending_since)) = self.pending_status.get(session_id) {
                if *pending_st == raw_status {
                    if pending_since.elapsed().as_millis() >= STATUS_DEBOUNCE_MS {
                        self.pending_status.remove(session_id);
                        return raw_status;
                    }
                    return previous_status; // still debouncing
                }
            }
            // New candidate status
            self.pending_status
                .insert(session_id.to_string(), (raw_status, Instant::now()));
            return previous_status;
        }

        // Status matches current — clear pending
        self.pending_status.remove(session_id);
        raw_status
    }

    /// Update running/idle duration tracking for notification logic
    pub fn track_durations(&mut self, session_id: &str, status: SessionStatus) {
        match status {
            SessionStatus::Running => {
                if !self.running_start_time.contains_key(session_id) {
                    self.running_start_time
                        .insert(session_id.to_string(), Instant::now());
                }
                self.idle_start_time.remove(session_id);
            }
            SessionStatus::Idle => {
                if !self.idle_start_time.contains_key(session_id) {
                    self.idle_start_time
                        .insert(session_id.to_string(), Instant::now());
                }
                // Record last running duration before clearing
                if let Some(start) = self.running_start_time.remove(session_id) {
                    self.last_sustained_running
                        .insert(session_id.to_string(), start.elapsed().as_millis());
                }
            }
            _ => {
                self.idle_start_time.remove(session_id);
                self.running_start_time.remove(session_id);
            }
        }

        // Update sustained running duration if still running
        if status == SessionStatus::Running {
            if let Some(start) = self.running_start_time.get(session_id) {
                let duration = start.elapsed().as_millis();
                self.last_sustained_running
                    .insert(session_id.to_string(), duration);
                // Reset notification tracking after sustained running
                if duration >= MIN_RUNNING_DURATION_MS {
                    self.last_notified_status.remove(session_id);
                }
            }
        }
    }

    /// Check if a notification should fire and fire it.
    /// Returns true if a notification was sent.
    pub fn maybe_notify(
        &mut self,
        session: &Session,
        new_status: SessionStatus,
        is_attached: bool,
        sound: bool,
    ) -> bool {
        if !session.notify || is_attached {
            return false;
        }

        // Check recently detached
        if let Some(detach_time) = self.recently_detached.get(&session.tmux_session) {
            if detach_time.elapsed().as_millis() < 5000 {
                return false;
            }
            self.recently_detached.remove(&session.tmux_session);
        }

        let last = self.last_notified_status.get(&session.id);

        let notified = match new_status {
            SessionStatus::Waiting if last != Some(&SessionStatus::Waiting) => {
                send_notification(NotificationOptions {
                    title: format!("\u{1F7E1} {}", session.title),
                    body: "Needs approval".to_string(),
                    subtitle: None,
                    sound,
                });
                true
            }
            SessionStatus::Paused if last != Some(&SessionStatus::Paused) => {
                send_notification(NotificationOptions {
                    title: format!("\u{1F535} {}", session.title),
                    body: "Asked you a question".to_string(),
                    subtitle: None,
                    sound,
                });
                true
            }
            SessionStatus::Idle if last != Some(&SessionStatus::Idle) => {
                let idle_duration = self
                    .idle_start_time
                    .get(&session.id)
                    .map(|t| t.elapsed().as_millis())
                    .unwrap_or(0);
                let was_running_enough = self
                    .last_sustained_running
                    .get(&session.id)
                    .copied()
                    .unwrap_or(0)
                    >= MIN_RUNNING_DURATION_MS;
                let is_sustained_idle = idle_duration >= MIN_IDLE_DURATION_MS;

                if was_running_enough && is_sustained_idle {
                    send_notification(NotificationOptions {
                        title: format!("\u{2705} {}", session.title),
                        body: "Completed its task".to_string(),
                        subtitle: None,
                        sound,
                    });
                    true
                } else {
                    false
                }
            }
            SessionStatus::Error if last != Some(&SessionStatus::Error) => {
                send_notification(NotificationOptions {
                    title: format!("\u{1F534} {}", session.title),
                    body: "Was interrupted".to_string(),
                    subtitle: None,
                    sound,
                });
                true
            }
            _ => false,
        };

        if notified {
            self.last_notified_status
                .insert(session.id.clone(), new_status);
        }

        notified
    }

    /// Create a new session (creates tmux session and saves to storage)
    pub fn create_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        options: SessionCreateOptions,
    ) -> Result<Session, String> {
        let title = options.title.unwrap_or_else(generate_title);
        let id = uuid::Uuid::new_v4().to_string();
        let tmux_name = crate::core::tmux::generate_session_name(&title);
        let command = options
            .command
            .unwrap_or_else(|| options.tool.command().to_string());

        let now = chrono::Utc::now().timestamp_millis();

        let mut env = HashMap::new();
        env.insert("AGENT_ORCHESTRATOR_SESSION".to_string(), id.clone());

        crate::core::tmux::create_session(
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
            group_path: options.group_path.unwrap_or_else(|| "my-sessions".to_string()),
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
            status_history: vec![crate::types::StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: now,
            }],
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
            crate::core::tmux::kill_session(&session.tmux_session)?;
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
                crate::core::tmux::kill_session(&session.tmux_session)?;
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
            crate::core::tmux::kill_session(&session.tmux_session)?;
            cache.remove(&session.tmux_session);
        }

        let new_tmux_name = crate::core::tmux::generate_session_name(&session.title);
        let mut env = HashMap::new();
        env.insert(
            "AGENT_ORCHESTRATOR_SESSION".to_string(),
            session.id.clone(),
        );

        crate::core::tmux::create_session(
            &new_tmux_name,
            Some(&session.command),
            Some(&session.project_path),
            Some(&env),
        )?;

        cache.register(&new_tmux_name);

        session.tmux_session = new_tmux_name;
        session.status = SessionStatus::Running;
        session.last_accessed = chrono::Utc::now().timestamp_millis();

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_session(id: &str, notify: bool) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp".to_string(),
            group_path: "my-sessions".to_string(),
            order: 0,
            command: "claude".to_string(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Running,
            tmux_session: format!("agentorch_{}", id),
            created_at: 0,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify,
            follow_up: false,
            status_changed_at: 0,
            restart_count: 0,
            status_history: vec![],
        }
    }

    #[test]
    fn test_resolve_status_debounces_non_waiting() {
        let mut mgr = SessionManager::new();
        // First call: starts debounce timer, returns previous
        let result = mgr.resolve_status("s1", SessionStatus::Idle, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Running); // still debouncing
    }

    #[test]
    fn test_resolve_status_waiting_is_immediate() {
        let mut mgr = SessionManager::new();
        let result = mgr.resolve_status("s1", SessionStatus::Waiting, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Waiting); // immediate
    }

    #[test]
    fn test_resolve_status_same_status_clears_pending() {
        let mut mgr = SessionManager::new();
        // Start a pending transition
        mgr.resolve_status("s1", SessionStatus::Idle, SessionStatus::Running);
        assert!(mgr.pending_status.contains_key("s1"));

        // Same as current — clears pending
        mgr.resolve_status("s1", SessionStatus::Running, SessionStatus::Running);
        assert!(!mgr.pending_status.contains_key("s1"));
    }

    #[test]
    fn test_resolve_status_error_hysteresis() {
        let mut mgr = SessionManager::new();
        // Error just started — should not immediately show
        let result = mgr.resolve_status("s1", SessionStatus::Error, SessionStatus::Running);
        assert_eq!(result, SessionStatus::Running); // error not sustained yet
    }

    #[test]
    fn test_generate_title_format() {
        let title = generate_title();
        let parts: Vec<&str> = title.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(NOUNS.contains(&parts[1]));
    }

    #[test]
    fn test_suppress_notification() {
        let mut mgr = SessionManager::new();
        mgr.suppress_notification("agentorch_test");
        assert!(mgr.recently_detached.contains_key("agentorch_test"));
    }

    #[test]
    fn test_maybe_notify_returns_false_when_not_enabled() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s1", false); // notify = false
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, false, false);
        assert!(!result);
    }

    #[test]
    fn test_maybe_notify_returns_false_when_attached() {
        let mut mgr = SessionManager::new();
        let session = make_test_session("s1", true);
        let result = mgr.maybe_notify(&session, SessionStatus::Waiting, true, false);
        assert!(!result);
    }

    #[test]
    fn test_track_durations_running() {
        let mut mgr = SessionManager::new();
        mgr.track_durations("s1", SessionStatus::Running);
        assert!(mgr.running_start_time.contains_key("s1"));
        assert!(!mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_track_durations_idle_clears_running() {
        let mut mgr = SessionManager::new();
        mgr.track_durations("s1", SessionStatus::Running);
        mgr.track_durations("s1", SessionStatus::Idle);
        assert!(!mgr.running_start_time.contains_key("s1"));
        assert!(mgr.idle_start_time.contains_key("s1"));
    }

    #[test]
    fn test_track_durations_other_clears_both() {
        let mut mgr = SessionManager::new();
        mgr.track_durations("s1", SessionStatus::Running);
        mgr.track_durations("s1", SessionStatus::Waiting);
        assert!(!mgr.running_start_time.contains_key("s1"));
        assert!(!mgr.idle_start_time.contains_key("s1"));
    }
}
```

- [ ] **Step 2: Verify tests pass**

```bash
cargo test core::session::tests
```

- [ ] **Step 3: Commit**

```
feat(session): add session lifecycle manager with debouncing and notification logic
```

---

### Task 9: CLI entry point with clap

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement CLI parsing**

Replace `src/main.rs`:

```rust
mod app;
mod core;
mod event;
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

    // Initialize app state
    let mut app = crate::app::App::new();

    // Load sessions from storage
    app.sessions = storage.load_sessions()?;
    app.clamp_selection();

    // If --attach was passed, store for immediate attach after TUI starts
    if let Some(ref session_id) = cli.attach {
        app.attach_session = Some(session_id.clone());
    }

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
        event::{self, Event, KeyCode, KeyModifiers},
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

    let mut session_cache = crate::core::tmux::SessionCache::new();
    let mut session_manager = crate::core::session::SessionManager::new();
    let status_interval = Duration::from_millis(500);
    let mut last_status_check = Instant::now();

    // Handle --attach: immediately attach to the session
    if let Some(session_id) = app.attach_session.take() {
        if let Some(session) = app.sessions.iter().find(|s| s.id == session_id) {
            if !session.tmux_session.is_empty() {
                // Temporarily leave TUI for attach
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                let tmux_name = session.tmux_session.clone();
                let _ = crate::core::tmux::attach_session_sync(&tmux_name);

                // Re-enter TUI
                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;
            }
        }
    }

    loop {
        // Render
        terminal.draw(|frame| {
            crate::ui::home::render(frame, &app);
        })?;

        // Poll for events (16ms timeout for ~60fps input responsiveness)
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                match app.overlay {
                    crate::app::Overlay::None => {
                        handle_main_key(&mut app, key, &storage, &mut session_cache, &mut session_manager, &mut terminal)?;
                    }
                    crate::app::Overlay::NewSession(_) => {
                        handle_new_session_key(&mut app, key, &storage, &mut session_cache, &session_manager)?;
                    }
                    crate::app::Overlay::Confirm(_) => {
                        handle_confirm_key(&mut app, key, &storage, &mut session_cache, &session_manager)?;
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Status refresh every 500ms
        if last_status_check.elapsed() >= status_interval {
            last_status_check = Instant::now();
            refresh_statuses(
                &mut app,
                &storage,
                &mut session_cache,
                &mut session_manager,
                &config,
            );
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}

fn handle_main_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_cache: &mut crate::core::tmux::SessionCache,
    session_manager: &mut crate::core::session::SessionManager,
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    use crossterm::{execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};

    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.should_quit = true;
        }
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            app.move_selection_up();
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            app.move_selection_down();
        }
        (KeyModifiers::NONE, KeyCode::Char('n')) => {
            app.overlay = crate::app::Overlay::NewSession(crate::app::NewSessionForm::new());
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // Attach to selected session
            if let Some(session) = app.selected_session() {
                if !session.tmux_session.is_empty()
                    && session.status != crate::types::SessionStatus::Stopped
                {
                    let tmux_name = session.tmux_session.clone();

                    // Leave TUI for attach
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                    let _ = crate::core::tmux::attach_session_sync(&tmux_name);
                    session_manager.suppress_notification(&tmux_name);

                    // Re-enter TUI
                    enable_raw_mode()?;
                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                    terminal.clear()?;

                    // Refresh sessions after returning
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                        app.clamp_selection();
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            // Stop selected session
            if let Some(session) = app.selected_session() {
                if session.status != crate::types::SessionStatus::Stopped {
                    let msg = format!("Stop session \"{}\"?", session.title);
                    app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                        message: msg,
                        action: crate::app::ConfirmAction::StopSession(session.id.clone()),
                    });
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            // Delete selected session
            if let Some(session) = app.selected_session() {
                let msg = format!("Delete session \"{}\"?", session.title);
                app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::DeleteSession(session.id.clone()),
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('r')) => {
            // Restart selected session
            if let Some(session) = app.selected_session() {
                let id = session.id.clone();
                if let Ok(updated) = session_manager.restart_session(storage, session_cache, &id) {
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                        app.clamp_selection();
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('!')) => {
            // Toggle notifications for selected session
            if let Some(session) = app.selected_session() {
                let new_val = !session.notify;
                let id = session.id.clone();
                let _ = storage.set_notify(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.clamp_selection();
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_new_session_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_cache: &mut crate::core::tmux::SessionCache,
    session_manager: &crate::core::session::SessionManager,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::NewSession(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Tab => {
                form.focused_field = (form.focused_field + 1) % 2;
            }
            KeyCode::BackTab => {
                form.focused_field = if form.focused_field == 0 { 1 } else { 0 };
            }
            KeyCode::Enter => {
                let title = if form.title.is_empty() {
                    None
                } else {
                    Some(form.title.clone())
                };
                let project_path = form.project_path.clone();

                let options = crate::types::SessionCreateOptions {
                    title,
                    project_path,
                    group_path: None,
                    tool: crate::types::Tool::Claude,
                    command: None,
                };

                match session_manager.create_session(storage, session_cache, options) {
                    Ok(_) => {
                        if let Ok(sessions) = storage.load_sessions() {
                            app.sessions = sessions;
                            // Select the newly created session (last one)
                            if !app.sessions.is_empty() {
                                app.selected_index = app.sessions.len() - 1;
                            }
                        }
                    }
                    Err(e) => {
                        // For now, just close the overlay on error
                        // A toast system would be better but is not in Phase 1 scope
                        eprintln!("Failed to create session: {}", e);
                    }
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Char(c) => {
                match form.focused_field {
                    0 => form.title.push(c),
                    1 => form.project_path.push(c),
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                match form.focused_field {
                    0 => { form.title.pop(); }
                    1 => { form.project_path.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_confirm_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_cache: &mut crate::core::tmux::SessionCache,
    session_manager: &crate::core::session::SessionManager,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Confirm(ref dialog) = app.overlay.clone() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match &dialog.action {
                    crate::app::ConfirmAction::DeleteSession(id) => {
                        let _ = session_manager.delete_session(storage, session_cache, id);
                    }
                    crate::app::ConfirmAction::StopSession(id) => {
                        let _ = session_manager.stop_session(storage, id);
                    }
                }
                // Refresh sessions
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.clamp_selection();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            _ => {}
        }
    }

    Ok(())
}

fn refresh_statuses(
    app: &mut crate::app::App,
    storage: &crate::core::storage::Storage,
    session_cache: &mut crate::core::tmux::SessionCache,
    session_manager: &mut crate::core::session::SessionManager,
    config: &crate::core::config::AppConfig,
) {
    session_cache.refresh();
    let attached = crate::core::tmux::get_attached_sessions();
    let sound = config.notifications.sound;

    let mut any_changed = false;

    for session in &app.sessions {
        if session.tmux_session.is_empty() {
            continue;
        }

        let exists = session_cache.session_exists(&session.tmux_session);
        if !exists {
            if session.status != crate::types::SessionStatus::Stopped {
                let _ = storage.write_status(&session.id, crate::types::SessionStatus::Stopped, session.tool);
                any_changed = true;
            }
            continue;
        }

        let is_active = session_cache.is_session_active(&session.tmux_session, 2);
        let previous_status = session.status;

        // Capture pane output and parse status
        let raw_status = match crate::core::tmux::capture_pane(&session.tmux_session, Some(-100)) {
            Ok(output) => {
                let tool_str = if session.tool == crate::types::Tool::Claude {
                    Some("claude")
                } else {
                    None
                };
                let parsed = crate::core::status::parse_tool_status(&output, tool_str);

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
        };

        // Apply debouncing and error hysteresis
        let resolved = session_manager.resolve_status(&session.id, raw_status, previous_status);

        if resolved != previous_status {
            let _ = storage.write_status(&session.id, resolved, session.tool);
            any_changed = true;
        }

        // Track durations for notification logic
        session_manager.track_durations(&session.id, resolved);

        // Fire notifications
        let is_attached = attached.contains(&session.tmux_session);
        session_manager.maybe_notify(session, resolved, is_attached, sound);
    }

    if any_changed {
        let _ = storage.touch();
    }

    // Reload sessions to pick up changes
    if let Ok(sessions) = storage.load_sessions() {
        app.sessions = sessions;
        app.clamp_selection();
    }
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build
```

- [ ] **Step 3: Verify CLI parsing**

```bash
cargo run -- --version
cargo run -- --help
```

- [ ] **Step 4: Commit**

```
feat: add CLI entry point with clap, event loop, and status refresh
```

---

### Task 10: Home screen rendering — session list

**Files:**
- Create: `src/ui/home.rs`

- [ ] **Step 1: Implement the home screen layout**

Add to `src/ui/home.rs`:

```rust
//! Home screen rendering — session list with status icons

use crate::app::{App, Overlay};
use crate::types::SessionStatus;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Main render function for the home screen
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Layout: header (1), body (fill), footer (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0]);
    render_session_list(frame, chunks[1], app);
    crate::ui::footer::render(frame, chunks[2], app);

    // Render overlay on top if active
    match &app.overlay {
        Overlay::NewSession(form) => {
            crate::ui::overlay::render_new_session(frame, area, form);
        }
        Overlay::Confirm(dialog) => {
            crate::ui::overlay::render_confirm(frame, area, dialog);
        }
        Overlay::None => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect) {
    let version = env!("CARGO_PKG_VERSION");
    let header = Line::from(vec![
        Span::styled("agent-view ", Style::default().fg(Color::Cyan).bold()),
        Span::styled(format!("v{}", version), Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(header), area);
}

fn render_session_list(frame: &mut Frame, area: Rect, app: &App) {
    if app.sessions.is_empty() {
        let msg = Paragraph::new("No sessions. Press 'n' to create one.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .enumerate()
        .map(|(i, session)| {
            let status_icon = session.status.icon();
            let status_color = status_color(session.status);

            let notify_indicator = if session.notify { " !" } else { "  " };
            let follow_up_indicator = if session.follow_up { "\u{1f514}" } else { "  " };

            // Format: [follow_up] [status_icon] [notify] title     project_path  age
            let age = format_age(session.created_at);
            let title_width = area.width as usize;

            let line = Line::from(vec![
                Span::raw(follow_up_indicator),
                Span::styled(
                    format!(" {} ", status_icon),
                    Style::default().fg(status_color),
                ),
                Span::styled(notify_indicator, Style::default().fg(Color::Yellow)),
                Span::styled(
                    session.title.clone(),
                    Style::default().fg(Color::White).bold(),
                ),
                Span::raw("  "),
                Span::styled(
                    truncate_path(&session.project_path, 30),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(age, Style::default().fg(Color::DarkGray)),
            ]);

            let style = if i == app.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default().bg(Color::DarkGray));
    frame.render_widget(list, area);
}

fn status_color(status: SessionStatus) -> Color {
    match status {
        SessionStatus::Running => Color::Green,
        SessionStatus::Waiting => Color::Yellow,
        SessionStatus::Paused => Color::Blue,
        SessionStatus::Compacting => Color::Magenta,
        SessionStatus::Idle => Color::DarkGray,
        SessionStatus::Error => Color::Red,
        SessionStatus::Stopped => Color::DarkGray,
    }
}

/// Format a millisecond timestamp as a human-readable age
fn format_age(created_at_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - created_at_ms;
    if diff_ms < 0 {
        return "just now".to_string();
    }

    let seconds = diff_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d", days)
    } else if hours > 0 {
        format!("{}h", hours)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        "just now".to_string()
    }
}

/// Truncate a path to fit within max_len, keeping the end
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        let start = path.len() - max_len + 1;
        format!("~{}", &path[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_age_days() {
        let now = chrono::Utc::now().timestamp_millis();
        let two_days_ago = now - 2 * 24 * 60 * 60 * 1000;
        assert_eq!(format_age(two_days_ago), "2d");
    }

    #[test]
    fn test_format_age_hours() {
        let now = chrono::Utc::now().timestamp_millis();
        let three_hours_ago = now - 3 * 60 * 60 * 1000;
        assert_eq!(format_age(three_hours_ago), "3h");
    }

    #[test]
    fn test_format_age_minutes() {
        let now = chrono::Utc::now().timestamp_millis();
        let five_min_ago = now - 5 * 60 * 1000;
        assert_eq!(format_age(five_min_ago), "5m");
    }

    #[test]
    fn test_format_age_just_now() {
        let now = chrono::Utc::now().timestamp_millis();
        assert_eq!(format_age(now), "just now");
    }

    #[test]
    fn test_truncate_path_short() {
        assert_eq!(truncate_path("/tmp/test", 30), "/tmp/test");
    }

    #[test]
    fn test_truncate_path_long() {
        let long_path = "/Users/mdoyle/projects/very-long-project-name/src";
        let result = truncate_path(long_path, 20);
        assert!(result.starts_with('~'));
        assert!(result.len() <= 20);
    }

    #[test]
    fn test_status_colors_are_distinct() {
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Error,
        ];
        let colors: Vec<Color> = statuses.iter().map(|s| status_color(*s)).collect();
        // Running, Waiting, Paused, Error should all be different colors
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }
}
```

- [ ] **Step 2: Verify tests pass**

```bash
cargo test ui::home::tests
```

- [ ] **Step 3: Commit**

```
feat(ui): add home screen with session list, status icons, and age display
```

---

### Task 11: Footer rendering — context-sensitive keybind hints

**Files:**
- Create: `src/ui/footer.rs`

- [ ] **Step 1: Implement footer**

Add to `src/ui/footer.rs`:

```rust
//! Context-sensitive footer with keybind hints

use crate::app::{App, Overlay};
use ratatui::prelude::*;
use ratatui::widgets::*;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let hints = match &app.overlay {
        Overlay::None => {
            if app.sessions.is_empty() {
                vec![("n", "new"), ("q", "quit")]
            } else {
                vec![
                    ("j/k", "navigate"),
                    ("Enter", "attach"),
                    ("n", "new"),
                    ("s", "stop"),
                    ("r", "restart"),
                    ("d", "delete"),
                    ("!", "notify"),
                    ("q", "quit"),
                ]
            }
        }
        Overlay::NewSession(_) => {
            vec![
                ("Tab", "next field"),
                ("Enter", "create"),
                ("Esc", "cancel"),
            ]
        }
        Overlay::Confirm(_) => {
            vec![("y", "confirm"), ("n/Esc", "cancel")]
        }
    };

    let spans: Vec<Span> = hints
        .iter()
        .enumerate()
        .flat_map(|(i, (key, action))| {
            let mut v = vec![
                Span::styled(
                    format!(" {} ", key),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::styled(
                    format!("{} ", action),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if i < hints.len() - 1 {
                v.push(Span::styled(" ", Style::default().fg(Color::DarkGray)));
            }
            v
        })
        .collect();

    let footer = Line::from(spans);
    frame.render_widget(Paragraph::new(footer), area);
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```
feat(ui): add context-sensitive footer with keybind hints
```

---

### Task 12: Overlay rendering — new session form and confirm dialog

**Files:**
- Create: `src/ui/overlay.rs`

- [ ] **Step 1: Implement overlay rendering**

Add to `src/ui/overlay.rs`:

```rust
//! Overlay rendering for new session form and confirm dialogs

use crate::app::{ConfirmDialog, NewSessionForm};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Render the new session creation form as a centered overlay
pub fn render_new_session(frame: &mut Frame, area: Rect, form: &NewSessionForm) {
    let overlay_width = 60u16.min(area.width.saturating_sub(4));
    let overlay_height = 9u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear background
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" New Session ")
        .title_style(Style::default().fg(Color::Cyan).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    // Layout fields vertically
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title label
            Constraint::Length(1), // Title input
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Path label
            Constraint::Length(1), // Path input
        ])
        .split(inner);

    // Title field
    let title_style = if form.focused_field == 0 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new("Title (leave empty for random):").style(title_style),
        chunks[0],
    );

    let title_display = if form.title.is_empty() && form.focused_field == 0 {
        "\u{2588}".to_string() // cursor block
    } else if form.focused_field == 0 {
        format!("{}\u{2588}", form.title)
    } else {
        if form.title.is_empty() {
            "(auto-generated)".to_string()
        } else {
            form.title.clone()
        }
    };
    frame.render_widget(
        Paragraph::new(title_display).style(Style::default().fg(Color::White)),
        chunks[1],
    );

    // Project path field
    let path_style = if form.focused_field == 1 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(
        Paragraph::new("Project Path:").style(path_style),
        chunks[3],
    );

    let path_display = if form.focused_field == 1 {
        format!("{}\u{2588}", form.project_path)
    } else {
        form.project_path.clone()
    };
    frame.render_widget(
        Paragraph::new(path_display).style(Style::default().fg(Color::White)),
        chunks[4],
    );
}

/// Render a confirmation dialog as a centered overlay
pub fn render_confirm(frame: &mut Frame, area: Rect, dialog: &ConfirmDialog) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Confirm ")
        .title_style(Style::default().fg(Color::Yellow).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(dialog.message.as_str()).style(Style::default().fg(Color::White)),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new("y/Enter = yes, n/Esc = no").style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```
feat(ui): add new session form and confirm dialog overlays
```

---

### Task 13: Integration smoke test — full build and manual verification

**Files:**
- No new files

- [ ] **Step 1: Run all tests**

```bash
cargo test
```

- [ ] **Step 2: Build release binary**

```bash
cargo build --release
```

- [ ] **Step 3: Verify binary size and that it runs**

```bash
ls -lh target/release/agent-view
target/release/agent-view --version
target/release/agent-view --help
```

- [ ] **Step 4: Manual smoke test (requires tmux)**

Launch the TUI and verify:
- Session list loads from existing database
- Status icons update in real-time
- Can navigate with j/k
- Can press 'n' to open new session form
- Can type title and path, press Enter to create
- Can press Enter on a session to attach
- Ctrl+Q detaches back to TUI
- Can press 's' then 'y' to stop a session
- Can press 'd' then 'y' to delete a session

```bash
target/release/agent-view
```

- [ ] **Step 5: Commit**

```
test: verify Phase 1 builds and passes all tests
```

---

### Task 14: Add .gitignore entries for Rust build artifacts

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Add Rust build artifacts to .gitignore**

Append to `.gitignore`:

```
# Rust build artifacts
/target/
Cargo.lock
```

Note: `Cargo.lock` is excluded because this is a binary project and the lock file should be committed. Actually, for binary projects, `Cargo.lock` _should_ be committed. Only exclude `target/`.

Append only:

```
# Rust build artifacts
/target/
```

- [ ] **Step 2: Commit**

```
chore: add Rust target/ to gitignore
```
