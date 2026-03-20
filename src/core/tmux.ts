/**
 * Tmux session management
 * Based on agent-view's tmux package with session caching
 */

import { spawn, exec } from "child_process"
import { promisify } from "util"

// Lazy load node-pty to avoid import errors in test environments
let pty: typeof import("node-pty") | null = null
async function getPty() {
  if (!pty) {
    pty = await import("node-pty")
  }
  return pty
}

const execAsync = promisify(exec)

export const SESSION_PREFIX = "agentorch_"

// Session cache - reduces subprocess spawns
interface SessionCache {
  data: Map<string, number> // session_name -> activity_timestamp
  timestamp: number
}

let sessionCache: SessionCache = {
  data: new Map(),
  timestamp: 0
}

const CACHE_TTL = 2000 // 2 seconds

/**
 * Check if tmux is available
 */
export async function isTmuxAvailable(): Promise<boolean> {
  try {
    await execAsync("tmux -V")
    return true
  } catch {
    return false
  }
}

/**
 * Refresh the session cache
 * Call this once per tick cycle
 */
export async function refreshSessionCache(): Promise<void> {
  try {
    const { stdout } = await execAsync(
      'tmux list-windows -a -F "#{session_name}\t#{window_activity}"'
    )

    const newCache = new Map<string, number>()
    for (const line of stdout.trim().split("\n")) {
      if (!line) continue
      const [name, activity] = line.split("\t")
      if (!name) continue
      const activityTs = parseInt(activity || "0", 10)
      // Keep maximum activity for sessions with multiple windows
      const existing = newCache.get(name) || 0
      if (activityTs > existing) {
        newCache.set(name, activityTs)
      }
    }

    sessionCache = {
      data: newCache,
      timestamp: Date.now()
    }
  } catch {
    // tmux not running or no sessions
    sessionCache = {
      data: new Map(),
      timestamp: Date.now()
    }
  }
}

/**
 * Check if session exists (from cache)
 */
export function sessionExists(name: string): boolean {
  if (Date.now() - sessionCache.timestamp > CACHE_TTL) {
    return false // Cache stale, caller should refresh
  }
  return sessionCache.data.has(name)
}

/**
 * Get session activity timestamp (from cache)
 */
export function getSessionActivity(name: string): number {
  if (Date.now() - sessionCache.timestamp > CACHE_TTL) {
    return 0
  }
  return sessionCache.data.get(name) || 0
}

/**
 * Register a new session in cache (prevents race condition)
 */
export function registerSessionInCache(name: string): void {
  sessionCache.data.set(name, Math.floor(Date.now() / 1000))
}

export interface TmuxSession {
  name: string
  exists: boolean
  activity: number
}

/**
 * Get the set of session names that currently have a tmux client attached.
 * Used to suppress notifications when the user is already looking at a session.
 */
export async function getAttachedSessions(): Promise<Set<string>> {
  try {
    const { stdout } = await execAsync("tmux list-clients -F '#{client_session}'")
    const names = stdout.trim().split("\n").filter(Boolean)
    return new Set(names)
  } catch {
    return new Set()
  }
}

/**
 * Check if a session has active output (activity within last N seconds)
 */
export function isSessionActive(name: string, thresholdSeconds = 2): boolean {
  const activity = getSessionActivity(name)
  if (!activity) return false
  const now = Math.floor(Date.now() / 1000)
  return now - activity < thresholdSeconds
}

/**
 * Create a new tmux session
 */
/**
 * Check if the tmux server version matches the client version.
 * A mismatch (e.g., after brew upgrade) causes "open terminal failed: not a terminal"
 * errors because the protocol between client and server has changed.
 */
export async function checkTmuxVersionMismatch(): Promise<boolean> {
  try {
    // Get client version
    const { stdout: clientOut } = await execAsync("tmux -V")
    const clientVersion = clientOut.trim()

    // Get server version (only works if server is running)
    const { stdout: serverOut } = await execAsync("tmux display-message -p '#{version}'")
    const serverVersion = serverOut.trim()

    if (!serverVersion) return false // Can't determine, assume OK
    return clientVersion !== `tmux ${serverVersion}`
  } catch {
    return false // Server not running or can't check
  }
}

