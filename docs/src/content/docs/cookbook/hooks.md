---
title: Hooks Cookbook
description: Copy-paste project-hook recipes — refresh the base branch, copy secrets, stand up a per-session preview, seed a database, prompt before setup, and tear down on delete.
---

Practical, copy-paste recipes for [project hooks](/spwn/reference/hooks/). Each one is a
**generic template** — safe no-op defaults with commented placeholders you swap for your
stack. If you haven't set up a hook before, start with the [Project Hooks](/spwn/reference/hooks/)
guide for the mechanics.

All of these also live, runnable, in
[`examples/hooks/cookbook/`](https://github.com/spwn-gg/spwn/tree/main/examples/hooks/cookbook)
in the repo.

## Recipes at a glance

| Recipe | Event | What it does |
|--------|-------|--------------|
| [Pull the base branch](#pull-the-base-branch) | `session-created` | Fast-forward the parent repo's base branch so the session starts current. |
| [Copy gitignored secrets](#copy-gitignored-secrets) | `session-created` | Copy `.env`, certs, and other untracked config into the fresh worktree. |
| [Per-session preview env](#per-session-preview-environment) | `session-created` | Install deps and start a dev server on a per-session port, backgrounded. |
| [Seed a database](#seed-a-database) | `session-created` | Migrate + seed a per-session database, idempotently. |
| [Prompt before setup](#prompt-before-setup) | `session-created` | Ask before expensive setup with `spwn prompt`, gating what runs. |
| [Tear down on delete](#tear-down-on-delete) | `session-deleted` | Stop the preview server and drop ephemeral data before the worktree is removed. |

:::note[One file per event]
spwn runs exactly **one file per event** — `.spwn/hooks/<event>.sh`. To combine several
`session-created` recipes, put them in the *same* `session-created.sh` and call them in
sequence (or paste the bodies together). The preview and teardown recipes are a **pair** —
the first writes `.spwn/run/preview.pid` / `.url`, the second consumes them.
:::

## Pull the base branch

When a new top-level session starts, fast-forward the parent repo's base branch so the
worktree is cut from up-to-date code. Skips child/fork (`spwn/…`) sessions, which inherit
their parent's tree as-is.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.sh
set -euo pipefail

base="${SPWN_BASE_BRANCH:-}"

# Child/fork sessions branch off a parent session's `spwn/…` branch, not a real
# branch. They should inherit the parent's tree as-is, so skip the pull for them.
# (`cm/*` is the legacy prefix — matched too so older sessions keep working.)
case "$base" in
  spwn/*|cm/*|"")
    echo "[$SPWN_EVENT] base is '$base' (child/unknown session); skipping base-branch pull"
    exit 0
    ;;
esac

# Worktrees live off SPWN_PROJECT_DIR (the shared parent checkout). Pull there so every
# new top-level session branches off an up-to-date base.
cd "$SPWN_PROJECT_DIR"
echo "[$SPWN_EVENT] refreshing base branch '$base' in $SPWN_PROJECT_DIR"

git fetch origin "$base"

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$current_branch" = "$base" ]; then
  # On the base branch: fast-forward only so we never create a merge commit or conflict.
  git merge --ff-only "origin/$base"
else
  # Parent checked out elsewhere: update the local base ref to match remote without
  # switching branches. No-op if it can't fast-forward.
  git fetch origin "$base:$base" \
    || echo "[$SPWN_EVENT] could not fast-forward local '$base'; skipping"
fi
```

## Copy gitignored secrets

A worktree is a fresh checkout, so files that aren't committed (`.env`, local certs,
service-account JSON) are **not** present. Copy them from the shared project dir.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.sh
set -euo pipefail

# Files to bring across (gitignored, so absent from the checkout). Adjust to taste.
files=(".env" ".env.local" ".envrc")

for f in "${files[@]}"; do
  src="$SPWN_PROJECT_DIR/$f"
  dst="$SPWN_WORKTREE/$f"

  if [ -e "$dst" ]; then
    echo "[$SPWN_EVENT] $f already present; leaving as-is"
    continue
  fi
  if [ -e "$src" ]; then
    cp "$src" "$dst"
    echo "[$SPWN_EVENT] copied $f from project dir"
  else
    echo "[$SPWN_EVENT] no $f in project dir; skipping"
  fi
done
```

:::tip[Safer than copying plaintext]
Fetch from a secrets manager instead of copying files around:

```sh
op read "op://vault/app/.env" > .env      # 1Password CLI
vault kv get -field=env secret/app > .env # HashiCorp Vault
```
:::

## Per-session preview environment

Install deps and start a dev server on a **per-session port** derived from
`SPWN_TERMINAL_ID`, so parallel sessions don't collide. Because hooks are
[synchronous](/spwn/reference/hooks/#hooks-are-synchronous) with no timeout, the server is
backgrounded (`& disown`) and its PID/URL written under `.spwn/run/` for
[teardown](#tear-down-on-delete) to consume.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.sh
set -euo pipefail

run_dir="$SPWN_WORKTREE/.spwn/run"
mkdir -p "$run_dir"

# Derive a deterministic port in 3000–3999 from the terminal id (no RNG, so it's stable
# across re-runs and unique per session).
hash="$(printf '%s' "$SPWN_TERMINAL_ID" | cksum | cut -d' ' -f1)"
port=$(( 3000 + (hash % 1000) ))
echo "[$SPWN_EVENT] preview port for $SPWN_TERMINAL_ID = $port"

# --- Install dependencies ---
# Heavy gitignored dirs (e.g. node_modules) may already be COW-seeded into the worktree,
# so guard the install to avoid redundant work.
if [ ! -d node_modules ]; then
  echo "[$SPWN_EVENT] installing dependencies…"
  # npm ci
  # bundle install
  # uv sync
  :
fi

# --- Start the dev server in the background ---
# Keep the `& disown` so the session doesn't block, and redirect output to a log the
# agent can tail.
echo "[$SPWN_EVENT] starting preview server on :$port"
PORT="$port" npm run dev >"$run_dir/preview.log" 2>&1 & disown
echo $! > "$run_dir/preview.pid"

# Record the URL so tooling / notifications / the agent can find it.
printf 'http://localhost:%s\n' "$port" > "$run_dir/preview.url"
echo "[$SPWN_EVENT] preview -> $(cat "$run_dir/preview.url") (pid $(cat "$run_dir/preview.pid"))"
```

## Seed a database

Run migrations and load fixtures against a database **namespaced per session**, so
parallel sessions don't clobber each other. Keep it idempotent — the Hooks panel's
**Run** button lets you re-fire it by hand.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.sh
set -euo pipefail

# Namespace the database per session so parallel sessions don't clobber each other.
db="session_${SPWN_TERMINAL_ID}"
echo "[$SPWN_EVENT] preparing database '$db'"

# --- Create (idempotent) ---
createdb "$db" 2>/dev/null || true

# --- Migrate ---
DATABASE_URL="postgres://localhost/$db" npm run migrate
# php artisan migrate --force
# rails db:migrate

# --- Seed / load fixtures ---
DATABASE_URL="postgres://localhost/$db" npm run seed
# python manage.py loaddata fixtures/dev.json

echo "[$SPWN_EVENT] database ready"
```

:::caution[Need the Claude session id?]
`SPWN_SESSION_ID` is **not** set during `session-created`. If your seed keys on it, move
this to `.spwn/hooks/session-ready.sh` instead.
:::

## Prompt before setup

Gate expensive or optional setup on your answer. [`spwn prompt`](/spwn/reference/hooks/#ask-the-user)
shows a picker in the app and blocks until you answer, printing the chosen label to stdout.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.sh
set -euo pipefail

# --- Yes/No confirm (no options ⇒ Yes/No) ---
if [ "$("$SPWN_BIN" prompt --header setup 'Seed the database for this session?')" = Yes ]; then
  echo "[$SPWN_EVENT] seeding…"
  ./.spwn/hooks/setup/seed-db.sh
else
  echo "[$SPWN_EVENT] skipping seed"
fi

# --- Multiple choice ---
# Give explicit options; the chosen label comes back on stdout. `--multi` allows several
# (returned comma-joined). Wrap in `if` to catch a decline / headless run.
if profile="$("$SPWN_BIN" prompt --header env 'Which services to start?' none web 'web+worker')"; then
  echo "[$SPWN_EVENT] starting profile: $profile"
  case "$profile" in
    web)        npm run dev & disown ;;
    web+worker) npm run dev & disown; npm run worker & disown ;;
    none)       : ;;
  esac
else
  echo "[$SPWN_EVENT] no selection (declined/headless); starting nothing"
fi
```

:::caution[`spwn prompt` needs the UI]
Exit codes: `0` = answered (label on stdout), `2` = declined (no window, or the ~5-minute
timeout), `3` = usage error. The code never encodes *which* option was picked, so
`answer=$(…)` is safe under `set -e` — branch on the string. **Headless/scheduled runs
auto-decline**, so always handle the non-answered branch with a sensible default.
:::

## Tear down on delete

Undo whatever `session-created` started. This runs synchronously **just before** the
worktree is removed, so it can still read the worktree's files. Guard every step
(`|| true`) and keep it idempotent — a failing teardown must never block the delete.

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-deleted.sh
set -euo pipefail

run_dir="$SPWN_WORKTREE/.spwn/run"

echo "[$SPWN_EVENT] tearing down session $SPWN_TERMINAL_ID (${SPWN_BRANCH:-<none>})"

# --- Stop the preview server started by the preview-env recipe ---
if [ -f "$run_dir/preview.pid" ]; then
  pid="$(cat "$run_dir/preview.pid" 2>/dev/null || true)"
  if [ -n "${pid:-}" ]; then
    echo "[$SPWN_EVENT] stopping preview server (pid $pid)"
    kill "$pid" 2>/dev/null || true
  fi
fi

# --- Bring down any ephemeral containers (namespace by terminal id) ---
# docker compose -p "$SPWN_TERMINAL_ID" down -v || true

# --- Drop ephemeral data / release resources ---
# dropdb "session_${SPWN_TERMINAL_ID}" 2>/dev/null || true
rm -rf "$run_dir" 2>/dev/null || true

echo "[$SPWN_EVENT] teardown complete"
```

## Install a recipe

spwn discovers exactly one file per event at `.spwn/hooks/<event>.sh`. Copy a recipe into
that path, then **commit** — hooks travel into each session via the git checkout, so an
uncommitted hook never runs.

```sh
mkdir -p .spwn/hooks
cp examples/hooks/cookbook/pull-base-branch.sh .spwn/hooks/session-created.sh
cp examples/hooks/cookbook/teardown.sh         .spwn/hooks/session-deleted.sh
chmod +x .spwn/hooks/*.sh
git add .spwn/hooks && git commit -m "Add spwn hooks"
```

## More ideas

Notifications on `session-ready` (Slack/desktop, open a draft PR), toolchain pinning
(`nvm use`, `asdf install`), ephemeral containers namespaced by `SPWN_TERMINAL_ID`,
tunnels (`ngrok`/`cloudflared`) for a shareable preview URL, salvaging uncommitted work on
delete, and deprovisioning cloud previews for the branch.

## Next

- [Project Hooks](/spwn/reference/hooks/) — the mechanics, events, and environment.
- [Branches & Merging](/spwn/guides/branches-and-merging/)
- [Scheduled Tasks](/spwn/guides/scheduled-tasks/)
