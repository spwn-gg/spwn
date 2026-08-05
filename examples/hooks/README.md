# spwn hooks example

A minimal illustration of spwn's [hooks](https://spwn-gg.github.io/spwn/reference/hooks/).
Copy the `.spwn/hooks/` tree into your own repo root and adapt.

## What it demonstrates

- **Directory-based hooks** — each event is a `<event>.d/` folder whose scripts run in
  **filename order**. Number them (`10-`, `20-`, …) to control ordering, and add or remove
  a step by dropping in / deleting a file — no need to edit the others.
- **Ordered steps** — `session-created.d/10-info.sh` runs before `20-install.sh` (swap in
  `npm install`, `docker compose up`, etc.).
- **Lifecycle events** — `session-created` (worktree ready), `session-ready` (Claude
  session id bound), `session-deleted` (just before the worktree is removed).
- **Injected environment** — each script echoes the `SPWN_*` variables it receives.
- **Opt-in** — no `.spwn/hooks/<event>.sh` or `<event>.d/`, no hook for that event.

A single script per event also works — `.spwn/hooks/<event>.sh` runs before any
`<event>.d/` scripts. Use whichever fits; a folder is handy once you have more than one
step.

## Files

```
.spwn/hooks/
  session-created.d/
    10-info.sh       # echoes the injected environment
    20-install.sh    # runs after 10-; put real setup here, writes a marker file
  session-ready.d/
    10-notify.sh     # runs once the Claude session id is known
  session-deleted.d/
    10-teardown.sh   # runs before the worktree is removed
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

## Global vs repo

These live in a **repo** (`.spwn/hooks/`), so they're committed and apply to that repo.
To run something for **every** project, put the same files in the shared global folder at
`~/.spwn/hooks/` instead — see the [Hooks reference](https://spwn-gg.github.io/spwn/reference/hooks/).

## Cookbook

For practical, copy-paste recipes (refresh the base branch, per-session preview
environment, copy secrets, seed a DB, prompt before setup with `spwn prompt`, teardown on
delete), see [`cookbook/`](cookbook/README.md).

## Running a session inside a container

The recipes above run things *alongside* a session, on your machine. A hook can also
report an **environment** for the session, and spwn will run the agent's TUI and its
shells inside it:

- [`docker-env/`](docker-env/README.md) — one container per session. The starting point.
- [`dev-env-services/`](dev-env-services/README.md) — the same, plus a database: shared
  server, per-session data, per-session port.

Both need more than a single script (an image, a create hook and a teardown hook), which
is why they're folders rather than entries in `cookbook/`.
