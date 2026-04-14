# Follow-Up Mark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent, manually-toggled follow-up flag to sessions, displayed as a bell+flag prefix before the status icon, toggled with `i`.

**Architecture:** New `followUp` boolean field on `Session`, backed by a `follow_up` SQLite column (schema v3 migration). The sync context exposes `toggleFollowUp`. The home screen row replaces the trailing `*` notify indicator with a fixed-width `🔔`/`⚑` prefix area before the status icon.

**Tech Stack:** Bun, Bun SQLite, SolidJS, OpenTUI

---

### Task 1: Add `followUp` to types and storage

**Files:**
- Modify: `src/core/types.ts`
- Modify: `src/core/storage.ts`
- Create: `src/core/storage.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/core/storage.test.ts`:

```typescript
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
```

- [ ] **Step 2: Run test to verify it fails**

```bash
bun test src/core/storage.test.ts
```

Expected: fail — `followUp` doesn't exist on `Session` yet.

- [ ] **Step 3: Add `followUp` to `Session` in `src/core/types.ts`**

After the `notify: boolean` line, add:

```typescript
  notify: boolean
  followUp: boolean
  statusChangedAt: Date
```

- [ ] **Step 4: Update `src/core/storage.ts`**

**4a.** Bump schema version:
```typescript
const SCHEMA_VERSION = 3
```

**4b.** Add v2→v3 migration block after the existing `if (!currentVersion || parseInt(currentVersion.value) < 2)` block, reusing the same `currentVersion` variable:

```typescript
    // Migration: v2 -> v3
    if (!currentVersion || parseInt(currentVersion.value) < 3) {
      try {
        this.db.exec("ALTER TABLE sessions ADD COLUMN follow_up INTEGER NOT NULL DEFAULT 0")
      } catch { /* column may already exist */ }
    }
```

**4c.** Update `saveSession` — add `follow_up` to columns and values (after `notify`):

```typescript
  saveSession(session: Session): void {
    const stmt = this.db.prepare(`
      INSERT OR REPLACE INTO sessions (
        id, title, project_path, group_path, sort_order,
        command, wrapper, tool, status, tmux_session,
        created_at, last_accessed,
        parent_session_id, worktree_path, worktree_repo, worktree_branch,
        tool_data, acknowledged,
        notify, follow_up, status_changed_at, restart_count, status_history
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)

    stmt.run(
      session.id,
      session.title,
      session.projectPath,
      session.groupPath,
      session.order,
      session.command,
      session.wrapper,
      session.tool,
      session.status,
      session.tmuxSession,
      session.createdAt.getTime(),
      session.lastAccessed.getTime(),
      session.parentSessionId,
      session.worktreePath,
      session.worktreeRepo,
      session.worktreeBranch,
      JSON.stringify(session.toolData),
      session.acknowledged ? 1 : 0,
      session.notify ? 1 : 0,
      session.followUp ? 1 : 0,
      session.statusChangedAt.getTime(),
      session.restartCount,
      JSON.stringify(session.statusHistory)
    )
  }