export async function createSession(options: {
  name: string
  command?: string
  cwd?: string
  env?: Record<string, string>
}): Promise<void> {
  const cwd = options.cwd || process.env.HOME || "/tmp"

  // Step 1: Create the tmux session first (detached, no command)
  const createCmd = `tmux new-session -d -s "${options.name}" -c "${cwd}"`
  await execAsync(createCmd)
  registerSessionInCache(options.name)

  // Step 2: Set environment variables in the tmux session
  const envVars = options.env || {}
  for (const [key, value] of Object.entries(envVars)) {
    await execAsync(`tmux set-environment -t "${options.name}" ${key} "${value}"`)
  }

  // Step 3: Send the command via send-keys (like agent-deck does)
  if (options.command) {
    let cmdToSend = options.command

    // IMPORTANT: Commands containing bash-specific syntax (like `session_id=$(...)`)
    // must be wrapped in `bash -c` for fish shell compatibility.
    // Fish uses different syntax: `set var (...)` instead of `var=$(...)`.
    if (options.command.includes("$(") || options.command.includes("session_id=")) {
      // Escape single quotes in the command for bash -c wrapper
      const escapedCmd = options.command.replace(/'/g, "'\"'\"'")
      cmdToSend = `bash -c '${escapedCmd}'`
    }

    // Send the command and press Enter
    await sendKeys(options.name, cmdToSend)
    await execAsync(`tmux send-keys -t "${options.name}" Enter`)
  }
}

/**
 * Kill a tmux session
 */
export async function killSession(name: string): Promise<void> {
  try {
    await execAsync(`tmux kill-session -t "${name}"`)
    sessionCache.data.delete(name)
  } catch {
    // Session might not exist
  }
}

/**
 * Send keys to a tmux session
 */
export async function sendKeys(name: string, keys: string): Promise<void> {
  // Escape special characters for tmux
  const escaped = keys
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\$/g, "\\$")

  await execAsync(`tmux send-keys -t "${name}" "${escaped}" Enter`)
}

/**
 * Send raw keys without Enter
 */
export async function sendRawKeys(name: string, keys: string): Promise<void> {
  await execAsync(`tmux send-keys -t "${name}" "${keys}"`)
}

/**
 * Capture pane content
 */
export async function capturePane(
  name: string,
  options: {
    startLine?: number
    endLine?: number
    escape?: boolean
    join?: boolean
  } = {}
): Promise<string> {
  const args = ["capture-pane", "-t", name, "-p"]

  if (options.startLine !== undefined) {
    args.push("-S", String(options.startLine))
  }
  if (options.endLine !== undefined) {
    args.push("-E", String(options.endLine))
  }
  if (options.escape) {
    args.push("-e") // Include escape sequences
  }
  if (options.join) {
    args.push("-J") // Join wrapped lines
  }

  try {
    const { stdout } = await execAsync(`tmux ${args.join(" ")}`, {
      timeout: 5000
    })
    return stdout
  } catch (err: any) {
    if (err.killed) {
      throw new Error("capture-pane timed out")
    }
    throw err
  }
}

/**
 * Capture the full tmux scrollback history, stripped of ANSI codes
 */
export async function captureFullScrollback(name: string): Promise<string> {
  const { stdout } = await execAsync(`tmux capture-pane -t "${name}" -p -S - -J`, {
    timeout: 10000,
    maxBuffer: 10 * 1024 * 1024 // 10MB
  })
  return stripAnsi(stdout)
}

/**
 * Get pane dimensions
 */
export async function getPaneDimensions(name: string): Promise<{ width: number; height: number }> {
  const { stdout } = await execAsync(
    `tmux display-message -t "${name}" -p "#{pane_width}\t#{pane_height}"`
  )
  const [width, height] = stdout.trim().split("\t").map(Number)
  return { width: width || 80, height: height || 24 }
}

