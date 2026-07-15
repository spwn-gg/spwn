# spwn compose-integration example

A minimal project showing spwn's per-session docker-compose integration. Copy
`spwn.yaml` + `docker-compose.yml` (and adapt) into your own repo root.

## What it demonstrates

- **Per-session isolation** — each spwn Claude session works in its own git
  worktree/branch, and gets its own compose stack named `spwn-<short>`. Open two
  sessions and both run concurrently on different ephemeral host ports.
- **Live service + URL** — the `app` service publishes a container-only port; spwn
  assigns a free host port and shows a clickable `http://localhost:<port>` in the
  session's **▸ Services** panel.
- **Live test harness** — the `test` service runs `node --test --watch`; view its
  output via the **Logs** button.
- **Shared backing service** — `db` is `scope: shared`, so one Postgres instance is
  ref-counted across all sessions instead of one-per-session.
- **Image reuse** — forking a session reuses the parent's built image (no rebuild)
  until dependencies change.
- **Lazy + idle-stop** — `lifecycle.up: on-demand` starts nothing until you click
  **Up** / the URL; `idle_stop: 15m` stops an idle stack (volumes stay warm).

## Try it

1. Add this project as a spwn project (point it at a git repo containing these
   files). Ensure Docker Desktop is running.
2. Open a Claude session → open **▸ Services** → click **Up**. When `app` is
   `running`, its live URL appears; open it to see the greeting from `server.js`.
3. Edit `greeting` in `server.js` → reload the URL → the change is live (hot reload
   via the bind mount).
4. Open a second session → it gets a distinct URL; both respond at once.
5. Delete the first session → its stack is torn down (`down -v`); the shared `db`
   keeps running while the second session still refs it.

## Files

| File | Role |
|------|------|
| `spwn.yaml` | spwn-specific config (scopes, roles, ports, lifecycle) |
| `docker-compose.yml` | the real stack spwn drives (never mutated by spwn) |
| `Dockerfile` | dev image; only deps determine the reusable content tag |
| `server.js` | tiny HTTP service with `/health` + an editable greeting |
| `server.test.js` | the live test harness |
