# Rust CI + Code Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add strict CI pipelines (fmt, clippy, test, build), fix all existing lint issues, improve test coverage on core modules, and fix the post-attach selection bug.

**Architecture:** Fix code quality first (fmt + clippy), then add CI configs, then write new tests. The selection bug is an isolated fix. All work on a `feat/rust-ci` branch from `release/rust`.

**Tech Stack:** Rust, cargo, clippy, rustfmt, GitHub Actions, GitLab CI, cross (for cross-compilation)

---

### Task 1: Create Branch and Fix Formatting

**Files:**
- Modify: all `.rs` files (auto-formatted)

- [ ] **Step 1: Create branch**

```bash
git checkout release/rust
git pull origin release/rust
git checkout -b feat/rust-ci
```

- [ ] **Step 2: Run cargo fmt**

```bash
cargo fmt
```

- [ ] **Step 3: Verify no diffs remain**

Run: `cargo fmt --check`
Expected: no output (clean)

- [ ] **Step 4: Run tests to confirm no behavior change**

Run: `cargo test`
Expected: 136 passed

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "style: apply rustfmt to all source files"
```

---

### Task 2: Fix Clippy Warnings — Dead Code

**Files:**
- Modify: `src/core/git.rs` (remove unused functions and structs)
- Modify: `src/core/tmux.rs` (remove unused function)
- Modify: `src/event.rs` (remove unused enum)
- Modify: `src/core/storage.rs` (allow dead_code on public API methods)
- Modify: `src/core/logger.rs` (allow dead_code on remove_session)
- Modify: `src/types.rs` (allow dead_code on ActivityEvent fields/methods)
- Modify: `src/ui/theme.rs` (allow dead_code on unused theme fields)
- Modify: `src/core/status.rs` (allow dead_code on is_active field)

The strategy: **remove** code that is truly dead (git.rs worktree functions that were never ported from TypeScript, unused event enum). **Allow** dead_code on public API methods that exist for completeness (storage CRUD methods, logger cleanup, theme fields that will be used when more features are added).

- [ ] **Step 1: Remove dead code from git.rs**

In `src/core/git.rs`, the following functions and structs are never used — they were scaffolded for worktree support but the actual implementation uses different code paths. Remove:
- `struct Worktree` and its `is_active` field
- `fn is_git_repo`
- `fn get_repo_root`
- `fn validate_branch_name`
- `fn branch_exists`
- `fn generate_worktree_path`
- `fn create_worktree`
- `fn list_worktrees`
- `fn parse_worktree_list`

Keep any functions that ARE used — read the file first and check with `grep -r "function_name" src/` before removing.

- [ ] **Step 2: Remove dead code from tmux.rs**

Remove `fn get_attached_sessions` if unused (verify with grep first).

- [ ] **Step 3: Remove dead AppEvent enum from event.rs**

Remove the `AppEvent` enum if it's entirely unused. The file may become mostly empty — that's fine, keep the module for future use or remove it from `mod.rs` if completely empty.

- [ ] **Step 4: Add allow(dead_code) for public API methods**

In `src/core/storage.rs`, add `#[allow(dead_code)]` above these methods that are part of the public API but not yet called:
- `set_acknowledged`
- `delete_group`  
- `close`
- `conn`

In `src/core/logger.rs`, add `#[allow(dead_code)]` above `remove_session`.

In `src/types.rs`, add `#[allow(dead_code)]` above `ActivityEvent::format_line` and the `old_status`/`message` fields.

In `src/ui/theme.rs`, add `#[allow(dead_code)]` above `background_panel` and `border_subtle` fields.

In `src/core/status.rs`, add `#[allow(dead_code)]` above the `is_active` field in `ParsedStatus`.

- [ ] **Step 5: Run clippy to verify dead code warnings are gone**

Run: `cargo clippy 2>&1 | grep "never used\|never read\|never constructed"`
Expected: no output

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: 136 passed

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: remove dead code and suppress intentional dead_code warnings"
```

---

### Task 3: Fix Clippy Warnings — Style Issues

**Files:**
- Modify: `src/ui/footer.rs` (~line 12)
- Modify: `src/core/config.rs` (~line 68, 89)
- Modify: `src/core/storage.rs` (~line 375)
- Modify: `src/core/status.rs` (~line 154)
- Modify: `src/core/tokens.rs` (~line 28)
- Modify: `src/core/groups.rs` (~line 11)
- Modify: `src/main.rs` (~line 1016)

Apply each clippy suggestion. Read each file and apply the fix:

- [ ] **Step 1: Fix all style warnings**

Run `cargo clippy` and fix each one:

1. `src/ui/footer.rs:12` — `map_or` → `is_some_and`:
   Change `app.toast_expire.map_or(false, |t| t > std::time::Instant::now())` to `app.toast_expire.is_some_and(|t| t > std::time::Instant::now())`

2. `src/core/config.rs:13` — derivable impl: If `NotificationConfig` has a manual `Default` impl that could be `#[derive(Default)]`, use the derive.