/**
 * Resize pane
 */
export async function resizePane(name: string, width: number, height: number): Promise<void> {
  await execAsync(`tmux resize-pane -t "${name}" -x ${width} -y ${height}`)
}

/**
 * Attach to a tmux session (replaces current terminal)
 */
export function attachSession(name: string): void {
  const child = spawn("tmux", ["attach-session", "-t", name], {
    stdio: "inherit",
    env: process.env
  })

  child.on("exit", (code) => {
    process.exit(code || 0)
  })
}

/**
 * List all sessions with our prefix
 */
export async function listSessions(): Promise<string[]> {
  try {
    const { stdout } = await execAsync("tmux list-sessions -F #{session_name}")
    return stdout
      .trim()
      .split("\n")
      .filter((name) => name.startsWith(SESSION_PREFIX))
  } catch {
    return []
  }
}

/**
 * Check if currently inside tmux
 */
export function insideTmux(): boolean {
  return !!process.env.TMUX
}

/**
 * Get the current tmux session name
 */
export async function getCurrentSession(): Promise<string | null> {
  if (!insideTmux()) return null

  try {
    const { stdout } = await execAsync("tmux display-message -p #{session_name}")
    return stdout.trim()
  } catch {
    return null
  }
}

/**
 * Generate a unique session name
 */
export function generateSessionName(title: string): string {
  const safe = title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 20)

  const timestamp = Date.now().toString(36)
  return `${SESSION_PREFIX}${safe}-${timestamp}`
}

/**
 * Parse output to detect tool status
 */
export interface ToolStatus {
  isActive: boolean
  isWaiting: boolean
  isCompacting: boolean
  isBusy: boolean
  hasError: boolean
  hasExited: boolean
}

/**
 * Strip ANSI escape codes from terminal output
 */
export function stripAnsi(text: string): string {
  // Remove ANSI escape sequences (colors, cursor movement, etc.)
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
    .replace(/\x1b\][^\x07]*\x07/g, "") // OSC sequences
    .replace(/\x1b[PX^_][^\x1b]*\x1b\\/g, "") // DCS, SOS, PM, APC sequences
}

// Claude Code busy indicators - agent is actively working (NOT waiting for input)
// These indicate Claude is in the middle of processing
const CLAUDE_BUSY_PATTERNS = [
  /ctrl\+c to interrupt/i,
  /….*tokens/i,  // Processing indicator with tokens count
]

// Spinner characters used by Claude Code when processing
const SPINNER_CHARS = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✳", "✽", "✶", "✢"]

// Claude Code waiting indicators - needs user input (permission prompts, questions)
// These indicate Claude is BLOCKED waiting for a specific user response
const CLAUDE_WAITING_PATTERNS = [
  // Permission prompts with numbered options (blocked on user decision)
  /Do you want to proceed\?/i,
  /\d\.\s*Yes\b/i,  // "1. Yes" pattern in selection UI
  /Esc to cancel.*Tab to amend/i,  // Permission prompt footer
  // Selection UI (blocked on user selection)
  /Enter to select.*to navigate/i,
  // Confirmation prompts
  /\(Y\/n\)/i,
  /Continue\?/i,
  /Approve this plan\?/i,
  /\[Y\/n\]/i,
  /\[y\/N\]/i,
  // Other blocking prompts
  /Yes,? allow once/i,
  /Allow always/i,
  /No,? and tell Claude/i,
]

// Patterns indicating Claude has exited (shell returned)
const CLAUDE_EXITED_PATTERNS = [
  /Resume this session with:/i,
  /claude --resume/i,
  /Press Ctrl-C again to exit/i,
]

// Patterns indicating Claude is compacting/summarizing conversation
const CLAUDE_COMPACTING_PATTERNS = [
  /compacting conversation/i,
  /summarizing conversation/i,
  /context window.*(compact|compress)/i,
]

// Generic waiting patterns (for other tools)
const WAITING_PATTERNS = [
  /\? \(y\/n\)/i,
  /\[Y\/n\]/i,
  /Press enter to continue/i,
  /waiting for.*input/i,
  /do you want to/i
]

