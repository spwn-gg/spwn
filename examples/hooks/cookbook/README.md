# spwn hooks cookbook

Practical, copy-paste [project hook](https://spwn-gg.github.io/spwn/guides/hooks/) recipes.
Each file is a **generic template** — safe no-op defaults with commented placeholders you
swap for your stack. Start from the minimal mechanics example in [`../`](../README.md).

## Recipes

| Recipe | Event | What it does |
|--------|-------|--------------|
| [`pull-base-branch.sh`](pull-base-branch.sh) | `session-created` | Fast-forward the parent repo's base branch so the session starts current. Skips child (`cm/…`) sessions. |
| [`copy-secrets.sh`](copy-secrets.sh) | `session-created` | Copy gitignored config (`.env`, certs) from the project dir into the worktree — a fresh checkout won't have them. |
| [`preview-env.sh`](preview-env.sh) | `session-created` | Install deps and start a dev server on a per-session port, backgrounded, with a pidfile. |
| [`seed-db.sh`](seed-db.sh) | `session-created` | Migrate + seed a per-session database (idempotent). |
| [`teardown.sh`](teardown.sh) | `session-deleted` | Stop what `preview-env.sh` started and drop ephemeral data, before the worktree is removed. |

`preview-env.sh` and `teardown.sh` are a **pair** — the first writes `.spwn/run/preview.pid`
/ `.url`, the second consumes them. Multiple recipes for the same event go in the *same*
`<event>.sh` file (spwn runs one file per event) — call them in sequence or paste the
bodies together.

## Install

spwn discovers exactly one file per event at `.spwn/hooks/<event>.sh`. Copy a recipe into
that path (or orchestrate it from an existing event file), then **commit** — hooks travel
into each session via the git checkout, so an uncommitted hook never runs.

```sh
mkdir -p .spwn/hooks
cp examples/hooks/cookbook/pull-base-branch.sh .spwn/hooks/session-created.sh
cp examples/hooks/cookbook/teardown.sh         .spwn/hooks/session-deleted.sh
chmod +x .spwn/hooks/*.sh
git add .spwn/hooks && git commit -m "Add spwn hooks"
```

## Test locally

The `spwn-hooks` skill driver reproduces spwn's exact invocation (worktree cwd + `SPWN_*`
env, run directly if executable else via `sh`):

```sh
.claude/skills/spwn-hooks/spwn-hooks.sh test session-created
```

## Caveats

- **Commit the hook** — uncommitted `.spwn/hooks/*.sh` isn't in the worktree, so it never runs.
- **One file per event** — only `session-created`, `session-ready`, `session-deleted` fire.
- **Synchronous, no timeout** — the session waits for the script. Background long-running
  work yourself (`server & disown`); a hook that hangs blocks the session.
- **`session-deleted` runs before removal** — keep teardown idempotent and `|| true`-guarded.
- **`SPWN_SESSION_ID` is absent on `session-created`** — need it? use `session-ready`.
- **Output is an 8 KB tail** in the ▸ Hooks panel; non-zero exit is non-fatal (red dot).

## Further ideas

Notifications on `session-ready` (Slack/desktop, open a draft PR), toolchain pinning
(`nvm use`, `asdf install`), ephemeral containers namespaced by `SPWN_TERMINAL_ID`,
tunnels (`ngrok`/`cloudflared`) for a shareable preview URL, salvaging uncommitted work on
delete, and deprovisioning cloud previews for the branch.