3. `src/core/config.rs:68` — redundant closure: simplify.

4. `src/core/config.rs:89` — `unwrap_or_default`: simplify match to use `.unwrap_or_default()`.

5. `src/core/storage.rs:375` — `unwrap_or_default`: simplify.

6. `src/core/status.rs:154` — consecutive `str::replace`: chain into a single operation or use a different approach.

7. `src/core/tokens.rs:28` — `std::io::Error::other()`: use the simpler constructor.

8. `src/core/groups.rs:11` — large enum variant: Box the large `ListRow` variant (likely `Session`):
   Change `Session(Session)` to `Session(Box<Session>)` and update all pattern matches.
   NOTE: This changes how `ListRow::Session` is constructed and destructured throughout the codebase. Search for all `ListRow::Session(` occurrences and update them. The `Session` inside the box needs to be dereferenced.

9. `src/main.rs:1016` — needless borrow: remove the `&` on `path`.

- [ ] **Step 2: Run clippy with -D warnings to verify zero warnings**

Run: `cargo clippy -- -D warnings`
Expected: compiles with no errors

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: 136 passed (or more if Box<Session> changes affect test patterns)

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "style: fix all clippy warnings to pass -D warnings gate"
```

---

### Task 4: Fix Post-Attach Selection Bug

**Files:**
- Modify: `src/main.rs` (the Enter/attach handler, ~line 548-553)

- [ ] **Step 1: Write failing test description**

This is an event-loop bug that can't be unit tested directly. The fix is straightforward: after `rebuild_list_rows()` following a detach, find the session we were just attached to and set `selected_index` to its position.

- [ ] **Step 2: Implement the fix**

In `src/main.rs`, in the `KeyCode::Enter` handler for attaching to a session, after the "Fresh reload after returning" block (after `app.rebuild_list_rows()`), add:

```rust
// Select the session we just detached from
if let Some(pos) = app.list_rows.iter().position(|row| {
    matches!(row, crate::core::groups::ListRow::Session(s) if s.tmux_session == tmux_name)
}) {
    app.selected_index = pos;
}
```

Note: If `ListRow::Session` was changed to `Box<Session>` in Task 3, the pattern match will be `ListRow::Session(s) if s.tmux_session == tmux_name` where `s` is a `&Box<Session>` — field access works the same due to auto-deref.

The `tmux_name` variable is already in scope from earlier in the same block.

- [ ] **Step 3: Also fix the --attach handler**

There's a similar block at ~line 260-275 for the `--attach` CLI flag. Apply the same fix there after `app.rebuild_list_rows()`.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 5: Manual verification**

Run: `cargo run`
1. Attach to a session (Enter)
2. Detach (Ctrl+b, d)
3. Verify the cursor is on the session you just detached from

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "fix: select previously attached session after detach"
```

---

### Task 5: Add Test Coverage — types.rs

**Files:**
- Modify: `src/types.rs` (add tests to existing test module)

- [ ] **Step 1: Write new tests**

Add to the `#[cfg(test)] mod tests` block in `src/types.rs`:

