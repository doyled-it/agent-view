# Rust Rewrite Phase 2: Feature Parity

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reach feature parity with the TypeScript version: groups with expand/collapse, detail panel, dark/light Catppuccin themes, session search, log export, rename/move/group overlays, command palette, git worktree support, follow-up mark toggle, and notification toggle wiring.

**Architecture:** Extends the existing `App` struct and `Overlay` enum. Group flattening produces a `Vec<ListRow>` that interleaves group headers and sessions for unified cursor navigation. A new `Theme` struct drives all colors. Each new overlay variant gets its own form struct and key handler. All tasks produce a compiling, runnable binary.

**Build/test commands:**
```bash
cargo build   # Must pass after every task
cargo test    # Run after tasks that add tests
```

---

### Task 1: Add group CRUD to Storage

**Files:** `src/core/storage.rs`

- [ ] **Step 1: Add `load_groups` method**

```rust
// In impl Storage, after the existing methods:

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
```

- [ ] **Step 2: Add `save_group` method**

```rust
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
```

- [ ] **Step 3: Add `delete_group` method**

```rust
/// Delete a group by path
pub fn delete_group(&self, path: &str) -> SqlResult<()> {
    self.conn
        .execute("DELETE FROM groups WHERE path = ?1", params![path])?;
    Ok(())
}
```

- [ ] **Step 4: Add `toggle_group_expanded` method**

```rust
/// Toggle the expanded state of a group
pub fn toggle_group_expanded(&self, path: &str) -> SqlResult<()> {
    self.conn.execute(
        "UPDATE groups SET expanded = CASE WHEN expanded = 1 THEN 0 ELSE 1 END WHERE path = ?1",
        params![path],
    )?;
    Ok(())
}
```

- [ ] **Step 5: Add `rename_session` method**

```rust
/// Rename a session
pub fn rename_session(&self, id: &str, new_title: &str) -> SqlResult<()> {
    self.conn.execute(
        "UPDATE sessions SET title = ?1 WHERE id = ?2",
        params![new_title, id],
    )?;
    Ok(())
}
```

- [ ] **Step 6: Add `move_session_to_group` method**

```rust
/// Move a session to a different group
pub fn move_session_to_group(&self, id: &str, group_path: &str) -> SqlResult<()> {
    self.conn.execute(
        "UPDATE sessions SET group_path = ?1 WHERE id = ?2",
        params![group_path, id],
    )?;
    Ok(())
}
```

- [ ] **Step 7: Add tests for all new storage methods**

```rust
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
```

**Verify:** `cargo test -- storage`

---

### Task 2: Group flattening logic

**Files:** Create `src/core/groups.rs`, modify `src/core/mod.rs`

- [ ] **Step 1: Create `src/core/groups.rs` with types and constants**

```rust
//! Group flattening logic — converts groups + sessions into a navigable list

use crate::types::{Group, Session};
use std::collections::HashMap;

pub const DEFAULT_GROUP_PATH: &str = "my-sessions";
pub const DEFAULT_GROUP_NAME: &str = "Ungrouped";

/// A row in the flattened list — either a group header or a session
#[derive(Debug, Clone)]
pub enum ListRow {
    Group {
        group: Group,
        session_count: usize,
        running_count: usize,
        waiting_count: usize,
    },
    Session(Session),
}
```

- [ ] **Step 2: Add `ensure_default_group` function**

```rust
/// Ensure the default "Ungrouped" group exists in the list.
/// Returns the groups with the default group inserted if missing.
pub fn ensure_default_group(groups: &[Group]) -> Vec<Group> {
    if groups.iter().any(|g| g.path == DEFAULT_GROUP_PATH) {
        return groups.to_vec();
    }

    let default = Group {
        path: DEFAULT_GROUP_PATH.to_string(),
        name: DEFAULT_GROUP_NAME.to_string(),
        expanded: true,
        order: 0,
        default_path: String::new(),
    };

    let mut result = vec![default];
    for g in groups {
        let mut g = g.clone();
        g.order += 1;
        result.push(g);
    }
    result
}
```

- [ ] **Step 3: Add `flatten_group_tree` function**

```rust
/// Flatten groups and sessions into a navigable list.
/// Groups appear as headers; if expanded, their sessions follow.
/// Orphan sessions (in groups that don't exist) get an implicit group.
pub fn flatten_group_tree(sessions: &[Session], groups: &[Group]) -> Vec<ListRow> {
    let mut result = Vec::new();

    let mut sorted_groups = groups.to_vec();
    sorted_groups.sort_by_key(|g| g.order);

    // Build map: group_path -> Vec<Session>
    let mut by_group: HashMap<String, Vec<&Session>> = HashMap::new();
    for session in sessions {
        let path = if session.group_path.is_empty() {
            DEFAULT_GROUP_PATH.to_string()
        } else {
            session.group_path.clone()
        };
        by_group.entry(path).or_default().push(session);
    }

    // Sort sessions within each group by created_at descending
    for group_sessions in by_group.values_mut() {
        group_sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }

    let known_paths: std::collections::HashSet<&str> =
        sorted_groups.iter().map(|g| g.path.as_str()).collect();

    for group in &sorted_groups {
        let group_sessions = by_group.get(&group.path).map(|v| v.as_slice()).unwrap_or(&[]);

        // Hide default group when empty
        if group.path == DEFAULT_GROUP_PATH && group_sessions.is_empty() {
            continue;
        }

        let running = group_sessions.iter().filter(|s| s.status == crate::types::SessionStatus::Running).count();
        let waiting = group_sessions.iter().filter(|s| s.status == crate::types::SessionStatus::Waiting).count();

        result.push(ListRow::Group {
            group: group.clone(),
            session_count: group_sessions.len(),
            running_count: running,
            waiting_count: waiting,
        });

        if group.expanded {
            for session in group_sessions {
                result.push(ListRow::Session((*session).clone()));
            }
        }
    }

    // Orphan sessions in unknown groups
    for (path, orphans) in &by_group {
        if known_paths.contains(path.as_str()) {
            continue;
        }
        let running = orphans.iter().filter(|s| s.status == crate::types::SessionStatus::Running).count();
        let waiting = orphans.iter().filter(|s| s.status == crate::types::SessionStatus::Waiting).count();

        result.push(ListRow::Group {
            group: Group {
                path: path.clone(),
                name: path.clone(),
                expanded: true,
                order: 999,
                default_path: String::new(),
            },
            session_count: orphans.len(),
            running_count: running,
            waiting_count: waiting,
        });

        for session in orphans {
            result.push(ListRow::Session((*session).clone()));
        }
    }

    result
}
```

- [ ] **Step 4: Register the module in `src/core/mod.rs`**

Add `pub mod groups;` to `src/core/mod.rs`.

