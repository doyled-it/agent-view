# Status Processing Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move all status processing (resolve, track, notify) from the blocked main thread to the always-running background thread so notifications fire while attached and statuses update immediately on return.

**Architecture:** The background thread owns a `StatusProcessor` that handles debouncing, duration tracking, and notifications. It writes resolved statuses to SQLite and calls `touch()`. The main thread polls storage mtime every 200ms and reloads the UI when it changes. An `Arc<Mutex<AttachState>>` communicates the attached session name from main to background for notification suppression. Session lifecycle operations (create/stop/delete/restart) remain on the main thread.

**Tech Stack:** Rust, SQLite (rusqlite), std::sync (Arc, Mutex), std::thread, crossterm, ratatui

**Spec:** `docs/superpowers/specs/2026-04-10-status-processing-refactor-design.md`

---

### Important Context

- `cargo test` requires `export PATH="$HOME/.cargo/bin:$PATH"` on this machine
- The background thread already opens its own `Storage` connection each tick (`Storage::open_default()`)
- `Storage::touch()` writes the current timestamp to the `metadata` table
- `Storage::get_meta("last_modified")` reads it back
- `SessionManager` currently bundles status processing AND session lifecycle in one struct
- The `session_manager` variable is passed to `handle_new_session_key`, `handle_confirm_key`, `handle_palette_key` for lifecycle operations (create, stop, delete, restart)
- Lines referenced are approximate — they shift as earlier tasks modify the file

---

### Task 1: Split SessionManager into StatusProcessor and session lifecycle functions

**Files:**
- Modify: `src/core/session.rs`

This task splits the monolithic `SessionManager` into two parts. `StatusProcessor` owns all status tracking state and methods (resolve, track, notify, suppress). The lifecycle methods become functions on a new `SessionOps` struct that has no state (just groups the methods).

- [ ] **Step 1: Rename `SessionManager` to `StatusProcessor` and extract `SessionOps`**

In `src/core/session.rs`, rename the struct and its `impl` block. Then create a new struct for lifecycle operations. The full replacement of the struct definition and impl blocks:

Replace the struct definition (lines 36-52):
```rust
/// Tracks debounce and notification state for status processing.
/// Lives in the background thread — processes status every poll cycle.
pub struct StatusProcessor {
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
```

Replace `impl SessionManager` (line 63) with `impl StatusProcessor`. Keep all the methods: `new()`, `suppress_notification()`, `resolve_status()`, `track_durations()`, `maybe_notify()`. Do NOT include `create_session`, `stop_session`, `delete_session`, `restart_session` — those move to `SessionOps`.

After the `StatusProcessor` impl block, add:

```rust
/// Session lifecycle operations (create, stop, delete, restart).
/// Stateless — lives on the main thread.
pub struct SessionOps;

impl SessionOps {
```

