/**
 * Home screen with dual-column layout
 * Shows session list on left, preview pane on right
 */

import { createMemo, createSignal, For, Show, createEffect, onCleanup } from "solid-js"
import { TextAttributes, ScrollBoxRenderable } from "@opentui/core"
import { useTerminalDimensions, useKeyboard, useRenderer } from "@opentui/solid"
import { useTheme } from "@tui/context/theme"
import { useSync } from "@tui/context/sync"
import { useDialog } from "@tui/ui/dialog"
import { useToast } from "@tui/ui/toast"
import { DialogNew } from "@tui/component/dialog-new"
import { DialogFork } from "@tui/component/dialog-fork"
import { DialogRename } from "@tui/component/dialog-rename"
import { DialogGroup } from "@tui/component/dialog-group"
import { DialogMove } from "@tui/component/dialog-move"
import { DialogConfirm } from "@tui/component/dialog-confirm"
import { attachSessionSync, capturePane, captureFullScrollback, wasCommandPaletteRequested } from "@/core/tmux"
import { canFork } from "@/core/claude"
import type { Session, Group } from "@/core/types"
import { formatRelativeTime, formatSmartTime, formatDurationShort, formatDuration, truncatePath } from "@tui/util/locale"
import { STATUS_ICONS } from "@tui/util/status"
import { sortSessionsByCreatedAt } from "@tui/util/session"
import {
  flattenGroupTree,
  ensureDefaultGroup,
  getGroupSessionCount,
  getGroupStatusSummary,
  DEFAULT_GROUP_PATH,
  type GroupedItem
} from "@tui/util/groups"
import fs from "fs"
import path from "path"
import os from "os"

const logFile = path.join(os.homedir(), ".agent-orchestrator", "debug.log")
function log(...args: unknown[]) {
  const msg = `[${new Date().toISOString()}] [HOME] ${args.map(a => typeof a === "object" ? JSON.stringify(a) : String(a)).join(" ")}\n`
  try { fs.appendFileSync(logFile, msg) } catch {}
}

const LOGO = `
 █████╗  ██████╗ ███████╗███╗   ██╗████████╗
██╔══██╗██╔════╝ ██╔════╝████╗  ██║╚══██╔══╝
███████║██║  ███╗█████╗  ██╔██╗ ██║   ██║
██╔══██║██║   ██║██╔══╝  ██║╚██╗██║   ██║
██║  ██║╚██████╔╝███████╗██║ ╚████║   ██║
╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚═══╝   ╚═╝
██╗   ██╗██╗███████╗██╗    ██╗
██║   ██║██║██╔════╝██║    ██║
██║   ██║██║█████╗  ██║ █╗ ██║
╚██╗ ██╔╝██║██╔══╝  ██║███╗██║
 ╚████╔╝ ██║███████╗╚███╔███╔╝
  ╚═══╝  ╚═╝╚══════╝ ╚══╝╚══╝
`.trim()

const SMALL_LOGO = `◆ AGENT VIEW`

