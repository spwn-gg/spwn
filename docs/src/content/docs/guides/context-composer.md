---
title: Composing Context
description: Build a per-project context from notes, files, and saved answers, then inject it into a new session.
---

Each project has a **context**: a collection of **notes**, **files**, and **answers
saved from other sessions**. **Inject** turns it into a first message and opens a new
Claude session seeded with it — so you stop re-explaining the same background every
time.

![The Context view for a project, with Note and File buttons for adding blocks and an "Inject → new Claude session" button](../../../assets/screenshots/context-composer.png)

## Open the Context view

On a project in the sidebar, open its **Context** view (the **▦ Context** row). The
context is **per-project** — each project has its own.

## Building blocks

A context is assembled from blocks you can add, edit, reorder, and remove:

- **Notes** — free text you write directly (instructions, requirements, a snippet).
- **Files** — files from the project pulled into the context.
- **Saved answers** — individual answers picked from the conversation view of *other*
  sessions. This is how you carry a useful exchange from one session into a fresh one.

Reorder blocks to control how the context reads, and remove anything that's gone
stale.

## Inject

When the context is ready, click **Inject**. spwn opens a **new Claude session** in
the project whose first message is your composed context — so it starts with exactly
the background you want, instead of starting cold.

## A typical loop

1. Work in a Claude session; notice an answer worth keeping.
2. Save that answer into the project's context.
3. Add a note or a file or two for framing.
4. **Inject** to spin up a fresh, well-seeded session.

The same context also powers [Scheduled Tasks](/spwn/guides/scheduled-tasks/): a
scheduled run can reuse it, so an automated report starts with the same background
you'd give it by hand.

## Next

- [Scheduled Tasks](/spwn/guides/scheduled-tasks/) — reuse this context on a schedule.
- [Claude Sessions](/spwn/guides/claude-sessions/)
- [Projects](/spwn/guides/projects/)
