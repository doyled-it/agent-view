# Follow-Up Mark Feature

## Overview

Add a persistent, manually-toggled "follow-up" flag to sessions. Useful for sessions that are idle or stopped but need a revisit — distinct from the auto-managed `notify` indicator.

## Data Model

Add `followUp: boolean` to the `Session` interface in `src/core/types.ts`.

Add `follow_up INTEGER NOT NULL DEFAULT 0` column to the `sessions` table in SQLite. A schema migration adds the column to existing databases (same pattern as existing migrations for `notify`, `status_changed_at`, etc.).

No new storage methods are needed — `updateSessionField` already handles arbitrary field updates, and the sync layer reads all columns on refresh.

## Session Row Layout

```
🔔 ⚑ ● Session title                    2h ago
```

Left-to-right prefix area:
- **`🔔`** — shown when `notify === true` (replaces old `*` indicator, moves from after title to before status icon)
- **`⚑`** — shown when `followUp === true`
- Both occupy a fixed-width slot (blank when not set) so the status icon stays at a consistent column

The old notification `*` after the title is removed.

## Keyboard Shortcut

- **`i`** — toggles `followUp` on the selected session; no-op on group rows
- Same implementation pattern as `!` (toggle notify)

## Footer Hints

Add `i  mark` to the footer key hint bar alongside the existing hints.

## Files to Change

1. `src/core/types.ts` — add `followUp` field to `Session`
2. `src/core/storage.ts` — add `follow_up` column + migration
3. `src/tui/routes/home.tsx` — toggle handler (`i` key), session row rendering (bell + flag prefix, remove trailing `*`)
