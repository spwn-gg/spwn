---
title: Sessions & Shells
description: The two things you open inside a project — an isolated Claude session, or a plain shell — and how they keep running across restarts.
---

Inside a project you open two kinds of thing:

- a **Session** — an isolated **Claude** workspace: its own conversation *and* its own
  copy of your code, on its own git branch, or
- a **Shell** — a plain terminal in the project folder.

Both keep running in the background and are right where you left them when you reopen
the app.

## Open one

From a project in the sidebar:

- **＋ New session** starts a Claude session (in a git repo, on its own `spwn/…` branch in
  its own worktree).
- **＋ New shell** starts an ordinary interactive shell in the project folder.

Each opens as a tab in the main area. Click any entry in the sidebar to jump back to it.

## Sessions vs shells

|  | Session | Shell |
|---|---|---|
| **What it is** | Claude, in a clean conversation view | A plain interactive terminal |
| **Isolation** | Own git branch + worktree (in a git repo) | Runs in the project folder directly |
| **Use it for** | Building features, exploring, agent work | git, builds, tests, one-off commands |
| **Fork / Timeline / Merge** | Yes | No |

A **shell** is just a terminal — no conversation, no branch. Reach for it when you want
to run something yourself. A **session** is the unit you explore, fork, and merge; see
[Claude Sessions](/spwn/guides/claude-sessions/).

## They survive restarts

Every session and shell keeps running even when you close spwn. Quit the app and reopen
it, and each is exactly where you left it — scrollback, history, and any running program
(a build, a dev server, a long Claude task) all intact. You never lose work just because
you closed the window.

## Next

- [Claude Sessions](/spwn/guides/claude-sessions/) — the conversation view.
- [Parallel Sessions](/spwn/guides/parallel-sessions/) — how sessions get their own branch.
- [Fork & Timeline](/spwn/guides/fork-and-rewind/) — branch and roll back sessions.
