# Agent View

**A lightweight terminal dashboard for managing multiple AI coding agent sessions.**

Run multiple AI coding agents in parallel and manage them from a single terminal UI. Agent View is a tmux session manager built for AI-assisted development workflows -- monitor agent status in real-time, get notifications when agents finish or need input, and seamlessly switch between sessions.

Forked from [Frayo44/agent-view](https://github.com/Frayo44/agent-view) (the original TypeScript implementation) and rewritten in Rust. The TypeScript version with additional features is preserved on the [`legacy/typescript`](https://github.com/doyled-it/agent-view/tree/legacy/typescript) branch.

Works with **Claude Code**, **Gemini CLI**, **OpenCode**, **Codex CLI**, and any custom command. Note: advanced features like status detection, token tracking, and smart notifications are optimized for **Claude Code** -- other tools get basic session management.

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
- **Token Tracking** -- Monitor token usage for Claude sessions
- **Session Uptime** -- Tracks time since last tmux session start, not just creation date
- **Persistent State** -- Sessions survive terminal restarts via tmux; data stored in SQLite

### Status Detection

Agent View monitors sessions and shows real-time status:

| Status | Meaning |
|--------|---------|
| Running | Agent is actively working |
| Waiting | Needs your input |
| Paused | Agent is paused/compacting |
| Idle | Ready for commands |
| Stopped | Session was stopped |
| Error | Something went wrong |

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
