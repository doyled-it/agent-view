# Remaining Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement all remaining features from the Rust rewrite design spec: session sorting, sparklines, bulk actions, continuous logging, session pinning, activity feed, token/cost tracking, config hot-reload, improved log export, and group reordering.

**Architecture:** Features are ordered by dependency — schema v4 migration first (pinning + token tracking columns), then independent features in parallel-safe order. Each feature touches 2-4 files max. All new state lives on the `App` struct. The background thread gets logging and token parsing additions.

**Tech Stack:** Rust, ratatui, rusqlite, crossterm, notify crate (new dep for config hot-reload)

---

### Task 1: Schema v4 Migration — Pinned + Tokens Columns

**Files:**
- Modify: `src/core/storage.rs` (migration + CRUD)
- Modify: `src/types.rs` (Session struct)

- [ ] **Step 1: Write failing test for v4 migration**

In `src/core/storage.rs`, add to the `tests` module:

```rust
#[test]
fn test_v4_columns_exist() {
    let (storage, _dir) = test_storage();
    storage
        .conn()
        .execute(
            "INSERT INTO sessions (id, title, project_path, created_at, pinned, tokens_used)
             VALUES ('test', 'Test', '/tmp', 0, 1, 5000)",
            [],
        )
        .unwrap();

    let (pinned, tokens): (i32, i64) = storage
        .conn()
        .query_row(
            "SELECT pinned, tokens_used FROM sessions WHERE id = 'test'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(pinned, 1);
    assert_eq!(tokens, 5000);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_v4_columns_exist -- --nocapture`
Expected: FAIL — `pinned` column does not exist

- [ ] **Step 3: Add pinned and tokens_used to Session struct**

In `src/types.rs`, add two fields to `Session` after `status_history`:

```rust
pub pinned: bool,
pub tokens_used: i64,
```

- [ ] **Step 4: Fix all Session construction sites**

Update `make_test_session` in `src/core/storage.rs` tests:
```rust
pinned: false,
tokens_used: 0,
```

Update `make_test_session` in `src/core/session.rs` tests:
```rust
pinned: false,
tokens_used: 0,
```

Update `make_session` in `src/core/groups.rs` tests:
```rust
pinned: false,
tokens_used: 0,
```

Update `SessionOps::create_session` in `src/core/session.rs`:
```rust
pinned: false,
tokens_used: 0,
```

- [ ] **Step 5: Implement v4 migration in storage**

In `src/core/storage.rs`:

1. Change `SCHEMA_VERSION` to `4`.

2. Add migration block after the v2->v3 block:
```rust
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
```

3. Update `save_session` to include `pinned` and `tokens_used` in the INSERT (add `?24, ?25` and params `session.pinned as i32, session.tokens_used`).

4. Update `load_sessions` and `get_session` to read columns at index 23 and 24:
```rust
pinned: row.get::<_, i32>(23)? == 1,
tokens_used: row.get(24)?,
```

