#!/usr/bin/env bash
# Per-session dev environment: an app container the agent runs in, plus a database.
#
# What's shared and what isn't is the whole point:
#   * app container   — PER SESSION (it's what the agent runs in, and holds the worktree)
#   * database SERVER — SHARED      (one postgres per session is minutes of startup and
#                                    gigabytes of RAM for nothing)
#   * database DATA   — PER SESSION (sessions must not clobber each other's rows)
#
# One server process, a separate database inside it per session. That middle ground is
# what makes running ten sessions at once practical.
#
# REPO scope, and it must run AFTER the worktree exists (spwn's global `10-worktree.sh`
# creates it), which is exactly when repo hooks run.
set -euo pipefail

net="spwn-shared"
db="spwn-shared-db"
app="spwn-$SPWN_TERMINAL_ID"
image="${SPWN_ENV_IMAGE:-spwn-session-env}"

# Absolute path, deliberately: spwn launches panes exec-style against the long-lived
# rmux daemon's PATH, not this script's, so a bare `docker` can resolve here and still
# be missing there.
docker="$(command -v docker)" || {
  echo "[$SPWN_EVENT] docker not found; leaving this session on the host"
  exit 0
}

# Hooks are synchronous with no timeout, so a cold pull would look like a hung session.
if ! "$docker" image inspect "$image" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] image '$image' not present locally — build it first:"
  echo "[$SPWN_EVENT]   docker build -t $image .spwn/env"
  echo "[$SPWN_EVENT] leaving this session on the host"
  exit 0
fi

# --- Shared network + database server: created once, reused by every session ---------
"$docker" network inspect "$net" >/dev/null 2>&1 || "$docker" network create "$net" >/dev/null

if ! "$docker" inspect "$db" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] starting the shared database"
  "$docker" run -d --name "$db" --network "$net" --restart unless-stopped \
    -e POSTGRES_USER=dev -e POSTGRES_PASSWORD=dev \
    -v spwn-shared-db-data:/var/lib/postgresql/data \
    postgres:16-alpine >/dev/null
fi
"$docker" start "$db" >/dev/null 2>&1 || true

# Postgres accepts connections a beat after the container starts. Without this wait,
# createdb below fails and the session comes up pointing at a database that isn't there.
ready=""
for _ in $(seq 30); do
  if "$docker" exec "$db" pg_isready -U dev -q 2>/dev/null; then ready=1; break; fi
  sleep 1
done
[ -n "$ready" ] || echo "[$SPWN_EVENT] warning: database never reported ready"

# --- A database per session, inside that shared server ------------------------------
# Postgres identifiers can't hold the terminal id's dashes, so sanitise it.
dbname="session_$(printf '%s' "$SPWN_TERMINAL_ID" | tr -cd '[:alnum:]' | cut -c1-40)"
if "$docker" exec "$db" createdb -U dev "$dbname" 2>/dev/null; then
  echo "[$SPWN_EVENT] created database $dbname"
else
  echo "[$SPWN_EVENT] database $dbname already exists"
fi

# --- The per-session app container the agent is scoped to ---------------------------
#
# Mounts use the SAME absolute path inside and out. Two things depend on it and both
# fail silently otherwise: spwn locates the transcript by a slug of the cwd, and a
# worktree's `.git` is a file holding an absolute pointer into the main repo. Ask git
# for that path rather than assuming `$SPWN_PROJECT_DIR/.git` — wrong when the project
# is a subdirectory, is itself a linked worktree, or when worktrees live elsewhere.
gitdir="$(git -C "$SPWN_WORKTREE" rev-parse --path-format=absolute --git-common-dir)"

if ! "$docker" inspect "$app" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] creating $app from $image"
  # `-p 127.0.0.1::3000` lets Docker pick a free host port, so N sessions never collide.
  "$docker" run -d --name "$app" --network "$net" --restart unless-stopped \
    -p 127.0.0.1::3000 \
    -v "$SPWN_WORKTREE:$SPWN_WORKTREE" \
    -v "$gitdir:$gitdir" \
    -v "$HOME/.claude:$HOME/.claude" \
    -e "HOME=$HOME" \
    -e "DATABASE_URL=postgres://dev:dev@$db:5432/$dbname" \
    -w "$SPWN_WORKTREE" \
    "$image" sleep infinity >/dev/null
fi
"$docker" start "$app" >/dev/null 2>&1 || true

# git refuses to touch a tree it thinks belongs to someone else, which is the default
# once uid mapping differs between host and container.
"$docker" exec "$app" git config --global --add safe.directory "$SPWN_WORKTREE" || true
"$docker" exec "$app" git config --global --add safe.directory "$gitdir" || true

# --- Surface the port Docker picked -------------------------------------------------
mkdir -p "$SPWN_WORKTREE/.spwn/run"
hostport="$("$docker" port "$app" 3000/tcp 2>/dev/null | head -1 | sed 's/.*://')"
if [ -n "${hostport:-}" ]; then
  printf 'http://localhost:%s\n' "$hostport" > "$SPWN_WORKTREE/.spwn/run/preview.url"
  echo "[$SPWN_EVENT] preview -> http://localhost:$hostport"
fi

# --- Hand the environment back to spwn ----------------------------------------------
#
# `-it` is not optional for the interactive prefix: `-t` allocates the tty the agent's
# TUI needs to render at all (without it every detect rule misses and the session looks
# permanently idle) and forwards terminal resizes; `-i` keeps stdin attached so keys,
# Escape/C-c and bracketed paste arrive. Headless runs get the same container WITHOUT a
# tty, because that path parses line-delimited JSON which a tty corrupts.
echo "::spwn:set:: exec=$docker exec -it -w $SPWN_WORKTREE $app"
echo "::spwn:set:: execHeadless=$docker exec -i -w $SPWN_WORKTREE $app"
echo "::spwn:set:: execShell=/bin/bash"

echo "[$SPWN_EVENT] session scoped to $app (db: $dbname)"