Move `create_session`, `stop_session`, `delete_session`, `restart_session` into `impl SessionOps`. Change them from `&self` to `&self` (they already don't use any `self` fields — they only use `storage` and `cache` parameters). The method signatures stay the same.

- [ ] **Step 2: Update all `SessionManager` references to the new names**

In `src/core/session.rs` tests, replace `SessionManager::new()` with `StatusProcessor::new()`.

- [ ] **Step 3: Update `src/main.rs` references**

Replace all occurrences:
- `SessionManager::new()` → `StatusProcessor::new()` (for the status processor)
- Add `let session_ops = crate::core::session::SessionOps;` for lifecycle operations
- `session_manager.create_session(` → `session_ops.create_session(`
- `session_manager.stop_session(` → `session_ops.stop_session(`
- `session_manager.delete_session(` → `session_ops.delete_session(`
- `session_manager.restart_session(` → `session_ops.restart_session(`
- Keep `session_manager.resolve_status(`, `session_manager.track_durations(`, `session_manager.maybe_notify(`, `session_manager.suppress_notification(` pointing to the StatusProcessor instance (these move to the background thread in Task 3)

Update function signatures that accept `&mut SessionManager` or `&SessionManager`:
- `handle_main_key`: change parameter type to `session_ops: &crate::core::session::SessionOps`
- `handle_new_session_key`: change parameter type to `session_ops: &crate::core::session::SessionOps`
- `handle_confirm_key`: change parameter type to `session_ops: &crate::core::session::SessionOps`
- `handle_palette_key`: change parameter type to `session_ops: &mut crate::core::session::SessionOps` (some callers pass `&mut`)

Note: `suppress_notification` is still called from the main thread in the attach handler. For now, keep a `session_manager` (StatusProcessor) on the main thread — Task 3 will move it.

- [ ] **Step 4: Run tests**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1`
Expected: All 109 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/core/session.rs src/main.rs
git commit -m "refactor(session): split SessionManager into StatusProcessor and SessionOps

StatusProcessor owns status debouncing, duration tracking, and
notification state. SessionOps handles session lifecycle (create,
stop, delete, restart). Prepares for moving status processing
to the background thread."
```

---

### Task 2: Add `last_modified()` method to Storage and `AttachState` struct

**Files:**
- Modify: `src/core/storage.rs`
- Create: `src/core/attach_state.rs`
- Modify: `src/core/mod.rs` (to add `pub mod attach_state;`)

- [ ] **Step 1: Add `last_modified()` to Storage**

In `src/core/storage.rs`, add this method after the existing `touch()` method (after line 496):

```rust
    /// Read the last_modified timestamp from metadata.
    /// Returns 0 if not set.
    pub fn last_modified(&self) -> i64 {
        self.get_meta("last_modified")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    }
```

- [ ] **Step 2: Create `AttachState` struct**

Create `src/core/attach_state.rs`:

```rust
//! Shared state between main thread and background thread for attach tracking.

/// Communicates attach state from the main thread to the background thread.
/// Protected by Arc<Mutex<>>.
pub struct AttachState {
    /// Which tmux session the user is currently inside (None = on home screen)
    pub attached_session: Option<String>,
    /// Tmux session names to add to recently_detached suppression
    pub suppress_queue: Vec<String>,
}

impl AttachState {
    pub fn new() -> Self {
        Self {
            attached_session: None,
            suppress_queue: Vec::new(),
        }
    }
}
```

- [ ] **Step 3: Register the module**

In `src/core/mod.rs`, add:

```rust
pub mod attach_state;
```

- [ ] **Step 4: Run tests**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/core/storage.rs src/core/attach_state.rs src/core/mod.rs
git commit -m "feat(core): add Storage::last_modified() and AttachState struct"
```

---

### Task 3: Move StatusProcessor into the background thread

**Files:**
- Modify: `src/main.rs` (major rewrite of background thread and main loop)

This is the core refactor. The background thread gets its own `StatusProcessor` and `Storage`. The main loop drops the mpsc channel and status processing block, replacing them with storage mtime polling.

- [ ] **Step 1: Rewrite the background thread**

Replace the entire background thread (from `let bg_sound = ...` through the `});` that closes the thread spawn) with:

```rust
    let bg_sound = config.notifications.sound;
    let _bg_handle = std::thread::spawn(move || {
        let mut cache = crate::core::tmux::SessionCache::new();
        let mut processor = crate::core::session::StatusProcessor::new();

        loop {
            std::thread::sleep(Duration::from_millis(500));

            // Open fresh storage connection each tick
            let bg_storage = match crate::core::storage::Storage::open_default() {
                Ok(s) => { let _ = s.migrate(); s }
                Err(_) => continue,
            };
            let sessions = bg_storage.load_sessions().unwrap_or_default();

            // Read attach state from main thread
            let attach_state = attached_for_bg.lock().ok().map(|g| {
                (g.attached_session.clone(), std::mem::take(&mut *g).suppress_queue)
            });
            let (attached, suppress_queue) = attach_state.unwrap_or((None, vec![]));

            // Process suppress queue from main thread
            for tmux_name in suppress_queue {
                processor.suppress_notification(&tmux_name);
            }

            cache.refresh();
            let mut any_changed = false;

            for session in &sessions {
                if session.tmux_session.is_empty() {
                    continue;
                }

                // Detect raw status
                let raw_status = if !cache.session_exists(&session.tmux_session) {
                    if session.status != crate::types::SessionStatus::Stopped {
                        crate::types::SessionStatus::Stopped
                    } else {
                        continue;
                    }
                } else {
                    let is_active = cache.is_session_active(&session.tmux_session, 2);
                    match crate::core::tmux::capture_pane(&session.tmux_session, Some(-100)) {
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
                    }
                };

                // Resolve status (debouncing + hysteresis)
                let previous = session.status;
                let resolved = processor.resolve_status(&session.id, raw_status, previous);

                // Write to DB if changed
                if resolved != previous {
                    let _ = bg_storage.write_status(&session.id, resolved, session.tool);
                    any_changed = true;
                }

                // Track durations and fire notifications
                processor.track_durations(&session.id, resolved);
                processor.maybe_notify(session, resolved, attached.as_deref(), bg_sound);
            }

            if any_changed {
                let _ = bg_storage.touch();
            }
        }
    });
```

- [ ] **Step 2: Replace the main loop status processing with mtime polling**

Remove these blocks from the main loop:
1. The `returning_from_attach` block (lines ~359-364)
2. The entire status processing block (lines ~366-399): `let mut any_changed = false; while let Ok(results) = status_rx.try_recv() { ... } if any_changed { ... }`

Replace both with:

```rust
        // Poll storage for changes from the background thread
        {
            let current_mtime = storage.last_modified();
            if current_mtime != app.last_storage_mtime {
                app.last_storage_mtime = current_mtime;
                app.sessions = storage.load_sessions().unwrap_or_default();
                app.groups = storage.load_groups().unwrap_or_default();
                app.rebuild_list_rows();
            }
        }
