/**
 * Agent Orchestrator
 * OpenTUI-based Agent Management
 */

import { tui } from "./tui/app"
import { getVersion } from "./core/version"

async function main() {
  const args = process.argv.slice(2)

  // Simple CLI argument handling
  if (args.includes("--help") || args.includes("-h")) {
    console.log(`
Agent Orchestrator - Terminal Agent Management

Usage:
  agent-orchestrator [options]

Options:
  --help, -h     Show this help message
  --version, -v  Show version
  --light        Use light mode theme

Keyboard Shortcuts (in TUI):
  Ctrl+K         Command palette
  Ctrl+L         Session list
  N              New session
  Q              Quit / Detach
  ?              Help
`)
    process.exit(0)
  }

  if (args.includes("--version") || args.includes("-v")) {
    console.log(`agent-view v${getVersion()}`)
    process.exit(0)
  }

  const mode = args.includes("--light") ? "light" : "dark"

  try {
    await tui({
      mode,
      onExit: async () => {
        console.log("Goodbye!")
      }
    })
  } catch (error) {
    console.error("Fatal error:", error)
    process.exit(1)
  }
}

main()
