# spwn project-hooks example

A minimal illustration of spwn's [project hooks](https://spwn-gg.github.io/spwn/guides/hooks/).
Copy the `.spwn/hooks/` tree into your own repo root and adapt.

## What it demonstrates

- **One file per event** — `.spwn/hooks/<event>.sh` is the single entry point for that
  event. It's a plain shell script, so it can orchestrate anything.
- **Orchestration** — `session-created.sh` calls `setup/install.sh`, showing how the one
  hook file drives other files/code (swap in `npm install`, `docker compose up`, etc.).
- **Lifecycle events** — `session-created` (worktree ready), `session-ready` (Claude
  session id bound), `session-deleted` (just before the worktree is removed).
- **Injected environment** — each script echoes the `SPWN_*` variables it receives.
- **Opt-in** — no `.spwn/hooks/<event>.sh`, no hook for that event.

## Files

```
.spwn/hooks/
  session-created.sh    # entry point; echoes env, calls setup/install.sh, writes a marker
  session-ready.sh      # runs once the Claude session id is known
  session-deleted.sh    # runs before the worktree is removed
  setup/
    install.sh          # a helper the created hook orchestrates
```

## Try it

1. Add these files at the root of a **git** project, commit them, and open it as a spwn
   project.
2. Open a Claude session → open **▸ Hooks** → you should see `session-created` ran (green
   dot); click **Output** to see the echoed environment, and check for `SPWN_SETUP.txt`
   in the session's files.
3. Once the session is live, `session-ready` fires. Delete the session → `session-deleted`
   runs before the worktree is cleaned up.

All scripts here are read-only/echo-style and safe to run.

## Cookbook

For practical, copy-paste recipes (refresh the base branch, per-session preview
environment, copy secrets, seed a DB, teardown on delete), see
[`cookbook/`](cookbook/README.md).