- [ ] **Step 5: Add tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SessionStatus, Tool};

    fn make_session(id: &str, group: &str, status: SessionStatus) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp".to_string(),
            group_path: group.to_string(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status,
            tmux_session: String::new(),
            created_at: 1700000000000,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: 0,
            restart_count: 0,
            status_history: vec![],
        }
    }

    fn make_group(path: &str, name: &str, order: i32) -> Group {
        Group {
            path: path.to_string(),
            name: name.to_string(),
            expanded: true,
            order,
            default_path: String::new(),
        }
    }

    #[test]
    fn test_ensure_default_group_adds_when_missing() {
        let groups = vec![make_group("work", "Work", 0)];
        let result = ensure_default_group(&groups);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, DEFAULT_GROUP_PATH);
    }

    #[test]
    fn test_ensure_default_group_noop_when_present() {
        let groups = vec![make_group(DEFAULT_GROUP_PATH, DEFAULT_GROUP_NAME, 0)];
        let result = ensure_default_group(&groups);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_flatten_basic() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![
            make_session("s1", "work", SessionStatus::Running),
            make_session("s2", "work", SessionStatus::Idle),
        ];
        let rows = flatten_group_tree(&sessions, &groups);
        assert_eq!(rows.len(), 3); // 1 group header + 2 sessions
        assert!(matches!(rows[0], ListRow::Group { .. }));
        assert!(matches!(rows[1], ListRow::Session(_)));
    }

    #[test]
    fn test_flatten_collapsed_group_hides_sessions() {
        let mut group = make_group("work", "Work", 0);
        group.expanded = false;
        let sessions = vec![make_session("s1", "work", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &[group]);
        assert_eq!(rows.len(), 1); // only group header
    }

    #[test]
    fn test_flatten_orphan_sessions_get_implicit_group() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![make_session("s1", "unknown", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &groups);
        // work group (empty, but it's not default so still shows) + unknown group + session
        assert!(rows.len() >= 2);
    }

    #[test]
    fn test_flatten_empty_default_group_hidden() {
        let groups = vec![
            make_group(DEFAULT_GROUP_PATH, DEFAULT_GROUP_NAME, 0),
            make_group("work", "Work", 1),
        ];
        let sessions = vec![make_session("s1", "work", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &groups);
        // Default group hidden (empty), work group + session
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_flatten_counts_statuses() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![
            make_session("s1", "work", SessionStatus::Running),
            make_session("s2", "work", SessionStatus::Waiting),
            make_session("s3", "work", SessionStatus::Idle),
        ];
        let rows = flatten_group_tree(&sessions, &groups);
        if let ListRow::Group { running_count, waiting_count, session_count, .. } = &rows[0] {
            assert_eq!(*running_count, 1);
            assert_eq!(*waiting_count, 1);
            assert_eq!(*session_count, 3);
        } else {
            panic!("Expected group row");
        }
    }
}
```

**Verify:** `cargo test -- groups`

---

### Task 3: Theme system

**Files:** Create `src/ui/theme.rs`, modify `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/theme.rs` with the Theme struct and Catppuccin palettes**

```rust
//! Dark/light theme definitions — Catppuccin Mocha (dark) and Latte (light)

use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub text: Color,
    pub text_muted: Color,
    pub selected_item_text: Color,
    pub background: Color,
    pub background_panel: Color,
    pub background_element: Color,
    pub border: Color,
    pub border_active: Color,
    pub border_subtle: Color,
}

/// Catppuccin Mocha (dark theme)
pub fn dark() -> Theme {
    Theme {
        primary: Color::Rgb(203, 166, 247),     // #cba6f7
        secondary: Color::Rgb(137, 180, 250),   // #89b4fa
        accent: Color::Rgb(245, 194, 231),      // #f5c2e7
        error: Color::Rgb(243, 139, 168),       // #f38ba8
        warning: Color::Rgb(250, 179, 135),     // #fab387
        success: Color::Rgb(166, 227, 161),     // #a6e3a1
        info: Color::Rgb(116, 199, 236),        // #74c7ec
        text: Color::Rgb(205, 214, 244),        // #cdd6f4
        text_muted: Color::Rgb(108, 112, 134),  // #6c7086
        selected_item_text: Color::Rgb(30, 30, 46),  // #1e1e2e
        background: Color::Rgb(30, 30, 46),     // #1e1e2e
        background_panel: Color::Rgb(49, 50, 68),    // #313244
        background_element: Color::Rgb(69, 71, 90),  // #45475a
        border: Color::Rgb(69, 71, 90),         // #45475a
        border_active: Color::Rgb(203, 166, 247),    // #cba6f7
        border_subtle: Color::Rgb(49, 50, 68),  // #313244
    }
}

/// Catppuccin Latte (light theme)
pub fn light() -> Theme {
    Theme {
        primary: Color::Rgb(136, 57, 239),      // #8839ef
        secondary: Color::Rgb(30, 102, 245),    // #1e66f5
        accent: Color::Rgb(234, 118, 203),      // #ea76cb
        error: Color::Rgb(210, 15, 57),         // #d20f39
        warning: Color::Rgb(254, 100, 11),      // #fe640b
        success: Color::Rgb(64, 160, 43),       // #40a02b
        info: Color::Rgb(4, 165, 229),          // #04a5e5
        text: Color::Rgb(76, 79, 105),          // #4c4f69
        text_muted: Color::Rgb(156, 160, 176),  // #9ca0b0
        selected_item_text: Color::Rgb(239, 241, 245),  // #eff1f5
        background: Color::Rgb(239, 241, 245),  // #eff1f5
        background_panel: Color::Rgb(230, 233, 239),    // #e6e9ef
        background_element: Color::Rgb(204, 208, 218),  // #ccd0da
        border: Color::Rgb(204, 208, 218),      // #ccd0da
        border_active: Color::Rgb(136, 57, 239),        // #8839ef
        border_subtle: Color::Rgb(230, 233, 239),       // #e6e9ef
    }
}

/// Get status color from the theme
pub fn status_color(theme: &Theme, status: crate::types::SessionStatus) -> Color {
    match status {
        crate::types::SessionStatus::Running => theme.success,
        crate::types::SessionStatus::Waiting => theme.warning,
        crate::types::SessionStatus::Paused => theme.secondary,
        crate::types::SessionStatus::Compacting => theme.accent,
        crate::types::SessionStatus::Idle => theme.text_muted,
        crate::types::SessionStatus::Error => theme.error,
        crate::types::SessionStatus::Stopped => theme.text_muted,
    }
}
```

- [ ] **Step 2: Register the module in `src/ui/mod.rs`**

Add `pub mod theme;` to `src/ui/mod.rs`.

- [ ] **Step 3: Add `theme` field to `App` struct**

In `src/app.rs`, add:

```rust
use crate::ui::theme::Theme;
```

Add to the `App` struct:

```rust
pub theme: Theme,
```

Update `App::new()` to accept a `light` bool:

```rust
pub fn new(light: bool) -> Self {
    Self {
        sessions: Vec::new(),
        selected_index: 0,
        overlay: Overlay::None,
        should_quit: false,
        returning_from_attach: false,
        last_status_refresh: std::time::Instant::now(),
        attach_session: None,
        theme: if light { crate::ui::theme::light() } else { crate::ui::theme::dark() },
    }
}
```

- [ ] **Step 4: Update `main.rs` to pass `cli.light` to `App::new`**

Change:
```rust
let mut app = crate::app::App::new();
```
To:
```rust
let mut app = crate::app::App::new(cli.light);
```

- [ ] **Step 5: Update `home.rs` to use theme colors**

Replace the hardcoded `status_color` function body and all `Color::Cyan`, `Color::DarkGray`, `Color::White` etc. references in `render_header`, `render_session_list` to use `app.theme.*` fields. The `status_color` function becomes a call to `crate::ui::theme::status_color(&app.theme, status)`.

For example, the header becomes:
```rust
fn render_header(frame: &mut Frame, area: Rect, theme: &crate::ui::theme::Theme) {
    let version = env!("CARGO_PKG_VERSION");
    let header = Line::from(vec![
        Span::styled("agent-view ", Style::default().fg(theme.primary).bold()),
        Span::styled(format!("v{}", version), Style::default().fg(theme.text_muted)),
    ]);
    frame.render_widget(Paragraph::new(header), area);
}
```

Update `render` signature to pass `&app.theme` down, and update the session list to use `theme.text`, `theme.text_muted`, `theme.background_element` for selection highlight, etc.

- [ ] **Step 6: Update `footer.rs` to use theme colors**

Change the render function to accept `theme: &crate::ui::theme::Theme` and replace `Color::Cyan` with `theme.secondary`, `Color::DarkGray` with `theme.text_muted`.

- [ ] **Step 7: Update `overlay.rs` to use theme colors**

Change both overlay render functions to accept `theme: &crate::ui::theme::Theme` and replace hardcoded colors with theme fields. `Color::Cyan` becomes `theme.primary`, `Color::Yellow` becomes `theme.warning`, `Color::White` becomes `theme.text`, `Color::DarkGray` becomes `theme.text_muted`.

**Verify:** `cargo build` (visual check by running the binary with `--light` flag)

---

### Task 4: Integrate groups into App state and home screen

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/home.rs`

