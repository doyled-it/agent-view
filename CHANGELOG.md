# Changelog

## [0.1.0] - 2026-04-03

Added
- Session count in header showing total, running, and waiting sessions (!1) (@mdoyle)
- Relative status timestamps (e.g., “Waiting - 5m ago”) (!1) (@mdoyle)
- Confirmation dialog before deleting sessions or groups (!1) (@mdoyle)
- Dynamic title truncation that only occurs when overlapping with the preview pane (!1) (@mdoyle)
- Desktop notifications (opt‑in per session) on status change to waiting or error, with sound support for macOS (osascript) and Linux (notify‑send) (!1) (@mdoyle)
- Notification toggle shortcut `!` on dashboard, checkbox in new session dialog, and `*` indicator (!1) (@mdoyle)
- Session duration elapsed time displayed in session list rows (!1) (@mdoyle)
- Session metrics panel showing duration, restart count, and time‑in‑status breakdown (!1) (@mdoyle)
- Log export (`e`) capturing full tmux scrollback to `~/.agent-view/logs/` with ANSI stripping (!1) (@mdoyle)
- Output search (`/`) with navigation (`n`/`N`), match highlighting, and context lines (!1) (@mdoyle)

Changed
- Rotate `debug.log` on startup when it exceeds 1 MB (!1) (@mdoyle)
- Migrate storage schema v1 → v2 adding `notify`, `statusChangedAt`, `restartCount`, `statusHistory` columns (!1) (@mdoyle)

Fixed
- Prevent multi‑user conflicts on shared machines by using per‑user signal file (!1) (@mdoyle)
- Unbind keys in a `finally` block to avoid sticky keybinds after crashes (!1) (@mdoyle)
- Distinguish idle prompt from waiting for approval and handle trailing content on Claude prompt lines (!1) (@mdoyle)
- Add paused status for sessions asking a question (!1) (@mdoyle)
- Fix status flickering and ensure notifications are only sent on meaningful status changes (!1) (@mdoyle)
- Debounce notifications to prevent repeats from rapid status changes (!1) (@mdoyle)
- Suppress notifications when user is attached to a session or after detaching (!1) (@mdoyle)
- Require sustained idle/running periods before sending completed notifications (!1) (@mdoyle)
- Reduce scroll effect when attaching to tmux sessions and clear terminal scrollback on attach (!1) (@mdoyle)
- Add compacting status for Claude Code conversation compaction (!1) (@mdoyle)
- Add Alt+Backspace word delete and Ctrl+U line clear in session UI (!1) (@mdoyle)
- Show app version in header and fix `--version` flag (!1) (@mdoyle)
- Add follow‑up marks and fix status flickering with Ctrl+K detach (!1) (@mdoyle)

## [0.0.8] - 2026-03-09

### Added

- GitLab CI/CD pipeline with automated binary builds for all platforms
- Automated version bumping from MR labels (Version::Major/Minor/Patch)
- LLM-generated release notes and CHANGELOG entries
- MITRE-specific install script (`curl | sh`)