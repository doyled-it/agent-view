# Changelog

## [1.15.0] - 2026-06-03

### Changed

- feat(sessions): add waiting marker (#65) (@doyled-it)

## [1.14.0] - 2026-06-03

### Changed

- feat(runner): add OpenCode runner (#64) (@doyled-it)

## [1.13.1] - 2026-06-01

### Changed

- fix(ui): align Codex quota pane body inset (#63) (@doyled-it)

## [1.13.0] - 2026-05-22

### Changed

- feat(cost): top-level Costs tab with per-runner / per-model / top-sessions breakdowns (#60) (@doyled-it)

## [1.12.0] - 2026-05-21

### Changed

- feat(runner): GeminiRunner — replace Tool::Gemini fallback with hook-driven runner (#57) (@doyled-it)

## [1.11.3] - 2026-05-21

### Changed

- ci: build x86_64-apple-darwin via cross-compile from macos-latest (#59) (@doyled-it)

## [1.11.2] - 2026-05-20

### Changed

- ci: explicitly install rustup target before build (#58) (@doyled-it)

## [1.11.1] - 2026-05-20

### Changed

- ci: native-only release matrix across 6 Linux/macOS targets (#56) (@doyled-it)

## [1.11.0] - 2026-05-20

### Changed

- feat(cost): Pricer + USD computation for cost_events (#55) (@doyled-it)

## [1.10.1] - 2026-05-16

### Changed

- fix(runner): codex draft and running status misreads (#54) (@doyled-it)

## [1.10.0] - 2026-05-12

### Changed

- feat(runner): CodexRunner with notify hooks + stale-sid resume safeguard (#47) (#53) (@doyled-it)

## [1.9.0] - 2026-05-10

### Changed

- feat(runner): runner picker in new-session overlay + ShellRunner (#46) (#52) (@doyled-it)

## [1.8.0] - 2026-05-09

### Changed

- feat(runner): hook-driven status detection + cost extraction (#45) (#51) (@doyled-it)

## [1.7.0] - 2026-05-08

### Changed

- feat: worktree-aware sessions (#38) (@doyled-it)

## [1.6.0] - 2026-05-06

### Changed

- refactor(app): group App fields into focused substructs (#43) (@doyled-it)

## [1.5.5] - 2026-05-01

### Changed

- fix(groups): unstick reorder on tied sort_orders, add d-key delete (#26) (@doyled-it)

## [1.5.4] - 2026-04-30

### Changed

- fix(usage): stop monitor warnings from bleeding into attached sessions (#19) (@doyled-it)

## [1.5.3] - 2026-04-30

### Changed

- fix(status): show monitoring instead of paused when monitor is attached (#22) (@doyled-it)

## [1.5.2] - 2026-04-30

### Changed

- fix(usage): make /usage parsing robust to partial renders and scrollback (#21) (@doyled-it)

## [1.5.1] - 2026-04-30

### Changed

- feat(status): add monitoring session state (#18) (@doyled-it)

## [1.5.0] - 2026-04-30

### Changed

- feat: add Anthropic status block and harden usage poller (#17) (@doyled-it)

## [1.4.3] - 2026-04-28

### Changed

- fix(groups): keep cursor on group title when reordering (#16) (@doyled-it)

## [1.4.2] - 2026-04-23

### Changed

- fix(usage): parse new Claude Code /usage format (#15) (@doyled-it)

## [1.4.1] - 2026-04-23

### Changed

- fix(ui): align activity feed columns (#14) (@doyled-it)

## [1.4.0] - 2026-04-22

### Changed

- feat(groups): add group deletion and fix activity feed names (#11) (@doyled-it)

## [1.3.0] - 2026-04-22

### Changed

- feat(usage): display account-level token usage in header (#12) (@doyled-it)

## [1.2.0] - 2026-04-21

### Changed

- feat: add scheduled routines system (#10) (@doyled-it)

## [1.1.2] - 2026-04-16

### Changed

- chore: remove legacy TypeScript codebase and JS tooling (#9) (@doyled-it)

## [1.1.1] - 2026-04-16

### Changed

- fix(ui): render autocomplete completions as multi-column grid (#8) (@doyled-it)

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