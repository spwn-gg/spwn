---
title: Settings
description: Point spwn at your agent CLIs, choose where session worktrees live, and manage updates.
---

The gear in the sidebar header opens **Settings** as its own tab, alongside your sessions
and shells. A rail down the left side splits it into five sections — **Agents**,
**Sessions**, **Hooks**, **GitHub** and **About** — and **Save** in the header commits
everything except the GitHub token, which saves on its own.

## Agents

Each agent is a `.toml` file describing how to drive one CLI. spwn ships definitions for
`claude`, `gemini` and `codex`, and reads any others you drop in `~/.spwn/agents` — see
[Agents](/spwn/guides/agents/).

Every agent gets a card showing whether its binary was found, which capabilities the
definition declares (transcript, status, rewind, scheduled), and a path field. spwn
**auto-detects** each binary on your `PATH`; fill the path in only when yours lives
somewhere unusual, and leave it blank to go back to auto-detection.

- **Open ~/.spwn/agents** — reveals the definitions folder so you can edit or add one.
- **Reload definitions** — re-reads that folder without a rebuild, reporting any file
  that failed to parse.
- **Default agent for new sessions** — which agent a session starts with when you don't
  pick one. Defaults to the first installed.

spwn doesn't handle agent authentication itself and nothing is re-uploaded or proxied —
but it can open a terminal for you to sign in from, which is what its setup screen does
on a machine where the CLI has never run. See
[How it works & your data](/spwn/reference/architecture/) for what spwn touches.

## Session worktree location

Each Claude session in a git repo runs in its own [worktree](/spwn/guides/branches-and-merging/).
**Session worktree location** controls where those worktrees are created. It applies to
**new sessions only** — existing worktrees stay where they were created.

| Option | Where worktrees go | Notes |
|--------|--------------------|-------|
| **Sibling** (default) | A dot-prefixed folder *beside* the repo | Outside the working tree, so builds, file watchers, and IDE indexers never recurse into it. |
| **Inside repo** | `<repo>/.spwn/worktrees/…` | Excluded via the repo's `.git/info/exclude` (your tracked `.gitignore` is untouched). Some tools with explicit include globs may still scan it. |
| **App data** | Under spwn's app-data directory | Away from your repos entirely. |

## Global hooks

spwn's shared [hooks](/spwn/reference/hooks/) live in `~/.spwn/hooks/` and run for every
session in every project. This is also where spwn's own built-in behavior ships as default
scripts — creating the session worktree, committing each turn, and taking checkpoints.

- **Run shared global hooks** — the toggle that enables or disables the whole
  `~/.spwn/hooks/` folder. Per-repo `.spwn/hooks` are unaffected.
- **Open hooks folder** — reveals `~/.spwn/hooks/` in Finder (creating it if needed), so
  you can read or edit the default scripts and add your own.

:::caution[Disabling turns off worktree management]
Because creating and removing session worktrees is one of those default global hooks,
turning global hooks **off** means spwn no longer manages worktrees: new sessions run in
the project folder with no isolated worktree or branch, existing session worktrees aren't
auto-removed on delete, and the per-turn commit + checkpoint stop. Leave it on unless you
specifically want spwn to stop managing sessions' git state.
:::

## GitHub

A personal access token lets spwn clone, fetch, pull and push private GitHub repos over
HTTPS, and lets git in your shells and agents do the same. Use a classic token with the
`repo` scope, or a fine-grained one with read and write access to Contents. It's saved on
its own in spwn's data folder, readable only by you, and **Save token** writes it
immediately — it doesn't wait for the Save button in the header.

## Updates

spwn has a built-in updater. When a new release is available, an update banner appears —
apply it and spwn relaunches on the new version. Updates it installs don't trigger the
macOS security prompt you see on a first download from GitHub (see
[Installation](/spwn/getting-started/installation/)).

## Staying available for scheduled tasks

[Scheduled tasks](/spwn/guides/scheduled-tasks/) only run while spwn is running, so spwn
stays available in the **menu bar** when you close its window. Open it again from the
menu-bar icon, or quit fully from that icon's menu.