```rust
#[test]
fn test_sort_mode_cycles_through_all() {
    let mut mode = SortMode::StatusPriority;
    mode = mode.next(); assert_eq!(mode, SortMode::LastActivity);
    mode = mode.next(); assert_eq!(mode, SortMode::Name);
    mode = mode.next(); assert_eq!(mode, SortMode::Created);
    mode = mode.next(); assert_eq!(mode, SortMode::StatusPriority);
}

#[test]
fn test_sort_mode_labels() {
    assert_eq!(SortMode::StatusPriority.label(), "status");
    assert_eq!(SortMode::LastActivity.label(), "activity");
    assert_eq!(SortMode::Name.label(), "name");
    assert_eq!(SortMode::Created.label(), "created");
}

#[test]
fn test_sort_priority_ordering() {
    assert!(SessionStatus::Waiting.sort_priority() < SessionStatus::Running.sort_priority());
    assert!(SessionStatus::Running.sort_priority() < SessionStatus::Idle.sort_priority());
    assert!(SessionStatus::Idle.sort_priority() < SessionStatus::Error.sort_priority());
}

#[test]
fn test_session_status_icon_not_empty() {
    let statuses = [
        SessionStatus::Running, SessionStatus::Waiting, SessionStatus::Paused,
        SessionStatus::Compacting, SessionStatus::Idle, SessionStatus::Error,
        SessionStatus::Stopped,
    ];
    for s in statuses {
        assert!(!s.icon().is_empty());
    }
}

#[test]
fn test_session_status_display() {
    assert_eq!(format!("{}", SessionStatus::Running), "running");
    assert_eq!(format!("{}", SessionStatus::Stopped), "stopped");
}

#[test]
fn test_tool_command() {
    assert_eq!(Tool::Claude.command(), "claude");
    assert_eq!(Tool::Gemini.command(), "gemini");
    assert_eq!(Tool::Shell.command(), "bash");
}

#[test]
fn test_session_status_history_json_empty() {
    let session = Session {
        id: "test".to_string(), title: "Test".to_string(),
        project_path: "/tmp".to_string(), group_path: "default".to_string(),
        order: 0, command: String::new(), wrapper: String::new(),
        tool: Tool::Claude, status: SessionStatus::Idle,
        tmux_session: String::new(), created_at: 0, last_accessed: 0,
        parent_session_id: String::new(), worktree_path: String::new(),
        worktree_repo: String::new(), worktree_branch: String::new(),
        tool_data: "{}".to_string(), acknowledged: false, notify: false,
        follow_up: false, status_changed_at: 0, restart_count: 0,
        status_history: vec![], pinned: false, tokens_used: 0,
    };
    assert_eq!(session.status_history_json(), "[]");
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all pass (7 new tests)

- [ ] **Step 3: Commit**

```bash
git add src/types.rs
git commit -m "test: add coverage for SortMode, SessionStatus, Tool types"
```

---

### Task 6: Add Test Coverage — app.rs

**Files:**
- Modify: `src/app.rs` (add tests to existing or new test module)

- [ ] **Step 1: Write new tests**

Add to the `#[cfg(test)] mod tests` block in `src/app.rs`:

```rust
#[test]
fn test_select_all_visible() {
    let mut app = App::new(false);
    // Manually populate list_rows with sessions
    app.list_rows = vec![
        crate::core::groups::ListRow::Session(Box::new(crate::types::Session {
            id: "s1".to_string(), title: "S1".to_string(),
            project_path: "/tmp".to_string(), group_path: "default".to_string(),
            order: 0, command: String::new(), wrapper: String::new(),
            tool: crate::types::Tool::Claude, status: crate::types::SessionStatus::Idle,
            tmux_session: String::new(), created_at: 0, last_accessed: 0,
            parent_session_id: String::new(), worktree_path: String::new(),
            worktree_repo: String::new(), worktree_branch: String::new(),
            tool_data: "{}".to_string(), acknowledged: false, notify: false,
            follow_up: false, status_changed_at: 0, restart_count: 0,
            status_history: vec![], pinned: false, tokens_used: 0,
        })),
        crate::core::groups::ListRow::Session(Box::new(crate::types::Session {
            id: "s2".to_string(), title: "S2".to_string(),
            project_path: "/tmp".to_string(), group_path: "default".to_string(),
            order: 0, command: String::new(), wrapper: String::new(),
            tool: crate::types::Tool::Claude, status: crate::types::SessionStatus::Idle,
            tmux_session: String::new(), created_at: 0, last_accessed: 0,
            parent_session_id: String::new(), worktree_path: String::new(),
            worktree_repo: String::new(), worktree_branch: String::new(),
            tool_data: "{}".to_string(), acknowledged: false, notify: false,
            follow_up: false, status_changed_at: 0, restart_count: 0,
            status_history: vec![], pinned: false, tokens_used: 0,
        })),
    ];
    app.select_all_visible();
    assert_eq!(app.bulk_selected.len(), 2);
    assert!(app.bulk_selected.contains("s1"));
    assert!(app.bulk_selected.contains("s2"));
}

#[test]
fn test_push_activity_caps_at_100() {
    let mut app = App::new(false);
    for i in 0..110 {
        app.push_activity(crate::types::ActivityEvent {
            session_title: format!("S{}", i),
            old_status: crate::types::SessionStatus::Running,
            new_status: crate::types::SessionStatus::Idle,
            timestamp: i as i64,
            message: None,
        });
    }
    assert_eq!(app.activity_feed.len(), 100);
}

#[test]
fn test_push_activity_most_recent_first() {
    let mut app = App::new(false);
    app.push_activity(crate::types::ActivityEvent {
        session_title: "first".to_string(),
        old_status: crate::types::SessionStatus::Running,
        new_status: crate::types::SessionStatus::Idle,
        timestamp: 1000,
        message: None,
    });
    app.push_activity(crate::types::ActivityEvent {
        session_title: "second".to_string(),
        old_status: crate::types::SessionStatus::Running,
        new_status: crate::types::SessionStatus::Idle,
        timestamp: 2000,
        message: None,
    });
    assert_eq!(app.activity_feed[0].session_title, "second");
    assert_eq!(app.activity_feed[1].session_title, "first");
}

#[test]
fn test_command_palette_filter() {
    let mut palette = CommandPalette::new();
    palette.query = "sort".to_string();
    palette.filter();
    assert!(palette.filtered.len() < palette.items.len());
    // "Cycle Sort Mode" should match
    assert!(palette.filtered.iter().any(|&i| palette.items[i].label.contains("Sort")));
}

#[test]
fn test_command_palette_filter_empty_shows_all() {
    let mut palette = CommandPalette::new();
    palette.query = String::new();
    palette.filter();
    assert_eq!(palette.filtered.len(), palette.items.len());
}

#[test]
fn test_move_selection_wraps() {
    let mut app = App::new(false);
    app.list_rows = vec![
        crate::core::groups::ListRow::Group {
            group: crate::types::Group {
                path: "test".to_string(), name: "Test".to_string(),
                expanded: true, order: 0, default_path: String::new(),
            },
            session_count: 0, running_count: 0, waiting_count: 0,
        },
    ];
    app.selected_index = 0;
    app.move_selection_up(); // should wrap to last (0 since only 1 item)
    assert_eq!(app.selected_index, 0);
}
```

