# Move Status Processing to Background Thread

## Problem

Status resolution, duration tracking, and notifications all live on the main thread, which blocks during `attach_session_sync()`. This causes three failures:

1. **No notifications while attached.** `maybe_notify` is called from the main event loop, which is frozen inside `attach_session_sync()`. The background thread notification patch only covers Waiting/Paused/Error — "completed" (Running → Idle) can't fire because it needs duration tracking.

2. **Wrong duration tracking on return.** When the main loop resumes after detach, it processes the entire queued backlog in a tight loop. `track_durations` uses `Instant::now()` — so a session that ran for 30 seconds and was idle for 60 seconds looks like it ran for 2ms. `was_running_enough` is false, "completed" notification never fires.

3. **8-second idle delay only counts while main loop runs.** The idle duration timer starts when the main loop first processes an Idle result, not when the session actually went idle. If the session went idle while the user was attached, the 8-second countdown only starts after detach.

## Architecture

### Before (broken)

```
Background Thread         Channel              Main Thread (BLOCKS during attach)
  detect raw status  ───> mpsc queue ────>  resolve_status()
                                             track_durations()  ← Instant::now()
                                             maybe_notify()
                                             write_status()
                                             reload UI
```

### After

```
Background Thread (always running)           Main Thread
  detect raw status                           check storage mtime (200ms)
  resolve_status()                            reload sessions if changed
  track_durations()                           render UI
  maybe_notify()                              handle input
  write_status() + touch()                    set/clear attach state
```

## What Moves to the Background Thread

`SessionManager`'s status processing moves entirely into the background thread. Specifically:

- `resolve_status()` — debouncing and error hysteresis
- `track_durations()` — running/idle duration tracking
- `maybe_notify()` — notification dispatch with attached session suppression
- `storage.write_status()` + `storage.touch()` — persist resolved status

These run on every poll cycle (500ms), continuously, regardless of whether the user is on the home screen or attached to a session.

## Communication Between Threads

### Background → Main: Storage mtime

The background thread writes resolved statuses to SQLite and calls `storage.touch()`. The main loop polls `storage.last_modified()` every 200ms (matching the TypeScript implementation). When the mtime changes, it reloads sessions and rebuilds the UI. No mpsc channel needed for status updates.

### Main → Background: AttachState

Two pieces of information flow from main to background:

1. `attached_session: Option<String>` — which tmux session the user is inside
2. `suppress_queue: Vec<String>` — tmux names to suppress after detach

Shared via `Arc<Mutex<AttachState>>`:

```rust
struct AttachState {
    attached_session: Option<String>,
    suppress_queue: Vec<String>,
}
```

The background thread reads `attached_session` and passes it to `maybe_notify`. It drains `suppress_queue` into `SessionManager::recently_detached` each tick.

## What Stays on the Main Thread

- **Session lifecycle:** `create_session`, `stop_session`, `delete_session`, `restart_session` — these create/kill tmux sessions and must happen in response to user input
- **UI rendering** and input handling
- **Storage mtime polling** for UI refresh
- **Attach/detach handling** — sets AttachState before entering tmux, clears after

## What Changes

### `src/main.rs`

- **Remove** the mpsc channel (`status_tx`/`status_rx`)
- **Remove** the status processing loop (`while let Ok(results) = status_rx.try_recv()`)
- **Remove** `returning_from_attach` flag and its handling
- **Remove** the inline background thread notification code (the patch from earlier)
- **Add** `Arc<Mutex<AttachState>>` shared between threads
- **Add** storage mtime polling (check every 200ms, reload sessions + rebuild UI if changed)
- **Move** `SessionManager` (status processing parts) into the background thread closure
- **Background thread** now calls `resolve_status`, `track_durations`, `maybe_notify`, and `storage.write_status` directly
- **Attach handler** sets `attached_session` on the shared AttachState before entering tmux, adds to `suppress_queue` and clears `attached_session` after returning

### `src/core/session.rs`

- Split `SessionManager` into two structs:
  - `StatusProcessor` — owns all status processing state (debounce timers, duration tracking, notification history, recently_detached). Lives in the background thread.
  - `SessionLifecycle` — owns session CRUD operations (create, stop, delete, restart). Lives on the main thread. These methods don't need any of the status tracking state.

### `src/app.rs`

- **Remove** `returning_from_attach: bool` field
- **Remove** `attached_tmux_session: Option<String>` field (replaced by shared AttachState)

## Notification Behavior

| Scenario | What happens |
|---|---|
| Home screen, session B goes Waiting | BG detects, resolves immediately (no debounce), fires notification, writes DB. Main sees mtime change within 200ms, reloads UI. |
| Attached to A, session B goes Waiting | BG detects, resolves, fires notification (B != attached). User hears it in tmux. |
| Attached to A, session A goes Waiting | BG detects, resolves, suppresses notification (A == attached). |
| Attached to A, session B completes | BG tracks real-time durations continuously. After 10s running + 8s idle, fires "completed" notification while user is still in A. |
| Detach from A, return to home screen | Main sets attached=None, adds A to suppress_queue. Next mtime check refreshes UI with current state. No backlog, no drain, no stale timestamps. |

## Testing

### Unit Tests

- `StatusProcessor::resolve_status` — same tests as before (debounce, hysteresis, immediate statuses)
- `StatusProcessor::track_durations` — same tests as before
- `StatusProcessor::maybe_notify` with `attached_session` — same tests as before
- `SessionLifecycle` — no new tests needed (methods unchanged)

### Integration / Manual Tests

- Attach to session A, wait for session B to go Waiting → notification fires while attached
- Attach to session A, wait for session B to complete (Running → Idle, 10s+) → "completed" notification fires while attached
- Return to home screen → statuses are immediately correct
- Session A notification suppressed for 5s after detach
- Create/stop/delete/restart sessions work as before
