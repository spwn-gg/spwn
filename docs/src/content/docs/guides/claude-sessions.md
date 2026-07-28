---
title: Claude Sessions
description: A clean conversation view, with the full Claude experience beside it.
---

A **Claude session** is the centerpiece of spwn. You get a clean, readable conversation
on one side and the complete Claude Code experience on the other — so you can read and
work comfortably without giving anything up.

## The full Claude experience

The working side is Claude Code itself, not a stripped-down copy. That means:

- You **type to Claude** directly.
- **Every slash-command** works.
- **Tool prompts** — permission requests, pickers — appear and are answered right there,
  exactly as they would in the terminal.

Nothing is reimplemented or held back; you get everything Claude Code can do.

In a git project, each session also works on **its own branch, in its own worktree**,
isolated from the others — so you can run several at once and merge the results back. See
[Parallel Sessions](/spwn/guides/parallel-sessions/).

## The conversation view

Beside it, spwn shows a clean, scrollable view of the conversation. It stays in sync with
the session and lets you act on individual turns:

- **＋ ctx** saves a turn into the project's [Merge tray](/spwn/guides/context-composer/),
  so a useful exchange from one session can seed the next.
- **⑂ Fork** branches a new session from that point.
- **↺ Return here** rolls the session back to that point (see the
  [Timeline](/spwn/guides/fork-and-rewind/)).

## Inspect, merge, and set up

Each session has an **Inspector** with tabs for its changed files, its **Timeline**, its
**Hooks**, and a **⤵ Merge…** action to bring its branch back. See
[Merging Work](/spwn/guides/merging/) and [Project Hooks](/spwn/guides/hooks/).

## Seeding a session with context

Instead of starting cold, you can **Inject** a project's Merge tray as the first message
of a new session. See [Merge Tray](/spwn/guides/context-composer/).

## Next

- [Parallel Sessions](/spwn/guides/parallel-sessions/)
- [Fork & Timeline](/spwn/guides/fork-and-rewind/)
- [Merge Tray](/spwn/guides/context-composer/)