- [ ] **Step 1: Add group fields to `App`**

In `src/app.rs`, add:

```rust
use crate::core::groups::ListRow;
use crate::types::Group;
```

Add to `App` struct:

```rust
pub groups: Vec<Group>,
pub list_rows: Vec<ListRow>,
```

Initialize both as empty vecs in `App::new()`.

- [ ] **Step 2: Add `rebuild_list_rows` method to `App`**

```rust
/// Rebuild the flattened list from current sessions and groups
pub fn rebuild_list_rows(&mut self) {
    let groups = crate::core::groups::ensure_default_group(&self.groups);
    self.list_rows = crate::core::groups::flatten_group_tree(&self.sessions, &groups);
    self.clamp_selection();
}
```

- [ ] **Step 3: Update `selected_session` to work with `list_rows`**

```rust
pub fn selected_session(&self) -> Option<&Session> {
    match self.list_rows.get(self.selected_index) {
        Some(ListRow::Session(s)) => Some(s),
        _ => None,
    }
}

pub fn selected_group(&self) -> Option<&Group> {
    match self.list_rows.get(self.selected_index) {
        Some(ListRow::Group { group, .. }) => Some(group),
        _ => None,
    }
}
```

- [ ] **Step 4: Update `move_selection_up/down` to use `list_rows.len()`**

Replace `self.sessions.len()` with `self.list_rows.len()` in `move_selection_up`, `move_selection_down`, and `clamp_selection`.

- [ ] **Step 5: Load groups in `main.rs` and call `rebuild_list_rows`**

After `app.sessions = storage.load_sessions()?;`, add:

```rust
app.groups = storage.load_groups().unwrap_or_default();
app.rebuild_list_rows();
```

Replace all existing `app.clamp_selection()` calls in `main.rs` with `app.rebuild_list_rows()` (after any session/group data changes).

- [ ] **Step 6: Rewrite `render_session_list` in `home.rs` to render `ListRow`s**

The function now iterates over `app.list_rows` and renders group headers differently from sessions:

```rust
fn render_session_list(frame: &mut Frame, area: Rect, app: &App) {
    if app.list_rows.is_empty() {
        let msg = Paragraph::new("No sessions. Press 'n' to create one.")
            .style(Style::default().fg(app.theme.text_muted))
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .list_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_selected = i == app.selected_index;
            match row {
                crate::core::groups::ListRow::Group {
                    group,
                    session_count,
                    running_count,
                    waiting_count,
                } => {
                    let arrow = if group.expanded { "\u{25BC}" } else { "\u{25B6}" };
                    let mut spans = vec![
                        Span::styled(
                            format!(" {} ", arrow),
                            Style::default().fg(if is_selected { app.theme.selected_item_text } else { app.theme.accent }),
                        ),
                        Span::styled(
                            group.name.clone(),
                            Style::default().fg(if is_selected { app.theme.selected_item_text } else { app.theme.text }).bold(),
                        ),
                        Span::styled(
                            format!("  ({})", session_count),
                            Style::default().fg(if is_selected { app.theme.selected_item_text } else { app.theme.text_muted }),
                        ),
                    ];

                    if *running_count > 0 {
                        spans.push(Span::styled(
                            format!("  \u{25CF}{}", running_count),
                            Style::default().fg(if is_selected { app.theme.selected_item_text } else { app.theme.success }),
                        ));
                    }
                    if *waiting_count > 0 {
                        spans.push(Span::styled(
                            format!("  \u{25D0}{}", waiting_count),
                            Style::default().fg(if is_selected { app.theme.selected_item_text } else { app.theme.warning }),
                        ));
                    }

                    let bg = if is_selected { app.theme.primary } else { app.theme.background_element };
                    ListItem::new(Line::from(spans)).style(Style::default().bg(bg))
                }
                crate::core::groups::ListRow::Session(session) => {
                    let status_color = crate::ui::theme::status_color(&app.theme, session.status);
                    let notify_indicator = if session.notify { " !" } else { "  " };
                    let follow_up_indicator = if session.follow_up { "F " } else { "  " };
                    let age = format_age(session.created_at);

                    let line = Line::from(vec![
                        Span::raw(follow_up_indicator),
                        Span::styled(format!("   {} ", session.status.icon()), Style::default().fg(status_color)),
                        Span::styled(notify_indicator, Style::default().fg(app.theme.warning)),
                        Span::styled(session.title.clone(), Style::default().fg(app.theme.text).bold()),
                        Span::raw("  "),
                        Span::styled(truncate_path(&session.project_path, 30), Style::default().fg(app.theme.text_muted)),
                        Span::raw("  "),
                        Span::styled(age, Style::default().fg(app.theme.text_muted)),
                    ]);

                    let bg = if is_selected { app.theme.background_element } else { app.theme.background };
                    ListItem::new(line).style(Style::default().bg(bg))
                }
            }
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, area);
}
```

- [ ] **Step 7: Add group expand/collapse keybinds in `handle_main_key`**

In `main.rs`, add Enter handling for groups and left/right for expand/collapse:

```rust
(KeyModifiers::NONE, KeyCode::Enter) => {
    if let Some(group) = app.selected_group() {
        let path = group.path.clone();
        let _ = storage.toggle_group_expanded(&path);
        app.groups = storage.load_groups().unwrap_or_default();
        app.rebuild_list_rows();
    } else if let Some(session) = app.selected_session() {
        // ... existing attach logic ...
    }
}
(KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::NONE, KeyCode::Char('l')) => {
    if let Some(group) = app.selected_group() {
        if !group.expanded {
            let path = group.path.clone();
            let _ = storage.toggle_group_expanded(&path);
            app.groups = storage.load_groups().unwrap_or_default();
            app.rebuild_list_rows();
        }
    }
}
(KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
    if let Some(group) = app.selected_group() {
        if group.expanded {
            let path = group.path.clone();
            let _ = storage.toggle_group_expanded(&path);
            app.groups = storage.load_groups().unwrap_or_default();
            app.rebuild_list_rows();
        }
    }
}
```

