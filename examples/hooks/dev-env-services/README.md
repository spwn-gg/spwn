# Per-session dev environment, with services

[`../docker-env/`](../docker-env) scopes a session to a single container. A real dev
environment is rarely one container — this adds a database, and the interesting part is
deciding what's shared.

| | Scope | Why |
|---|---|---|
| App container | **Per session** | It's what the agent runs in, and it holds the worktree. |
| Database **server** | **Shared** | One postgres per session is minutes of startup and gigabytes of RAM for nothing. |
| Database **contents** | **Per session** | Sessions must not clobber each other's rows. |

One server process, a separate database inside it per session. That middle ground is what
makes running ten sessions at once practical.

```
        ┌── spwn-<session-a> ──┐            per-session app containers,
        │  agent runs here     │──┐         each with its own worktree
        │  :3000 → :NNNNN      │  │         and an ephemeral host port
        └──────────────────────┘  │
        ┌── spwn-<session-b> ──┐  ├──▶ spwn-shared-db
        │  agent runs here     │──┘     ├── session_a   ← own database
        │  :3000 → :MMMMM      │        └── session_b   ← own database
        └──────────────────────┘        (one server, shared network)
```

## Install

```sh
mkdir -p .spwn
cp -R examples/hooks/dev-env-services/.spwn/hooks .spwn/
cp -R examples/hooks/dev-env-services/.spwn/env   .spwn/
chmod +x .spwn/hooks/*/*.sh
docker build -t spwn-session-env .spwn/env    # the hook won't pull; build it first
git add .spwn && git commit -m "Per-session dev environments"
```

**Commit them** — repo hooks reach a session through its git checkout, so an uncommitted
hook never runs. Set `SPWN_ENV_IMAGE` to use a different image.

## What each session gets

- Its own app container, `spwn-<terminal-id>`, which the agent's TUI and any shell you
  open on the session run inside.
- Its own database on the shared server, wired up as `DATABASE_URL` in the container.
- Its own host port, chosen by Docker (`-p 127.0.0.1::3000`) so parallel sessions never
  collide. It's printed to the hook output — visible in the session's **Hooks** tab — and
  written to `.spwn/run/preview.url`. spwn has no live-URL panel; that went away with the
  old services integration.

## Teardown, and what is deliberately left behind

`session-deleted` drops this session's database and removes its app container. It does
**not** remove the shared server or network: a session's teardown must never take down a
server other live sessions are still using. Ref-counting shared services was one of the
things that made spwn's old built-in compose integration too complicated to keep, so this
doesn't attempt it. Clean up by hand when you're done:

```sh
docker rm -f spwn-shared-db && docker network rm spwn-shared
docker volume rm spwn-shared-db-data     # also drops the data
```

## Verified behavior

Both hooks were exercised against real Docker:

- Two concurrent sessions each got their own container, their own database and their own
  host port, sharing one postgres server.
- The app container reaches the database over the shared network via `DATABASE_URL`.
- Re-running the create hook is idempotent — same port, no duplicate container, existing
  database left alone. That's what makes the Hooks panel's **Run** button a working
  "rebuild this environment".
- Tearing down one session left the other session's container, database and connectivity
  fully intact.

## Caveats

Everything in [`../docker-env/`](../docker-env) applies — identical absolute mount paths
(the transcript slug and git's `gitdir:` pointer both depend on it), the image needing the
agent CLI on its PATH, `docker` needing to be on the rmux daemon's PATH, `-it` vs `-i`,
file ownership on Linux hosts, and bind-mount speed. Two more here:

- **The password is `dev`, hardcoded.** Fine for a local server on a private Docker
  network; don't publish its port or reuse this for anything real.
- **The database survives a reboot** (named volume + `--restart unless-stopped`), so
  sessions deleted while Docker was down leave their databases behind. `dropdb` them, or
  drop the volume.

## Files

```
.spwn/
  env/Dockerfile                          # the session environment image
  hooks/
    session-created.d/20-dev-env.sh       # shared db + per-session db + app container
    session-deleted.d/50-dev-env.sh       # drop this session's db + container only
```