const ERROR_PATTERNS = [
  /error:/i,
  /failed:/i,
  /exception:/i,
  /traceback/i,
  /panic:/i
]

/**
 * Check if output contains spinner characters (Claude is processing)
 */
function hasSpinner(text: string): boolean {
  return SPINNER_CHARS.some(char => text.includes(char))
}

/**
 * Parse output to detect tool status
 * @param output - Raw terminal output
 * @param tool - Optional tool type for tool-specific detection
 */
export function parseToolStatus(output: string, tool?: string): ToolStatus {
  const cleaned = stripAnsi(output)
  // Filter out trailing empty lines before slicing - Claude Code TUI often has blank padding
  const allLines = cleaned.split("\n")
  let lastNonEmptyIdx = allLines.length - 1
  while (lastNonEmptyIdx >= 0 && allLines[lastNonEmptyIdx]?.trim() === "") {
    lastNonEmptyIdx--
  }
  const trimmedLines = allLines.slice(0, lastNonEmptyIdx + 1)
  const lastLines = trimmedLines.slice(-30).join("\n")
  const lastFewLines = trimmedLines.slice(-10).join("\n")

  let isWaiting = false
  let isCompacting = false
  let isBusy = false
  let hasError = false
  let hasExited = false

  if (tool === "claude") {
    // Claude Code specific detection

    // Check if Claude has exited (shell returned)
    hasExited = CLAUDE_EXITED_PATTERNS.some(p => p.test(lastLines))

    if (!hasExited) {
      // Check for compacting (conversation summarization)
      isCompacting = CLAUDE_COMPACTING_PATTERNS.some(p => p.test(lastLines))

      // Check for busy indicators (actively working)
      isBusy = CLAUDE_BUSY_PATTERNS.some(p => p.test(lastLines)) || hasSpinner(lastFewLines)

      // Check for waiting indicators (needs user input)
      isWaiting = CLAUDE_WAITING_PATTERNS.some(p => p.test(lastLines))
    }
    // If Claude has exited, both isBusy and isWaiting stay false -> will become idle
  } else {
    // Generic tool detection
    isWaiting = WAITING_PATTERNS.some(p => p.test(lastLines))
  }

  // Only flag errors if the tool is NOT actively working.
  // Transient errors (lint failures, test failures) that the agent is fixing
  // should not show as "error" status while the agent is still busy.
  if (!isBusy) {
    hasError = ERROR_PATTERNS.some(p => p.test(lastLines))
  }

  return {
    isActive: false, // Determined by activity timestamp
    isWaiting,
    isCompacting,
    isBusy,
    hasError,
    hasExited
  }
}

/**
 * Attach to a tmux session with PTY support
 * Intercepts Ctrl+Q (ASCII 17) to detach and return control to the TUI
 * Based on agent-view's pty.go implementation
 */
export async function attachWithPty(sessionName: string): Promise<void> {
  const ptyModule = await getPty()
  return new Promise((resolve) => {
    // Spawn tmux attach with PTY
    const ptyProcess = ptyModule.spawn("tmux", ["attach-session", "-t", sessionName], {
      name: "xterm-256color",
      cols: process.stdout.columns || 80,
      rows: process.stdout.rows || 24,
      cwd: process.cwd(),
      env: process.env as { [key: string]: string }
    })

    let isDetaching = false

    ptyProcess.onData((data: string) => {
      if (!isDetaching) {
        process.stdout.write(data)
      }
    })

    ptyProcess.onExit(() => {
      cleanup()
      resolve()
    })

    const handleResize = () => {
      ptyProcess.resize(
        process.stdout.columns || 80,
        process.stdout.rows || 24
      )
    }
    process.stdout.on("resize", handleResize)

    // Put stdin in raw mode to capture Ctrl+Q
    const wasRaw = process.stdin.isRaw
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(true)
    }
    process.stdin.resume()

    // Intercept Ctrl+Q (ASCII 17) for detach
    const handleStdin = (data: Buffer) => {
      if (data.length === 1 && data[0] === 17) {
        isDetaching = true
        cleanup()
        resolve()
        return
      }
      ptyProcess.write(data.toString())
    }
    process.stdin.on("data", handleStdin)

    function cleanup() {
      process.stdin.removeListener("data", handleStdin)
      process.stdout.removeListener("resize", handleResize)

      if (process.stdin.isTTY) {
        process.stdin.setRawMode(wasRaw ?? false)
      }

      try {
        ptyProcess.kill()
      } catch {
        // PTY may already be closed
      }

      // Clear screen before returning to TUI
      process.stdout.write("\x1b[2J\x1b[H")
    }
  })
}