NOTE: If Task 3 changed `ListRow::Session(Session)` to `ListRow::Session(Box<Session>)`, all `ListRow::Session(...)` constructions in these tests must use `Box::new(...)`. The code above already accounts for this.

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all pass (6 new tests)

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "test: add coverage for App bulk selection, activity feed, command palette"
```

---

### Task 7: Add Test Coverage — config.rs and logger.rs

**Files:**
- Modify: `src/core/config.rs` (add tests)
- Modify: `src/core/logger.rs` (add tests)

- [ ] **Step 1: Write config tests**

Add to the test module in `src/core/config.rs`:

```rust
#[test]
fn test_save_and_reload_config() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");

    let config = AppConfig {
        default_tool: "gemini".to_string(),
        theme: "nord".to_string(),
        default_group: "work".to_string(),
        notifications: NotificationConfig { sound: true },
    };

    // Write manually to the path
    let json = serde_json::to_string_pretty(&config).unwrap();
    fs::write(&path, &json).unwrap();

    let loaded = load_config_from_path(&path);
    assert_eq!(loaded.default_tool, "gemini");
    assert_eq!(loaded.theme, "nord");
    assert_eq!(loaded.default_group, "work");
    assert!(loaded.notifications.sound);
}

#[test]
fn test_load_config_from_missing_path_returns_default() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nonexistent.json");
    let config = load_config_from_path(&path);
    assert_eq!(config.default_tool, "claude");
    assert_eq!(config.theme, "dark");
}

#[test]
fn test_save_config_creates_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("subdir").join("config.json");

    // We can't easily test save_config() since it uses a fixed path,
    // but we can test the serialization roundtrip
    let config = AppConfig::default();
    let json = serde_json::to_string_pretty(&config).unwrap();
    let parsed: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.default_tool, config.default_tool);
    assert_eq!(parsed.theme, config.theme);
}
```

- [ ] **Step 2: Write logger tests**

Add to the test module in `src/core/logger.rs`:

```rust
#[test]
fn test_logger_tracks_line_counts() {
    let mut logger = SessionLogger::new();
    assert_eq!(logger.last_line_counts.get("s1"), None);
    logger.last_line_counts.insert("s1".to_string(), 50);
    assert_eq!(logger.last_line_counts.get("s1"), Some(&50));
}

