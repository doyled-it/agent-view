# Agent View — Rust Rewrite Design

## Goal

Rewrite agent-view from TypeScript (Bun/SolidJS/OpenTUI) to Rust (ratatui/crossterm). Produce a single static binary with no runtime dependencies, no Gatekeeper issues, and a simpler architecture while preserving all core functionality and adding new features.

## Motivation

- macOS Gatekeeper blocks the `node-pty` native module, breaking PTY and potentially notification functionality
- The SolidJS reactive model (8 contexts, signals, stores, effects) is over-engineered for a TUI polling loop
- Rust produces a single static binary — no runtime, no native module signing issues
- Opportunity to simplify the codebase (~9,600 LOC TypeScript → estimated ~5,000-6,000 LOC Rust)

## What's Preserved

- Session lifecycle: create, stop, restart, delete via tmux
- Group management with expand/collapse
- Claude Code status detection: running, waiting, paused, idle, error, stopped, compacting
- Status debouncing (2s) and error hysteresis (5s)
- Idle prompt detection (`❯` at prompt, question detection for paused vs idle)
- SQLite storage with same schema (v3, WAL mode) — existing databases work as-is
- Desktop notifications with colored emoji per status type (terminal-notifier + osascript fallback)
- Follow-up marks (`i` key)
- Git worktree creation in new session dialog
- Keyboard shortcuts (same keys as current)
- Dark/light themes
- Notification toggle per session (`!` key)
- Session search (`/` key)
- Log export (`e` key)
- Version display in header

## What's Removed

- **Fork sessions** — unused feature, removes 550 LOC (dialog-fork + claude.ts session detection)
- **Multi-tool status patterns** — Claude Code only. Other tools get generic "is tmux active?" detection
- **Tree-sitter syntax highlighting** — preview pane becomes metadata-only
- **Preview pane output capture** — no more capturePane for the preview, just session metadata
- **SolidJS reactivity** — replaced by simple state struct + event loop
- **OpenTUI** — replaced by ratatui
- **node-pty** — tmux handles all PTY needs
- **Dialog stack system** — replaced by enum-based single overlay
- **fuzzysort dependency** — simple substring or tiny fuzzy crate

## What's Simplified

### State Management

Replace 8 SolidJS contexts (Sync, Theme, Keybind, Route, Config, KV, Helper, Layout) with a single `App` struct:

```rust
struct App {
    sessions: Vec<Session>,
    groups: Vec<Group>,
    config: Config,
    theme: Theme,
    selected_index: usize,
    overlay: Overlay,
    activity_feed: VecDeque<ActivityEvent>,
    toasts: Vec<Toast>,
    sort_mode: SortMode,
    search: Option<SearchState>,
}
```

### Event Loop

Tick-based loop (~60fps for input, 500ms for status polling):

```
loop {
    // 1. Poll crossterm events (keyboard, mouse, resize) with 16ms timeout
    // 2. Every 500ms: refresh session statuses from tmux
    // 3. Render current state to terminal via ratatui
}
```

### Overlays

Single enum instead of dialog stack:

```rust
enum Overlay {
    None,
    NewSession(NewSessionForm),
    Rename(RenameForm),
    Move(MoveForm),
    Group(GroupForm),
    Confirm(ConfirmDialog),
    CommandPalette(CommandState),
}
```

## New Features

### 1. Session Sorting

Toggle with `s` key. Cycles through sort modes:
- **Status priority** (default): waiting → paused → running → compacting → idle → stopped → error
- **Last activity**: most recently active first
- **Name**: alphabetical
- **Created**: newest first

Sort applies within each group. Current sort mode shown in footer.

### 2. Status History Sparkline

Each session row shows a tiny sparkline of status transitions over the last hour:

```
⍾ ⚑ ◆ BIS              ▁▃█▃▁   7d Mar 27
```

Sparkline encoding: idle=▁, running=█, waiting=▅, paused=▃, error=▇(red), compacting=▃

Uses ratatui's built-in `Sparkline` widget. Data source: the existing `status_history` array in SQLite (already tracked, currently unused in the UI).

### 3. Bulk Actions

- `space` toggles selection on current session
- `Ctrl+A` selects all visible sessions
- When selection active, footer shows bulk actions: `d` delete all, `m` move all, `s` stop all
- `Escape` clears selection
- Visual indicator: highlighted background on selected rows

### 4. Continuous Session Logging

Background thread streams tmux pane output to log files:

- Path: `~/.agent-view/logs/<session-id>.log`
- Captures every 5 seconds (non-blocking, separate thread)
- Rotates at 10MB per log file (keep last 2 rotations)
- `e` key exports from log file instead of live capture (faster, more complete)
- Searchable with external tools (grep, ripgrep)

### 5. Session Pinning

- `p` key toggles pin on selected session
- Pinned sessions appear at the top of their group, above the sort order
- Pin indicator: `📌` or `▲` prefix
- Stored as `pinned` boolean in SQLite (schema v4 migration)

### 6. Activity Feed

Bottom section of the home screen (3-4 lines, collapsible with `a` key):

```
┌─ Activity ──────────────────────────────────────────┐
│ 2m ago  BIS → paused "Asked you a question"         │
│ 5m ago  irmbpmn → running                           │
│ 12m ago Tag Filter → idle "Completed its task"      │
└─────────────────────────────────────────────────────┘
```

