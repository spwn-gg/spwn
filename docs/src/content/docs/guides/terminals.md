---
title: Sessions
description: Shell and Claude sessions, and how they keep running across restarts.
---

Inside a project you open **sessions**. A session is either a **shell** (the default)
or a **Claude** session. Both live in the project's folder and both keep running in
the background.

## Open a session

From a project in the sidebar, start a new session and choose its type:

- a **shell** session, or
- a **Claude** session.

It opens as a tab in the main area. Click any session in the sidebar to jump back to
it.

## Sessions survive restarts

Every session keeps running even when you close spwn. Quit the app and reopen it, and
the session is exactly where you left it — its scrollback, history, and any running
program (a build, a dev server, a long Claude task) all intact. You never lose work
just because you closed the window.

## Shell sessions

A shell session is an ordinary interactive shell running in your project's folder.
Use it for git, builds, tests, and anything else you'd do in a terminal.

## Claude sessions

A Claude session lets you work with Claude in the project, with a clean conversation
view and the full Claude experience beside it. See
[Claude Sessions](/spwn/guides/claude-sessions/).

## Next

- [Claude Sessions](/spwn/guides/claude-sessions/) — the conversation view.
- [Fork & Rewind](/spwn/guides/fork-and-rewind/) — branch and roll back sessions.
