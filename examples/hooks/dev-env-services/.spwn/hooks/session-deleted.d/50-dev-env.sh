#!/usr/bin/env bash
# Tear down this session's dev environment.
#
# Removes what belongs to THIS session — its app container and its database — and
# deliberately leaves the shared server and network alone: a session's teardown must
# never take down a server other live sessions are still using. Ref-counting shared
# services was one of the things that made spwn's old built-in compose integration too
# complicated to keep, so this doesn't try. Clean up by hand when you're done:
#
#   docker rm -f spwn-shared-db && docker network rm spwn-shared
#
# Numbered 50- so it runs before spwn's global `90-worktree.sh` removes the worktree.
# Every step is guarded: a failing teardown must never block a delete.
set -euo pipefail

app="spwn-$SPWN_TERMINAL_ID"
db="spwn-shared-db"
docker="$(command -v docker)" || exit 0

# Drop this session's database, but only if the shared server is actually up — after a
# reboot it may not be, and that must not turn into a failed delete.
if "$docker" inspect "$db" >/dev/null 2>&1; then
  dbname="session_$(printf '%s' "$SPWN_TERMINAL_ID" | tr -cd '[:alnum:]' | cut -c1-40)"
  if "$docker" exec "$db" dropdb -U dev --if-exists "$dbname" 2>/dev/null; then
    echo "[$SPWN_EVENT] dropped database $dbname"
  fi
fi

# `rm -f`, not `stop`: the container was created with `--restart unless-stopped`, so a
# merely-stopped one comes back on the next Docker restart, long after its session is gone.
if "$docker" inspect "$app" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] removing $app"
  "$docker" rm -f "$app" >/dev/null 2>&1 || true
fi

echo "[$SPWN_EVENT] teardown complete (shared database left running)"