// Signal file for command palette request (per-user to avoid conflicts)
export function getSignalFilePath(): string {
  const uid = typeof process.getuid === "function" ? process.getuid() : process.pid
  return `/tmp/agent-view-cmd-palette-${uid}`
}

/**
 * Check if command palette was requested during attached session
 */
export function wasCommandPaletteRequested(): boolean {
  const fs = require("fs")
  try {
    if (fs.existsSync(getSignalFilePath())) {
      fs.unlinkSync(getSignalFilePath())
      return true
    }
  } catch {
    // Ignore errors
  }
  return false
}

/**
 * Attach to a tmux session with Ctrl+Q to detach
 * Configures tmux to use Ctrl+Q as detach key, then uses spawnSync
 */
export function attachSessionSync(sessionName: string): void {
  const { spawnSync } = require("child_process")
  const fs = require("fs")

  // Clear any existing signal
  try {
    fs.unlinkSync(getSignalFilePath())
  } catch {
    // Ignore if doesn't exist
  }

  // Clear the screen and show cursor, but stay in the alternate screen buffer.
  // Exiting the alternate screen (\x1b[?1049l) would restore the main buffer's
  // old scrollback, causing a visible "scroll from top" effect as tmux redraws.
  process.stdout.write("\x1b[2J\x1b[H\x1b[?25h")

  // Bind Ctrl+Q to detach in this session (C-q = ASCII 17)
  spawnSync("tmux", ["bind-key", "-n", "C-q", "detach-client"], { stdio: "ignore" })

  // Bind Ctrl+K to create signal file and detach
  spawnSync("tmux", ["bind-key", "-n", "C-k", "run-shell", `touch ${getSignalFilePath()}`, "\\;", "detach-client"], { stdio: "ignore" })

  // Bind Ctrl+T to open a terminal pane (split horizontally, half screen)
  spawnSync("tmux", ["bind-key", "-n", "C-t", "split-window", "-v", "-c", "#{pane_current_path}"], { stdio: "ignore" })

  // Configure status bar with shortcuts
  spawnSync("tmux", ["set-option", "-t", sessionName, "status", "on"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-position", "bottom"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-style", "bg=#1e1e2e,fg=#cdd6f4"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-left", ""], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-right-length", "120"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-right", "#[fg=#89b4fa]Ctrl+K#[fg=#6c7086] cmd  #[fg=#89b4fa]Ctrl+T#[fg=#6c7086] terminal  #[fg=#89b4fa]Ctrl+Q#[fg=#6c7086] detach  #[fg=#89b4fa]Ctrl+C#[fg=#6c7086] cancel"], { stdio: "ignore" })

  try {
    // Attach to tmux — blocks until user detaches
    const attachResult = spawnSync("tmux", ["attach-session", "-t", sessionName], {
      stdio: ["inherit", "inherit", "pipe"],
      env: process.env
    })

    if (attachResult.status !== 0) {
      const stderr = attachResult.stderr?.toString?.() || ""
      if (stderr.includes("not a terminal")) {
        throw new Error(
          "tmux attach failed: this is usually caused by a tmux version mismatch. " +
          "The tmux server was started with an older version. " +
          "Run 'tmux kill-server' in a terminal to fix this. " +
          "Agent View will recreate your sessions automatically."
        )
      }
    }
  } finally {
    // Always unbind keys, even if attach crashed
    spawnSync("tmux", ["unbind-key", "-n", "C-q"], { stdio: "ignore" })
    spawnSync("tmux", ["unbind-key", "-n", "C-k"], { stdio: "ignore" })
    spawnSync("tmux", ["unbind-key", "-n", "C-t"], { stdio: "ignore" })

    // Clear screen for TUI — we stayed in the alternate buffer, so just clear it
    process.stdout.write("\x1b[2J\x1b[H")
  }
}

/**
 * Attach to a tmux session asynchronously.
 * Unlike attachSessionSync, this does NOT block the event loop,
 * so setInterval-based polling (e.g. notifications) keeps running.
 */
export function attachSessionAsync(sessionName: string): Promise<void> {
  const { spawnSync } = require("child_process")
  const fs = require("fs")

  // Clear any existing signal
  try {
    fs.unlinkSync(getSignalFilePath())
  } catch {
    // Ignore if doesn't exist
  }

  // Clear the screen and show cursor, but stay in the alternate screen buffer.
  // Exiting the alternate screen (\x1b[?1049l) would restore the main buffer's
  // old scrollback, causing a visible "scroll from top" effect as tmux redraws.
  // By staying in the alternate buffer, tmux draws directly on a clean screen.
  process.stdout.write("\x1b[2J\x1b[H\x1b[?25h")

  // Cancel any copy-mode in the target pane so we see the live view, not scrollback
  spawnSync("tmux", ["send-keys", "-t", sessionName, "-X", "cancel"], { stdio: "ignore" })

  // Bind keys (these are fast, sync is fine)
  spawnSync("tmux", ["bind-key", "-n", "C-q", "detach-client"], { stdio: "ignore" })
  spawnSync("tmux", ["bind-key", "-n", "C-k", "run-shell", `touch ${getSignalFilePath()}`, "\\;", "detach-client"], { stdio: "ignore" })
  spawnSync("tmux", ["bind-key", "-n", "C-t", "split-window", "-v", "-c", "#{pane_current_path}"], { stdio: "ignore" })

  // Configure status bar
  spawnSync("tmux", ["set-option", "-t", sessionName, "status", "on"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-position", "bottom"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-style", "bg=#1e1e2e,fg=#cdd6f4"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-left", ""], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-right-length", "120"], { stdio: "ignore" })
  spawnSync("tmux", ["set-option", "-t", sessionName, "status-right", "#[fg=#89b4fa]Ctrl+K#[fg=#6c7086] cmd  #[fg=#89b4fa]Ctrl+T#[fg=#6c7086] terminal  #[fg=#89b4fa]Ctrl+Q#[fg=#6c7086] detach  #[fg=#89b4fa]Ctrl+C#[fg=#6c7086] cancel"], { stdio: "ignore" })

  return new Promise<void>((resolve, reject) => {
    const child = spawn("tmux", ["attach-session", "-t", sessionName], {
      stdio: ["inherit", "inherit", "pipe"],
      env: process.env
    })

    let stderr = ""
    child.stderr?.on("data", (data: Buffer) => { stderr += data.toString() })

    child.on("close", (code) => {
      // Always unbind keys
      spawnSync("tmux", ["unbind-key", "-n", "C-q"], { stdio: "ignore" })
      spawnSync("tmux", ["unbind-key", "-n", "C-k"], { stdio: "ignore" })
      spawnSync("tmux", ["unbind-key", "-n", "C-t"], { stdio: "ignore" })

      // Clear screen for TUI — we stayed in the alternate buffer, so just clear it
      process.stdout.write("\x1b[2J\x1b[H")

      if (code !== 0 && stderr.includes("not a terminal")) {
        reject(new Error(
          "tmux attach failed: this is usually caused by a tmux version mismatch. " +
          "The tmux server was started with an older version. " +
          "Run 'tmux kill-server' in a terminal to fix this. " +
          "Agent View will recreate your sessions automatically."
        ))
      } else {
        resolve()
      }
    })
  })
}
