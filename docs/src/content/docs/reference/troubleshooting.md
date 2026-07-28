---
title: Troubleshooting
description: Common questions — Claude not found, first-launch Gatekeeper, sessions and merges, scheduled tasks, hooks — and how to remove spwn and its data.
---

Short answers to the things people hit first.

## "Claude CLI not found"

spwn runs **your** authenticated `claude` command and auto-detects it on your `PATH`. If
it can't find it:

- Confirm `claude` works in a terminal (`which claude`).
- In **Settings → Claude CLI path**, set the full path to your `claude` binary.

spwn doesn't handle Claude authentication itself — if `claude` works in your terminal, it
works in spwn. See [How it works & your data](/spwn/reference/architecture/).

## macOS blocks the app on first launch

Releases are ad-hoc signed but **not notarized**, so a download from GitHub is quarantined
once. Open **System Settings → Privacy & Security → Open Anyway**, or clear the flag:

```sh
xattr -dr com.apple.quarantine "/Applications/spwn.app"
```

Updates installed by the in-app updater are never quarantined. See
[Installation](/spwn/getting-started/installation/).

## My session has no branch / no `⎇` chip

Per-session branches and worktrees only exist when the project folder is a **git
repository**. In a non-git folder, sessions run in the folder directly. Run `git init`
in the project folder and start a new session to get isolation. See
[Branches & Merging](/spwn/guides/branches-and-merging/).

## A merge conflicted, or I can't merge

**⤵ Merge** uses ordinary git. If it can't fast-forward or hits a conflict, resolve it
the way you would any merge — in a shell, `git merge spwn/<id>` from the base branch and
fix the conflicts. The session's branch is real git history, so nothing is stuck. See
[Branches & Merging](/spwn/guides/branches-and-merging/).

## Deleting a session warns I'll lose work

That's by design: if the session's branch has commits not in its base — or uncommitted
changes — the confirm dialog names exactly what you'd lose. **Merge first** to keep it.

## A scheduled task didn't run

Scheduled tasks run **only while spwn is running**. spwn stays in the **menu bar** when
you close its window, but if you quit it fully, tasks won't fire. A task scheduled for a
time the app was closed runs once the next time spwn is open. See
[Scheduled Tasks](/spwn/guides/scheduled-tasks/).

## My hook didn't fire

- Hooks only run for sessions with their **own worktree** (a git repo).
- A hook must be **committed** at `.spwn/hooks/<event>.sh` — it travels into a session via
  the git checkout, so an uncommitted hook never runs.
- Check the **Hooks** tab in the session's Inspector for the last run's output and exit
  status. See [Project Hooks](/spwn/reference/hooks/).

## Where does spwn store my data?

Everything is **local**, under `~/Library/Application Support/com.markbarta.spwn/`:
projects and Merge trays, settings, per-session checkpoints, and (for the *App data*
worktree layout) worktrees. spwn only **reads** your Claude history under `~/.claude/`;
it never writes there. See [How it works & your data](/spwn/reference/architecture/).

## Uninstall / remove all data

1. Quit spwn from its menu-bar icon.
2. Delete `/Applications/spwn.app`.
3. Remove its data folder: `~/Library/Application Support/com.markbarta.spwn/`.
4. Optionally delete any leftover session branches or worktrees in repos you used —
   `spwn/…` branches (and legacy `cm/…` ones from older versions): `git branch -D
   spwn/<id>`, then `git worktree prune`.

Your Claude login and history under `~/.claude/` are untouched — spwn never owned them.