```

**4d.** Update `saveSessions` — same addition to the INSERT statement and `insertStmt.run(...)` call:

```typescript
  saveSessions(sessions: Session[]): void {
    const deleteStmt = this.db.prepare("DELETE FROM sessions WHERE id NOT IN (" +
      sessions.map(() => "?").join(",") + ")")
    const insertStmt = this.db.prepare(`
      INSERT OR REPLACE INTO sessions (
        id, title, project_path, group_path, sort_order,
        command, wrapper, tool, status, tmux_session,
        created_at, last_accessed,
        parent_session_id, worktree_path, worktree_repo, worktree_branch,
        tool_data, acknowledged,
        notify, follow_up, status_changed_at, restart_count, status_history
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)

    const transaction = this.db.transaction(() => {
      if (sessions.length === 0) {
        this.db.exec("DELETE FROM sessions")
      } else {
        deleteStmt.run(...sessions.map(s => s.id))
      }

      for (const session of sessions) {
        insertStmt.run(
          session.id,
          session.title,
          session.projectPath,
          session.groupPath,
          session.order,
          session.command,
          session.wrapper,
          session.tool,
          session.status,
          session.tmuxSession,
          session.createdAt.getTime(),
          session.lastAccessed.getTime(),
          session.parentSessionId,
          session.worktreePath,
          session.worktreeRepo,
          session.worktreeBranch,
          JSON.stringify(session.toolData),
          session.acknowledged ? 1 : 0,
          session.notify ? 1 : 0,
          session.followUp ? 1 : 0,
          session.statusChangedAt.getTime(),
          session.restartCount,
          JSON.stringify(session.statusHistory)
        )
      }
    })

    transaction()
  }
```

**4e.** Update `loadSessions` — add `follow_up` to SELECT and mapping:

In the SELECT statement, add `follow_up` after `notify`:
```sql
notify, follow_up, status_changed_at, restart_count, status_history
```

In the row mapping, add after `notify: row.notify === 1,`:
```typescript
      followUp: row.follow_up === 1,
```

**4f.** Update `getSession` — same SELECT and mapping additions as `loadSessions` (4e).

**4g.** Add `setFollowUp` method after `setNotify`:

```typescript
  setFollowUp(id: string, followUp: boolean): void {
    const stmt = this.db.prepare("UPDATE sessions SET follow_up = ? WHERE id = ?")
    stmt.run(followUp ? 1 : 0, id)
  }
```

**4h.** Add `followUp` to `updateSessionField` columnMap:

```typescript
    const columnMap: Record<string, string> = {
      projectPath: "project_path",
      groupPath: "group_path",
      sortOrder: "sort_order",
      tmuxSession: "tmux_session",
      createdAt: "created_at",
      lastAccessed: "last_accessed",
      parentSessionId: "parent_session_id",
      worktreePath: "worktree_path",
      worktreeRepo: "worktree_repo",
      worktreeBranch: "worktree_branch",
      toolData: "tool_data",
      followUp: "follow_up",
    }
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
bun test src/core/storage.test.ts
```

Expected: 4 pass, 0 fail.

- [ ] **Step 6: Run full test suite**

```bash
bun test
```

Expected: 78 pass, 0 fail (74 existing + 4 new).

- [ ] **Step 7: Commit**

```bash
git add src/core/types.ts src/core/storage.ts src/core/storage.test.ts
git commit -m "feat(storage): add followUp field and schema v3 migration"
```

---

### Task 2: Add `toggleFollowUp` to sync context

**Files:**
- Modify: `src/tui/context/sync.tsx`

- [ ] **Step 1: Add `toggleFollowUp` to the session methods**

In `src/tui/context/sync.tsx`, locate the `toggleNotify` method and add `toggleFollowUp` directly after it:

```typescript
        toggleNotify(id: string): void {
          const session = store.sessions.find(s => s.id === id)
          if (!session) return
          storage.setNotify(id, !session.notify)
          storage.touch()
          refresh()
        },
        toggleFollowUp(id: string): void {
          const session = store.sessions.find(s => s.id === id)
          if (!session) return
          storage.setFollowUp(id, !session.followUp)
          storage.touch()
          refresh()
        }
```

- [ ] **Step 2: Run tests**

```bash
bun test
```

Expected: 78 pass, 0 fail.

- [ ] **Step 3: Commit**

```bash
git add src/tui/context/sync.tsx
git commit -m "feat(sync): add toggleFollowUp to session context"
```

---

### Task 3: Update home screen — row rendering, key handler, footer

**Files:**
- Modify: `src/tui/routes/home.tsx`

- [ ] **Step 1: Update session row prefix rendering**

In `src/tui/routes/home.tsx`, find the `SessionItem` function's `reservedChars` comment and update the calculation. Replace:

```typescript
    // Reserve space for: padding (1+indent + 1), status icon (2), notify (*1), space (1), right-side content (~20)
    const reservedChars = (1 + indent) + 1 + 2 + 1 + 1 + rightContentWidth
```

with:

```typescript
    // Reserve: padding (1+indent left + 1 right), bell (2), flag (1), space (1), status (1), space (1), right content
    const reservedChars = (1 + indent) + 1 + 2 + 1 + 1 + 1 + 1 + rightContentWidth
```

- [ ] **Step 2: Replace the status icon / notify block in the row JSX**

Find and replace this block in `SessionItem`:

```tsx
        {/* Status icon */}
        <text fg={isSelected() ? theme.selectedListItemText : statusColor()}>
          {STATUS_ICONS[props.session.status]}
        </text>
        <Show when={props.session.notify}>
          <text fg={isSelected() ? theme.selectedListItemText : theme.accent}>*</text>
        </Show>
        <text> </text>
```

with:

```tsx
        {/* Bell (notify) — fixed 2-wide slot */}
        <text fg={isSelected() ? theme.selectedListItemText : theme.accent}>
          {props.session.notify ? "🔔" : "  "}
        </text>
        {/* Follow-up flag — fixed 1-wide slot */}
        <text fg={isSelected() ? theme.selectedListItemText : theme.warning}>
          {props.session.followUp ? "⚑" : " "}
        </text>
        <text> </text>
        {/* Status icon */}
        <text fg={isSelected() ? theme.selectedListItemText : statusColor()}>
          {STATUS_ICONS[props.session.status]}
        </text>
        <text> </text>
```

- [ ] **Step 3: Add the `i` key handler**

Find the `! to toggle notifications` block:

```typescript
    // ! to toggle notifications
    if (evt.name === "!" || (evt.shift && evt.name === "1")) {
      const session = selectedSession()
      if (session) {
        sync.session.toggleNotify(session.id)
        toast.show({
          message: session.notify ? `Notifications off for ${session.title}` : `Notifications on for ${session.title}`,
          variant: "info",
          duration: 2000
        })
      }
```

Add the `i` handler immediately after the closing `}` of that block:

```typescript
    // i to toggle follow-up mark
    if (evt.name === "i") {
      const session = selectedSession()
      if (session) {
        sync.session.toggleFollowUp(session.id)
        toast.show({
          message: session.followUp ? `Follow-up cleared for ${session.title}` : `Marked for follow-up: ${session.title}`,
          variant: "info",
          duration: 2000
        })
      }
    }
```

- [ ] **Step 4: Add `i  mark` to the footer hints**

Find the `!` hint block in the footer:

```tsx
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>!</text>
          <text fg={theme.textMuted}>notify</text>
        </box>
```

Add the `i` hint immediately after it:

```tsx
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>!</text>
          <text fg={theme.textMuted}>notify</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>i</text>
          <text fg={theme.textMuted}>mark</text>
        </box>
```

- [ ] **Step 5: Build and run tests**

```bash
bun run build && bun test
```

Expected: build succeeds, 78 pass, 0 fail.

- [ ] **Step 6: Commit**

```bash
git add src/tui/routes/home.tsx
git commit -m "feat(home): add follow-up mark indicator and i key toggle

- Bell emoji prefix replaces trailing * for notify
- Flag ⚑ prefix shown when followUp is set
- Both slots fixed-width so status icon stays aligned
- i key toggles follow-up with toast confirmation
- Footer hint added"
```
