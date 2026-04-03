import { describe, test, expect } from "bun:test"
import { getSignalFilePath } from "./tmux"

describe("getSignalFilePath", () => {
  test("includes process uid", () => {
    const path = getSignalFilePath()
    expect(path).toContain(String(process.getuid!()))
    expect(path).toStartWith("/tmp/agent-view-cmd-palette-")
  })
})
