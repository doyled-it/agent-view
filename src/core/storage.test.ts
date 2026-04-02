import { describe, test, expect, beforeEach, afterEach } from "bun:test"
import { Storage } from "./storage"
import type { Session } from "./types"
import fs from "fs"
import path from "path"
import os from "os"

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "test-1",
    title: "Test Session",
    projectPath: "/tmp/test",
    groupPath: "my-sessions",
    order: 0,
    command: "claude",
    wrapper: "",
    tool: "claude",
    status: "idle",
    tmuxSession: "agentorch_test",
    createdAt: new Date(),
    lastAccessed: new Date(),
    parentSessionId: "",
    worktreePath: "",
    worktreeRepo: "",
    worktreeBranch: "",
    toolData: {},
    acknowledged: false,
    notify: false,
    followUp: false,
    statusChangedAt: new Date(),
    restartCount: 0,
    statusHistory: [],
    ...overrides,
  }
}

let storage: Storage
let dbPath: string

beforeEach(() => {
  dbPath = path.join(os.tmpdir(), `av-test-${Date.now()}.db`)
  storage = new Storage({ dbPath })
  storage.migrate()
})

afterEach(() => {
  storage.close()
  try { fs.unlinkSync(dbPath) } catch {}
})

describe("followUp persistence", () => {
  test("defaults to false on save and load", () => {
    storage.saveSession(makeSession({ followUp: false }))
    const sessions = storage.loadSessions()
    expect(sessions[0]!.followUp).toBe(false)
  })

  test("persists true on save and load", () => {
    storage.saveSession(makeSession({ followUp: true }))
    const sessions = storage.loadSessions()
    expect(sessions[0]!.followUp).toBe(true)
  })

  test("setFollowUp toggles the flag", () => {
    storage.saveSession(makeSession({ followUp: false }))
    storage.setFollowUp("test-1", true)
    const sessions = storage.loadSessions()
    expect(sessions[0]!.followUp).toBe(true)
  })

  test("getSession includes followUp", () => {
    storage.saveSession(makeSession({ followUp: true }))
    const session = storage.getSession("test-1")
    expect(session?.followUp).toBe(true)
  })
})