**Verify:** `cargo build && cargo test`

---

### Task 5: Detail panel

**Files:** Create `src/ui/detail.rs`, modify `src/ui/mod.rs`, `src/ui/home.rs`

- [ ] **Step 1: Create `src/ui/detail.rs` with metadata rendering**

```rust
//! Detail panel — shows session metadata on the right side

use crate::types::Session;
use crate::ui::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Minimum terminal width to show the detail panel
pub const DETAIL_PANEL_MIN_WIDTH: u16 = 100;

/// Render the detail panel for the selected session
pub fn render(frame: &mut Frame, area: Rect, session: &Session, theme: &Theme) {
    let block = Block::default()
        .title(" Details ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let status_color = crate::ui::theme::status_color(theme, session.status);

    let created = format_timestamp(session.created_at);
    let duration = format_session_duration(session.created_at, session.status);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                format!("{} {}", session.status.icon(), session.status.as_str()),
                Style::default().fg(status_color),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(theme.text_muted)),
            Span::styled(session.tool.as_str(), Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().fg(theme.text_muted)),
            Span::styled(&session.project_path, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Group: ", Style::default().fg(theme.text_muted)),
            Span::styled(&session.group_path, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Created: ", Style::default().fg(theme.text_muted)),
            Span::styled(created, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Duration: ", Style::default().fg(theme.text_muted)),
            Span::styled(duration, Style::default().fg(theme.text)),
        ]),
    ];

    if !session.worktree_path.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Worktree: ", Style::default().fg(theme.text_muted)),
            Span::styled(&session.worktree_path, Style::default().fg(theme.text)),
        ]));
        if !session.worktree_branch.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(theme.text_muted)),
                Span::styled(&session.worktree_branch, Style::default().fg(theme.secondary)),
            ]));
        }
    }

    if session.notify {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Notifications: ", Style::default().fg(theme.text_muted)),
            Span::styled("on", Style::default().fg(theme.success)),
        ]));
    }

    if session.follow_up {
        lines.push(Line::from(vec![
            Span::styled("Follow-up: ", Style::default().fg(theme.text_muted)),
            Span::styled("marked", Style::default().fg(theme.warning)),
        ]));
    }

    if session.restart_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("Restarts: ", Style::default().fg(theme.text_muted)),
            Span::styled(session.restart_count.to_string(), Style::default().fg(theme.text)),
        ]));
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn format_timestamp(ms: i64) -> String {
    use chrono::{TimeZone, Utc, Local};
    let dt = Utc.timestamp_millis_opt(ms).single();
    match dt {
        Some(utc) => {
            let local = utc.with_timezone(&Local);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        None => "unknown".to_string(),
    }
}

fn format_session_duration(created_at_ms: i64, status: crate::types::SessionStatus) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - created_at_ms;
    if diff_ms < 0 {
        return "just started".to_string();
    }

    let seconds = diff_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d {}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes % 60)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        "< 1m".to_string()
    }
}
```

- [ ] **Step 2: Register the module in `src/ui/mod.rs`**

Add `pub mod detail;` to `src/ui/mod.rs`.

- [ ] **Step 3: Update `home.rs` to show the detail panel**

In the `render` function, split the body area horizontally when the terminal is wide enough:

```rust
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0], &app.theme);

    // Dual-column layout when wide enough
    if area.width >= crate::ui::detail::DETAIL_PANEL_MIN_WIDTH {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(55),
                Constraint::Percentage(45),
            ])
            .split(chunks[1]);

        render_session_list(frame, body[0], app);

        if let Some(session) = app.selected_session() {
            crate::ui::detail::render(frame, body[1], session, &app.theme);
        }
    } else {
        render_session_list(frame, chunks[1], app);
    }

    crate::ui::footer::render(frame, chunks[2], app);

    // Overlays on top
    match &app.overlay {
        Overlay::NewSession(form) => {
            crate::ui::overlay::render_new_session(frame, area, form, &app.theme);
        }
        Overlay::Confirm(dialog) => {
            crate::ui::overlay::render_confirm(frame, area, dialog, &app.theme);
        }
        Overlay::None => {}
        // Other overlay variants rendered here as they are added
        _ => {}
    }
}
```

**Verify:** `cargo build`

---

### Task 6: Search mode

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/home.rs`, `src/ui/footer.rs`

- [ ] **Step 1: Add search state to `App`**

In `src/app.rs`:

```rust
pub search_query: Option<String>,
```

Initialize as `None` in `App::new()`.

- [ ] **Step 2: Add `filtered_list_rows` method**

```rust
/// Return list rows filtered by the current search query.
/// When no search is active, returns all rows.
pub fn visible_rows(&self) -> &[ListRow] {
    &self.list_rows
}