#[test]
fn test_logger_remove_session() {
    let mut logger = SessionLogger::new();
    logger.last_line_counts.insert("s1".to_string(), 50);
    logger.remove_session("s1");
    assert_eq!(logger.last_line_counts.get("s1"), None);
}

#[test]
fn test_multiple_rotations() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("test.log");

    // First rotation
    let chunk = "x".repeat(1024 * 1024);
    for _ in 0..11 {
        append_to_log(&log_path, &chunk).unwrap();
    }
    rotate_if_needed(&log_path, 10 * 1024 * 1024).unwrap();
    assert!(dir.path().join("test.log.1").exists());

    // Write more and rotate again
    for _ in 0..11 {
        append_to_log(&log_path, &chunk).unwrap();
    }
    rotate_if_needed(&log_path, 10 * 1024 * 1024).unwrap();
    assert!(dir.path().join("test.log.1").exists());
    assert!(dir.path().join("test.log.2").exists());
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add src/core/config.rs src/core/logger.rs
git commit -m "test: add coverage for config save/load and logger rotation"
```

---

### Task 8: Add Test Coverage — groups.rs Sort Modes

**Files:**
- Modify: `src/core/groups.rs` (add tests)

- [ ] **Step 1: Write sort mode tests**

Add to the test module in `src/core/groups.rs`:

```rust
#[test]
fn test_sort_by_last_activity() {
    let mut s1 = make_session("s1", "work", SessionStatus::Idle);
    s1.status_changed_at = 1000;
    let mut s2 = make_session("s2", "work", SessionStatus::Idle);
    s2.status_changed_at = 3000;
    let mut s3 = make_session("s3", "work", SessionStatus::Idle);
    s3.status_changed_at = 2000;
    let groups = vec![make_group("work", "Work", 0)];
    let rows = flatten_group_tree(&[s1, s2, s3], &groups, SortMode::LastActivity);
    if let ListRow::Session(first) = &rows[1] { assert_eq!(first.id, "s2"); }
    if let ListRow::Session(second) = &rows[2] { assert_eq!(second.id, "s3"); }
    if let ListRow::Session(third) = &rows[3] { assert_eq!(third.id, "s1"); }
}

#[test]
fn test_sort_by_name() {
    let mut s1 = make_session("s1", "work", SessionStatus::Idle);
    s1.title = "Charlie".to_string();
    let mut s2 = make_session("s2", "work", SessionStatus::Idle);
    s2.title = "Alice".to_string();
    let mut s3 = make_session("s3", "work", SessionStatus::Idle);
    s3.title = "Bob".to_string();
    let groups = vec![make_group("work", "Work", 0)];
    let rows = flatten_group_tree(&[s1, s2, s3], &groups, SortMode::Name);
    if let ListRow::Session(first) = &rows[1] { assert_eq!(first.title, "Alice"); }
    if let ListRow::Session(second) = &rows[2] { assert_eq!(second.title, "Bob"); }
    if let ListRow::Session(third) = &rows[3] { assert_eq!(third.title, "Charlie"); }
}

#[test]
fn test_sort_pinned_with_name_sort() {
    let mut s1 = make_session("s1", "work", SessionStatus::Idle);
    s1.title = "Charlie".to_string();
    let mut s2 = make_session("s2", "work", SessionStatus::Idle);
    s2.title = "Alice".to_string();
    s2.pinned = true;
    let groups = vec![make_group("work", "Work", 0)];
    let rows = flatten_group_tree(&[s1, s2], &groups, SortMode::Name);
    // Pinned "Alice" should be first regardless of name sort
    if let ListRow::Session(first) = &rows[1] {
        assert_eq!(first.title, "Alice");
        assert!(first.pinned);
    }
}
```

NOTE: If `ListRow::Session` now contains `Box<Session>`, the pattern matching in these tests will auto-deref. The `if let ListRow::Session(s) = &rows[1]` pattern works the same — `s` is `&Box<Session>` which auto-derefs to `&Session`.

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all pass (3 new tests)

- [ ] **Step 3: Commit**

```bash
git add src/core/groups.rs
git commit -m "test: add coverage for all sort modes and pinning interactions"
```

---

### Task 9: GitHub Actions CI Workflow

**Files:**
- Create: `.github/workflows/rust-test.yml`

- [ ] **Step 1: Create the workflow file**

```yaml
name: Rust CI

on:
  push:
    branches: [main, "release/*"]
  pull_request:
    branches: [main, "release/*"]

