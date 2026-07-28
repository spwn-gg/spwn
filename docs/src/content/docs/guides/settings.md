---
title: Settings
description: Point spwn at your Claude CLI, choose where session worktrees live, and manage updates.
---

Open **Settings** to configure how spwn finds Claude, where sessions keep their files,
and how it updates.

## Claude CLI path

spwn uses your own authenticated `claude` command. It **auto-detects** it in the usual
locations and shows the path it found. If your `claude` lives somewhere unusual, set the
**Claude CLI path** here to point spwn at it.

Because spwn uses your existing Claude login, there's nothing else to sign in to — and
nothing is re-uploaded or proxied. See
[How it works & your data](/spwn/reference/architecture/) for what spwn touches.

## Session worktree location

Each Claude session in a git repo runs in its own [worktree](/spwn/guides/parallel-sessions/).
**Session worktree location** controls where those worktrees are created. It applies to
**new sessions only** — existing worktrees stay where they were created.

| Option | Where worktrees go | Notes |
|--------|--------------------|-------|
| **Sibling** (default) | A dot-prefixed folder *beside* the repo | Outside the working tree, so builds, file watchers, and IDE indexers never recurse into it. |
| **Inside repo** | `<repo>/.spwn/worktrees/…` | Excluded via the repo's `.git/info/exclude` (your tracked `.gitignore` is untouched). Some tools with explicit include globs may still scan it. |
| **App data** | Under spwn's app-data directory | Away from your repos entirely. |

## Updates

spwn has a built-in updater. When a new release is available, an update banner appears —
apply it and spwn relaunches on the new version. Updates it installs don't trigger the
macOS security prompt you see on a first download from GitHub (see
[Installation](/spwn/getting-started/installation/)).

## Staying available for scheduled tasks

[Scheduled tasks](/spwn/guides/scheduled-tasks/) only run while spwn is running, so spwn
stays available in the **menu bar** when you close its window. Open it again from the
menu-bar icon, or quit fully from that icon's menu.