5. Add helper methods:
```rust
pub fn set_pinned(&self, id: &str, pinned: bool) -> SqlResult<()> {
    self.conn.execute(
        "UPDATE sessions SET pinned = ?1 WHERE id = ?2",
        params![pinned as i32, id],
    )?;
    Ok(())
}

pub fn add_tokens(&self, id: &str, tokens: i64) -> SqlResult<()> {
    self.conn.execute(
        "UPDATE sessions SET tokens_used = tokens_used + ?1 WHERE id = ?2",
        params![tokens, id],
    )?;
    Ok(())
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: ALL PASS, including `test_v4_columns_exist` and `test_migrate_sets_schema_version` (now "4")

- [ ] **Step 7: Fix schema version test**

Update `test_migrate_sets_schema_version`:
```rust
assert_eq!(version, Some("4".to_string()));
```

- [ ] **Step 8: Commit**

```bash
git add src/types.rs src/core/storage.rs
git commit -m "feat(storage): add schema v4 migration for pinned and tokens_used columns"
```

---

### Task 2: Session Sorting

**Files:**
- Modify: `src/types.rs` (SortMode enum)
- Modify: `src/app.rs` (sort_mode field)
- Modify: `src/core/groups.rs` (sort within groups)
- Modify: `src/main.rs` (handle `S` key — use Shift+S since `s` is stop)
- Modify: `src/ui/footer.rs` (show sort mode)

- [ ] **Step 1: Write failing test for sort ordering**

In `src/core/groups.rs` tests:

```rust
#[test]
fn test_sort_sessions_by_status_priority() {
    let sessions = vec![
        make_session("s1", "work", SessionStatus::Idle),
        make_session("s2", "work", SessionStatus::Waiting),
        make_session("s3", "work", SessionStatus::Running),
    ];
    let mut sorted = sessions.clone();
    sort_sessions(&mut sorted, SortMode::StatusPriority);
    assert_eq!(sorted[0].id, "s2"); // waiting first
    assert_eq!(sorted[1].id, "s3"); // running second
    assert_eq!(sorted[2].id, "s1"); // idle last
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_sort_sessions_by_status_priority`
Expected: FAIL — `SortMode` not found, `sort_sessions` not found

- [ ] **Step 3: Add SortMode enum to types.rs**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    StatusPriority,
    LastActivity,
    Name,
    Created,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            Self::StatusPriority => Self::LastActivity,
            Self::LastActivity => Self::Name,
            Self::Name => Self::Created,
            Self::Created => Self::StatusPriority,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::StatusPriority => "status",
            Self::LastActivity => "activity",
            Self::Name => "name",
            Self::Created => "created",
        }
    }
}

impl SessionStatus {
    pub fn sort_priority(&self) -> u8 {
        match self {
            Self::Waiting => 0,
            Self::Paused => 1,
            Self::Running => 2,
            Self::Compacting => 3,
            Self::Idle => 4,
            Self::Stopped => 5,
            Self::Error => 6,
        }
    }
}
```

- [ ] **Step 4: Add sort_sessions function to groups.rs**

```rust
use crate::types::SortMode;

pub fn sort_sessions(sessions: &mut [&Session], mode: SortMode) {
    match mode {
        SortMode::StatusPriority => {
            sessions.sort_by_key(|s| s.status.sort_priority());
        }
        SortMode::LastActivity => {
            sessions.sort_by(|a, b| b.status_changed_at.cmp(&a.status_changed_at));
        }
        SortMode::Name => {
            sessions.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        }
        SortMode::Created => {
            sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }
    }
}
```

- [ ] **Step 5: Integrate sorting into flatten_group_tree**

Change `flatten_group_tree` signature to accept `sort_mode: SortMode`:

```rust
pub fn flatten_group_tree(sessions: &[Session], groups: &[Group], sort_mode: SortMode) -> Vec<ListRow> {
```

After `group_sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));`, replace with:
```rust
sort_sessions(group_sessions, sort_mode);
```

Do the same for orphan sessions.

- [ ] **Step 6: Add sort_mode to App struct**

In `src/app.rs`, add to `App`:
```rust
pub sort_mode: crate::types::SortMode,
```

In `App::new`, add:
```rust
sort_mode: crate::types::SortMode::StatusPriority,
```

Update `rebuild_list_rows`:
```rust
self.list_rows = crate::core::groups::flatten_group_tree(&self.sessions, &groups, self.sort_mode);
```

- [ ] **Step 7: Add Shift+S keybinding in main.rs**

In `handle_main_key`, add a new arm (the existing `s` is stop):

```rust
(KeyModifiers::SHIFT, KeyCode::Char('S')) => {
    app.sort_mode = app.sort_mode.next();
    app.rebuild_list_rows();
    let label = app.sort_mode.label();
    app.toast_message = Some(format!("Sort: {}", label));
    app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
}
```

- [ ] **Step 8: Show sort mode in footer**

In `src/ui/footer.rs`, in the `Overlay::None` branch with sessions, add after `("s", "stop")`:
```rust
("S", "sort"),
```

- [ ] **Step 9: Update command palette**

In `src/app.rs`, add to `CommandAction`:
```rust
CycleSort,
```

Add to `CommandPalette::new()` items:
```rust
CommandItem { label: "Cycle Sort Mode".to_string(), key_hint: "S".to_string(), action: CommandAction::CycleSort },
```

Handle in `handle_palette_key` in `main.rs`:
```rust
CommandAction::CycleSort => {
    app.sort_mode = app.sort_mode.next();
    app.rebuild_list_rows();
    app.toast_message = Some(format!("Sort: {}", app.sort_mode.label()));
    app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
}
```

- [ ] **Step 10: Fix existing tests**

Update all `flatten_group_tree` calls in `src/core/groups.rs` tests to pass `SortMode::Created` (preserving existing behavior — newest first):
```rust
let rows = flatten_group_tree(&sessions, &groups, SortMode::Created);
```

- [ ] **Step 11: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 12: Commit**

```bash
git add src/types.rs src/app.rs src/core/groups.rs src/main.rs src/ui/footer.rs
git commit -m "feat(sort): add session sorting with Shift+S to cycle modes"
```

---

### Task 3: Session Pinning

**Files:**
- Modify: `src/main.rs` (handle `p` key)
- Modify: `src/core/groups.rs` (pin sorting)
- Modify: `src/ui/home.rs` (pin indicator)
- Modify: `src/ui/footer.rs` (hint)
- Modify: `src/app.rs` (command palette)

- [ ] **Step 1: Write failing test for pinned sessions floating to top**

In `src/core/groups.rs` tests:

```rust
#[test]
fn test_pinned_sessions_sort_first() {
    let mut s1 = make_session("s1", "work", SessionStatus::Idle);
    let mut s2 = make_session("s2", "work", SessionStatus::Idle);
    s2.pinned = true;
    let groups = vec![make_group("work", "Work", 0)];
    let rows = flatten_group_tree(&[s1, s2], &groups, SortMode::Created);
    // s2 is pinned so should appear first
    if let ListRow::Session(first) = &rows[1] {
        assert_eq!(first.id, "s2");
        assert!(first.pinned);
    } else {
        panic!("Expected session row");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_pinned_sessions_sort_first`
Expected: FAIL — pinned sessions don't sort first yet

- [ ] **Step 3: Update sort_sessions to respect pinning**

In `src/core/groups.rs`, modify `sort_sessions` to put pinned first:

```rust
pub fn sort_sessions(sessions: &mut [&Session], mode: SortMode) {
    sessions.sort_by(|a, b| {
        // Pinned sessions always come first
        match (b.pinned, a.pinned) {
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            _ => {}
        }
        match mode {
            SortMode::StatusPriority => a.status.sort_priority().cmp(&b.status.sort_priority()),
            SortMode::LastActivity => b.status_changed_at.cmp(&a.status_changed_at),
            SortMode::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            SortMode::Created => b.created_at.cmp(&a.created_at),
        }
    });
}
```

- [ ] **Step 4: Add `p` keybinding in main.rs**

In `handle_main_key`:

```rust
(KeyModifiers::NONE, KeyCode::Char('p')) => {
    if let Some(session) = app.selected_session() {
        let new_val = !session.pinned;
        let id = session.id.clone();
        let title = session.title.clone();
        let _ = storage.set_pinned(&id, new_val);
        if let Ok(sessions) = storage.load_sessions() {
            app.sessions = sessions;
            app.groups = storage.load_groups().unwrap_or_default();
            app.rebuild_list_rows();
        }
        let msg = if new_val {
            format!("Pinned: {}", title)
        } else {
            format!("Unpinned: {}", title)
        };
        app.toast_message = Some(msg);
        app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
    }
}
```

- [ ] **Step 5: Add pin indicator in session row rendering**

In `src/ui/home.rs`, in the `ListRow::Session` arm, change the `follow_up_indicator`:

```rust
let pin_indicator = if session.pinned { "^ " } else { "  " };
let follow_up_indicator = if session.follow_up { "F " } else { "  " };
```

Then in the `Line::from` vec, prepend before `follow_up_indicator`:

```rust
Span::styled(pin_indicator, Style::default().fg(theme.accent)),
```

- [ ] **Step 6: Add footer hint and command palette entry**

In `src/ui/footer.rs`, add `("p", "pin")` to the hints.

In `src/app.rs`, add `PinSession` to `CommandAction` and a `CommandItem` for it.

Handle in `handle_palette_key` similarly to the `p` key handler.

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/core/groups.rs src/ui/home.rs src/ui/footer.rs src/app.rs
git commit -m "feat(pin): add session pinning with p key, pinned float to top"
```

---

### Task 4: Group Reordering

**Files:**
- Modify: `src/main.rs` (Ctrl+Up/Down keys)
- Modify: `src/core/storage.rs` (swap_group_order)
- Modify: `src/ui/footer.rs` (hint)

- [ ] **Step 1: Write failing test for group order swapping**

In `src/core/storage.rs` tests:

```rust
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
    assert_eq!(groups[0].path, "personal"); // was order 1, now 0
    assert_eq!(groups[1].path, "work");     // was order 0, now 1
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_swap_group_order`
Expected: FAIL — `swap_group_order` not found

- [ ] **Step 3: Implement swap_group_order**

In `src/core/storage.rs`:

```rust
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
```

- [ ] **Step 4: Add Ctrl+Up/Down keybindings**

In `handle_main_key` in `src/main.rs`:

```rust
(KeyModifiers::CONTROL, KeyCode::Up) => {
    if let Some(group) = app.selected_group() {
        let path = group.path.clone();
        let groups = storage.load_groups().unwrap_or_default();
        if let Some(pos) = groups.iter().position(|g| g.path == path) {
            if pos > 0 {
                let prev_path = groups[pos - 1].path.clone();
                let _ = storage.swap_group_order(&path, &prev_path);
                app.groups = storage.load_groups().unwrap_or_default();
                app.rebuild_list_rows();
                // Move selection up to follow the group
                app.move_selection_up();
                let _ = storage.touch();
            }
        }
    }
}
(KeyModifiers::CONTROL, KeyCode::Down) => {
    if let Some(group) = app.selected_group() {
        let path = group.path.clone();
        let groups = storage.load_groups().unwrap_or_default();
        if let Some(pos) = groups.iter().position(|g| g.path == path) {
            if pos < groups.len() - 1 {
                let next_path = groups[pos + 1].path.clone();
                let _ = storage.swap_group_order(&path, &next_path);
                app.groups = storage.load_groups().unwrap_or_default();
                app.rebuild_list_rows();
                // Move selection down to follow the group
                app.move_selection_down();
                let _ = storage.touch();
            }
        }
    }
}
```

- [ ] **Step 5: Add footer hint**

In `src/ui/footer.rs`, add `("C-↑↓", "reorder group")` to the session hints when a group is selected. This requires passing whether the selection is a group to the footer. For simplicity, just add it to the general hints:

```rust
("C-↑↓", "move group"),
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/core/storage.rs src/ui/footer.rs
git commit -m "feat(groups): add group reordering with Ctrl+Up/Down"
```

---

### Task 5: Status History Sparklines

**Files:**
- Modify: `src/ui/home.rs` (render sparkline in session row)

- [ ] **Step 1: Write failing test for sparkline rendering**

In `src/ui/home.rs` tests:

```rust
use crate::types::StatusHistoryEntry;

#[test]
fn test_sparkline_from_history() {
    let history = vec![
        StatusHistoryEntry { status: "idle".to_string(), timestamp: 1000 },
        StatusHistoryEntry { status: "running".to_string(), timestamp: 2000 },
        StatusHistoryEntry { status: "waiting".to_string(), timestamp: 3000 },
        StatusHistoryEntry { status: "idle".to_string(), timestamp: 4000 },
    ];
    let spark = render_sparkline_str(&history, 4);
    assert_eq!(spark, "▁█▅▁");
}

#[test]
fn test_sparkline_empty_history() {
    let spark = render_sparkline_str(&[], 4);
    assert_eq!(spark, "");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_sparkline_from_history`
Expected: FAIL — `render_sparkline_str` not found

- [ ] **Step 3: Implement sparkline rendering function**

In `src/ui/home.rs`:

```rust
fn status_to_sparkline_char(status: &str) -> char {
    match status {
        "idle" | "stopped" => '▁',
        "running" => '█',
        "waiting" => '▅',
        "paused" | "compacting" => '▃',
        "error" => '▇',
        _ => '▁',
    }
}

fn render_sparkline_str(history: &[crate::types::StatusHistoryEntry], max_width: usize) -> String {
    if history.is_empty() {
        return String::new();
    }
    let start = if history.len() > max_width {
        history.len() - max_width
    } else {
        0
    };
    history[start..]
        .iter()
        .map(|entry| status_to_sparkline_char(&entry.status))
        .collect()
}
```

- [ ] **Step 4: Add sparkline to session row rendering**

In `src/ui/home.rs`, in the `ListRow::Session` arm, after the age span, add:

```rust
let sparkline = render_sparkline_str(&session.status_history, 8);
```

Then add to the `Line::from` vec before the age span:
```rust
Span::styled(
    format!(" {} ", sparkline),
    Style::default().fg(theme.text_muted),
),
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src/ui/home.rs
git commit -m "feat(ui): add status history sparklines to session rows"
```

---

### Task 6: Activity Feed

**Files:**
- Modify: `src/types.rs` (ActivityEvent struct)
- Modify: `src/app.rs` (activity_feed field, show_activity_feed toggle)
- Modify: `src/ui/home.rs` (render activity feed panel)
- Modify: `src/main.rs` (populate feed on status change, `a` key toggle)
- Modify: `src/ui/footer.rs` (hint)

- [ ] **Step 1: Write failing test for activity event formatting**

In `src/types.rs` tests:

```rust
#[test]
fn test_activity_event_display() {
    let event = ActivityEvent {
        session_title: "BIS".to_string(),
        old_status: SessionStatus::Running,
        new_status: SessionStatus::Paused,
        timestamp: chrono::Utc::now().timestamp_millis(),
        message: Some("Asked a question".to_string()),
    };
    let display = event.format_line();
    assert!(display.contains("BIS"));
    assert!(display.contains("paused"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_activity_event_display`
Expected: FAIL — `ActivityEvent` not found

- [ ] **Step 3: Add ActivityEvent to types.rs**

```rust
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub session_title: String,
    pub old_status: SessionStatus,
    pub new_status: SessionStatus,
    pub timestamp: i64,
    pub message: Option<String>,
}

impl ActivityEvent {
    pub fn format_line(&self) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        let ago_ms = now - self.timestamp;
        let ago = if ago_ms < 60_000 {
            "just now".to_string()
        } else if ago_ms < 3_600_000 {
            format!("{}m ago", ago_ms / 60_000)
        } else {
            format!("{}h ago", ago_ms / 3_600_000)
        };

        match &self.message {
            Some(msg) => format!("{:<10} {} -> {} \"{}\"", ago, self.session_title, self.new_status.as_str(), msg),
            None => format!("{:<10} {} -> {}", ago, self.session_title, self.new_status.as_str()),
        }
    }
}
```

- [ ] **Step 4: Add activity_feed to App struct**

In `src/app.rs`:

```rust
use std::collections::VecDeque;
```

Add to `App`:
```rust
pub activity_feed: VecDeque<crate::types::ActivityEvent>,
pub show_activity_feed: bool,
```

In `App::new`:
```rust
activity_feed: VecDeque::new(),
show_activity_feed: true,
```

Add method:
```rust
pub fn push_activity(&mut self, event: crate::types::ActivityEvent) {
    self.activity_feed.push_front(event);
    if self.activity_feed.len() > 100 {
        self.activity_feed.pop_back();
    }
}
```

- [ ] **Step 5: Populate activity feed on storage reload**

In `src/main.rs`, in the storage polling block (where `current_mtime != app.last_storage_mtime`), before reloading sessions, diff the old and new statuses:

```rust
{
    let current_mtime = storage.last_modified();
    if current_mtime != app.last_storage_mtime {
        app.last_storage_mtime = current_mtime;
        let new_sessions = storage.load_sessions().unwrap_or_default();

        // Diff statuses for activity feed
        for new_s in &new_sessions {
            if let Some(old_s) = app.sessions.iter().find(|s| s.id == new_s.id) {
                if old_s.status != new_s.status {
                    app.push_activity(crate::types::ActivityEvent {
                        session_title: new_s.title.clone(),
                        old_status: old_s.status,
                        new_status: new_s.status,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        message: None,
                    });
                }
            }
        }

        app.sessions = new_sessions;
        app.groups = storage.load_groups().unwrap_or_default();
        app.rebuild_list_rows();
    }
}
```

- [ ] **Step 6: Render activity feed panel**

In `src/ui/home.rs`, modify the layout in `render()` to include an optional activity area. Change the vertical layout:

```rust
let show_feed = app.show_activity_feed && !app.activity_feed.is_empty();
let feed_height = if show_feed { 4 } else { 0 };

let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(1),           // header
        Constraint::Min(0),              // session list
        Constraint::Length(feed_height), // activity feed
        Constraint::Length(1),           // footer
    ])
    .split(list_area);
```

Then render the feed before the footer:

```rust
if show_feed {
    render_activity_feed(frame, chunks[2], app);
}
```

Adjust footer rendering to use `chunks[3]` instead of `chunks[2]`.

Add the render function:

```rust
fn render_activity_feed(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .title(" Activity ")
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = app
        .activity_feed
        .iter()
        .take(inner.height as usize)
        .map(|event| {
            let status_color = crate::ui::theme::status_color(theme, event.new_status);
            Line::from(vec![
                Span::styled(
                    format_activity_age(event.timestamp),
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled(
                    format!(" {} ", event.session_title),
                    Style::default().fg(theme.text),
                ),
                Span::styled("-> ", Style::default().fg(theme.text_muted)),
                Span::styled(
                    event.new_status.as_str(),
                    Style::default().fg(status_color),
                ),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn format_activity_age(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let ago_ms = now - timestamp;
    if ago_ms < 60_000 {
        " <1m ".to_string()
    } else if ago_ms < 3_600_000 {
        format!(" {}m  ", ago_ms / 60_000)
    } else {
        format!(" {}h  ", ago_ms / 3_600_000)
    }
}
```

- [ ] **Step 7: Add `a` key toggle**

In `handle_main_key` in `src/main.rs`:

```rust
(KeyModifiers::NONE, KeyCode::Char('a')) => {
    app.show_activity_feed = !app.show_activity_feed;
}
```

Add `("a", "activity")` to the footer hints.

- [ ] **Step 8: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 9: Commit**

```bash
git add src/types.rs src/app.rs src/ui/home.rs src/main.rs src/ui/footer.rs
git commit -m "feat(activity): add collapsible activity feed showing status transitions"
```

---

### Task 7: Bulk Actions

**Files:**
- Modify: `src/app.rs` (selected_sessions set)
- Modify: `src/main.rs` (space, Ctrl+A, bulk handlers)
- Modify: `src/ui/home.rs` (selection highlight)
- Modify: `src/ui/footer.rs` (bulk hints)

- [ ] **Step 1: Write test for bulk selection state**

In `src/app.rs` tests (create a new test module if needed):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_bulk_selection() {
        let mut app = App::new(false);
        app.toggle_bulk_select("s1");
        assert!(app.bulk_selected.contains("s1"));
        app.toggle_bulk_select("s1");
        assert!(!app.bulk_selected.contains("s1"));
    }

    #[test]
    fn test_clear_bulk_selection() {
        let mut app = App::new(false);
        app.toggle_bulk_select("s1");
        app.toggle_bulk_select("s2");
        app.clear_bulk_selection();
        assert!(app.bulk_selected.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_toggle_bulk_selection`
Expected: FAIL — `bulk_selected` not found

- [ ] **Step 3: Add bulk selection to App struct**

In `src/app.rs`:

```rust
use std::collections::HashSet;
```

Add to `App`:
```rust
pub bulk_selected: HashSet<String>,
```

In `App::new`:
```rust
bulk_selected: HashSet::new(),
```

Add methods:
```rust
pub fn toggle_bulk_select(&mut self, session_id: &str) {
    if self.bulk_selected.contains(session_id) {
        self.bulk_selected.remove(session_id);
    } else {
        self.bulk_selected.insert(session_id.to_string());
    }
}

pub fn clear_bulk_selection(&mut self) {
    self.bulk_selected.clear();
}

pub fn select_all_visible(&mut self) {
    for row in &self.list_rows {
        if let crate::core::groups::ListRow::Session(s) = row {
            self.bulk_selected.insert(s.id.clone());
        }
    }
}
```

- [ ] **Step 4: Add keybindings**

In `handle_main_key` in `src/main.rs`:

```rust
(KeyModifiers::NONE, KeyCode::Char(' ')) => {
    if let Some(session) = app.selected_session() {
        let id = session.id.clone();
        app.toggle_bulk_select(&id);
    }
}
(KeyModifiers::CONTROL, KeyCode::Char('a')) => {
    app.select_all_visible();
}
```

Also in the `Esc` handling (when no search is active and no overlay), clear bulk selection:
Add a new arm before the wildcard:
```rust
(KeyModifiers::NONE, KeyCode::Esc) => {
    if !app.bulk_selected.is_empty() {
        app.clear_bulk_selection();
    }
}
```

- [ ] **Step 5: Bulk action handlers for d, m, s**

In `handle_main_key`, modify the `d`, `m`, and `s` handlers to check for bulk mode:

For `d` (delete):
```rust
(KeyModifiers::NONE, KeyCode::Char('d')) => {
    if !app.bulk_selected.is_empty() {
        let count = app.bulk_selected.len();
        app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
            message: format!("Delete {} selected sessions?", count),
            action: crate::app::ConfirmAction::BulkDelete,
        });
    } else if let Some(session) = app.selected_session() {
        let msg = format!("Delete session \"{}\"?", session.title);
        app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
            message: msg,
            action: crate::app::ConfirmAction::DeleteSession(session.id.clone()),
        });
    }
}
```

Similarly for `s` (stop):
```rust
(KeyModifiers::NONE, KeyCode::Char('s')) => {
    if !app.bulk_selected.is_empty() {
        let count = app.bulk_selected.len();
        app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
            message: format!("Stop {} selected sessions?", count),
            action: crate::app::ConfirmAction::BulkStop,
        });
    } else if let Some(session) = app.selected_session() {
        if session.status != crate::types::SessionStatus::Stopped {
            let msg = format!("Stop session \"{}\"?", session.title);
            app.overlay = crate::app::Overlay::Confirm(crate::app::ConfirmDialog {
                message: msg,
                action: crate::app::ConfirmAction::StopSession(session.id.clone()),
            });
        }
    }
}
```

- [ ] **Step 6: Add BulkDelete and BulkStop to ConfirmAction**

In `src/app.rs`:
```rust
pub enum ConfirmAction {
    DeleteSession(String),
    StopSession(String),
    BulkDelete,
    BulkStop,
}
```

- [ ] **Step 7: Handle bulk confirm actions**

In `handle_confirm_key` in `src/main.rs`, add handling for `BulkDelete` and `BulkStop`:

```rust
crate::app::ConfirmAction::BulkDelete => {
    let ids: Vec<String> = app.bulk_selected.iter().cloned().collect();
    let mut cache = crate::core::tmux::SessionCache::new();
    for id in &ids {
        let _ = session_ops.delete_session(storage, &mut cache, id);
    }
    app.clear_bulk_selection();
    if let Ok(sessions) = storage.load_sessions() {
        app.sessions = sessions;
        app.groups = storage.load_groups().unwrap_or_default();
        app.rebuild_list_rows();
    }
}
crate::app::ConfirmAction::BulkStop => {
    let ids: Vec<String> = app.bulk_selected.iter().cloned().collect();
    for id in &ids {
        let _ = session_ops.stop_session(storage, id);
    }
    app.clear_bulk_selection();
    if let Ok(sessions) = storage.load_sessions() {
        app.sessions = sessions;
        app.groups = storage.load_groups().unwrap_or_default();
        app.rebuild_list_rows();
    }
}
```

- [ ] **Step 8: Visual indicator for selected sessions**

In `src/ui/home.rs`, in the `ListRow::Session` arm:

```rust
let is_bulk_selected = app.bulk_selected.contains(&session.id);
```

Change the background:
```rust
let bg = if is_selected {
    theme.background_element
} else if is_bulk_selected {
    theme.secondary
} else {
    theme.background
};
```

- [ ] **Step 9: Update footer for bulk mode**

In `src/ui/footer.rs`, when bulk selection is active, show different hints:

```rust
Overlay::None => {
    if !app.bulk_selected.is_empty() {
        let count = app.bulk_selected.len();
        vec![
            ("Space", "toggle"),
            ("d", "delete all"),
            ("s", "stop all"),
            ("Esc", "clear"),
            ("C-a", "select all"),
        ]
    } else if app.sessions.is_empty() {
```

- [ ] **Step 10: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 11: Commit**

```bash
git add src/app.rs src/main.rs src/ui/home.rs src/ui/footer.rs
git commit -m "feat(bulk): add bulk selection with space/Ctrl+A and bulk delete/stop"
```

---

### Task 8: Continuous Session Logging

**Files:**
- Create: `src/core/logger.rs`
- Modify: `src/core/mod.rs` (add logger module)
- Modify: `src/main.rs` (spawn logger thread)

- [ ] **Step 1: Write failing test for log file writing**

Create `src/core/logger.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_append_to_log() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("test.log");
        append_to_log(&log_path, "hello world\n").unwrap();
        append_to_log(&log_path, "second line\n").unwrap();
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("hello world"));
        assert!(content.contains("second line"));
    }

    #[test]
    fn test_rotate_log() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("test.log");
        // Write >10MB of data
        let chunk = "x".repeat(1024 * 1024); // 1MB
        for _ in 0..11 {
            append_to_log(&log_path, &chunk).unwrap();
        }
        rotate_if_needed(&log_path, 10 * 1024 * 1024).unwrap();
        // Original should be gone or small, .1 should exist
        assert!(dir.path().join("test.log.1").exists());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_append_to_log`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement logger module**

In `src/core/logger.rs`:

```rust
//! Continuous session logging — streams tmux pane output to log files

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024; // 10MB
const MAX_ROTATIONS: usize = 2;

pub fn log_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    home.join(".agent-view").join("logs")
}

pub fn session_log_path(session_id: &str) -> PathBuf {
    log_dir().join(format!("{}.log", session_id))
}

pub fn append_to_log(path: &Path, content: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

pub fn rotate_if_needed(path: &Path, max_size: u64) -> Result<(), std::io::Error> {
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()), // file doesn't exist, nothing to rotate
    };

    if metadata.len() <= max_size {
        return Ok(());
    }

    // Delete oldest rotation
    let oldest = format!("{}.{}", path.display(), MAX_ROTATIONS);
    let _ = fs::remove_file(&oldest);

    // Shift rotations down
    for i in (1..MAX_ROTATIONS).rev() {
        let from = format!("{}.{}", path.display(), i);
        let to = format!("{}.{}", path.display(), i + 1);
        let _ = fs::rename(&from, &to);
    }

    // Rotate current
    let rotated = format!("{}.1", path.display());
    fs::rename(path, &rotated)?;

    Ok(())
}

/// Captures output from all active sessions and appends to their log files.
/// Tracks last captured line count to avoid duplicating content.
pub struct SessionLogger {
    last_line_counts: HashMap<String, usize>,
}

impl SessionLogger {
    pub fn new() -> Self {
        Self {
            last_line_counts: HashMap::new(),
        }
    }

    pub fn capture_and_log(&mut self, tmux_session: &str, session_id: &str) {
        let output = match crate::core::tmux::capture_pane(tmux_session, Some(-10000)) {
            Ok(o) => o,
            Err(_) => return,
        };

        let lines: Vec<&str> = output.lines().collect();
        let total_lines = lines.len();
        let last_count = self.last_line_counts.get(session_id).copied().unwrap_or(0);

        if total_lines <= last_count {
            return; // no new content
        }

        let new_lines = &lines[last_count..];
        let new_content = new_lines.join("\n") + "\n";

        let log_path = session_log_path(session_id);
        let _ = append_to_log(&log_path, &new_content);
        let _ = rotate_if_needed(&log_path, MAX_LOG_SIZE);

        self.last_line_counts.insert(session_id.to_string(), total_lines);
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.last_line_counts.remove(session_id);
    }
}
```

- [ ] **Step 4: Register module**

In `src/core/mod.rs`, add:
```rust
pub mod logger;
```

- [ ] **Step 5: Integrate into background thread**

In `src/main.rs`, in the background thread, add a logger and tick counter:

Before the loop:
```rust
let mut logger = crate::core::logger::SessionLogger::new();
let mut log_tick: u32 = 0;
```

Inside the loop, after status processing (after `if any_changed`):
```rust
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
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add src/core/logger.rs src/core/mod.rs src/main.rs
git commit -m "feat(logging): add continuous session logging with rotation"
```

---

### Task 9: Improved Log Export

**Files:**
- Modify: `src/main.rs` (update export_session_log to prefer log files)

- [ ] **Step 1: Write test for log file existence check**

In `src/core/logger.rs` tests:

```rust
#[test]
fn test_session_log_path_format() {
    let path = session_log_path("abc-123");
    assert!(path.to_string_lossy().contains("abc-123.log"));
    assert!(path.to_string_lossy().contains(".agent-view/logs"));
}
```

- [ ] **Step 2: Update export_session_log to use log files first**

In `src/main.rs`, replace `export_session_log`:

```rust
fn export_session_log(tmux_session: &str, title: &str, session_id: &str) -> Result<String, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let export_dir = home.join(".agent-view").join("exports");
    std::fs::create_dir_all(&export_dir).map_err(|e| format!("Cannot create exports dir: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe_name: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .take(30)
        .collect();
    let filename = format!("{}-{}.log", safe_name, timestamp);
    let filepath = export_dir.join(&filename);

    // Try continuous log file first
    let log_path = crate::core::logger::session_log_path(session_id);
    if log_path.exists() {
        std::fs::copy(&log_path, &filepath)
            .map_err(|e| format!("Copy failed: {}", e))?;
        return Ok(filepath.to_string_lossy().to_string());
    }

    // Fallback to live capture
    let output = crate::core::tmux::capture_pane(tmux_session, Some(-10000))
        .map_err(|e| format!("Capture failed: {}", e))?;
    std::fs::write(&filepath, &output)
        .map_err(|e| format!("Write failed: {}", e))?;

    Ok(filepath.to_string_lossy().to_string())
}
```

- [ ] **Step 3: Update all call sites**

There are two call sites for `export_session_log` — one in `handle_main_key` (the `e` key) and one in `handle_palette_key` (`CommandAction::ExportLog`). Both need the session_id added.

In `handle_main_key`:
```rust
(KeyModifiers::NONE, KeyCode::Char('e')) => {
    if let Some(session) = app.selected_session() {
        if !session.tmux_session.is_empty() {
            let tmux_name = session.tmux_session.clone();
            let title = session.title.clone();
            let id = session.id.clone();
            match export_session_log(&tmux_name, &title, &id) {
```

In `handle_palette_key`, `CommandAction::ExportLog`:
```rust
CommandAction::ExportLog => {
    if let Some(session) = app.selected_session() {
        if !session.tmux_session.is_empty() {
            let tmux_name = session.tmux_session.clone();
            let title = session.title.clone();
            let id = session.id.clone();
            match export_session_log(&tmux_name, &title, &id) {
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/core/logger.rs
git commit -m "feat(export): prefer continuous log files for export, fallback to live capture"
```

---

### Task 10: Token/Cost Tracking

**Files:**
- Create: `src/core/tokens.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/main.rs` (parse tokens in background)
- Modify: `src/ui/detail.rs` (show token count)

- [ ] **Step 1: Write failing test for token parsing**

Create `src/core/tokens.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_token_count_k() {
        assert_eq!(parse_token_count("↓ 20.4k tokens"), Some(20400));
    }

    #[test]
    fn test_parse_token_count_m() {
        assert_eq!(parse_token_count("... 1.2M tokens"), Some(1200000));
    }

    #[test]
    fn test_parse_token_count_plain() {
        assert_eq!(parse_token_count("500 tokens"), Some(500));
    }

    #[test]
    fn test_parse_no_tokens() {
        assert_eq!(parse_token_count("hello world"), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_parse_token_count_k`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement token parser**

In `src/core/tokens.rs`:

```rust
//! Token counting from Claude Code output

use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref TOKEN_PATTERN: Regex =
        Regex::new(r"(\d+(?:\.\d+)?)\s*([kKmM])?\s*tokens?").unwrap();
}

pub fn parse_token_count(text: &str) -> Option<i64> {
    let caps = TOKEN_PATTERN.captures(text)?;
    let num: f64 = caps.get(1)?.as_str().parse().ok()?;
    let multiplier = match caps.get(2).map(|m| m.as_str()) {
        Some("k") | Some("K") => 1_000.0,
        Some("m") | Some("M") => 1_000_000.0,
        _ => 1.0,
    };
    Some((num * multiplier) as i64)
}

/// Parse the last occurrence of a token count from pane output.
/// Scans the last N lines for the most recent token mention.
pub fn extract_latest_tokens(output: &str) -> Option<i64> {
    output
        .lines()
        .rev()
        .take(50)
        .filter_map(|line| parse_token_count(line))
        .next()
}

/// Format a token count for display: "45.2k", "1.2M", etc.
pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}
```

- [ ] **Step 4: Register module**

In `src/core/mod.rs`, add:
```rust
pub mod tokens;
```

- [ ] **Step 5: Add token tracking to background thread**

In `src/main.rs`, inside the background thread loop, after the log capture block, add token parsing for Claude sessions:

```rust
// Parse tokens from Claude sessions every 10 ticks
if log_tick == 0 {
    for session in &sessions {
        if session.tool == crate::types::Tool::Claude
            && !session.tmux_session.is_empty()
            && session.status != crate::types::SessionStatus::Stopped
        {
            if let Ok(output) = crate::core::tmux::capture_pane(&session.tmux_session, Some(-50)) {
                if let Some(tokens) = crate::core::tokens::extract_latest_tokens(&output) {
                    if tokens > session.tokens_used {
                        let diff = tokens - session.tokens_used;
                        if diff > 0 {
                            let _ = bg_storage.add_tokens(&session.id, diff);
                        }
                    }
                }
            }
        }
    }
}
```

Note: This reuses the `log_tick == 0` check from the logger (every 5s).

- [ ] **Step 6: Show tokens in detail panel**

In `src/ui/detail.rs`, after the restart count display:

```rust
if session.tokens_used > 0 {
    lines.push(Line::from(vec![
        Span::styled("Tokens: ", Style::default().fg(theme.text_muted)),
        Span::styled(
            crate::core::tokens::format_tokens(session.tokens_used),
            Style::default().fg(theme.text),
        ),
    ]));
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 8: Commit**

```bash
git add src/core/tokens.rs src/core/mod.rs src/main.rs src/ui/detail.rs
git commit -m "feat(tokens): add token usage tracking and display for Claude sessions"
```

---

### Task 11: Config Hot-Reload

**Files:**
- Modify: `Cargo.toml` (add notify crate)
- Modify: `src/core/config.rs` (watcher)
- Modify: `src/main.rs` (spawn watcher, handle reload)
- Modify: `src/app.rs` (config_changed flag)

- [ ] **Step 1: Add notify dependency**

In `Cargo.toml`, add:
```toml
notify = "7"
```

- [ ] **Step 2: Write test for config reload detection**

In `src/core/config.rs` tests:

```rust
#[test]
fn test_config_reload_detects_changes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path, r#"{ "theme": "dark" }"#).unwrap();

    let config1 = load_config_from_path(&path);
    assert_eq!(config1.theme, "dark");

    fs::write(&path, r#"{ "theme": "light" }"#).unwrap();
    let config2 = load_config_from_path(&path);
    assert_eq!(config2.theme, "light");
}
```

- [ ] **Step 3: Add load_config_from_path helper**

In `src/core/config.rs`:

```rust
pub fn load_config_from_path(path: &std::path::Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => config,
            Err(_) => AppConfig::default(),
        },
        Err(_) => AppConfig::default(),
    }
}
```

Update `load_config` to use it:
```rust
pub fn load_config() -> AppConfig {
    load_config_from_path(&config_path())
}
```

- [ ] **Step 4: Add config_changed flag to App**

In `src/app.rs`, add:
```rust
pub config_changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
```

In `App::new`:
```rust
config_changed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
```

- [ ] **Step 5: Spawn config file watcher**

In `src/main.rs`, after loading config and before `run_tui`:

```rust
let config_changed = app.config_changed.clone();
let _config_watcher = {
    use notify::{Watcher, RecursiveMode, Event, EventKind};
    let config_path = crate::core::config::config_path();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                config_changed.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }).ok();
    if let Some(ref mut w) = watcher {
        let dir = crate::core::config::config_dir();
        let _ = w.watch(&dir, RecursiveMode::NonRecursive);
    }
    watcher
};
```

- [ ] **Step 6: Handle config reload in event loop**

In `src/main.rs`, in the main loop, after toast clearing:

```rust
// Check for config hot-reload
if app.config_changed.load(std::sync::atomic::Ordering::Relaxed) {
    app.config_changed.store(false, std::sync::atomic::Ordering::Relaxed);
    let new_config = crate::core::config::load_config();
    // Apply theme change
    if new_config.theme == "light" {
        app.theme = crate::ui::theme::Theme::light();
    } else {
        app.theme = crate::ui::theme::Theme::dark();
    }
    app.toast_message = Some("Config reloaded".to_string());
    app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 8: Verify build**

Run: `cargo build`
Expected: SUCCESS (notify crate compiles)

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/core/config.rs src/main.rs src/app.rs
git commit -m "feat(config): add config.json hot-reload with notify crate"
```

---

### Task 12: Final Integration Test

**Files:** None new — validation only

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 2: Build release binary**

Run: `cargo build --release`
Expected: SUCCESS

- [ ] **Step 3: Check binary size**

Run: `ls -lh target/release/agent-view`
Expected: ~5-6MB static binary

- [ ] **Step 4: Manual smoke test**

Run: `./target/release/agent-view`

Verify:
- Session list renders with sparklines
- `S` cycles sort modes (toast shows mode)
- `p` pins/unpins a session (pin indicator shows)
- `Space` selects sessions, `Esc` clears
- Activity feed shows status transitions at bottom
- `a` toggles activity feed
- Ctrl+Up/Down reorders groups
- Detail panel shows token count for Claude sessions
- Edit `~/.agent-view/config.json` and see "Config reloaded" toast

- [ ] **Step 5: Commit any final fixes**

If any issues found during smoke testing, fix and commit individually.
