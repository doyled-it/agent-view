# Agent View

Rust/ratatui terminal UI for managing AI coding agent sessions via tmux.

## Build & Test

```bash
cargo build            # Debug build
cargo build --release  # Release build
cargo test             # Run all tests
cargo fmt --check      # Check formatting
cargo clippy -- -D warnings  # Lint (warnings are errors in CI)
```

IMPORTANT: CI runs `rust:latest` which may be newer than the local toolchain. Always run `cargo clippy -- -D warnings` before pushing. If available, use `rustup run nightly cargo clippy -- -D warnings` to catch upcoming lint changes.

## Code Style

- Run `cargo fmt` and `cargo clippy -- -D warnings` before every commit
- Pre-commit hooks enforce fmt and clippy — do not skip with `--no-verify`
- Prefer match guards over if-blocks inside match arms (clippy `collapsible_match`)

## Architecture

- `src/core/` — business logic, storage, tmux integration (no UI imports)
- `src/ui/` — ratatui rendering only (no mutation of app state)
- `src/input/` — keyboard handlers that mutate `App` state
- `src/app.rs` — central `App` struct, overlay enums, command palette
- `src/types.rs` — shared types used across modules

## Key Patterns

- Overlays (dialogs) are rendered in `src/ui/overlay.rs`, input handled in `src/input/session.rs` and `src/input/overlay.rs`
- Session status is detected by parsing tmux pane output in `src/core/status.rs`
- All storage goes through `src/core/storage.rs` (SQLite via rusqlite with bundled feature)
- Themes are defined in `src/ui/theme.rs` — all colors come from the `Theme` struct, never hardcoded
- Usage tracking runs a hidden `__agentview_meta_usage` tmux session managed by `src/core/usage.rs` — parser, monitor thread, and shared state via `Arc<Mutex<>>`. Sessions prefixed with `__agentview_meta_` are filtered from the UI and poller.

## Testing

- Tests live alongside source in `#[cfg(test)] mod tests` blocks
- Storage tests use in-memory SQLite (`:memory:`)
- No mocking framework — use real implementations where possible
