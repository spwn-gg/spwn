---
name: spwn-dev
description: Manage spwn's local CLI dev loop — start/stop/restart the Rust web server (`spwn serve`) and the Vite frontend, tail their logs, typecheck, or do a release build. Use when asked to run/start/stop/restart the dev server, bring up spwn locally for development, reload the backend after Rust changes, check the app is up, view dev logs, or hot-reload the frontend. NOT for the one-off release binary (that's a plain `npm run build:app`) — this manages the running dev processes.
---

# Manage the spwn dev loop

spwn is a **CLI + web server**: a Rust binary (`spwn serve`, axum HTTP + WebSocket)
that serves the SvelteKit SPA. In development this is **two processes** bridged by
Vite's dev proxy:

- **Backend** — `spwn serve --no-open` on `:4317` (HTTP `/api/invoke/*`, WebSocket `/ws`).
- **Frontend** — Vite on `:1420` with HMR; it proxies `/api` and `/ws` to the backend.

**You open `http://localhost:1420`** (the Vite URL) — that gives frontend hot-reload
against the real backend. The backend's own port only serves the pre-built SPA.

The driver **`.claude/skills/spwn-dev/dev.sh`** manages both as background processes
(pids + logs under a temp dir, keyed per worktree), so a single command starts them
and returns instead of blocking. Run it from anywhere; it resolves the repo root itself.

## Commands

```sh
.claude/skills/spwn-dev/dev.sh start      # build + start backend, start frontend, wait until both are up
.claude/skills/spwn-dev/dev.sh backend    # rebuild + restart ONLY the backend  ← after editing Rust
.claude/skills/spwn-dev/dev.sh frontend   # restart ONLY the frontend           (rarely needed — HMR handles .svelte/.ts)
.claude/skills/spwn-dev/dev.sh status     # what's running + the URL to open
.claude/skills/spwn-dev/dev.sh logs [backend|frontend|both]   # tail the logs
.claude/skills/spwn-dev/dev.sh restart    # restart both
.claude/skills/spwn-dev/dev.sh stop       # stop both
.claude/skills/spwn-dev/dev.sh check      # svelte-check + cargo check
.claude/skills/spwn-dev/dev.sh build      # release build (npm run build:app)
```

## The loop

1. **`dev.sh start`** — first run compiles the Rust crate (cold: a few minutes; warm:
   seconds), launches the server, then Vite. It waits for `GET /api/version` and prints
   the URL. Open **http://localhost:1420**.
2. **Edit frontend** (`src/**`) — Vite HMR applies it live; no restart.
3. **Edit backend** (`backend/src/**`) — run **`dev.sh backend`** to rebuild and
   relaunch just the server. Terminals persist across this (rmux shell sessions survive;
   Claude sidecars are killed and reattach). The browser's WebSocket auto-reconnects.
4. **`dev.sh status`** / **`dev.sh logs`** when something looks wrong.
5. **`dev.sh stop`** when done.

## Notes & gotchas

- **cargo on PATH.** The driver exports `~/.cargo/bin` (cargo is a rustup shim a fresh
  shell doesn't add); if running raw, `export PATH="$HOME/.cargo/bin:$PATH"` first.
- **Ports.** Override with `SPWN_BACKEND_PORT` / `SPWN_FRONTEND_PORT` env vars. The
  driver passes `SPWN_BACKEND` to Vite so the proxy follows a non-default backend port.
- **`claude` required at runtime.** The server shells out to your own authenticated host
  `claude` CLI (path configurable in the app's Settings); the dev loop doesn't provide it.
- **Restarting the backend** drops the current process; rmux shell terminals persist
  (they run under the rmux daemon), Claude sidecars are killed on shutdown and respawn on
  reattach. HMR + the client's reconnecting WebSocket mean you rarely reload the tab.
- **Backend won't come up?** `dev.sh logs backend`. A common cause is a compile error
  (the build step fails before launch) or the port already in use.
- **This is the dev loop, not the deliverable.** For a distributable binary use
  `dev.sh build` (= `npm run build:app` → `backend/target/release/spwn`, SPA embedded).