```

- [ ] **Step 3: Update variable setup at the top of `run_tui`**

Remove:
- `let mut session_manager = crate::core::session::StatusProcessor::new();` — moves to bg thread
- `let (status_tx, status_rx) = mpsc::channel::<...>();` — no longer needed
- `use std::sync::mpsc;` — no longer needed (keep `Arc` and `Mutex`)

Change the `attached_session_shared` from `Arc<Mutex<Option<String>>>` to `Arc<Mutex<AttachState>>`:

```rust
    use std::sync::{Arc, Mutex};
    let attach_state: Arc<Mutex<crate::core::attach_state::AttachState>> =
        Arc::new(Mutex::new(crate::core::attach_state::AttachState::new()));
    let attached_for_bg = Arc::clone(&attach_state);
```

Add `SessionOps`:
```rust
    let session_ops = crate::core::session::SessionOps;
```

- [ ] **Step 4: Update the attach handler in `handle_main_key`**

Replace the attach state setting/clearing with AttachState operations:

Before attach:
```rust
                    if let Ok(mut guard) = attached_session_shared.lock() {
                        *guard = Some(tmux_name.clone());
                    }
```
Becomes:
```rust
                    if let Ok(mut guard) = attach_state_shared.lock() {
                        guard.attached_session = Some(tmux_name.clone());
                    }
```

After attach (replacing both `suppress_notification` call and the attach state clear):
```rust
                    if let Ok(mut guard) = attach_state_shared.lock() {
                        guard.suppress_queue.push(tmux_name.clone());
                        guard.attached_session = None;
                    }
```

Remove the direct `session_manager.suppress_notification(&tmux_name);` call — the background thread now reads `suppress_queue` and handles it.

Remove `app.returning_from_attach = true;` and `app.attached_tmux_session = None;`.

Update the `handle_main_key` signature: replace the `attached_session_shared` parameter with `attach_state_shared: &Arc<Mutex<crate::core::attach_state::AttachState>>` and change `session_manager: &mut StatusProcessor` to `session_ops: &crate::core::session::SessionOps`.

- [ ] **Step 5: Update the `--attach` handler**

Same pattern as the attach handler. Replace the `attached_session_shared.lock()` calls with `attach_state.lock()` operating on the `AttachState` fields. Remove direct `session_manager.suppress_notification()` call.

- [ ] **Step 6: Add `last_storage_mtime` field to App**

In `src/app.rs`, add:
```rust
    pub last_storage_mtime: i64,
```
Initialize to `0` in `App::new()`.

Remove `returning_from_attach: bool` and `attached_tmux_session: Option<String>` fields.

- [ ] **Step 7: Update handler function signatures and call sites**

All handler functions that took `session_manager: &mut SessionManager` or `&SessionManager` now take `session_ops: &SessionOps`. The handlers that called status processing methods (resolve, track, notify) no longer do — those only happen in the background thread.

Update the call sites in the main event loop to pass `&session_ops` instead of `&mut session_manager`.

- [ ] **Step 8: Fix the AttachState `take` in background thread**

The background thread drains `suppress_queue` using `std::mem::take`. This needs a mutable reference. Fix the lock pattern:

```rust
            let (attached, suppress_queue) = if let Ok(mut guard) = attached_for_bg.lock() {
                let attached = guard.attached_session.clone();
                let queue = std::mem::take(&mut guard.suppress_queue);
                (attached, queue)
            } else {
                (None, vec![])
            };
```

- [ ] **Step 9: Run tests**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 10: Build release binary**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build --release 2>&1`
Expected: Compiles with only pre-existing warnings.

- [ ] **Step 11: Commit**

```bash
git add src/main.rs src/app.rs
git commit -m "refactor(status): move status processing to background thread

The background thread now owns StatusProcessor and handles all status
resolution, duration tracking, and notifications. The main thread
polls storage mtime (200ms) and reloads the UI when it changes.

This fixes:
- Notifications not firing while attached to a session
- Wrong duration tracking when processing backlogged results
- Completed notification requiring idle time on the main thread

Communication between threads uses Arc<Mutex<AttachState>> for
the attached session name and notification suppression queue."
```

---

### Task 4: Clean up dead code

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Remove unused imports and variables**

Remove any unused `mpsc` imports, the old `bg_last_notified` HashMap, the old `status_tx`/`status_rx` channel, and any remaining references to `attached_tmux_session` or `returning_from_attach` in `app.rs`.

- [ ] **Step 2: Run tests and build**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build --release 2>&1`
Expected: All tests pass, binary builds.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs src/app.rs
git commit -m "chore: remove dead code from status processing refactor"
```

---

### Verification

After all tasks, build the release binary and test manually:

1. `export PATH="$HOME/.cargo/bin:$PATH" && cargo build --release`
2. `./target/release/agent-view`
3. **Create two sessions** with notifications enabled (`!` to toggle)
4. **Attach to session A**, let session B finish (Running → Idle after 10+ seconds)
5. **Verify:** "completed" notification fires while you're still in session A
6. **Verify:** detach from A, home screen shows correct statuses immediately
7. **Create a session**, let it go to Waiting (approval prompt)
8. **Attach to another session**, verify Waiting notification fires while attached
9. **Verify:** no notification fires for the session you're currently inside
