# Fix Status Polling and Notifications While Attached

## Problem

When the user is attached to a tmux session, two things go wrong:

1. **Status results are discarded on return.** The drain at `main.rs:288` (`while status_rx.try_recv().is_ok() {}`) throws away all status transitions that happened while attached. The UI doesn't reflect reality until the next poll cycle (500ms) plus debounce (2000ms) — up to 2.5 seconds of stale UI.

2. **Notifications don't fire for other sessions.** The `is_attached` parameter in `maybe_notify` is hardcoded to `false` (to skip the expensive `tmux list-clients` subprocess), and the 5-second `recently_detached` window blocks notifications after return. No notifications fire while attached or immediately after detaching.

## Desired Behavior

- While attached to session A, if session B changes status (e.g., finishes, needs approval), a desktop notification fires immediately.
- Notifications for the session the user is currently looking at (session A) are suppressed — the user can already see what's happening.
- On return to the home screen, all session statuses are correct immediately — no delay, no stale data.
- The `recently_detached` suppression only applies to the session the user just left, not all sessions.

## Changes

### 1. Process pending results instead of draining them

**File:** `src/main.rs` (lines 287-290)

Replace the blind drain with normal result processing. When `returning_from_attach` is true, iterate pending results and apply them: resolve status, track durations, trigger notifications. Pass the attached session's tmux name so notifications are suppressed only for that session.

Before:
```rust
if app.returning_from_attach {
    while status_rx.try_recv().is_ok() {}
    app.returning_from_attach = false;
}
```

After: process results the same way the main loop does, but with the attached session identity available for notification suppression.

### 2. Change `maybe_notify` signature from `is_attached: bool` to `attached_session: Option<&str>`

**File:** `src/core/session.rs` (line 184)

Instead of a global boolean, accept the tmux session name of the session the user is currently inside (or `None` if on the home screen). Compare it against the session being evaluated:

- If the session being checked matches `attached_session` — suppress notification (user is looking at it)
- If it doesn't match — allow notification (user can't see this session)

This replaces the early return at line 191-193 (`if is_attached { return false }`).

### 3. Track the attached session name

**File:** `src/main.rs`

Add a field to `App` (or pass through the event loop) that stores which tmux session the user is attached to. Set it before entering attach, clear it on return. The main processing loop and the return-from-attach processing both use this value when calling `maybe_notify`.

### 4. No changes to `recently_detached`

The existing `suppress_notification` call at `main.rs:417` already passes the specific tmux session name. The `recently_detached` HashMap in `session.rs:196-201` already checks per-session. This correctly suppresses only the session the user just detached from. No change needed.

## What doesn't change

- Background polling thread continues running during attach — no pause mechanism
- Status debouncing (2000ms) and error hysteresis (5000ms) unchanged
- Duration tracking for completion notifications unchanged
- The skip of `tmux list-clients` stays gone — the `attached_session` parameter replaces it without subprocess overhead

## Key Files

| File | What changes |
|------|-------------|
| `src/main.rs:287-290` | Replace drain with process loop |
| `src/main.rs:295-320` | Pass `attached_session` to `maybe_notify` |
| `src/main.rs:391-434` | Store attached session name before attach, clear on return |
| `src/core/session.rs:184-268` | Change `maybe_notify` signature and suppression logic |
| `src/app.rs` | Add `attached_tmux_session: Option<String>` field |

## Testing

### Unit Tests

- `maybe_notify` with `attached_session: Some("session-a")` — verify session A is suppressed, session B is not
- `maybe_notify` with `attached_session: None` — verify all sessions can notify (home screen behavior)
- Process-on-return: verify status transitions from pending results are applied, not discarded

### Manual Tests

- Attach to session A, wait for session B to change status (Running -> Waiting) — verify notification fires while still inside session A
- Attach to session A, session A changes status — verify NO notification fires (user can see it)
- Detach from session A — verify home screen immediately shows correct statuses for all sessions
- Detach from session A — verify notification for session A is suppressed for 5 seconds (recently_detached), but notifications for session B fire immediately