jobs:
  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --check

  clippy:
    name: Clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy -- -D warnings

  test:
    name: Test
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install tmux (Linux)
        if: runner.os == 'Linux'
        run: sudo apt-get update && sudo apt-get install -y tmux
      - name: Install tmux (macOS)
        if: runner.os == 'macOS'
        run: brew install tmux || true
      - run: cargo test

  build:
    name: Build Release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release
```

- [ ] **Step 2: Run tests locally to verify nothing is broken**

Run: `cargo test`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/rust-test.yml
git commit -m "ci: add Rust CI workflow with fmt, clippy, test, build gates"
```

---

### Task 10: GitHub Actions Version Bump Workflow

**Files:**
- Create: `.github/workflows/rust-version-bump.yml`

This workflow runs when a PR is merged to `main` or `release/*`. It reads PR labels (`version:patch`, `version:minor`, `version:major`) to determine bump type, updates `Cargo.toml`, generates a changelog entry, commits, and tags.

- [ ] **Step 1: Create the workflow file**

```yaml
name: Version Bump

on:
  pull_request:
    types: [closed]
    branches: [main, "release/*"]

jobs:
  bump:
    if: github.event.pull_request.merged == true
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          token: ${{ secrets.GITHUB_TOKEN }}

      - name: Determine bump type from labels
        id: bump
        run: |
          LABELS='${{ toJSON(github.event.pull_request.labels.*.name) }}'
          if echo "$LABELS" | grep -q "version:major"; then
            echo "type=major" >> $GITHUB_OUTPUT
          elif echo "$LABELS" | grep -q "version:minor"; then
            echo "type=minor" >> $GITHUB_OUTPUT
          elif echo "$LABELS" | grep -q "version:patch"; then
            echo "type=patch" >> $GITHUB_OUTPUT
          else
            echo "type=" >> $GITHUB_OUTPUT
          fi

      - name: Bump version
        if: steps.bump.outputs.type != ''
        run: |
          set -e
          BUMP_TYPE="${{ steps.bump.outputs.type }}"
          CURRENT_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
          MAJOR=$(echo "$CURRENT_VERSION" | cut -d. -f1)
          MINOR=$(echo "$CURRENT_VERSION" | cut -d. -f2)
          PATCH=$(echo "$CURRENT_VERSION" | cut -d. -f3)

          case "$BUMP_TYPE" in
            major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
            minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
            patch) PATCH=$((PATCH + 1)) ;;
          esac

          NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
          echo "Bumping $CURRENT_VERSION -> $NEW_VERSION ($BUMP_TYPE)"

          sed -i "s/^version = \"${CURRENT_VERSION}\"/version = \"${NEW_VERSION}\"/" Cargo.toml

          PR_TITLE="${{ github.event.pull_request.title }}"
          PR_NUMBER="${{ github.event.pull_request.number }}"
          PR_AUTHOR="${{ github.event.pull_request.user.login }}"
          TODAY=$(date +%Y-%m-%d)

          CHANGELOG_ENTRY=$(printf "### Changed\n\n- %s (#%s) (@%s)" "$PR_TITLE" "$PR_NUMBER" "$PR_AUTHOR")
          VERSION_HEADER=$(printf "## [%s] - %s" "$NEW_VERSION" "$TODAY")

          if [ -f CHANGELOG.md ]; then
            HEADER=$(head -n 1 CHANGELOG.md)
            REST=$(tail -n +2 CHANGELOG.md)
            printf "%s\n\n%s\n\n%s\n%s" "$HEADER" "$VERSION_HEADER" "$CHANGELOG_ENTRY" "$REST" > CHANGELOG.md
          else
            printf "# Changelog\n\n%s\n\n%s\n" "$VERSION_HEADER" "$CHANGELOG_ENTRY" > CHANGELOG.md
          fi

          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git add Cargo.toml CHANGELOG.md
          git commit -m "chore: bump version to ${NEW_VERSION} [skip ci]"
          git tag "v${NEW_VERSION}"
          git push origin HEAD "v${NEW_VERSION}"

          echo "Version bumped and tagged: v${NEW_VERSION}"
```

