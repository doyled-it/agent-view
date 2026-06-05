# Agent View

**A lightweight terminal dashboard for managing multiple AI coding agent sessions.**

Run multiple AI coding agents in parallel and manage them from a single terminal UI. Agent View is a tmux session manager built for AI-assisted development workflows -- monitor agent status in real-time, get notifications when agents finish or need input, and seamlessly switch between sessions.

Forked from [Frayo44/agent-view](https://github.com/Frayo44/agent-view) (the original TypeScript implementation) and rewritten in Rust. The TypeScript version with additional features is preserved on the [`legacy/typescript`](https://github.com/doyled-it/agent-view/tree/legacy/typescript) branch.

Works with **Claude Code**, **Gemini CLI**, **OpenCode**, **Codex CLI**, and any custom command. Tool-specific status detection is wired up for Claude Code, Codex CLI, Gemini CLI, and OpenCode; custom commands get basic session management. Token tracking is currently Claude-only.

## Supported Platforms

| Platform | Architecture | Status |
|----------|--------------|--------|
| macOS    | Apple Silicon (arm64) | Supported |
| macOS    | Intel (x64) | Supported |
| Linux    | arm64 | Supported |
| Linux    | x64 | Supported |
| WSL      | x64 | Supported |

## Why Agent View?

When working with AI coding agents, you often need multiple agents running on different tasks -- one refactoring a module, another writing tests, a third exploring a bug. Agent View lets you orchestrate all of them from one place instead of juggling terminal tabs.

## Demo

![Demo](assets/demo.gif?v=2)

## Features

- **Multi-Agent Dashboard** -- View all sessions at a glance with real-time status indicators and 24-hour activity timelines
- **Scheduled Routines** -- Create recurring tasks (Claude prompts or shell commands) that run on a cron schedule via macOS LaunchAgent, whether or not the TUI is open
- **Smart Notifications** -- Get notified when an agent finishes or needs input
- **Session Management** -- Create, stop, restart, rename, and delete sessions with keyboard shortcuts
- **Git Worktree Integration** -- Create isolated git worktrees for each session
- **Tool Agnostic** -- Works with Claude Code, Gemini CLI, OpenCode, Codex CLI, or any custom command
- **12 Themes** -- dark, light, tokyo-night, dracula, gruvbox, nord, solarized, rose-pine, kanagawa, everforest, one-dark, moonfly (press `t` to preview and switch)
- **Session Groups** -- Organize sessions into named groups, reorder with Shift+J/K
- **Session Pinning** -- Pin important sessions to the top of their group
- **Bulk Operations** -- Select multiple sessions with Space/Ctrl+A, then stop or delete in bulk
- **Activity Feed** -- Collapsible feed showing real-time status transitions (press `a`)
- **Sort Modes** -- Cycle through status, activity, name, and creation time sorting (Shift+S)
- **Search** -- Fuzzy search across session names (press `/`)
- **Command Palette** -- Quick access to all actions (Ctrl+K)
- **Account Usage Tracking** -- Live display of Claude account-level rate limits (session, weekly, per-model) with colored progress bars
- **Token Tracking** -- Monitor token usage for Claude sessions
- **Session Uptime** -- Tracks time since last tmux session start, not just creation date
- **Persistent State** -- Sessions survive terminal restarts via tmux; data stored in SQLite

### Status Detection

Agent View monitors sessions and shows real-time status:

| Status | Meaning |
|--------|---------|
| Running | Agent is actively working |
| Waiting | Agent is asking for your input |
| Draft | You've typed input but haven't sent it yet |
| Paused | Agent is blocked on a yes/no or approval prompt |
| Compacting | Agent is compacting context to free token budget |
| Monitoring | Agent is watching for events (e.g. `claude --monitor`) |
| Idle | Session is ready for commands |
| Stopped | Session was stopped manually |
| Crashed | tmux session is no longer running |
| Error | Something went wrong |

Claude Code, Codex CLI, Gemini CLI, and OpenCode use runner-specific hooks, pane parsing, or both to surface these states. Custom commands use basic tmux lifecycle detection.

For OpenCode, Agent View installs a global plugin at
`~/.config/opencode/plugins/agent-view.js` that forwards `session.created`,
`session.status`, `session.idle`, `session.compacted`, `session.error`,
`permission.asked`, `permission.updated`, and `permission.replied`.
`session.status` maps busy, active, or running to Running; idle to Idle; and
retry or error to Error. Permission asked or updated maps to Waiting, and
permission replied maps back to Running. Pane parsing still covers idle, busy,
waiting, exited, and draft states when hook data is absent.

Sessions are sorted with the most attention-needing states first (Crashed, Waiting, Draft, Paused), so anything wanting your eyes floats to the top of the list.

### MCP profiles for new sessions

New sessions default to all configured MCP servers. Selecting an MCP profile or toggling servers in the new-session form narrows only that session, and the details pane records the selected profile id plus enabled server ids.

Claude and Codex enforce server-level selections at launch: Claude writes a session-scoped MCP config from the enabled servers, while Codex adds config overrides for disabled servers. OpenCode does not have enforceable MCP filtering yet, so narrowed selections that would filter servers or tools are blocked with a launch error instead of silently starting with broader access.

Per-tool MCP filtering is available only when the selected runner can enforce it. Codex currently accepts whole-server selections and rejects per-tool limits; unsupported runners keep their default all-server behavior unless their launch path can apply the selection exactly.

Use **Sync MCP Servers** from the command palette to compare Claude and Codex MCP configuration. The sync view shows which servers are configured, missing, or unsupported per runner, previews the exact config addition, and requires confirmation before writing `~/.claude/settings.json` or `~/.codex/config.toml`. OpenCode is marked unsupported for config sync until its MCP configuration can be safely copied and enforced.

MCP sync makes servers available to runners; new-session MCP profiles still decide which of those configured servers a specific session can use.

### Account Usage Tracking

A usage pane at the bottom of the screen shows your Claude account rate limits with colored progress bars (green < 50%, yellow 50-79%, red >= 80%). Displays session, weekly, and per-model buckets with reset times.

This works by running a hidden tmux session that periodically queries Claude's `/usage` command. The hidden session is automatically created on startup and cleaned up on exit -- it does not appear in the session list.

### Cost Dashboard

Press `Tab` until you reach the **Costs** tab to see an aggregated breakdown of `cost_events` across all sessions:

- API-rate cost for the selected period (Today / This week / This month / All time)
- For Claude Pro/Max subscribers: pro-rated plan cost and "Saved vs. API" line. Configure via `costs.plan` in `~/.agent-view/config.json`:

  ```json
  {
    "costs": {
      "plan": {
        "claude": "pro"
      }
    }
  }
  ```

  Accepted values: `api`, `pro`, `max-5x`, `max-20x`. Codex and Gemini default to `api` regardless of config.
- Per-runner totals (Claude/Codex/Gemini), per-model breakdown, top 10 most-expensive sessions.

Use `←` / `→` to rotate the time period.

### Scheduled Routines

Routines let you schedule recurring tasks that run automatically via system-level jobs (macOS LaunchAgent). Press `Tab` to switch to the Routines tab.

**Creating a routine:**
1. Press `n` to open the creation form
2. Set a name, working directory, and schedule (hourly/daily/weekly/monthly/yearly or raw cron)
3. Add one or more steps -- each step is either a Claude prompt or a shell command
4. Press `Enter` to save -- the routine is immediately enabled and scheduled

**How it works:**
- System jobs call `agent-view exec-routine <id>` on schedule, even when the TUI is closed
- Each step runs sequentially in a tmux session -- Claude steps use `--permission-mode bypassPermissions` for unattended execution
- Run history is tracked with archived logs viewable in the preview pane
- Inspect past runs with `r` (attaches to an ephemeral tmux session) or promote them to full sessions with `P`
- On startup, agent-view reconciles scheduler state: re-registers missing jobs, detects stale binary paths, and marks crashed runs

**Schedule builder frequencies:**
- **Hourly** -- at minute :MM (Up/Down to adjust)
- **Daily** -- at HH:MM
- **Weekly** -- specific days at HH:MM (Space to toggle days)
- **Monthly** -- on day N at HH:MM (+/- to adjust day)
- **Yearly** -- month and day at HH:MM (+/- month, [/] day)
- **Advanced** -- raw cron expression

## Installation

### Quick Install

Downloads a pre-built binary for your platform:

```bash
curl -fsSL https://raw.githubusercontent.com/doyled-it/agent-view/main/install.sh | bash
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/doyled-it/agent-view/main/install.sh | bash -s -- -v 1.0.0
```

### Build from Source

Requires [Rust](https://rustup.rs/) and [tmux](https://github.com/tmux/tmux):

```bash
git clone https://github.com/doyled-it/agent-view.git
cd agent-view
cargo build --release
cp target/release/agent-view ~/.local/bin/
```

## Contributing

This repo uses the Rust-native hook runner [`prek`](https://github.com/j178/prek)
with the tracked native `prek.toml` config. Cargo does not have a Poetry/uv-style
developer dependency group for standalone tools; `[dev-dependencies]` are for
crates linked into tests/examples/benches. Install `prek` as a local developer
tool instead:

```bash
cargo install --locked prek
prek install
prek run --all-files
```

The hook config mirrors CI and runs:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --all-features
```

### Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/doyled-it/agent-view/main/uninstall.sh | bash
```

## Usage

```bash
agent-view
# or use the short alias
av
```

### Keyboard Shortcuts

**Navigation:**

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Enter` | Attach to session / toggle group |
| `l` / `Right` | Expand group (or attach) |
| `h` / `Left` | Collapse group |
| `1`-`9` | Jump to group by number |
| `/` | Search sessions |
| `Ctrl+K` | Command palette |
| `?` | Help overlay |
| `q` | Quit |

**Session Management:**

| Key | Action |
|-----|--------|
| `n` | New session |
| `s` | Stop session |
| `r` | Restart session |
| `d` | Delete session or group |
| `R` | Rename session or group |
| `m` | Move session to group |
| `e` | Export session log |
| `!` | Toggle notifications |
| `i` | Toggle follow-up flag |
| `p` | Toggle pin |

**Organization:**

| Key | Action |
|-----|--------|
| `g` | Create new group |
| `Shift+J` / `Shift+K` | Reorder groups |
| `Shift+S` | Cycle sort mode |
| `Space` | Toggle bulk select |
| `Ctrl+A` | Select all visible |
| `a` | Toggle activity feed |
| `t` | Theme selector |

**Routines Tab** (press `Tab` to switch):

| Key | Action |
|-----|--------|
| `n` | New routine |
| `e` | Edit routine |
| `Space` | Enable/disable routine (installs/removes system job) |
| `Enter` | Expand routine to show run history |
| `d` | Delete routine or run |
| `p` | Pin/unpin routine |
| `R` | Rename routine |
| `m` | Move to group |
| `/` | Search routines |
| `r` | Inspect/resume a run |
| `P` | Promote run to full session |

**Inside an attached session:**

Detach with `Ctrl+B`, `D` (standard tmux detach) to return to the dashboard.

### Configuration

Config lives at `~/.agent-view/config.json`:

```json
{
  "default_tool": "claude",
  "theme": "dark",
  "default_group": "default",
  "notifications": {
    "sound": false
  }
}
```

**Available themes:** `dark`, `light`, `tokyo-night`, `dracula`, `gruvbox`, `nord`, `solarized`, `rose-pine`, `kanagawa`, `everforest`, `one-dark`, `moonfly`

## Requirements

- [tmux](https://github.com/tmux/tmux) (`brew install tmux` on macOS, `apt install tmux` on Linux)
- At least one AI coding tool installed (claude, gemini, opencode, codex, etc.)

## Acknowledgments

Forked from [Frayo44/agent-view](https://github.com/Frayo44/agent-view), which was inspired by [agent-deck](https://github.com/asheshgoplani/agent-deck). The original TypeScript implementation was extended with additional features and then rewritten from scratch in Rust for this version.

## License

MIT
