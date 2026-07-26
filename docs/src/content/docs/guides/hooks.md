---
title: Project Hooks
description: Run your own shell scripts on session lifecycle events — discovered inside your repo, per branch, with the session's details in the environment.
---

Each Claude session works on [its own branch in its own worktree](/spwn/guides/parallel-sessions/).
**Project hooks** let you run your own shell scripts when a session's lifecycle changes —
to set up (start a dev server, seed a database, install extras) and tear down per session.

spwn just discovers executable scripts in your repo and runs them with the session's
details in the environment. It has no opinion about what they do — Docker, plain shell,
anything. This is entirely **opt-in**: no hooks directory, nothing changes.

## Add a hook

Commit **one script per event** — `.spwn/hooks/<event>.sh` — at your repo root:

```
.spwn/
  hooks/
    session-created.sh
    session-ready.sh
    session-deleted.sh
```

Because a session works in a checkout of your repo, a committed hook travels into every
session automatically. Each file is the single entry point for its event — it's a plain
script, so **orchestrate from there**: call other scripts, run a build, bring up
containers, whatever you need.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.sh
set -euo pipefail
echo "Setting up session for $SPWN_BRANCH in $SPWN_WORKTREE"
npm install
./.spwn/hooks/setup/seed-db.sh   # orchestrate other files/code
```

If the file has its execute bit set it runs directly (honoring its shebang); otherwise
spwn runs it via `sh`. A missing `<event>.sh` simply means no hook for that event.

A complete, runnable example lives in
[`examples/hooks/`](https://github.com/spwn-gg/spwn/tree/main/examples/hooks) in the repo.

## Events

| Event | When it fires |
|-------|---------------|
| `session-created` | Right after the session's git worktree is created. |
| `session-ready` | The first time the Claude session id is known (after the sidecar starts). |
| `session-deleted` | Just **before** the worktree is removed on delete. |

Hooks run only for sessions that have their own worktree (a git repo). A session that
falls back to the plain project directory doesn't fire hooks.

### Hooks are synchronous

Every hook runs **synchronously** — the session waits for the script to finish before
proceeding (and `session-deleted` completes before the worktree is removed). This keeps
the model simple and predictable. If you want something to run in the background, do it
in your script — start it and detach:

```sh
# .spwn/hooks/session-created.sh
my-dev-server & disown     # returns immediately; server keeps running
```

## Environment

Each script is run with the **worktree as its working directory** and these variables:

| Variable | Value |
|----------|-------|
| `SPWN_EVENT` | The event name (`session-created`, …). |
| `SPWN_TERMINAL_ID` | The session's stable id. |
| `SPWN_PROJECT_DIR` | The project's root directory (the main checkout). |
| `SPWN_WORKTREE` | The session's worktree path (also the working directory). |
| `SPWN_BRANCH` | The session's branch (`cm/<short>`). |
| `SPWN_BASE_BRANCH` | The branch it will merge back into. |
| `SPWN_SESSION_ID` | The Claude session id — set for `session-ready`/`session-deleted`; absent on `session-created` (not known yet). |

## The Hooks panel

When a session has a worktree, a **▸ Hooks** button appears in the conversation toolbar.
It lists each event with:

- a status dot (green = the last run passed, red = the hook exited non-zero),
- the hook file discovered for that event (if any),
- an **Output** toggle showing the last run's combined stdout/stderr,
- a **Run** button to fire that event's hook manually.

If a hook exits non-zero, spwn shows a one-line advisory — the session still opens.

## Requirements

- A hook lives at `.spwn/hooks/<event>.sh`. It runs directly if executable
  (`chmod +x`, honoring its shebang), otherwise via `sh`.
- The feature activates only when a session has its own worktree.

## Next

- [Parallel Sessions](/spwn/guides/parallel-sessions/)
- [Scheduled Tasks](/spwn/guides/scheduled-tasks/)
