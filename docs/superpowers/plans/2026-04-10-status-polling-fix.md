# Fix Status Polling and Notifications While Attached — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix notifications so they fire for OTHER sessions while attached to a tmux session, and stop discarding status transitions on return from attach.

**Architecture:** Change `maybe_notify` to accept `attached_session: Option<&str>` instead of `is_attached: bool`, so it can suppress only the session the user is inside. Replace the blind drain of pending status results with normal processing that passes the attached session identity. Add a field to `App` to track which tmux session is attached.

**Tech Stack:** Rust, crossterm, ratatui, mpsc channels

**Spec:** `docs/superpowers/specs/2026-04-10-status-polling-fix-design.md`

---

### Important Context

- The Rust source is in `src/` at the repo root (not `src-tauri/`)
- The background polling thread sends `Vec<(String, SessionStatus)>` over an `mpsc::channel` every 500ms
- The main loop in `run_tui()` processes results with `status_rx.try_recv()` in a non-blocking loop
- `SessionManager` lives in `src/core/session.rs` and owns all debounce/notification state
- Tests run with `cargo test`
- The existing test helper `make_test_session(id, notify)` creates a `Session` with `tmux_session: "agentorch_{id}"`

---

### Task 1: Change `maybe_notify` signature and suppression logic

**Files:**
- Modify: `src/core/session.rs:184-193` (signature and early return)
- Modify: `src/core/session.rs:513-526` (existing tests)

- [ ] **Step 1: Write failing tests for the new signature**

Add these tests to the `mod tests` block in `src/core/session.rs`, after the existing tests (after line 552):

```rust
#[test]
fn test_maybe_notify_suppresses_attached_session() {
    let mut mgr = SessionManager::new();
    let session = make_test_session("s1", true);
    // Attached to this exact session — should suppress
    let result = mgr.maybe_notify(&session, SessionStatus::Waiting, Some("agentorch_s1"), false);
    assert!(!result);
}

#[test]
fn test_maybe_notify_allows_other_sessions_when_attached() {
    let mut mgr = SessionManager::new();
    let session = make_test_session("s2", true);
    // Attached to a DIFFERENT session — should allow notification
    let result = mgr.maybe_notify(&session, SessionStatus::Waiting, Some("agentorch_s1"), false);
    assert!(result);
}

#[test]
fn test_maybe_notify_allows_all_when_not_attached() {
    let mut mgr = SessionManager::new();
    let session = make_test_session("s1", true);
    // Not attached to anything — should allow notification
    let result = mgr.maybe_notify(&session, SessionStatus::Waiting, None, false);
    assert!(result);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_maybe_notify_suppresses_attached_session test_maybe_notify_allows_other_sessions_when_attached test_maybe_notify_allows_all_when_not_attached 2>&1`
Expected: Compilation errors — `maybe_notify` still takes `bool`, not `Option<&str>`

- [ ] **Step 3: Update `maybe_notify` signature and logic**

In `src/core/session.rs`, change the method signature and the early return check (lines 184-193):

Replace:
```rust
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
```

With:
```rust
    pub fn maybe_notify(
        &mut self,
        session: &Session,
        new_status: SessionStatus,
        attached_session: Option<&str>,
        sound: bool,
    ) -> bool {
        if !session.notify {
            return false;
        }
        // Suppress notifications for the session the user is currently looking at
        if let Some(attached) = attached_session {
            if session.tmux_session == attached {
                return false;
            }
        }
```

- [ ] **Step 4: Update existing tests that use the old signature**

In `src/core/session.rs`, update the two existing tests that call `maybe_notify` with a `bool`:

Replace the test at line 513-517:
```rust
#[test]
fn test_maybe_notify_returns_false_when_not_enabled() {
    let mut mgr = SessionManager::new();
    let session = make_test_session("s1", false); // notify = false
    let result = mgr.maybe_notify(&session, SessionStatus::Waiting, None, false);
    assert!(!result);
}
```

Replace the test at line 520-525:
```rust
#[test]
fn test_maybe_notify_returns_false_when_attached() {
    let mut mgr = SessionManager::new();
    let session = make_test_session("s1", true);
    let result = mgr.maybe_notify(&session, SessionStatus::Waiting, Some("agentorch_s1"), false);
    assert!(!result);
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass (existing + 3 new)

- [ ] **Step 6: Commit**

```bash
git add src/core/session.rs
git commit -m "fix(notify): change maybe_notify to suppress only the attached session

Replace is_attached: bool with attached_session: Option<&str> so
notifications fire for other sessions while attached to one."
```

---

### Task 2: Add `attached_tmux_session` field to `App`

**Files:**
- Modify: `src/app.rs:141-174` (struct definition and `new()`)

- [ ] **Step 1: Add the field to the `App` struct**

In `src/app.rs`, add a new field after `returning_from_attach` (line 148):

Replace:
```rust
    pub returning_from_attach: bool,
