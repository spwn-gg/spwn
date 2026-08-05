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
| [Per-session container](#per-session-container-environment) | `session-created` | Run the agent itself inside its own Docker container. |

:::note[Combining recipes]
Each recipe below is a repo hook committed at `.spwn/hooks/<event>.sh`. To run several for
the same event, either paste the bodies into that one script, or split them into a
[`<event>.d/` folder](/spwn/reference/hooks/#where-hooks-live) of numbered files
(`session-created.d/20-secrets.sh`, `30-seed.sh`, …) that spwn runs in order. To apply a
recipe to **every** project instead of one repo, put it in the shared `~/.spwn/hooks/`
folder. The preview and teardown recipes are a **pair** — the first writes
`.spwn/run/preview.pid` / `.url`, the second consumes them.
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

## Per-session container environment

The recipes above run things *alongside* a session. This one runs the session **inside**
a container: the agent's TUI and any shell you open on it execute there, so its builds,
tests and installs never touch your machine — and two sessions can want two different
toolchains without a fight.

The mechanism is one reported value. spwn prepends the prefix to every pane's argv; it
has no Docker code of its own. See
[Running a session somewhere else](/spwn/reference/hooks/#running-a-session-somewhere-else).

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-created.d/20-container.sh
set -euo pipefail

name="spwn-$SPWN_TERMINAL_ID"
image="${SPWN_ENV_IMAGE:-spwn-session-env}"

# Absolute path: spwn launches panes against the rmux daemon's PATH, not this script's.
docker="$(command -v docker)" || { echo "no docker; staying on the host"; exit 0; }

# Don't pull here — hooks are synchronous, so a cold pull looks like a hung session.
"$docker" image inspect "$image" >/dev/null 2>&1 || {
  echo "build $image first"; exit 0; }

# Mount at the SAME absolute path inside and out. spwn finds the transcript by a slug
# of the cwd, and a worktree's .git holds an absolute pointer into the main repo —
# a different path breaks the Timeline, rewind and git, silently.
gitdir="$(git -C "$SPWN_WORKTREE" rev-parse --path-format=absolute --git-common-dir)"

if ! "$docker" inspect "$name" >/dev/null 2>&1; then
  "$docker" run -d --name "$name" --restart unless-stopped \
    -v "$SPWN_WORKTREE:$SPWN_WORKTREE" \
    -v "$gitdir:$gitdir" \
    -v "$HOME/.claude:$HOME/.claude" \
    -e "HOME=$HOME" -w "$SPWN_WORKTREE" \
    "$image" sleep infinity >/dev/null
fi
"$docker" start "$name" >/dev/null 2>&1 || true

# -it for the TUI (it needs a tty to render); -i only for headless JSON parsing.
echo "::spwn:set:: exec=$docker exec -it -w $SPWN_WORKTREE $name"
echo "::spwn:set:: execHeadless=$docker exec -i -w $SPWN_WORKTREE $name"
echo "::spwn:set:: execShell=/bin/bash"
```

Pair it with teardown, numbered to run before spwn's `90-worktree.sh`:

```sh
#!/usr/bin/env bash
# .spwn/hooks/session-deleted.d/50-container.sh
set -euo pipefail
docker="$(command -v docker)" || exit 0
# rm -f, not stop: --restart unless-stopped would resurrect a stopped container.
"$docker" rm -f "spwn-$SPWN_TERMINAL_ID" >/dev/null 2>&1 || true
```

:::caution[The image needs the agent CLI]
`{bin}` resolves against the *container's* `PATH`, and your host's macOS `claude` can't
execute in a Linux image — so the image must install its own. It also needs `env`, `git`
and a shell. The full example, with a Dockerfile, is in
[`examples/hooks/docker-env/`](https://github.com/spwn-gg/spwn/tree/main/examples/hooks/docker-env).
:::

:::note[It's an environment boundary, not a sandbox]
The container shares your `~/.claude` (that's what reuses your login and puts the
transcript where spwn reads it) and the worktree is bind-mounted, so host-side git,
per-turn commits and checkpoints keep working. What's isolated is the *toolchain*.
:::

## Install a recipe

Copy a recipe to `.spwn/hooks/<event>.sh` in your repo, then **commit** — hooks travel into
each session via the git checkout, so an uncommitted hook never runs.

```sh
mkdir -p .spwn/hooks
cp examples/hooks/cookbook/pull-base-branch.sh .spwn/hooks/session-created.sh
cp examples/hooks/cookbook/teardown.sh         .spwn/hooks/session-deleted.sh
chmod +x .spwn/hooks/*.sh
git add .spwn/hooks && git commit -m "Add spwn hooks"
```

To run several recipes for one event, use a `<event>.d/` folder of numbered scripts
instead of a single file — spwn runs `.spwn/hooks/<event>.d/*` in filename order (see
[Where hooks live](/spwn/reference/hooks/#where-hooks-live)):

```sh
mkdir -p .spwn/hooks/session-created.d
cp examples/hooks/cookbook/pull-base-branch.sh .spwn/hooks/session-created.d/10-pull-base.sh
cp examples/hooks/cookbook/copy-secrets.sh     .spwn/hooks/session-created.d/20-secrets.sh
chmod +x .spwn/hooks/session-created.d/*.sh
git add .spwn/hooks && git commit -m "Add spwn hooks"
```

To apply a recipe to **every** project, drop it in the shared `~/.spwn/hooks/` folder
instead of a repo (no commit needed — it isn't tied to any repo).

## More ideas

Notifications on `session-ready` (Slack/desktop, open a draft PR), toolchain pinning
(`nvm use`, `asdf install`), tunnels (`ngrok`/`cloudflared`) for a shareable preview URL,
salvaging uncommitted work on delete, and deprovisioning cloud previews for the branch.

## Next

- [Project Hooks](/spwn/reference/hooks/) — the mechanics, events, and environment.
- [Branches & Merging](/spwn/guides/branches-and-merging/)
- [Scheduled Tasks](/spwn/guides/scheduled-tasks/)
