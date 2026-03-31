import { describe, test, expect } from "bun:test"
import { buildNotificationCommand } from "./notify"

describe("buildNotificationCommand", () => {
  test("builds macOS command with sound", () => {
    const cmd = buildNotificationCommand("Title", "Body", true, "darwin")
    // Either terminal-notifier or osascript is valid depending on what's installed
    expect(cmd.includes("terminal-notifier") || cmd.includes("osascript")).toBe(true)
    expect(cmd).toContain("Title")
    expect(cmd).toContain("Body")
    // Both backends include a sound indicator when sound is requested
    expect(cmd.includes("-sound default") || cmd.includes("sound name")).toBe(true)
  })

  test("builds macOS command without sound", () => {
    const cmd = buildNotificationCommand("Title", "Body", false, "darwin")
    expect(cmd.includes("terminal-notifier") || cmd.includes("osascript")).toBe(true)
    expect(cmd).not.toContain("sound name")
    expect(cmd).not.toContain("-sound")
  })

  test("builds Linux command", () => {
    const cmd = buildNotificationCommand("Title", "Body", false, "linux")
    expect(cmd).toContain("notify-send")
  })

  test("escapes quotes in title and body", () => {
    const cmd = buildNotificationCommand('Say "hello"', 'Body "test"', false, "darwin")
    expect(cmd).toContain('\\"hello\\"')
    expect(cmd).toContain('\\"test\\"')
  })
})
