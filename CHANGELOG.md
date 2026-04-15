# Changelog

## [1.1.0] - 2026-04-15

### Changed

- feat: path autocomplete and terminal preview pane (#7) (@doyled-it)

## [1.0.0] - 2026-04-15

### Changed

- feat!: v1.0.0 — session notes, crash recovery, Claude resume (#5) (@doyled-it)

## [1.0.0] - 2026-04-14

### Added

- Complete rewrite from TypeScript to Rust
- 12 themes with live-preview selector
- Session notes (mini-journal per session)
- Tmux crash recovery with Claude Code conversation resume
- 24-hour time-bucketed activity timeline
- Session uptime tracking (last_started_at)
- Session pinning, bulk operations, follow-up flags
- Activity feed with real-time status transitions
- Sort modes, group reordering, search, command palette
- Token usage tracking for Claude sessions
- Continuous session logging with rotation
- GitHub Actions CI (fmt, clippy, test, build)
- GitHub Actions version bump (PR label-driven) and release workflows
- GitLab CI mirroring with tag-reactive releases
- Pre-commit hooks for cargo fmt + clippy
- Cross-platform release builds (linux/darwin, x64/arm64)
- 184+ tests

### Changed

- Binary is now Rust-compiled (no Bun/Node runtime)
- Config format uses snake_case keys
- SQLite schema v6 (auto-migrates from earlier versions)

### Fixed

- Post-attach cursor now returns to the session you detached from
- Background fills with theme color (no terminal default bleed)