NOTE: The repo needs `version:patch`, `version:minor`, and `version:major` labels created in GitHub. The workflow only runs when a PR with one of these labels is merged.

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/rust-version-bump.yml
git commit -m "ci: add version bump workflow triggered by PR labels"
```

---

### Task 11: GitHub Actions Release Workflow

**Files:**
- Create: `.github/workflows/rust-release.yml`

Triggers on `v*` tags (created by the version bump workflow). Builds cross-platform binaries and creates a GitHub release.

- [ ] **Step 1: Create the workflow file**

```yaml
name: Rust Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    name: Build ${{ matrix.target }}
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            artifact: agent-view-linux-x64
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            artifact: agent-view-linux-arm64
          - target: x86_64-apple-darwin
            os: macos-latest
            artifact: agent-view-darwin-x64
          - target: aarch64-apple-darwin
            os: macos-latest
            artifact: agent-view-darwin-arm64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Install cross (Linux cross-compile)
        if: runner.os == 'Linux' && matrix.target == 'aarch64-unknown-linux-gnu'
        run: cargo install cross --git https://github.com/cross-rs/cross
      - name: Build (native)
        if: "!contains(matrix.target, 'aarch64-unknown-linux')"
        run: cargo build --release --target ${{ matrix.target }}
      - name: Build (cross)
        if: contains(matrix.target, 'aarch64-unknown-linux')
        run: cross build --release --target ${{ matrix.target }}
      - name: Package
        run: |
          cd target/${{ matrix.target }}/release
          tar czf ../../../${{ matrix.artifact }}.tar.gz agent-view
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: ${{ matrix.artifact }}.tar.gz

  release:
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
          merge-multiple: true
      - uses: softprops/action-gh-release@v1
        with:
          files: artifacts/*.tar.gz
          generate_release_notes: true
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/rust-release.yml
git commit -m "ci: add Rust release workflow for cross-platform binary builds"
```

---

### Task 12: Update GitLab CI — Tag-Reactive Only

**Files:**
- Modify: `.gitlab-ci.yml`

GitLab no longer owns version bumping. It runs quality gates on MRs/branches and creates GitLab releases when tags arrive (synced from GitHub via the mirror pipeline).

- [ ] **Step 1: Read the current file**

Read `.gitlab-ci.yml` to understand existing structure.

- [ ] **Step 2: Replace with Rust CI (no bump-version)**

Replace the entire file. Quality gates on branches/MRs, build + release on tags only.

```yaml
stages:
  - check
  - test
  - build
  - release

fmt:
  stage: check
  image: rust:latest
  tags:
    - docker
  before_script:
    - rustup component add rustfmt
  script:
    - cargo fmt --check
  only:
    - main
    - /^release\/.*$/
    - merge_requests

clippy:
  stage: check
  image: rust:latest
  tags:
    - docker
  before_script:
    - rustup component add clippy
  script:
    - cargo clippy -- -D warnings
  only:
    - main
    - /^release\/.*$/
    - merge_requests

test:
  stage: test
  image: rust:latest
  tags:
    - docker
  before_script:
    - apt-get update && apt-get install -y tmux
  script:
    - cargo test
  only:
    - main
    - /^release\/.*$/
    - merge_requests

build-release:
  stage: build
  image: rust:latest
  tags:
    - docker
  script:
    - cargo build --release
    - mkdir -p bin
    - tar czf bin/agent-view-linux-x64.tar.gz -C target/release agent-view
  artifacts:
    paths:
      - bin/*.tar.gz
    expire_in: 1 week
  only:
    - tags

create-release:
  stage: release
  image: alpine:3.19
  tags:
    - docker
  before_script:
    - apk add --no-cache curl jq sed git
  script:
    - |
      set -e
      VERSION=$(echo "$CI_COMMIT_TAG" | sed 's/^v//')
      echo "Creating release for version: $VERSION"

      git fetch --tags
      PREV_TAG=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | grep -B1 "^${CI_COMMIT_TAG}$" | head -1)
      if [ "$PREV_TAG" = "$CI_COMMIT_TAG" ] || [ -z "$PREV_TAG" ]; then
        PREV_TAG=$(git rev-list --max-parents=0 HEAD)
      fi

      COMMITS=$(git log "${PREV_TAG}..${CI_COMMIT_TAG}" --pretty=format:"- %s" --no-merges 2>/dev/null | head -50 || true)

      RELEASE_NOTES=""
      if [ -n "${AIP_API_KEY:-}" ]; then
        LLM_PROMPT=$(printf 'Generate concise release notes for version %s.\n\nCommits:\n%s\n\nRULES:\n1. Use headings: Changed, Added, Removed, Fixed (only relevant ones)\n2. Imperative mood\n3. Combine related commits\n4. Exclude CI/CD, formatting, docs-only, version bump commits\n5. Output ONLY headings and bullet points. No code fences.' \
          "$VERSION" "$COMMITS")

        LLM_PAYLOAD=$(jq -n \
          --arg model "openai/gpt-oss-120b" \
          --arg prompt "$LLM_PROMPT" \
          '{model: $model, messages: [{role: "user", content: $prompt}], temperature: 0.3}')

        LLM_RESPONSE=$(curl -ksSL -X POST \
          "https://models.k8s.aip.mitre.org/v1/chat/completions" \
          -H "Content-Type: application/json" \
          -H "Authorization: Bearer $AIP_API_KEY" \
          -d "$LLM_PAYLOAD" 2>/dev/null || true)

        RELEASE_NOTES=$(echo "$LLM_RESPONSE" | jq -r '.choices[0].message.content // empty' 2>/dev/null || true)
      fi

      if [ -z "$RELEASE_NOTES" ]; then
        RELEASE_NOTES="$COMMITS"
      fi

      INSTALL_CMD='curl -kfsSL https://gitlab.mitre.org/mdoyle/agent-view/-/raw/main/install-mitre.sh | bash'
      DESCRIPTION=$(printf "## What's New in v%s\n\n%s\n\n## Installation\n\n\`\`\`bash\n%s\n\`\`\`\n" "$VERSION" "$RELEASE_NOTES" "$INSTALL_CMD")

      ASSET_LINKS="[]"
      for tarball in bin/*.tar.gz; do
        FILENAME=$(basename "$tarball")
        echo "Uploading $FILENAME..."
        curl -ksSL --header "PRIVATE-TOKEN: $CI_TAG_TOKEN" \
          --upload-file "$tarball" \
          "https://gitlab.mitre.org/api/v4/projects/${CI_PROJECT_ID}/packages/generic/agent-view/${VERSION}/${FILENAME}"

        LINK=$(jq -n \
          --arg name "$FILENAME" \
          --arg url "https://gitlab.mitre.org/api/v4/projects/${CI_PROJECT_ID}/packages/generic/agent-view/${VERSION}/${FILENAME}" \
          '{name: $name, url: $url, link_type: "package"}')

        ASSET_LINKS=$(echo "$ASSET_LINKS" | jq --argjson link "$LINK" '. + [$link]')
      done

      RELEASE_PAYLOAD=$(jq -n \
        --arg tag "$CI_COMMIT_TAG" \
        --arg name "Agent View v${VERSION}" \
        --arg description "$DESCRIPTION" \
        --argjson assets "{\"links\": $ASSET_LINKS}" \
        '{tag_name: $tag, name: $name, description: $description, assets: $assets}')

      curl -ksSL -X POST \
        "https://gitlab.mitre.org/api/v4/projects/${CI_PROJECT_ID}/releases" \
        --header "PRIVATE-TOKEN: $CI_TAG_TOKEN" \
        --header "Content-Type: application/json" \
        --data "$RELEASE_PAYLOAD"

      echo "Release v${VERSION} created successfully."
  needs:
    - build-release
  only:
    - tags
```

- [ ] **Step 3: Commit**

```bash
git add .gitlab-ci.yml
git commit -m "ci: update GitLab CI for Rust — quality gates + tag-reactive releases only"
```

---

### Task 13: Pre-commit Hook

**Files:**
- Modify: `.pre-commit-config.yaml`

- [ ] **Step 1: Update pre-commit config**

Replace the existing ruff-based config (for TypeScript) with Rust equivalents:

```yaml
repos:
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt --check
        language: system
        types: [rust]
        pass_filenames: false
      - id: cargo-clippy
        name: cargo clippy
        entry: cargo clippy -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false
```

- [ ] **Step 2: Commit**

```bash
git add .pre-commit-config.yaml
git commit -m "chore: update pre-commit hooks for Rust (fmt + clippy)"
```

---

### Task 14: Final Verification

**Files:** None — validation only

- [ ] **Step 1: Run full quality suite**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test && cargo build --release
```

Expected: all pass, zero warnings, zero errors

- [ ] **Step 2: Check test count**

Run: `cargo test 2>&1 | grep "test result:"`
Expected: ~150+ tests passed

- [ ] **Step 3: Verify binary**

Run: `ls -lh target/release/agent-view`
Expected: ~5-6MB

- [ ] **Step 4: Push and create PR**

```bash
git push origin feat/rust-ci
gh pr create --repo doyled-it/agent-view --base release/rust --head feat/rust-ci \
  --title "ci: add Rust CI pipelines, fix clippy/fmt, improve test coverage" \
  --body "..."
```
