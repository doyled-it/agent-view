/**
 * Desktop notification support
 * macOS: osascript, Linux: notify-send
 */

import { exec } from "child_process"

export function buildNotificationCommand(
  title: string,
  body: string,
  sound: boolean,
  platform: string = process.platform
): string {
  const safeTitle = title.replace(/"/g, '\\"')
  const safeBody = body.replace(/"/g, '\\"')

  if (platform === "darwin") {
    const soundClause = sound ? ' sound name "default"' : ""
    return `osascript -e 'display notification "${safeBody}" with title "${safeTitle}"${soundClause}'`
  }

  return `notify-send "${safeTitle}" "${safeBody}"`
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
