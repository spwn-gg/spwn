#!/usr/bin/env bash
# COOKBOOK RECIPE — place in .spwn/hooks/session-created.sh
#
# Stand up a per-session preview environment: install deps and start a dev server.
# Because hooks are SYNCHRONOUS (the session waits) and have NO timeout, the server is
# started in the BACKGROUND (`& disown`) so this script returns immediately.
#
# A stable, per-session port is derived from SPWN_TERMINAL_ID so parallel sessions don't
# collide. The PID and URL are written under .spwn/run/ so teardown.sh can find them.
set -euo pipefail

run_dir="$SPWN_WORKTREE/.spwn/run"
mkdir -p "$run_dir"

# Derive a deterministic port in 3000–3999 from the terminal id (no RNG, so it's stable
# across re-runs and unique per session).
hash="$(printf '%s' "$SPWN_TERMINAL_ID" | cksum | cut -d' ' -f1)"
port=$(( 3000 + (hash % 1000) ))
echo "[$SPWN_EVENT] preview port for $SPWN_TERMINAL_ID = $port"

# --- Install dependencies -------------------------------------------------------------
# Heavy gitignored dirs (e.g. node_modules) may already be COW-seeded into the worktree,
# so guard the install to avoid redundant work.
if [ ! -d node_modules ]; then
  echo "[$SPWN_EVENT] installing dependencies…"
  # npm ci
  # bundle install
  # uv sync
  :
fi

# --- Start the dev server in the background -------------------------------------------
echo "[$SPWN_EVENT] starting preview server on :$port"
# Replace the placeholder below with your server command. Keep the `& disown` so the
# session doesn't block, and redirect output to a log the agent can tail.
#
#   PORT="$port" npm run dev >"$run_dir/preview.log" 2>&1 & disown
#   echo $! > "$run_dir/preview.pid"
#
# Placeholder no-op so this template is safe to `test` as-is:
( sleep 0 ) & disown
echo $! > "$run_dir/preview.pid"

# Record the URL so tooling / notifications / the agent can find it.
printf 'http://localhost:%s\n' "$port" > "$run_dir/preview.url"
echo "[$SPWN_EVENT] preview -> $(cat "$run_dir/preview.url") (pid $(cat "$run_dir/preview.pid"))"