function stripAnsi(str: string): string {
  return str.replace(/\x1B\[[0-9;]*[a-zA-Z]/g, "")
}

// Minimum width for dual-column layout
const DUAL_COLUMN_MIN_WIDTH = 100
const LEFT_PANEL_RATIO = 0.35

export function Home() {
  const dimensions = useTerminalDimensions()
  const { theme } = useTheme()
  const sync = useSync()
  const dialog = useDialog()
  const toast = useToast()
  const renderer = useRenderer()

  const [selectedIndex, setSelectedIndex] = createSignal(0)
  const [previewContent, setPreviewContent] = createSignal<string>("")
  const [previewLoading, setPreviewLoading] = createSignal(false)
  const [searchActive, setSearchActive] = createSignal(false)
  const [searchQuery, setSearchQuery] = createSignal("")
  const [searchResults, setSearchResults] = createSignal<string[]>([])
  const [searchMatchIndex, setSearchMatchIndex] = createSignal(0)
  const [searchTotalMatches, setSearchTotalMatches] = createSignal(0)
  let scrollRef: ScrollBoxRenderable | undefined
  let previewDebounceTimer: ReturnType<typeof setTimeout> | undefined
  let previewFetchAbort = false

  // Check if we should use dual-column layout
  const useDualColumn = createMemo(() => dimensions().width >= DUAL_COLUMN_MIN_WIDTH)

  // Calculate panel widths
  const leftWidth = createMemo(() => {
    if (!useDualColumn()) return dimensions().width
    return Math.floor(dimensions().width * LEFT_PANEL_RATIO)
  })

  const rightWidth = createMemo(() => {
    if (!useDualColumn()) return 0
    return dimensions().width - leftWidth() - 1 // -1 for separator
  })

  // Ensure default group exists on first load
  createEffect(() => {
    const currentGroups = sync.group.list()
    const withDefault = ensureDefaultGroup(currentGroups)
    if (withDefault.length !== currentGroups.length) {
      sync.group.save(withDefault)
    }
  })

  // Get all sessions
  const allSessions = createMemo(() => sync.session.list())

  // Get grouped items (groups + sessions flattened)
  const groupedItems = createMemo(() => {
    const groups = ensureDefaultGroup(sync.group.list())
    return flattenGroupTree(allSessions(), groups)
  })

  // Keep selection in bounds
  createEffect(() => {
    const len = groupedItems().length
    if (selectedIndex() >= len && len > 0) {
      setSelectedIndex(len - 1)
    }
  })

  // Get the selected item (could be group or session)
  const selectedItem = createMemo(() => groupedItems()[selectedIndex()])

  // Get the selected session (only if a session is selected)
  const selectedSession = createMemo(() => {
    const item = selectedItem()
    return item?.type === "session" ? item.session : undefined
  })

  // Get the selected group (only if a group is selected)
  const selectedGroup = createMemo(() => {
    const item = selectedItem()
    return item?.type === "group" ? item.group : undefined
  })

  // Fetch preview with debounce; keep showing previous content while loading
  createEffect(() => {
    const session = selectedSession()

    if (previewDebounceTimer) {
      clearTimeout(previewDebounceTimer)
    }

    if (!session || !session.tmuxSession) {
      setPreviewContent("")
      setPreviewLoading(false)
      return
    }

    // Only show loading if we have no content yet (first load)
    if (!previewContent()) {
      setPreviewLoading(true)
    }
    previewFetchAbort = false

    // Debounce: 150ms delay to prevent rapid fetching during navigation
    previewDebounceTimer = setTimeout(async () => {
      if (previewFetchAbort) return

      try {
        const content = await capturePane(session.tmuxSession, {
          startLine: -200, // Last 200 lines
          join: true
        })

        if (!previewFetchAbort) {
          setPreviewContent(content)
        }
      } catch {
        // Keep existing content on error, don't clear
      } finally {
        if (!previewFetchAbort) {
          setPreviewLoading(false)
        }
      }
    }, 150)
  })

  onCleanup(() => {
    previewFetchAbort = true
    if (previewDebounceTimer) {
      clearTimeout(previewDebounceTimer)
    }
  })

  // Session stats
  const stats = createMemo(() => {
    const byStatus = sync.session.byStatus()
    return {
      running: byStatus.running.length,
      waiting: byStatus.waiting.length,
      total: sync.session.list().length
    }
  })

  function move(delta: number) {
    const len = groupedItems().length
    if (len === 0) return
    let next = selectedIndex() + delta
    if (next < 0) next = len - 1
    if (next >= len) next = 0
    setSelectedIndex(next)
  }

  // Jump to group by index (1-9)
  function jumpToGroup(groupIndex: number) {
    const items = groupedItems()
    const idx = items.findIndex(item => item.type === "group" && item.groupIndex === groupIndex)
    if (idx >= 0) {
      setSelectedIndex(idx)
    }
  }

  // Handle deleting a group
  async function handleDeleteGroup(group: Group) {
    if (group.path === DEFAULT_GROUP_PATH) {
      toast.show({ message: "Cannot delete the default group", variant: "error", duration: 2000 })
      return
    }

    const sessionCount = getGroupSessionCount(allSessions(), group.path)
    const message = sessionCount > 0
      ? `Delete group "${group.name}"? Its ${sessionCount} session(s) will be moved to the default group.`
      : `Delete group "${group.name}"?`

    dialog.push(() => (
      <DialogConfirm
        title="Delete Group"
        message={message}
        onConfirm={() => {
          if (sessionCount > 0) {
            const sessionsInGroup = allSessions().filter(s => s.groupPath === group.path)
            for (const session of sessionsInGroup) {
              sync.session.moveToGroup(session.id, DEFAULT_GROUP_PATH)
            }
          }
          sync.group.delete(group.path)
          toast.show({ message: `Deleted group "${group.name}"`, variant: "info", duration: 2000 })
          sync.refresh()
        }}
      />
    ))
  }

  function handleAttach(session: Session) {
    if (!session.tmuxSession) {
      toast.show({ message: "Session has no tmux session", variant: "error", duration: 2000 })
      return
    }

    previewFetchAbort = true
    renderer.suspend()
    let attachError: Error | undefined
    try {
      attachSessionSync(session.tmuxSession)
    } catch (err) {
      attachError = err as Error
    }
    renderer.resume()
    sync.refresh()

    if (attachError) {
      if (attachError.message.includes("version mismatch")) {
        toast.show({
          title: "tmux version mismatch",
          message: "Run 'tmux kill-server' in a terminal, then restart Agent View",
          variant: "error",
          duration: 8000
        })
      } else {
        toast.show({ message: `Attach failed: ${attachError.message}`, variant: "error", duration: 4000 })
      }
      return
    }

    // Consume the signal file if it exists (Ctrl+K just goes home)
    wasCommandPaletteRequested()
  }

  async function handleDelete(session: Session) {
    dialog.push(() => (
      <DialogConfirm
        title="Delete Session"
        message={`Delete "${session.title}"? This will kill the tmux session.`}
        onConfirm={async () => {
          try {
            await sync.session.delete(session.id)
            toast.show({ message: `Deleted ${session.title}`, variant: "info", duration: 2000 })
          } catch (err) {
            toast.error(err as Error)
          }
        }}
      />
    ))
  }

  async function handleRestart(session: Session) {
    try {
      await sync.session.restart(session.id)
      toast.show({ message: "Session restarted", variant: "success", duration: 2000 })
      sync.refresh()
    } catch (err) {
      toast.error(err as Error)
    }
  }

  async function handleFork(session: Session) {
    log("handleFork called for session:", session.id, "tool:", session.tool, "projectPath:", session.projectPath)

    // Only Claude sessions can be forked
    if (session.tool !== "claude") {
      log("Fork rejected: not a claude session")
      toast.show({ message: "Only Claude sessions can be forked", variant: "error", duration: 2000 })
      return
    }

    // Check if session has an active Claude session ID
    log("Checking canFork for projectPath:", session.projectPath)
    const canForkSession = await canFork(session.projectPath)
    log("canFork result:", canForkSession)

    if (!canForkSession) {
      log("Fork rejected: no active Claude session")
      toast.show({
        message: "Cannot fork: no active Claude session detected (session must be running)",
        variant: "error",
        duration: 3000
      })
      return
    }

    try {
      log("Calling sync.session.fork")
      const forked = await sync.session.fork({ sourceSessionId: session.id })
      log("Fork successful:", forked.id)
      toast.show({ message: `Forked as ${forked.title}`, variant: "success", duration: 2000 })
      sync.refresh()
    } catch (err) {
      log("Fork error:", err)
      toast.error(err as Error)
    }
  }

  async function handleExport(session: Session) {
    if (!session.tmuxSession) {
      toast.show({ message: "Session has no tmux session", variant: "error", duration: 2000 })
      return
    }

    try {
      const content = await captureFullScrollback(session.tmuxSession)

      const logsDir = path.join(os.homedir(), ".agent-view", "logs")
      fs.mkdirSync(logsDir, { recursive: true })

      const timestamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19)
      const safeName = session.title.replace(/[^a-z0-9-]/gi, "-").slice(0, 30)
      const logPath = path.join(logsDir, `${safeName}-${timestamp}.log`)

      fs.writeFileSync(logPath, content)

      toast.show({
        message: `Exported to ${logPath}`,
        variant: "success",
        duration: 4000
      })
    } catch (err) {
      toast.error(err as Error)
    }
  }

  async function executeSearch(query: string) {
    const session = selectedSession()
    if (!session?.tmuxSession || !query) {
      setSearchResults([])
      setSearchTotalMatches(0)
      return
    }

    try {
      const content = await captureFullScrollback(session.tmuxSession)
      const lines = content.split("\n")
      const lowerQuery = query.toLowerCase()

      const matchingBlocks: string[] = []
      let totalMatches = 0

      for (let i = 0; i < lines.length; i++) {
        if (lines[i]!.toLowerCase().includes(lowerQuery)) {
          totalMatches++
          const start = Math.max(0, i - 3)
          const end = Math.min(lines.length - 1, i + 3)
          const block = lines.slice(start, end + 1).map((line, idx) => {
            const lineNum = start + idx
            const prefix = lineNum === i ? ">>> " : "    "
            return `${prefix}${line}`
          }).join("\n")
          matchingBlocks.push(block)
        }
      }

      setSearchResults(matchingBlocks)
      setSearchTotalMatches(totalMatches)
      setSearchMatchIndex(0)
    } catch {
      setSearchResults([])
      setSearchTotalMatches(0)
    }
  }

  // Keyboard navigation
  useKeyboard((evt) => {
    log("Home useKeyboard:", evt.name, "dialog.stack.length:", dialog.stack.length)

    // Handle search mode — capture all input so dashboard shortcuts don't fire
    if (searchActive()) {
      if (evt.name === "escape") {
        setSearchActive(false)
        setSearchQuery("")
        setSearchResults([])
        return
      }
      if (evt.name === "return") {
        executeSearch(searchQuery())
        return
      }
      // n/N navigate matches only when results exist
      if (evt.name === "n" && !evt.shift && searchResults().length > 0) {
        setSearchMatchIndex((searchMatchIndex() + 1) % searchResults().length)
        return
      }
      if (evt.name === "n" && evt.shift && searchResults().length > 0) {
        setSearchMatchIndex((searchMatchIndex() - 1 + searchResults().length) % searchResults().length)
        return
      }
      // Backspace
      if (evt.name === "backspace") {
        setSearchQuery(q => q.slice(0, -1))
        return
      }
      // Printable characters — append to query
      if (evt.name.length === 1 && !evt.ctrl && !evt.meta) {
        setSearchQuery(q => q + evt.name)
        return
      }
      // Swallow everything else so dashboard shortcuts don't fire
      return
    }

    // Skip if dialog is open
    if (dialog.stack.length > 0) return

    if (evt.name === "up" || evt.name === "k") {
      move(-1)
    }
    if (evt.name === "down" || evt.name === "j") {
      move(1)
    }
    if (evt.name === "pageup") {
      move(-10)
    }
    if (evt.name === "pagedown") {
      move(10)
    }
    if (evt.name === "home") {
      setSelectedIndex(0)
    }
    if (evt.name === "end") {
      setSelectedIndex(Math.max(0, groupedItems().length - 1))
    }

    // Number keys 1-9 to jump to groups
    if (/^[1-9]$/.test(evt.name)) {
      jumpToGroup(parseInt(evt.name, 10))
    }

    // Right arrow: expand group (or attach to session)
    if (evt.name === "right" || evt.name === "l") {
      const item = selectedItem()
      if (item?.type === "group" && item.group && !item.group.expanded) {
        sync.group.toggle(item.group.path)
      } else if (item?.type === "session" && item.session) {
        handleAttach(item.session)
      }
    }

    // Left arrow: collapse group
    if (evt.name === "left" || evt.name === "h") {
      const item = selectedItem()
      if (item?.type === "group" && item.group && item.group.expanded) {
        sync.group.toggle(item.group.path)
      } else if (item?.type === "session") {
        // When on a session, collapse its parent group
        const groupItem = groupedItems().find(
          i => i.type === "group" && i.groupPath === item.groupPath
        )
        if (groupItem?.group?.expanded) {
          sync.group.toggle(groupItem.group.path)
        }
      }
    }

    // Enter: attach to session OR toggle group expand/collapse
    if (evt.name === "return") {
      const item = selectedItem()
      if (item?.type === "session" && item.session) {
        handleAttach(item.session)
      } else if (item?.type === "group" && item.group) {
        sync.group.toggle(item.group.path)
      }
    }

    // d to delete session OR group
    if (evt.name === "d") {
      const item = selectedItem()
      if (item?.type === "session" && item.session) {
        handleDelete(item.session)
      } else if (item?.type === "group" && item.group) {
        handleDeleteGroup(item.group)
      }
    }

    // r to restart (lowercase only, sessions only)
    if (evt.name === "r" && !evt.shift) {
      const session = selectedSession()
      if (session) {
        handleRestart(session)
      }
    }

    // R (Shift+r) to rename session OR group
    if (evt.name === "r" && evt.shift) {
      const item = selectedItem()
      if (item?.type === "session" && item.session) {
        dialog.push(() => <DialogRename session={item.session!} />)
      } else if (item?.type === "group" && item.group) {
        dialog.push(() => <DialogGroup mode="rename" group={item.group!} />)
      }
    }

    // g to create new group
    if (evt.name === "g" && !evt.shift) {
      dialog.push(() => <DialogGroup mode="create" />)
    }

    // m to move session to group
    if (evt.name === "m") {
      const session = selectedSession()
      if (session) {
        dialog.push(() => <DialogMove session={session} />)
      }
    }

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
    }

    // f to fork (quick)
    if (evt.name === "f" && !evt.shift) {
      log("f pressed, selectedSession:", selectedSession()?.id, selectedSession()?.tool)
      const session = selectedSession()
      if (session) {
        log("Calling handleFork for session:", session.id)
        handleFork(session)
      }
    }

    // F (Shift+f) to fork with options dialog
    if (evt.name === "f" && evt.shift) {
      const session = selectedSession()
      if (session) {
        if (session.tool !== "claude") {
          toast.show({ message: "Only Claude sessions can be forked", variant: "error", duration: 2000 })
          return
        }
        dialog.push(() => <DialogFork session={session} />)
      }
    }

    // e to export session log
    if (evt.name === "e") {
      const session = selectedSession()
      if (session) {
        handleExport(session)
      }
    }

    // / to start search
    if (evt.name === "/") {
      setSearchActive(true)
      setSearchQuery("")
      setSearchResults([])
      return
    }
  })

  // Get preview lines that fit in the available height
  const previewLines = createMemo(() => {
    const content = previewContent()
    if (!content) return []

    const lines = content.split("\n")
    // Strip trailing empty lines
    while (lines.length > 0 && lines[lines.length - 1]?.trim() === "") {
      lines.pop()
    }
    return lines
  })

  // Render group header
  function GroupHeader(props: { group: Group; index: number }) {
    const isSelected = createMemo(() => props.index === selectedIndex())
    const sessionCount = createMemo(() => getGroupSessionCount(allSessions(), props.group.path))
    const statusSummary = createMemo(() => getGroupStatusSummary(allSessions(), props.group.path))

    // Find group index for hotkey hint
    const item = createMemo(() => groupedItems()[props.index])
    const groupIndex = createMemo(() => item()?.groupIndex)

    return (
      <box
        flexDirection="row"
        paddingLeft={1}
        paddingRight={1}
        height={1}
        backgroundColor={isSelected() ? theme.primary : theme.backgroundElement}
        onMouseUp={() => {
          setSelectedIndex(props.index)
          sync.group.toggle(props.group.path)
        }}
        onMouseOver={() => setSelectedIndex(props.index)}
      >
        {/* Expand/collapse arrow */}
        <text fg={isSelected() ? theme.selectedListItemText : theme.accent}>
          {props.group.expanded ? "\u25BC" : "\u25B6"}
        </text>
        <text> </text>

        {/* Group name */}
        <text
          fg={isSelected() ? theme.selectedListItemText : theme.text}
          attributes={TextAttributes.BOLD}
        >
          {props.group.name}
        </text>

        {/* Spacer */}
        <text flexGrow={1}> </text>

        {/* Status indicators */}
        <Show when={statusSummary().running > 0}>
          <text fg={isSelected() ? theme.selectedListItemText : theme.success}>
            {STATUS_ICONS.running}{statusSummary().running}
          </text>
          <text> </text>
        </Show>
        <Show when={statusSummary().waiting > 0}>
          <text fg={isSelected() ? theme.selectedListItemText : theme.warning}>
            {STATUS_ICONS.waiting}{statusSummary().waiting}
          </text>
          <text> </text>
        </Show>

        {/* Session count */}
        <text fg={isSelected() ? theme.selectedListItemText : theme.textMuted}>
          ({sessionCount()})
        </text>

        {/* Hotkey hint */}
        <Show when={groupIndex()}>
          <text> </text>
          <text fg={isSelected() ? theme.selectedListItemText : theme.textMuted}>
            [{groupIndex()}]
          </text>
        </Show>
      </box>
    )
  }

  // Render session list item (indented under group)
  function SessionItem(props: { session: Session; index: number; indented?: boolean }) {
    const isSelected = createMemo(() => props.index === selectedIndex())
    const statusColor = createMemo(() => {
      switch (props.session.status) {
        case "running": return theme.success
        case "waiting": return theme.warning
        case "error": return theme.error
        default: return theme.textMuted
      }
    })

    // Indentation for sessions under groups
    const indent = props.indented ? 2 : 0

    // Dynamic title truncation based on available panel width
    // Reserve space for: padding (1+indent + 1), status icon (2), notify (*1), space (1), right-side content (~20)
    const rightContentWidth = useDualColumn() ? 18 : 28 // duration + timestamp (+ tool in single col)
    const reservedChars = (1 + indent) + 1 + 2 + 1 + 1 + rightContentWidth
    const maxTitleLen = createMemo(() => Math.max(8, leftWidth() - reservedChars))
    const title = createMemo(() => {
      const max = maxTitleLen()
      return props.session.title.length > max
        ? props.session.title.slice(0, max - 2) + ".."
        : props.session.title
    })

    const duration = createMemo(() => {
      const elapsed = Date.now() - props.session.createdAt.getTime()
      return formatDurationShort(elapsed)
    })

    return (
      <box
        flexDirection="row"
        paddingLeft={1 + indent}
        paddingRight={1}
        height={1}
        backgroundColor={isSelected() ? theme.primary : undefined}
        onMouseUp={() => {
          setSelectedIndex(props.index)
          handleAttach(props.session)
        }}
        onMouseOver={() => setSelectedIndex(props.index)}
      >
        {/* Status icon */}
        <text fg={isSelected() ? theme.selectedListItemText : statusColor()}>
          {STATUS_ICONS[props.session.status]}
        </text>
        <Show when={props.session.notify}>
          <text fg={isSelected() ? theme.selectedListItemText : theme.accent}>*</text>
        </Show>
        <text> </text>

        {/* Title */}
        <text
          fg={isSelected() ? theme.selectedListItemText : theme.text}
          attributes={isSelected() ? TextAttributes.BOLD : undefined}
        >
          {title()}
        </text>

        {/* Spacer */}
        <text flexGrow={1}> </text>

        {/* Tool (only in single column) */}
        <Show when={!useDualColumn()}>
          <text fg={isSelected() ? theme.selectedListItemText : theme.accent}>
            {props.session.tool}
          </text>
          <text> </text>
        </Show>

        {/* Duration */}
        <text fg={isSelected() ? theme.selectedListItemText : theme.textMuted}>
          {duration()}
        </text>
        <text> </text>

        {/* Status timestamp — show "waiting 5m ago" for non-running, normal time for running */}
        <text fg={isSelected() ? theme.selectedListItemText : theme.textMuted}>
          {props.session.status !== "running" && props.session.status !== "idle"
            ? formatRelativeTime(props.session.statusChangedAt)
            : formatSmartTime(props.session.lastAccessed)
          }
        </text>
      </box>
    )
  }

  // Render preview pane header
  function PreviewHeader() {
    const session = selectedSession()
    if (!session) return null

    const statusColor = createMemo(() => {
      switch (session.status) {
        case "running": return theme.success
        case "waiting": return theme.warning
        case "error": return theme.error
        default: return theme.textMuted
      }
    })

    // Compute time-in-status from statusHistory
    const statusTimes = createMemo(() => {
      const history = session.statusHistory || []
      const times: Record<string, number> = {}
      for (let i = 0; i < history.length; i++) {
        const entry = history[i]!
        const nextTime = i + 1 < history.length ? history[i + 1]!.timestamp : Date.now()
        const duration = nextTime - entry.timestamp
        times[entry.status] = (times[entry.status] || 0) + duration
      }
      return times
    })

    const elapsed = createMemo(() => {
      return formatDuration(Date.now() - session.createdAt.getTime())
    })

    return (
      <box flexDirection="column" paddingLeft={1} paddingRight={1}>
        {/* Session title and status */}
        <box flexDirection="row" justifyContent="space-between" height={1}>
          <text fg={theme.text} attributes={TextAttributes.BOLD}>
            {session.title}
          </text>
          <box flexDirection="row" gap={1}>
            <text fg={statusColor()}>{STATUS_ICONS[session.status]}</text>
            <text fg={statusColor()}>{session.status}</text>
          </box>
        </box>

        {/* Session info */}
        <box flexDirection="row" gap={2} height={1}>
          <text fg={theme.textMuted}>{truncatePath(session.projectPath, rightWidth() - 20)}</text>
        </box>

        {/* Metrics line */}
        <box flexDirection="row" gap={2} height={1}>
          <text fg={theme.accent}>{session.tool}</text>
          <text fg={theme.textMuted}>Duration: {elapsed()}</text>
          <Show when={session.restartCount > 0}>
            <text fg={theme.textMuted}>Restarts: {session.restartCount}</text>
          </Show>
          <Show when={statusTimes().waiting}>
            <text fg={theme.warning}>Waiting: {formatDuration(statusTimes().waiting || 0)}</text>
          </Show>
          <Show when={session.worktreeBranch}>
            <text fg={theme.info}>{session.worktreeBranch}</text>
          </Show>
        </box>

        {/* Separator */}
        <box height={1}>
          <text fg={theme.border}>{"─".repeat(rightWidth() - 2)}</text>
        </box>
      </box>
    )
  }

  // Render empty state with logo
  function EmptyState() {
    return (
      <box flexGrow={1} alignItems="center" justifyContent="center" flexDirection="column" gap={2}>
        <text fg={theme.primary}>{LOGO}</text>
        <box height={1} />
        <text fg={theme.textMuted}>No sessions yet</text>
        <box flexDirection="row">
          <text fg={theme.textMuted}>Press </text>
          <text fg={theme.text} attributes={TextAttributes.BOLD}>n</text>
          <text fg={theme.textMuted}> to create a new session</text>
        </box>
      </box>
    )
  }

  // Render logo in preview when no session
  function PreviewLogo() {
    return (
      <box flexGrow={1} alignItems="center" justifyContent="center" flexDirection="column">
        <text fg={theme.primary}>{LOGO}</text>
        <box height={2} />
        <text fg={theme.textMuted}>Select a session to see preview</text>
      </box>
    )
  }

  return (
    <box
      flexDirection="column"
      width={dimensions().width}
      height={dimensions().height}
      backgroundColor={theme.background}
    >
      {/* Header */}
      <box
        flexDirection="row"
        justifyContent="space-between"
        paddingLeft={2}
        paddingRight={2}
        height={1}
        backgroundColor={theme.backgroundPanel}
      >
        <text fg={theme.primary} attributes={TextAttributes.BOLD}>
          AGENT VIEW
        </text>
        <box flexDirection="row" gap={2}>
          <Show when={stats().running > 0}>
            <text fg={theme.success}>● {stats().running}</text>
          </Show>
          <Show when={stats().waiting > 0}>
            <text fg={theme.warning}>◐ {stats().waiting}</text>
          </Show>
          <text fg={theme.textMuted}>{stats().total} sessions</text>
        </box>
      </box>

      {/* Main content area */}
      <Show
        when={allSessions().length > 0}
        fallback={<EmptyState />}
      >
        <box flexDirection="row" flexGrow={1}>
          {/* Left panel: Session list */}
          <box flexDirection="column" width={leftWidth()}>
            {/* Panel title */}
            <box
              height={1}
              paddingLeft={1}
              paddingRight={1}
              backgroundColor={theme.backgroundElement}
            >
              <text fg={theme.textMuted} attributes={TextAttributes.BOLD}>
                SESSIONS
              </text>
            </box>

            {/* Session list (grouped) */}
            <scrollbox
              flexGrow={1}
              scrollbarOptions={{ visible: true }}
              ref={(r: ScrollBoxRenderable) => { scrollRef = r }}
            >
              <For each={groupedItems()}>
                {(item, index) => (
                  <Show
                    when={item.type === "group"}
                    fallback={
                      <SessionItem
                        session={item.session!}
                        index={index()}
                        indented={true}
                      />
                    }
                  >
                    <GroupHeader group={item.group!} index={index()} />
                  </Show>
                )}
              </For>
            </scrollbox>
          </box>

          {/* Separator */}
          <Show when={useDualColumn()}>
            <box width={1} backgroundColor={theme.border}>
              <text fg={theme.border}>│</text>
            </box>
          </Show>

          {/* Right panel: Preview */}
          <Show when={useDualColumn()}>
            <box flexDirection="column" width={rightWidth()}>
              {/* Panel title */}
              <box
                height={1}
                paddingLeft={1}
                paddingRight={1}
                backgroundColor={theme.backgroundElement}
              >
                <text fg={theme.textMuted} attributes={TextAttributes.BOLD}>
                  PREVIEW
                </text>
              </box>

              {/* Preview content */}
              <Show
                when={selectedSession()}
                fallback={<PreviewLogo />}
              >
                <box flexDirection="column" flexGrow={1}>
                  <PreviewHeader />

                  {/* Terminal output OR search results */}
                  <Show
                    when={searchActive() && searchResults().length > 0}
                    fallback={
                      <scrollbox flexGrow={1} scrollbarOptions={{ visible: true }}>
                        <Show
                          when={previewLines().length > 0}
                          fallback={
                            <box paddingLeft={1} paddingTop={1}>
                              <text fg={theme.textMuted}>
                                {previewLoading() ? "Loading..." : "No output yet"}
                              </text>
                            </box>
                          }
                        >
                          <box flexDirection="column" paddingLeft={1}>
                            <For each={previewLines().slice(-50)}>
                              {(line) => (
                                <text fg={theme.text}>{stripAnsi(line).slice(0, rightWidth() - 4)}</text>
                              )}
                            </For>
                          </box>
                        </Show>
                      </scrollbox>
                    }
                  >
                    <scrollbox flexGrow={1} scrollbarOptions={{ visible: true }}>
                      <box flexDirection="column" paddingLeft={1}>
                        <For each={searchResults()[searchMatchIndex()]?.split("\n") || []}>
                          {(line) => (
                            <text fg={line.startsWith(">>>") ? theme.warning : theme.text}>
                              {line.slice(0, rightWidth() - 4)}
                            </text>
                          )}
                        </For>
                      </box>
                    </scrollbox>
                  </Show>
                </box>
              </Show>

              {/* Search bar */}
              <Show when={searchActive()}>
                <box height={1} paddingLeft={1} backgroundColor={theme.backgroundElement} flexDirection="row">
                  <text fg={theme.primary}>/</text>
                  <text fg={theme.text}>{searchQuery()}</text>
                  <text fg={theme.primary}>█</text>
                  <text flexGrow={1}> </text>
                  <Show when={searchTotalMatches() > 0}>
                    <text fg={theme.textMuted}>
                      Match {searchMatchIndex() + 1}/{searchTotalMatches()} (n/N)
                    </text>
                  </Show>
                </box>
              </Show>
            </box>
          </Show>
        </box>
      </Show>

      {/* Footer with keybinds */}
      <box
        flexDirection="row"
        width={dimensions().width}
        paddingLeft={2}
        paddingRight={2}
        height={2}
        backgroundColor={theme.backgroundPanel}
        justifyContent="space-between"
      >
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>↑↓</text>
          <text fg={theme.textMuted}>navigate</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>←→</text>
          <text fg={theme.textMuted}>fold</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>Enter</text>
          <text fg={theme.textMuted}>attach</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>n</text>
          <text fg={theme.textMuted}>new</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>g</text>
          <text fg={theme.textMuted}>group</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>m</text>
          <text fg={theme.textMuted}>move</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>d</text>
          <text fg={theme.textMuted}>delete</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>R</text>
          <text fg={theme.textMuted}>rename</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>f</text>
          <text fg={theme.textMuted}>fork</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>1-9</text>
          <text fg={theme.textMuted}>jump</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>!</text>
          <text fg={theme.textMuted}>notify</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>e</text>
          <text fg={theme.textMuted}>export</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>/</text>
          <text fg={theme.textMuted}>search</text>
        </box>
        <box flexDirection="column" alignItems="center">
          <text fg={theme.text}>q</text>
          <text fg={theme.textMuted}>quit</text>
        </box>
      </box>
    </box>
  )
}
