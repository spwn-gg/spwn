---
title: Scheduled Tasks
description: Have Claude run a read-only task against a project on a daily or weekly schedule, using the project's context.
---

A **scheduled task** has Claude run a prompt against a project on a schedule, so a
report or review is waiting for you without you starting it. Runs are **read-only** —
they can read your project and its context but never change your files — and each run
reuses the project's [composed context](/spwn/guides/context-composer/).

## Create a task

On a project in the sidebar, open its **◷ Scheduled Tasks** view, then add a task:

1. **Name** it (e.g. "Morning status report").
2. Write the **prompt** — what you want Claude to do each run.
3. Pick a **time**, and optionally the **days** of the week it should run on. Leave
   the days empty to run every day.
4. Choose whether to **use the project context** — on by default, so the run starts
   with the same background you'd give it by hand.

Tasks can be enabled or disabled at any time, and **Run now** fires one immediately if
you want to see what it produces.

## Where results go

At the scheduled time, spwn opens a new Claude session under the project and runs your
task in it. When it finishes, the session is **flagged for your attention** in the
sidebar. Open it to read the result like any other session — with the full
conversation, ready to fork or reuse.

Because runs reuse your project context, an automated review or summary starts from
the same notes and files you'd normally provide, not from scratch.

## Read-only by design

Scheduled runs are **read-only**: Claude can read files, search the project, and use
its context, but it can't edit files or make changes. That makes them safe to run
unattended — a scheduled task reports back, it doesn't act on your project.

## Keep spwn available

Scheduled tasks run **while spwn is running**. So they can fire even when you're not
looking at the app, spwn stays available in the **menu bar** when you close its
window — it doesn't fully quit. A task scheduled for a time the app was closed runs
once the next time spwn is open.

To stop everything, quit spwn from its menu-bar icon.

## Good tasks to schedule

- A **daily status report** summarizing recent changes in the project.
- A **nightly review** of open work or a specific area of the codebase.
- A **weekly digest** that pulls together what changed and what's worth attention.

## Next

- [Composing Context](/spwn/guides/context-composer/) — build the context these runs reuse.
- [Settings](/spwn/guides/settings/) — the menu-bar behavior that keeps tasks running.