/// Get the indices of sessions matching the search query
pub fn search_matches(&self) -> Vec<usize> {
    let query = match &self.search_query {
        Some(q) if !q.is_empty() => q.to_lowercase(),
        _ => return Vec::new(),
    };

    self.list_rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| match row {
            ListRow::Session(s) if s.title.to_lowercase().contains(&query) => Some(i),
            _ => None,
        })
        .collect()
}
```

- [ ] **Step 3: Add search key handling in `main.rs`**

In `handle_main_key`, add a branch for `/` to enter search mode:

```rust
(KeyModifiers::NONE, KeyCode::Char('/')) => {
    app.search_query = Some(String::new());
}
```

Add a new match branch in the main event loop (in `run_tui`) for when `app.search_query.is_some()` that captures keystrokes:

```rust
if app.search_query.is_some() {
    match key.code {
        KeyCode::Esc => {
            app.search_query = None;
        }
        KeyCode::Enter => {
            // Jump to first match if any
            let matches = app.search_matches();
            if let Some(&idx) = matches.first() {
                app.selected_index = idx;
            }
            app.search_query = None;
        }
        KeyCode::Backspace => {
            if let Some(ref mut q) = app.search_query {
                q.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut q) = app.search_query {
                q.push(c);
                // Auto-jump to first match
                let query = q.to_lowercase();
                for (i, row) in app.list_rows.iter().enumerate() {
                    if let ListRow::Session(s) = row {
                        if s.title.to_lowercase().contains(&query) {
                            app.selected_index = i;
                            break;
                        }
                    }
                }
            }
        }
        _ => {}
    }
} else {
    // existing overlay dispatch
}
```

- [ ] **Step 4: Render search bar in `home.rs`**

When `app.search_query.is_some()`, render a search bar at the bottom of the session list area (or replace the footer):

```rust
// In render(), before the footer, if search is active:
if let Some(ref query) = app.search_query {
    let matches = app.search_matches();
    let search_line = Line::from(vec![
        Span::styled(" / ", Style::default().fg(app.theme.primary).bold()),
        Span::styled(query.as_str(), Style::default().fg(app.theme.text)),
        Span::styled("\u{2588}", Style::default().fg(app.theme.primary)),
        Span::styled(
            format!("  {} match{}", matches.len(), if matches.len() == 1 { "" } else { "es" }),
            Style::default().fg(app.theme.text_muted),
        ),
    ]);
    // Render in footer area
    frame.render_widget(Paragraph::new(search_line), footer_area);
} else {
    crate::ui::footer::render(frame, footer_area, app);
}
```

- [ ] **Step 5: Highlight matching rows in the session list**

In `render_session_list`, when a search query is active, highlight the title text of matching sessions with the theme `info` color instead of the default text color. Check with `app.search_matches().contains(&i)`.

**Verify:** `cargo build`

---

### Task 7: Log export

**Files:** `src/main.rs`

- [ ] **Step 1: Add the `e` keybind in `handle_main_key`**

```rust
(KeyModifiers::NONE, KeyCode::Char('e')) => {
    if let Some(session) = app.selected_session() {
        if !session.tmux_session.is_empty() {
            let tmux_name = session.tmux_session.clone();
            let title = session.title.clone();
            match export_session_log(&tmux_name, &title) {
                Ok(path) => {
                    app.toast_message = Some(format!("Exported to {}", path));
                }
                Err(e) => {
                    app.toast_message = Some(format!("Export failed: {}", e));
                }
            }
        }
    }
}
```

- [ ] **Step 2: Add `toast_message` field to `App`**

In `src/app.rs`, add:

```rust
pub toast_message: Option<String>,
pub toast_expire: Option<std::time::Instant>,
```

Initialize as `None` in `App::new()`.

- [ ] **Step 3: Implement `export_session_log` function in `main.rs`**

```rust
fn export_session_log(tmux_session: &str, title: &str) -> Result<String, String> {
    // Capture full scrollback
    let output = crate::core::tmux::capture_pane(tmux_session, Some(-10000))
        .map_err(|e| format!("Capture failed: {}", e))?;

    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let logs_dir = home.join(".agent-view").join("logs");
    std::fs::create_dir_all(&logs_dir).map_err(|e| format!("Cannot create logs dir: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe_name: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .take(30)
        .collect();
    let filename = format!("{}-{}.log", safe_name, timestamp);
    let filepath = logs_dir.join(&filename);

    std::fs::write(&filepath, &output)
        .map_err(|e| format!("Write failed: {}", e))?;

    Ok(filepath.to_string_lossy().to_string())
}
```

- [ ] **Step 4: Render toast messages in `home.rs`**

In `render()`, if `app.toast_message.is_some()` and not expired, render a brief message in the footer area:

```rust
if let Some(ref msg) = app.toast_message {
    if app.toast_expire.map_or(false, |t| t > std::time::Instant::now()) {
        let toast = Line::from(Span::styled(msg.as_str(), Style::default().fg(app.theme.info)));
        frame.render_widget(Paragraph::new(toast), footer_area);
    } else {
        // Clear expired toast during next event cycle
    }
}
```

- [ ] **Step 5: Set toast expiry when creating toast in `handle_main_key`**

After setting `app.toast_message`, also set:
```rust
app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
```

Add toast expiry clearing in the main loop:
```rust
if let Some(expire) = app.toast_expire {
    if expire < std::time::Instant::now() {
        app.toast_message = None;
        app.toast_expire = None;
    }
}
```

**Verify:** `cargo build`

---

### Task 8: Follow-up mark toggle

**Files:** `src/main.rs`

- [ ] **Step 1: Add the `i` keybind in `handle_main_key`**

```rust
(KeyModifiers::NONE, KeyCode::Char('i')) => {
    if let Some(session) = app.selected_session() {
        let new_val = !session.follow_up;
        let id = session.id.clone();
        let title = session.title.clone();
        let _ = storage.set_follow_up(&id, new_val);
        if let Ok(sessions) = storage.load_sessions() {
            app.sessions = sessions;
            app.rebuild_list_rows();
        }
        let msg = if new_val {
            format!("Marked for follow-up: {}", title)
        } else {
            format!("Follow-up cleared: {}", title)
        };
        app.toast_message = Some(msg);
        app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
    }
}
```

- [ ] **Step 2: Update footer hints to include `i` for follow-up**

In `src/ui/footer.rs`, add `("i", "follow-up")` to the hints list when sessions are present.

**Verify:** `cargo build`

---

### Task 9: Rename overlay

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/overlay.rs`

- [ ] **Step 1: Add `RenameForm` struct and `Overlay::Rename` variant**

In `src/app.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct RenameForm {
    pub target_id: String,
    pub target_type: RenameTarget,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenameTarget {
    Session,
    Group,
}
```

Add to `Overlay` enum:

```rust
Rename(RenameForm),
```

- [ ] **Step 2: Add `R` keybind in `handle_main_key`**

```rust
(KeyModifiers::SHIFT, KeyCode::Char('R')) => {
    if let Some(session) = app.selected_session() {
        app.overlay = Overlay::Rename(RenameForm {
            target_id: session.id.clone(),
            target_type: RenameTarget::Session,
            input: session.title.clone(),
        });
    } else if let Some(group) = app.selected_group() {
        app.overlay = Overlay::Rename(RenameForm {
            target_id: group.path.clone(),
            target_type: RenameTarget::Group,
            input: group.name.clone(),
        });
    }
}
```

- [ ] **Step 3: Add `handle_rename_key` function in `main.rs`**

```rust
fn handle_rename_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Rename(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Enter => {
                let new_name = form.input.trim().to_string();
                if !new_name.is_empty() {
                    match form.target_type {
                        crate::app::RenameTarget::Session => {
                            let _ = storage.rename_session(&form.target_id, &new_name);
                        }
                        crate::app::RenameTarget::Group => {
                            // Load, update name, save
                            if let Ok(groups) = storage.load_groups() {
                                if let Some(mut group) = groups.into_iter().find(|g| g.path == form.target_id) {
                                    group.name = new_name;
                                    let _ = storage.save_group(&group);
                                }
                            }
                        }
                    }
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                    }
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Backspace => {
                form.input.pop();
            }
            KeyCode::Char(c) => {
                form.input.push(c);
            }
            _ => {}
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Wire `handle_rename_key` into the event dispatch in `run_tui`**

Add `crate::app::Overlay::Rename(_)` match arm in the overlay dispatch that calls `handle_rename_key`.

- [ ] **Step 5: Add `render_rename` function in `src/ui/overlay.rs`**

```rust
pub fn render_rename(
    frame: &mut Frame,
    area: Rect,
    form: &crate::app::RenameForm,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = match form.target_type {
        crate::app::RenameTarget::Session => " Rename Session ",
        crate::app::RenameTarget::Group => " Rename Group ",
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new("New name:").style(Style::default().fg(theme.text_muted)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("{}\u{2588}", form.input))
            .style(Style::default().fg(theme.text)),
        chunks[1],
    );
}
```

- [ ] **Step 6: Call `render_rename` from `home.rs` overlay dispatch**

Add to the overlay match:
```rust
Overlay::Rename(form) => {
    crate::ui::overlay::render_rename(frame, area, form, &app.theme);
}
```

**Verify:** `cargo build`

---

### Task 10: Move overlay

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/overlay.rs`

- [ ] **Step 1: Add `MoveForm` struct and `Overlay::Move` variant**

In `src/app.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct MoveForm {
    pub session_id: String,
    pub session_title: String,
    pub groups: Vec<(String, String)>, // (path, name)
    pub selected: usize,
}
```

Add `Move(MoveForm)` to `Overlay` enum.

- [ ] **Step 2: Add `m` keybind in `handle_main_key`**

```rust
(KeyModifiers::NONE, KeyCode::Char('m')) => {
    if let Some(session) = app.selected_session() {
        let groups: Vec<(String, String)> = app.groups.iter()
            .map(|g| (g.path.clone(), g.name.clone()))
            .collect();
        if !groups.is_empty() {
            app.overlay = crate::app::Overlay::Move(crate::app::MoveForm {
                session_id: session.id.clone(),
                session_title: session.title.clone(),
                groups,
                selected: 0,
            });
        }
    }
}
```

- [ ] **Step 3: Add `handle_move_key` function in `main.rs`**

```rust
fn handle_move_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::Move(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if form.selected > 0 {
                    form.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if form.selected < form.groups.len().saturating_sub(1) {
                    form.selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some((ref path, ref name)) = form.groups.get(form.selected) {
                    let _ = storage.move_session_to_group(&form.session_id, path);
                    if let Ok(sessions) = storage.load_sessions() {
                        app.sessions = sessions;
                    }
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                    app.toast_message = Some(format!("Moved to {}", name));
                    app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                }
                app.overlay = crate::app::Overlay::None;
            }
            _ => {}
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Wire into event dispatch and add render function**

Add `crate::app::Overlay::Move(_)` match arm calling `handle_move_key`.

Add `render_move` function to `overlay.rs`:

```rust
pub fn render_move(
    frame: &mut Frame,
    area: Rect,
    form: &crate::app::MoveForm,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_height = (form.groups.len() as u16 + 4).min(area.height.saturating_sub(4));
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = format!(" Move \"{}\" ", form.session_title);
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let items: Vec<ListItem> = form.groups.iter().enumerate().map(|(i, (_, name))| {
        let style = if i == form.selected {
            Style::default().bg(theme.primary).fg(theme.selected_item_text)
        } else {
            Style::default().fg(theme.text)
        };
        ListItem::new(format!("  {}", name)).style(style)
    }).collect();

    frame.render_widget(List::new(items), inner);
}
```

Add to overlay dispatch in `home.rs`.

**Verify:** `cargo build`

---

### Task 11: Group management overlay

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/overlay.rs`

- [ ] **Step 1: Add `GroupForm` struct and `Overlay::GroupManage` variant**

In `src/app.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct GroupForm {
    pub name: String,
}
```

Add `GroupManage(GroupForm)` to `Overlay` enum.

- [ ] **Step 2: Add `g` keybind in `handle_main_key`**

```rust
(KeyModifiers::NONE, KeyCode::Char('g')) => {
    app.overlay = crate::app::Overlay::GroupManage(crate::app::GroupForm {
        name: String::new(),
    });
}
```

- [ ] **Step 3: Add `handle_group_key` function in `main.rs`**

```rust
fn handle_group_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::GroupManage(ref mut form) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Enter => {
                let name = form.name.trim().to_string();
                if !name.is_empty() {
                    let path = name.to_lowercase()
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '-' })
                        .collect::<String>();
                    let path = path.trim_matches('-').to_string();

                    let order = app.groups.len() as i32;
                    let group = crate::types::Group {
                        path,
                        name,
                        expanded: true,
                        order,
                        default_path: String::new(),
                    };
                    let _ = storage.save_group(&group);
                    app.groups = storage.load_groups().unwrap_or_default();
                    app.rebuild_list_rows();
                }
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Backspace => {
                form.name.pop();
            }
            KeyCode::Char(c) => {
                form.name.push(c);
            }
            _ => {}
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Wire into event dispatch and add render function**

Add `crate::app::Overlay::GroupManage(_)` match arm and a `render_group_manage` function in `overlay.rs` (similar structure to `render_rename` — single text input for group name).

**Verify:** `cargo build`

---

### Task 12: Command palette

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/overlay.rs`

- [ ] **Step 1: Add `CommandPalette` struct and `Overlay::CommandPalette` variant**

In `src/app.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct CommandPalette {
    pub query: String,
    pub items: Vec<CommandItem>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandItem {
    pub label: String,
    pub key_hint: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandAction {
    NewSession,
    StopSession,
    RestartSession,
    DeleteSession,
    RenameSession,
    MoveSession,
    ToggleNotify,
    ToggleFollowUp,
    ExportLog,
    CreateGroup,
    Search,
    Quit,
}

impl CommandPalette {
    pub fn new() -> Self {
        let items = vec![
            CommandItem { label: "New Session".to_string(), key_hint: "n".to_string(), action: CommandAction::NewSession },
            CommandItem { label: "Stop Session".to_string(), key_hint: "s".to_string(), action: CommandAction::StopSession },
            CommandItem { label: "Restart Session".to_string(), key_hint: "r".to_string(), action: CommandAction::RestartSession },
            CommandItem { label: "Delete Session".to_string(), key_hint: "d".to_string(), action: CommandAction::DeleteSession },
            CommandItem { label: "Rename".to_string(), key_hint: "R".to_string(), action: CommandAction::RenameSession },
            CommandItem { label: "Move to Group".to_string(), key_hint: "m".to_string(), action: CommandAction::MoveSession },
            CommandItem { label: "Toggle Notifications".to_string(), key_hint: "!".to_string(), action: CommandAction::ToggleNotify },
            CommandItem { label: "Toggle Follow-up".to_string(), key_hint: "i".to_string(), action: CommandAction::ToggleFollowUp },
            CommandItem { label: "Export Log".to_string(), key_hint: "e".to_string(), action: CommandAction::ExportLog },
            CommandItem { label: "Create Group".to_string(), key_hint: "g".to_string(), action: CommandAction::CreateGroup },
            CommandItem { label: "Search Sessions".to_string(), key_hint: "/".to_string(), action: CommandAction::Search },
            CommandItem { label: "Quit".to_string(), key_hint: "q".to_string(), action: CommandAction::Quit },
        ];
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self { query: String::new(), items, filtered, selected: 0 }
    }

    pub fn filter(&mut self) {
        let q = self.query.to_lowercase();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            self.filtered = self.items.iter().enumerate()
                .filter(|(_, item)| item.label.to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
    }
}
```

Add `CommandPalette(CommandPalette)` to `Overlay` enum.

- [ ] **Step 2: Add Ctrl+K keybind in `handle_main_key`**

```rust
(KeyModifiers::CONTROL, KeyCode::Char('k')) => {
    app.overlay = crate::app::Overlay::CommandPalette(crate::app::CommandPalette::new());
}
```

- [ ] **Step 3: Add `handle_palette_key` function in `main.rs`**

```rust
fn handle_palette_key(
    app: &mut crate::app::App,
    key: crossterm::event::KeyEvent,
    storage: &crate::core::storage::Storage,
    session_manager: &mut crate::core::session::SessionManager,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::KeyCode;

    if let crate::app::Overlay::CommandPalette(ref mut palette) = app.overlay {
        match key.code {
            KeyCode::Esc => {
                app.overlay = crate::app::Overlay::None;
            }
            KeyCode::Up | KeyCode::BackTab => {
                if palette.selected > 0 {
                    palette.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if palette.selected < palette.filtered.len().saturating_sub(1) {
                    palette.selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(&idx) = palette.filtered.get(palette.selected) {
                    let action = palette.items[idx].action.clone();
                    app.overlay = crate::app::Overlay::None;
                    execute_command_action(app, action, storage, session_manager)?;
                }
            }
            KeyCode::Backspace => {
                palette.query.pop();
                palette.filter();
            }
            KeyCode::Char(c) => {
                palette.query.push(c);
                palette.filter();
            }
            _ => {}
        }
    }
    Ok(())
}

fn execute_command_action(
    app: &mut crate::app::App,
    action: crate::app::CommandAction,
    storage: &crate::core::storage::Storage,
    session_manager: &mut crate::core::session::SessionManager,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::app::{CommandAction, Overlay};

    match action {
        CommandAction::NewSession => {
            app.overlay = Overlay::NewSession(crate::app::NewSessionForm::new());
        }
        CommandAction::Search => {
            app.search_query = Some(String::new());
        }
        CommandAction::CreateGroup => {
            app.overlay = Overlay::GroupManage(crate::app::GroupForm { name: String::new() });
        }
        CommandAction::Quit => {
            app.should_quit = true;
        }
        // Session-specific actions — only fire if a session is selected
        CommandAction::StopSession => {
            if let Some(session) = app.selected_session() {
                let msg = format!("Stop session \"{}\"?", session.title);
                app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::StopSession(session.id.clone()),
                });
            }
        }
        CommandAction::DeleteSession => {
            if let Some(session) = app.selected_session() {
                let msg = format!("Delete session \"{}\"?", session.title);
                app.overlay = Overlay::Confirm(crate::app::ConfirmDialog {
                    message: msg,
                    action: crate::app::ConfirmAction::DeleteSession(session.id.clone()),
                });
            }
        }
        CommandAction::RestartSession => {
            if let Some(session) = app.selected_session() {
                let id = session.id.clone();
                let mut cache = crate::core::tmux::SessionCache::new();
                let _ = session_manager.restart_session(storage, &mut cache, &id);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::RenameSession => {
            if let Some(session) = app.selected_session() {
                app.overlay = Overlay::Rename(crate::app::RenameForm {
                    target_id: session.id.clone(),
                    target_type: crate::app::RenameTarget::Session,
                    input: session.title.clone(),
                });
            }
        }
        CommandAction::MoveSession => {
            if let Some(session) = app.selected_session() {
                let groups: Vec<(String, String)> = app.groups.iter()
                    .map(|g| (g.path.clone(), g.name.clone()))
                    .collect();
                if !groups.is_empty() {
                    app.overlay = Overlay::Move(crate::app::MoveForm {
                        session_id: session.id.clone(),
                        session_title: session.title.clone(),
                        groups,
                        selected: 0,
                    });
                }
            }
        }
        CommandAction::ToggleNotify => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.notify;
                let id = session.id.clone();
                let _ = storage.set_notify(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::ToggleFollowUp => {
            if let Some(session) = app.selected_session() {
                let new_val = !session.follow_up;
                let id = session.id.clone();
                let _ = storage.set_follow_up(&id, new_val);
                if let Ok(sessions) = storage.load_sessions() {
                    app.sessions = sessions;
                    app.rebuild_list_rows();
                }
            }
        }
        CommandAction::ExportLog => {
            if let Some(session) = app.selected_session() {
                if !session.tmux_session.is_empty() {
                    let tmux_name = session.tmux_session.clone();
                    let title = session.title.clone();
                    match export_session_log(&tmux_name, &title) {
                        Ok(path) => {
                            app.toast_message = Some(format!("Exported to {}", path));
                            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                        }
                        Err(e) => {
                            app.toast_message = Some(format!("Export failed: {}", e));
                            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Add `render_command_palette` to `overlay.rs`**

```rust
pub fn render_command_palette(
    frame: &mut Frame,
    area: Rect,
    palette: &crate::app::CommandPalette,
    theme: &crate::ui::theme::Theme,
) {
    let max_items = 10;
    let visible = palette.filtered.len().min(max_items);
    let overlay_height = (visible as u16 + 4).min(area.height.saturating_sub(4));
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.height / 6; // Near the top
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Commands ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // Search input
    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme.primary)),
        Span::styled(palette.query.as_str(), Style::default().fg(theme.text)),
        Span::styled("\u{2588}", Style::default().fg(theme.primary)),
    ]);
    frame.render_widget(Paragraph::new(input_line), chunks[0]);

    // Filtered items
    let items: Vec<ListItem> = palette.filtered.iter().enumerate().take(max_items).map(|(i, &idx)| {
        let item = &palette.items[idx];
        let style = if i == palette.selected {
            Style::default().bg(theme.primary).fg(theme.selected_item_text)
        } else {
            Style::default().fg(theme.text)
        };
        let line = Line::from(vec![
            Span::styled(format!("  {} ", item.label), style),
            Span::styled(
                format!("  {}", item.key_hint),
                if i == palette.selected {
                    Style::default().bg(theme.primary).fg(theme.selected_item_text)
                } else {
                    Style::default().fg(theme.text_muted)
                },
            ),
        ]);
        ListItem::new(line)
    }).collect();

    frame.render_widget(List::new(items), chunks[1]);
}
```

Wire into overlay dispatch in `home.rs` and event loop.

**Verify:** `cargo build`

---

### Task 13: Git worktree support

**Files:** Create `src/core/git.rs`, modify `src/core/mod.rs`, `src/app.rs`, `src/main.rs`, `src/ui/overlay.rs`

- [ ] **Step 1: Create `src/core/git.rs` with worktree functions**

```rust
//! Git worktree operations

