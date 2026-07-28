---
title: Merge Tray
description: Collect notes, files, and saved turns from any session into a per-project tray, then Inject them as the first message of a fresh session.
---

Each project has a **Merge tray**: a collection of **notes**, **files**, and **turns
saved from any session**. **Inject** turns it into a first message and opens a new Claude
session seeded with it — so you stop re-explaining the same background every time.

:::note[Why "Merge tray" and not "Context"?]
It's named *Merge tray* — not *Context* — so it isn't confused with the model's own
**context window**. It's where you gather the good parts of several sessions to merge
into one fresh, well-primed session.
:::

![The Merge tray for a project, with note and file blocks and an "Inject → new session" button](../../../assets/screenshots/context-composer.png)

## Open the Merge tray

On a project in the sidebar, open its **▦ Merge tray** row. The tray is
**per-project** — each project has its own.

## Building blocks

A tray is assembled from blocks you can add, edit, reorder, and remove:

- **Notes** — free text you write directly (instructions, requirements, a snippet).
- **Files** — files from the project pulled into the tray.
- **Saved turns** — individual answers picked from the conversation view of *other*
  sessions with **＋ ctx**. This is how you carry a useful exchange from one session into
  a fresh one.

Reorder blocks to control how the message reads, and remove anything that's gone stale.

## Inject

When the tray is ready, click **Inject → new session**. spwn opens a **new Claude
session** in the project whose first message is your composed tray — so it starts with
exactly the background you want, instead of starting cold.

## A typical loop

1. Work in a Claude session; notice an answer worth keeping.
2. **＋ ctx** to save that turn into the project's Merge tray.
3. Add a note or a file or two for framing.
4. **Inject → new session** to spin up a fresh, well-seeded session.

The same tray also powers [Scheduled Tasks](/spwn/guides/scheduled-tasks/): a scheduled
run can reuse it, so an automated report starts with the same background you'd give it by
hand.

## Next

- [Scheduled Tasks](/spwn/guides/scheduled-tasks/) — reuse this tray on a schedule.
- [Sessions & Shells](/spwn/guides/terminals/)
- [Branches & Merging](/spwn/guides/branches-and-merging/)
