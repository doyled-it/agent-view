# Agent View

Rust/ratatui terminal UI for managing AI coding agent sessions via tmux.

## Build & Test

```bash
cargo fmt --check                                      # Check formatting
cargo clippy --all-targets --all-features -- -D warnings  # Lint all targets/features
cargo test --all-features                              # Run all tests/features
cargo build --release --all-features                   # Release build
```

The Rust toolchain is pinned in `rust-toolchain.toml` — both local and CI run the same `cargo`/`clippy`/`rustfmt`, so a green local `cargo clippy --all-targets --all-features -- -D warnings` matches CI exactly. Bump the pin periodically (every release cycle or sooner). When you bump, run clippy + tests on a feature branch and resolve any new lints in the same PR — that surfaces new lint sets before they hit other contributors.

`AGENTS.md`, `prek.toml`, GitHub Actions, and GitLab CI should stay in sync
with the commands above. If one command changes, update all four
places in the same commit.

## Code Style

- Use `prek` as the Rust-native hook runner for `prek.toml`.
- Install hooks with `cargo install --locked prek && prek install`.
- Run `prek run --all-files` before every commit when available; otherwise run the four commands above directly.
- Hooks mirror CI — do not skip with `--no-verify`
- Prefer match guards over if-blocks inside match arms (clippy `collapsible_match`)

## Architecture

- `src/core/` — business logic, storage, tmux integration (no UI imports)
- `src/ui/` — ratatui rendering only (no mutation of app state)
- `src/input/` — keyboard handlers that mutate `App` state
- `src/app.rs` — central `App` struct, overlay enums, command palette
- `src/types.rs` — shared types used across modules

## Runner trait

Each supported tool (Claude, Codex, Shell, plus fallbacks) implements the `Runner` trait in `src/core/runner/mod.rs`. Per-tool code lives under its own subdirectory (`src/core/runner/claude/`, `src/core/runner/codex/`). The trait covers launch command, status parsing, session-id extraction, restart command, and an idempotent `install_hooks` invoked at startup over `implemented_tools()`. To add a tool, create a new subdir, impl `Runner`, register in `runner_for(Tool::…)`, and the startup loop wires hook installation automatically.

Status is composed in `compose_status` (runner/mod.rs) with three tiers: fresh-hook (Running/Waiting/Compacting authoritative; Idle allows parse_status to add a Draft/Paused/Monitoring overlay), then pane-title marker, then full regex via `resolve_session_status`. Hook freshness window is 1.1s.

## Key Patterns

- Overlays (dialogs) are rendered in `src/ui/overlay.rs`, input handled in `src/input/session.rs` and `src/input/overlay.rs`
- Session status flows through the Runner trait: notify/Claude hooks write to `~/.agent-orchestrator/hooks/<session_id>.json`, the poller reads those plus `tmux capture-pane` and feeds both to `compose_status`
- All storage goes through `src/core/storage/` (SQLite via rusqlite with bundled feature; one file per table — `sessions.rs`, `routines.rs`, `runs.rs`, etc.)
- Themes are defined in `src/ui/theme.rs` — all colors come from the `Theme` struct, never hardcoded
- Usage tracking runs a hidden `__agentview_meta_usage` tmux session managed by `src/core/usage/` — parser, monitor thread, and shared state via `Arc<Mutex<>>`. Sessions prefixed with `__agentview_meta_` are filtered from the UI and poller.

### Pane scraping pitfalls

TUI-style runners (Codex) draw with absolute cursor positioning and pad the bottom of `tmux capture-pane` with blank lines. **Always strip trailing whitespace-only lines before applying a tail window or computing a `skip` offset** — otherwise the prompt/content sits outside the scan area and detection silently never fires. See `src/core/runner/codex/mod.rs::parse_status` and `src/ui/detail/{session,routine}.rs::render_preview`.

## Testing

- Tests live alongside source in `#[cfg(test)] mod tests` blocks
- Storage tests use in-memory SQLite (`:memory:`)
- No mocking framework — use real implementations where possible
