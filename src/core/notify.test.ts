import { describe, test, expect } from "bun:test"
import { buildNotificationCommand } from "./notify"

describe("buildNotificationCommand", () => {
  test("builds macOS command with sound", () => {
    const cmd = buildNotificationCommand({ title: "Title", body: "Body", sound: true }, "darwin")
    // Either terminal-notifier or osascript is valid depending on what's installed
    expect(cmd.includes("terminal-notifier") || cmd.includes("osascript")).toBe(true)
    expect(cmd).toContain("Title")
    expect(cmd).toContain("Body")
    // Both backends include a sound indicator when sound is requested
    expect(cmd.includes("-sound default") || cmd.includes("sound name")).toBe(true)
  })

  test("builds macOS command without sound", () => {
    const cmd = buildNotificationCommand({ title: "Title", body: "Body", sound: false }, "darwin")
    expect(cmd.includes("terminal-notifier") || cmd.includes("osascript")).toBe(true)
    expect(cmd).not.toContain("sound name")
    expect(cmd).not.toContain("-sound")
  })

  test("builds macOS command with subtitle", () => {
    const cmd = buildNotificationCommand({ title: "Title", subtitle: "Sub", body: "Body" }, "darwin")
    if (cmd.includes("terminal-notifier")) {
      expect(cmd).toContain('-subtitle "Sub"')
    } else {
      expect(cmd).toContain('subtitle "Sub"')
    }
  })

  test("builds Linux command", () => {
    const cmd = buildNotificationCommand({ title: "Title", body: "Body" }, "linux")
    expect(cmd).toContain("notify-send")
  })

  test("escapes quotes in title and body", () => {
    const cmd = buildNotificationCommand({ title: 'Say "hello"', body: 'Body "test"' }, "darwin")
    expect(cmd).toContain('\\"hello\\"')
    expect(cmd).toContain('\\"test\\"')
  })

  test("includes timeout for terminal-notifier", () => {
    const cmd = buildNotificationCommand({ title: "Title", body: "Body" }, "darwin")
    if (cmd.includes("terminal-notifier")) {
      expect(cmd).toContain("-timeout 30")
    }
  })
})
