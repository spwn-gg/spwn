---
title: Quick Start
description: Create a project, open a Claude session, seed it with context, and schedule a task.
---

This walkthrough gets you from a fresh launch to a Claude session seeded with your
own context — and a scheduled task running on its own.

## 1. Create a project

A **project** is a name plus a folder that groups everything you do there.

1. Open spwn.
2. Click **New Project**, give it a name, and pick the folder you want Claude to work
   in.

![The project tree](../../../assets/screenshots/project-tree.png)

The tree on the left lists your projects and the sessions under each, and updates
automatically as sessions start — including ones created by forking.

## 2. Open a session

Inside a project, open a session:

- A **shell** session (the default) for git, builds, tests, and anything else you'd
  do in a terminal, or
- A **Claude** session to work with Claude in that project.

Either way, the session keeps running in the background and is **still there when you
reopen the app**.

![A Claude session: the conversation with the full Claude experience beside it](../../../assets/screenshots/claude-session.png)

A Claude session gives you a clean, scrollable conversation on one side and the full
Claude experience — every slash-command and tool prompt — on the other.

## 3. Build a context and seed a session

Rather than starting a Claude session cold, seed it with the context that matters:

1. Open the project's **Context** view.
2. Add notes, files, and answers worth keeping from other sessions.
3. Click **Inject**.

spwn opens a **new Claude session** that starts with exactly the context you
composed, so you skip re-explaining the project.

![The Context view: add Note and File blocks, then Inject to open a new Claude session seeded with them](../../../assets/screenshots/context-composer.png)

## 4. Fork or rewind as you go

- **Fork** a session to branch off and try a different direction — the original keeps
  running untouched, so you never lose it.
- **Rewind** a session to roll it back to an earlier point and continue from there.

See [Fork & Rewind](/spwn/guides/fork-and-rewind/) for details.

## 5. Schedule a task

Have Claude check in on the project while you're away:

1. Open the project's **Scheduled Tasks** view.
2. Add a task — a prompt, and a daily or weekly time to run it.
3. Leave spwn running (it tucks into the menu bar).

At the scheduled time, Claude runs the task **read-only** using the project's
context, and the result is waiting as a new session under the project.

See [Scheduled Tasks](/spwn/guides/scheduled-tasks/) for details.

## Where to go next

- [Projects](/spwn/guides/projects/)
- [Sessions](/spwn/guides/terminals/)
- [Claude Sessions](/spwn/guides/claude-sessions/)
- [Composing Context](/spwn/guides/context-composer/)
- [Scheduled Tasks](/spwn/guides/scheduled-tasks/)
