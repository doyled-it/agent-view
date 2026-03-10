import { describe, test, expect } from "bun:test"
import { buildNotificationCommand } from "./notify"

describe("buildNotificationCommand", () => {
  test("builds macOS command with sound", () => {
    const cmd = buildNotificationCommand("Title", "Body", true, "darwin")
    expect(cmd).toContain("osascript")
    expect(cmd).toContain("Title")
    expect(cmd).toContain("Body")
    expect(cmd).toContain("sound name")
  })

  test("builds macOS command without sound", () => {
    const cmd = buildNotificationCommand("Title", "Body", false, "darwin")
    expect(cmd).toContain("osascript")
    expect(cmd).not.toContain("sound name")
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
