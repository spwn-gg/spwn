---
title: Per-Session Services
description: Give each session its own running service and live test harness with docker-compose — isolated per branch, so many sessions run at once.
---

Each Claude session already works on [its own branch in its own files](/spwn/guides/parallel-sessions/).
Per-session services take that one step further: with a small `spwn.yaml` in your
repo, spwn can bring up **each session's own running service and live test harness**
in Docker — isolated per session, so several can run at once, each with its own live
URL.

This is entirely **opt-in**. Without a `spwn.yaml`, nothing changes. With one (and
Docker running), each session gets its own stack.

## Add a `spwn.yaml`

Commit a `spwn.yaml` at your repo root. Because a session works in a checkout of your
repo, this file — and the `docker-compose.yml` it points at — travel into every
session automatically.

It's a **thin wrapper** over your existing `docker-compose.yml`: your compose file
stays the source of truth for how services build and run; `spwn.yaml` only adds the
things spwn needs to know.

```yaml
# spwn.yaml — per-session service integration (opt-in).
version: 1
compose: docker-compose.yml     # the compose file spwn drives

services:
  db:
    scope: shared               # ONE instance shared across all sessions
  app:
    scope: session              # forked per session (the default)
    role: service               # long-running; spwn surfaces its live URL
    wait_for_healthy: true      # wait for the compose healthcheck before "ready"
    ports: [3000]               # CONTAINER ports to publish + show as live URLs
  test:
    scope: session
    role: harness               # a live test watcher; view its output via "Logs"

lifecycle:
  up: on-demand                 # start on first use (or "on-session-start" / "manual")
  idle_stop: 15m                # stop an idle stack; resumes instantly on demand

resources:                      # optional per-session caps
  memory: 1g
  cpus: 1.5
```

A complete, runnable example lives in
[`examples/compose-integration/`](https://github.com/spwn-gg/spwn/tree/main/examples/compose-integration)
in the repo.

## Many sessions at once, no collisions

spwn runs each session's stack under its own compose project name
(`spwn-<session>`), so containers, networks, and volumes never clash between
sessions.

Ports are handled the same way. Publish **container-only** ports in your compose
file (`ports: ["3000"]`, with no host side) and Docker picks a free host port for
each session. spwn finds that port and shows a **clickable live URL** in the session's
Services panel — so two sessions running the same app each get their own working URL,
and you never have to manage port numbers.

## The Services panel

When a session has a `spwn.yaml`, a **▸ Services** button appears in the conversation
toolbar. It shows each service with:

- a status dot (running / stopped) and a **session** or **shared** badge,
- a clickable **live URL** for `service`-role services,
- a **Logs** button to peek at recent output (great for the test harness),
- **Up** / **Down** controls.

The live URL also gives you a one-click way to open the running app for any session.

## Shared vs. per-session services

Running a full stack for every session would be wasteful — you don't want ten
Postgres containers for ten sessions. Tag heavy backing services (databases, caches,
queues) with `scope: shared` and spwn runs **one** instance for them, shared across
all sessions and **ref-counted** so deleting one session never tears down a database
the others are still using.

Everything else defaults to `scope: session` and is forked per session. Shared
services share their data across sessions, so if a branch needs its **own** isolated
copy of something, give that service `scope: session` instead.

## Lazy start and idle-stop

By default (`lifecycle.up: on-demand`) a session **doesn't** start its stack when it
opens — it starts the first time you click the URL or press **Up**. Set
`idle_stop: 15m` and spwn stops a stack that's been idle that long, freeing CPU and
memory while keeping its data and volumes warm; the next request resumes it. The net
effect: ten open sessions cost roughly one shared database plus a handful of mostly
stopped app containers, not ten full stacks.

To start every session's stack up front instead, set `lifecycle.up: on-session-start`.

## Fast forks: image reuse

Building a container image for every session would be slow. Instead, spwn tags each
built image by a hash of its **dependency** inputs (its `Dockerfile` and lockfiles —
never your source, which is bind-mounted for hot reload). A freshly
[forked](/spwn/guides/fork-and-rewind/) session has the same dependencies as its
parent, so it **reuses the parent's image with no rebuild**. It only builds a fresh
image once you change dependencies.

## Editing code with a running service

Your session's files are bind-mounted into the container, so the service hot-reloads
as the agent (or you) edits — the classic dev-server experience, one per session. The
agent itself still runs on your machine in the session's checkout, exactly as before;
Docker only runs the service and test harness.

### A note on `node_modules`

By default the container uses your checkout's copy of `node_modules` (the one spwn
already clones into each session). If your host is macOS but your container is Linux
and your dependencies include **native** modules, those won't load across that
boundary — set `volumes.node_modules: per-session` in `spwn.yaml` to give the
container its own installed copy instead.

## Cleanup

Deleting a session tears its stack down (`docker compose down -v`) and drops its
per-session volumes before its branch checkout is removed. Shared services keep
running until the last session using them is gone. If the app was closed while a
session was deleted, spwn reaps the leftover stack on the next launch.

## Requirements

- **Docker** must be installed and running (Docker Desktop on macOS). If it isn't,
  sessions still open normally — you'll just see a one-time notice that services were
  skipped.
- The feature activates only when a session's repo has a `spwn.yaml`.

## Next

- [Parallel Sessions](/spwn/guides/parallel-sessions/)
- [Scheduled Tasks](/spwn/guides/scheduled-tasks/)
