---
title: Sessions & Shells
description: The two things you open inside a project — an isolated Claude session with a clean conversation view, or a plain shell — and how they keep running across restarts.
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
to run something yourself. A **session** is the unit you explore, fork, and merge.

## The full Claude experience

A session is the centerpiece of spwn: a clean, readable conversation on one side and the
complete Claude Code experience on the other — so you can read and work comfortably
without giving anything up. The working side is Claude Code itself, not a stripped-down
copy:

- You **type to Claude** directly.
- **Every slash-command** works.
- **Tool prompts** — permission requests, pickers — appear and are answered right there,
  exactly as they would in the terminal.

In a git project, each session works on **its own branch, in its own worktree**, isolated
from the others — so you can run several at once and merge the results back. See
[Branches & Merging](/spwn/guides/branches-and-merging/).

## The conversation view

Beside it, spwn shows a clean, scrollable view of the conversation. It stays in sync with
the session and lets you act on individual turns:

- **＋ ctx** saves a turn into the project's [Merge tray](/spwn/guides/context-composer/),
  so a useful exchange from one session can seed the next.
- **⑂ Fork** branches a new session from that point.
- **↺ Return here** rolls the session back to that point (see the
  [Timeline](/spwn/guides/fork-and-rewind/)).

Each session also has an **Inspector** with tabs for its changed files, its **Timeline**,
its **Hooks**, and a **⤵ Merge…** action to bring its branch back. See
[Branches & Merging](/spwn/guides/branches-and-merging/) and
[Project Hooks](/spwn/reference/hooks/).

## They survive restarts

Every session and shell keeps running even when you close spwn. Quit the app and reopen
it, and each is exactly where you left it — scrollback, history, and any running program
(a build, a dev server, a long Claude task) all intact. You never lose work just because
you closed the window.

## Next

- [Branches & Merging](/spwn/guides/branches-and-merging/) — how sessions get their own branch.
- [Fork & Timeline](/spwn/guides/fork-and-rewind/) — branch and roll back sessions.
- [Merge Tray](/spwn/guides/context-composer/) — seed a new session with saved context.
