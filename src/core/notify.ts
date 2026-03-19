/**
 * Desktop notification support
 * macOS: terminal-notifier (persistent) or osascript (fallback)
 * Linux: notify-send
 */

import { exec, execSync } from "child_process"

// Cache whether terminal-notifier is available
let hasTerminalNotifier: boolean | null = null
function checkTerminalNotifier(): boolean {
  if (hasTerminalNotifier === null) {
    try {
      execSync("which terminal-notifier", { stdio: "ignore" })
      hasTerminalNotifier = true
    } catch {
      hasTerminalNotifier = false
    }
  }
  return hasTerminalNotifier
}

export function buildNotificationCommand(
  title: string,
  body: string,
  sound: boolean,
  platform: string = process.platform
): string {
  const safeTitle = title.replace(/"/g, '\\"')
  const safeBody = body.replace(/"/g, '\\"')

  if (platform === "darwin") {
    // Prefer terminal-notifier for persistent (alert-style) notifications
    if (checkTerminalNotifier()) {
      const soundFlag = sound ? ' -sound default' : ""
      return `terminal-notifier -title "${safeTitle}" -message "${safeBody}" -timeout 0${soundFlag}`
    }
    const soundClause = sound ? ' sound name "default"' : ""
    return `osascript -e 'display notification "${safeBody}" with title "${safeTitle}"${soundClause}'`
  }

  // Linux: -u critical makes notifications persistent until dismissed
  const urgency = "-u critical"
  return `notify-send ${urgency} "${safeTitle}" "${safeBody}"`
}

export function sendNotification(title: string, body: string, sound: boolean = false): void {
  const cmd = buildNotificationCommand(title, body, sound)

  exec(cmd, (err) => {
    if (err && sound) {
      process.stdout.write("\x07")
    }
  })

  if (process.platform === "linux" && sound) {
    process.stdout.write("\x07")
  }
}
