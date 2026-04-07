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

export interface NotificationOptions {
  title: string
  subtitle?: string
  body: string
  sound?: boolean
}

export function buildNotificationCommand(
  options: NotificationOptions,
  platform: string = process.platform
): string {
  const safeTitle = options.title.replace(/"/g, '\\"')
  const safeBody = options.body.replace(/"/g, '\\"')
  const safeSubtitle = options.subtitle?.replace(/"/g, '\\"')

  if (platform === "darwin") {
    // Prefer terminal-notifier for persistent (alert-style) notifications
    if (checkTerminalNotifier()) {
      const soundFlag = options.sound ? ' -sound default' : ""
      const subtitleFlag = safeSubtitle ? ` -subtitle "${safeSubtitle}"` : ""
      // -timeout 30: keep notification visible for 30 seconds
      // -group: allows replacing stale notifications from the same session
      return `terminal-notifier -title "${safeTitle}" -message "${safeBody}"${subtitleFlag} -timeout 30${soundFlag}`
    }
    const soundClause = options.sound ? ' sound name "default"' : ""
    const subtitleClause = safeSubtitle ? ` subtitle "${safeSubtitle}"` : ""
    return `osascript -e 'display notification "${safeBody}" with title "${safeTitle}"${subtitleClause}${soundClause}'`
  }

  // Linux: -u critical makes notifications persistent until dismissed
  const urgency = "-u critical"
  return `notify-send ${urgency} "${safeTitle}" "${safeBody}"`
}

export function sendNotification(title: string, body: string, sound?: boolean): void
export function sendNotification(options: NotificationOptions): void
export function sendNotification(titleOrOptions: string | NotificationOptions, body?: string, sound?: boolean): void {
  const options: NotificationOptions = typeof titleOrOptions === "string"
    ? { title: titleOrOptions, body: body!, sound }
    : titleOrOptions

  const cmd = buildNotificationCommand(options)

  exec(cmd, (err) => {
    if (err && options.sound) {
      process.stdout.write("\x07")
    }
  })

  if (process.platform === "linux" && options.sound) {
    process.stdout.write("\x07")
  }
}