use std::path::Path;
use std::process::Command;

/// Check if a directory is inside a git repository
pub fn is_git_repo(dir: &str) -> bool {
    Command::new("git")
        .args(["-C", dir, "rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the repository root
pub fn get_repo_root(dir: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", dir, "rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        return Err("Not a git repository".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Validate a branch name
pub fn validate_branch_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("branch name cannot be empty".to_string());
    }
    if name.trim() != name {
        return Some("branch name cannot have leading or trailing spaces".to_string());
    }
    if name.contains("..") {
        return Some("branch name cannot contain '..'".to_string());
    }
    if name.starts_with('.') {
        return Some("branch name cannot start with '.'".to_string());
    }
    if name.ends_with(".lock") {
        return Some("branch name cannot end with '.lock'".to_string());
    }
    let invalid = [' ', '\t', '~', '^', ':', '?', '*', '[', '\\'];
    for c in &invalid {
        if name.contains(*c) {
            return Some(format!("branch name cannot contain '{}'", c));
        }
    }
    if name.contains("@{") {
        return Some("branch name cannot contain '@{'".to_string());
    }
    if name == "@" {
        return Some("branch name cannot be just '@'".to_string());
    }
    None
}

/// Check if a branch exists
pub fn branch_exists(repo_dir: &str, branch: &str) -> bool {
    Command::new("git")
        .args(["-C", repo_dir, "show-ref", "--verify", "--quiet", &format!("refs/heads/{}", branch)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a worktree path: <repo>/.worktrees/<branch-sanitized>
pub fn generate_worktree_path(repo_dir: &str, branch: &str) -> String {
    let sanitized: String = branch.replace('/', "-").replace(' ', "-");
    Path::new(repo_dir)
        .join(".worktrees")
        .join(&sanitized)
        .to_string_lossy()
        .to_string()
}

/// Create a git worktree. Returns the worktree path on success.
pub fn create_worktree(repo_dir: &str, branch: &str) -> Result<String, String> {
    if let Some(err) = validate_branch_name(branch) {
        return Err(format!("Invalid branch name: {}", err));
    }
    if !is_git_repo(repo_dir) {
        return Err("Not a git repository".to_string());
    }

    let wt_path = generate_worktree_path(repo_dir, branch);

    let args = if branch_exists(repo_dir, branch) {
        vec!["worktree", "add", &wt_path, branch]
    } else {
        vec!["worktree", "add", "-b", branch, &wt_path]
    };

    let output = Command::new("git")
        .args(["-C", repo_dir])
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to create worktree: {}", stderr));
    }

    Ok(wt_path)
}
```

- [ ] **Step 2: Register module in `src/core/mod.rs`**

Add `pub mod git;` to `src/core/mod.rs`.

- [ ] **Step 3: Add tests for git validation**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_branch_name_valid() {
        assert!(validate_branch_name("feature/new-thing").is_none());
        assert!(validate_branch_name("fix-123").is_none());
    }

    #[test]
    fn test_validate_branch_name_invalid() {
        assert!(validate_branch_name("").is_some());
        assert!(validate_branch_name("has space").is_some());
        assert!(validate_branch_name("has..dots").is_some());
        assert!(validate_branch_name(".starts-with-dot").is_some());
        assert!(validate_branch_name("ends.lock").is_some());
        assert!(validate_branch_name("has~tilde").is_some());
    }

    #[test]
    fn test_generate_worktree_path() {
        let path = generate_worktree_path("/repo", "feature/my-branch");
        assert!(path.contains(".worktrees"));
        assert!(path.contains("feature-my-branch"));
    }
}
```

- [ ] **Step 4: Add worktree fields to `NewSessionForm`**

In `src/app.rs`, add to `NewSessionForm`:

```rust
pub use_worktree: bool,
pub branch_name: String,
```

Initialize both (`false`, `String::new()`) in `NewSessionForm::new()`. Increment `focused_field` modulo to 4 (title, path, worktree toggle, branch name).

- [ ] **Step 5: Update `handle_new_session_key` for worktree fields**

Add Tab cycling through 4 fields. Add a toggle for `use_worktree` (space when focused on field 2). Handle char/backspace for field 3 (branch name).

On Enter, if `use_worktree` is true and branch is non-empty:
```rust
if form.use_worktree && !form.branch_name.is_empty() {
    match crate::core::git::create_worktree(&form.project_path, &form.branch_name) {
        Ok(wt_path) => {
            // Use wt_path as the project_path and store worktree metadata
            options.project_path = wt_path.clone();
            // Set worktree fields on the session after creation
        }
        Err(e) => {
            app.toast_message = Some(format!("Worktree error: {}", e));
            app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
            return Ok(());
        }
    }
}
```

- [ ] **Step 6: Update `render_new_session` in `overlay.rs` for worktree fields**

Add two more field rows: a `[x]`/`[ ]` checkbox for "Create in worktree" and a branch name input. Increase `overlay_height` to accommodate the extra fields.

**Verify:** `cargo build && cargo test -- git`

---

### Task 14: Update footer hints for all new keybinds

**Files:** `src/ui/footer.rs`

- [ ] **Step 1: Update the main-mode hint list**

```rust
Overlay::None => {
    if app.sessions.is_empty() {
        vec![("n", "new"), ("g", "group"), ("q", "quit")]
    } else {
        vec![
            ("j/k", "navigate"),
            ("Enter", "attach"),
            ("n", "new"),
            ("s", "stop"),
            ("r", "restart"),
            ("d", "delete"),
            ("R", "rename"),
            ("m", "move"),
            ("g", "group"),
            ("e", "export"),
            ("i", "follow-up"),
            ("!", "notify"),
            ("/", "search"),
            ("C-k", "commands"),
            ("q", "quit"),
        ]
    }
}
```

- [ ] **Step 2: Add hints for all new overlay types**

```rust
Overlay::Rename(_) => {
    vec![("Enter", "save"), ("Esc", "cancel")]
}
Overlay::Move(_) => {
    vec![("j/k", "select"), ("Enter", "move"), ("Esc", "cancel")]
}
Overlay::GroupManage(_) => {
    vec![("Enter", "create"), ("Esc", "cancel")]
}
Overlay::CommandPalette(_) => {
    vec![("Tab/arrows", "navigate"), ("Enter", "execute"), ("Esc", "close")]
}
```

**Verify:** `cargo build`

---

### Task 15: Wire notification toggle toast message

**Files:** `src/main.rs`

- [ ] **Step 1: Update the `!` keybind handler to show a toast**

In the existing `!` handler, add toast feedback after toggling:

```rust
(KeyModifiers::NONE, KeyCode::Char('!')) => {
    if let Some(session) = app.selected_session() {
        let new_val = !session.notify;
        let id = session.id.clone();
        let title = session.title.clone();
        let _ = storage.set_notify(&id, new_val);
        if let Ok(sessions) = storage.load_sessions() {
            app.sessions = sessions;
            app.rebuild_list_rows();
        }
        let msg = if new_val {
            format!("Notifications on: {}", title)
        } else {
            format!("Notifications off: {}", title)
        };
        app.toast_message = Some(msg);
        app.toast_expire = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
    }
}
```

**Verify:** `cargo build`

---

### Task 16: Final integration — ensure all overlay variants compile

**Files:** `src/app.rs`, `src/main.rs`, `src/ui/home.rs`

- [ ] **Step 1: Verify the `Overlay` enum has all variants**

The final `Overlay` enum should be:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NewSession(NewSessionForm),
    Confirm(ConfirmDialog),
    Rename(RenameForm),
    Move(MoveForm),
    GroupManage(GroupForm),
    CommandPalette(CommandPalette),
}
```

- [ ] **Step 2: Ensure all overlay variants are handled in the event dispatch**

In `run_tui`, the overlay match must cover all variants:

```rust
match app.overlay {
    Overlay::None => handle_main_key(...),
    Overlay::NewSession(_) => handle_new_session_key(...),
    Overlay::Confirm(_) => handle_confirm_key(...),
    Overlay::Rename(_) => handle_rename_key(...),
    Overlay::Move(_) => handle_move_key(...),
    Overlay::GroupManage(_) => handle_group_key(...),
    Overlay::CommandPalette(_) => handle_palette_key(...),
}
```

But also handle search mode (when `app.search_query.is_some()`) BEFORE the overlay dispatch, since search is modal but not an overlay.

- [ ] **Step 3: Ensure all overlay variants are rendered in `home.rs`**

The overlay match in `render()`:

```rust
match &app.overlay {
    Overlay::NewSession(form) => crate::ui::overlay::render_new_session(frame, area, form, &app.theme),
    Overlay::Confirm(dialog) => crate::ui::overlay::render_confirm(frame, area, dialog, &app.theme),
    Overlay::Rename(form) => crate::ui::overlay::render_rename(frame, area, form, &app.theme),
    Overlay::Move(form) => crate::ui::overlay::render_move(frame, area, form, &app.theme),
    Overlay::GroupManage(form) => crate::ui::overlay::render_group_manage(frame, area, form, &app.theme),
    Overlay::CommandPalette(palette) => crate::ui::overlay::render_command_palette(frame, area, palette, &app.theme),
    Overlay::None => {}
}
```

- [ ] **Step 4: Run full test suite and build**

```bash
cargo test
cargo build --release
```

**Verify:** `cargo build --release && cargo test`