- Rolling log of status transitions with timestamps
- Shows notification message when applicable
- Stored in memory (VecDeque, last 100 events), not persisted
- Populated from status change events during the session

### 7. Quick-Resume from Notification Click

Use `terminal-notifier -execute` flag to run a command when the user clicks a notification:

```
terminal-notifier -title "🔵 BIS" -message "Asked you a question" \
  -execute "agent-view --attach <session-id>"
```

Add `--attach <session-id>` CLI flag that:
1. Starts agent-view
2. Immediately attaches to the specified session
3. On detach, shows the normal home screen

For osascript fallback (which doesn't support click actions), notifications remain non-interactive.

### 8. Session Token/Cost Tracking

Parse Claude Code output for token counts:

- Pattern: `↓ 20.4k tokens`, `... 1.2M tokens`, token count in status line
- Accumulate per-session in a new `tokens_used` SQLite column
- Display in detail panel: "Tokens: 45.2k" 
- Track per-session, reset on restart
- Schema v4 migration adds `tokens_used INTEGER NOT NULL DEFAULT 0`

### 9. Config Hot-Reload

Watch `~/.agent-view/config.json` with the `notify` crate (file system events):

- On change: re-parse config, apply theme and keybind changes
- No restart needed for theme switching or keybind remapping
- Toast notification: "Config reloaded"
- Error handling: if parse fails, keep current config and show error toast

## Crate Dependencies

| Crate | Purpose |
|-------|---------|
| ratatui | TUI framework |
| crossterm | Terminal backend (input, raw mode, alternate screen) |
| rusqlite (bundled) | SQLite with statically linked sqlite3 |
| tokio | Async runtime for subprocess management |
| regex | Status pattern matching |
| serde + serde_json | Config and JSON parsing |
| chrono | Timestamps and duration formatting |
| dirs | Home directory, config/data paths |
| notify | File system watching (config hot-reload) |
| clap + clap_complete | CLI parsing + shell completions |

Desktop notifications: direct `osascript` / `terminal-notifier` via `std::process::Command` (no crate needed — keeps it simple and matches current approach).

## File Structure

```
src/
├── main.rs              — entry point, clap CLI, event loop bootstrap
├── app.rs               — App state, event dispatch, overlay management
├── event.rs             — Event enum (Key, Tick, StatusRefresh, ConfigReload)
├── core/
│   ├── mod.rs
│   ├── tmux.rs          — tmux subprocess wrapper, session cache, output capture
│   ├── status.rs        — Claude Code pattern matching, status detection
│   ├── storage.rs       — SQLite CRUD, migrations (v1→v4)
│   ├── git.rs           — worktree operations, branch validation
│   ├── notify.rs        — desktop notifications (terminal-notifier + osascript)
│   ├── session.rs       — session lifecycle, debouncing, token tracking
│   ├── config.rs        — config loading, hot-reload watcher
│   └── logger.rs        — continuous session log streaming
├── ui/
│   ├── mod.rs
│   ├── home.rs          — session list, detail panel, activity feed, sparklines
│   ├── overlay.rs       — new session, rename, move, group, confirm, command palette
│   ├── theme.rs         — dark/light color schemes
│   ├── footer.rs        — keybind hints (context-sensitive)
│   └── toast.rs         — toast notifications
└── types.rs             — Session, Group, SessionStatus, SortMode, ActivityEvent, etc.
```

## Schema Migration (v4)

```sql
-- v3 → v4
ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN tokens_used INTEGER NOT NULL DEFAULT 0;
```

Backwards compatible — existing v3 databases upgrade transparently.

## Database Compatibility

The Rust version reads and writes the same SQLite database at `~/.agent-orchestrator/state.db`. Users can switch between the TypeScript and Rust versions without data loss. The Rust version applies v4 migration on first run; the TypeScript version ignores unknown columns.

## CLI Interface

```
agent-view [OPTIONS]

Options:
  --light              Use light mode theme
  --attach <ID>        Attach to session immediately (for notification click)
  --version, -v        Show version
  --help, -h           Show help
  --completions <SHELL>  Generate shell completions (bash, zsh, fish)
```

## Build & Distribution

- `cargo build --release` produces a single static binary
- Cross-compile via `cross` for linux-x86_64, linux-aarch64, darwin-x86_64, darwin-aarch64
- CI builds all targets on tag push (same pipeline structure as current)
- Ad-hoc codesign on macOS (same as current, but no native modules to block)
- Install script updated to download Rust binary instead of Bun binary

## Testing Strategy

- **Unit tests**: status pattern matching, storage CRUD, config parsing, git branch validation
- **Integration tests**: tmux session lifecycle (create → status → stop → delete)
- **Snapshot tests**: ratatui supports terminal snapshot testing for UI layout verification
- Test with `cargo test`, same CI integration as current `bun test`

## Migration Path

1. Build Rust version in parallel (new `rust/` directory or separate branch)
2. Keep TypeScript version working until Rust reaches feature parity
3. Swap the compiled binary — same install path, same database, same config
4. Users don't notice the switch (same UI, same keys, same behavior)