```

With:
```rust
    pub returning_from_attach: bool,
    pub attached_tmux_session: Option<String>,
```

- [ ] **Step 2: Initialize the field in `App::new()`**

In `src/app.rs`, add initialization after `returning_from_attach: false,` (line 166):

Replace:
```rust
            returning_from_attach: false,
```

With:
```rust
            returning_from_attach: false,
            attached_tmux_session: None,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1`
Expected: Compilation errors in `main.rs` where `maybe_notify` is called with `false` — that's expected, we fix it in Task 3.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add attached_tmux_session field to App state"
```

---

### Task 3: Wire up attach handler and main loop

**Files:**
- Modify: `src/main.rs:286-320` (drain logic + status processing)
- Modify: `src/main.rs:398-420` (attach handler)

- [ ] **Step 1: Set `attached_tmux_session` before attach and clear on return**

In `src/main.rs`, in the attach handler (around line 402), add the field set. Replace the block:

```rust
                    let tmux_name = session.tmux_session.clone();

                    // Leave TUI for attach
                    disable_raw_mode()?;
```

With:

```rust
                    let tmux_name = session.tmux_session.clone();
                    app.attached_tmux_session = Some(tmux_name.clone());

                    // Leave TUI for attach
                    disable_raw_mode()?;
```

Then after the attach returns and the TUI is restored, clear it. Replace:

```rust
                    // Signal main loop to drain stale status results
                    app.returning_from_attach = true;
```

With:

```rust
                    // Signal main loop to process pending results (not drain them)
                    app.returning_from_attach = true;
                    app.attached_tmux_session = None;
```

- [ ] **Step 2: Replace the drain with processing in the main loop**

In `src/main.rs`, replace the drain block (lines 286-290):

Replace:
```rust
        // After returning from attach, discard stale status results
        if app.returning_from_attach {
            while status_rx.try_recv().is_ok() {}
            app.returning_from_attach = false;
        }
```

With:
```rust
        // After returning from attach, process pending results (don't discard them).
        // The attached session's notifications were already suppressed during the
        // normal processing loop below — we just need to clear the flag.
        if app.returning_from_attach {
            app.returning_from_attach = false;
        }
```

- [ ] **Step 3: Pass `attached_tmux_session` to `maybe_notify` in the main processing loop**

In `src/main.rs`, in the status processing loop (around line 311), replace:

```rust
                    // Skip the expensive get_attached_sessions() subprocess call here;
                    // treat sessions as not attached (slight over-notification is better than lag)
                    session_manager.maybe_notify(session, resolved, false, sound);
```

With:

```rust
                    session_manager.maybe_notify(
                        session,
                        resolved,
                        app.attached_tmux_session.as_deref(),
                        sound,
                    );
```

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo test 2>&1`
Expected: All tests pass. No compilation errors.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "fix(notify): process pending results on return instead of draining

- Set attached_tmux_session before entering tmux, clear on return
- Stop discarding status results accumulated while attached
- Pass attached session identity to maybe_notify so only the
  attached session's notifications are suppressed"
```

---

### Task 4: Handle `--attach` flag with the new field

**Files:**
- Modify: `src/main.rs:168-183` (immediate attach on startup)

The `--attach` handler at lines 168-183 also needs the same treatment. Currently it doesn't set `returning_from_attach` or `attached_tmux_session`.

- [ ] **Step 1: Update the `--attach` handler**

Replace:

```rust
    // Handle --attach: immediately attach to the session
    if let Some(session_id) = app.attach_session.take() {
        if let Some(session) = app.sessions.iter().find(|s| s.id == session_id) {
            if !session.tmux_session.is_empty() {
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                let tmux_name = session.tmux_session.clone();
                let _ = crate::core::tmux::attach_session_sync(&tmux_name);

                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;
            }
        }
    }
```

With:

```rust
    // Handle --attach: immediately attach to the session
    if let Some(session_id) = app.attach_session.take() {
        if let Some(session) = app.sessions.iter().find(|s| s.id == session_id) {
            if !session.tmux_session.is_empty() {
                let tmux_name = session.tmux_session.clone();
                app.attached_tmux_session = Some(tmux_name.clone());

                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

                let _ = crate::core::tmux::attach_session_sync(&tmux_name);
                session_manager.suppress_notification(&tmux_name);

                app.attached_tmux_session = None;

                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;
            }
        }
    }
```

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(notify): handle --attach flag with attached session tracking"
```

---

### Verification

After all tasks, manually test:

1. **Start two sessions** — A and B, both with notifications enabled
2. **Attach to session A** — let session B finish its work (goes Running -> Idle)
3. **Verify** — desktop notification fires for session B while you're inside session A
4. **Verify** — no notification fires for session A while you're looking at it
5. **Detach from session A** — verify home screen shows correct statuses immediately (no 2.5s delay)
6. **Verify** — notification for session A is suppressed for 5 seconds after detach (recently_detached)
