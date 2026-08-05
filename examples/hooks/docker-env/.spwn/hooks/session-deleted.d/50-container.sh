#!/usr/bin/env bash
# Tear down the session's Docker environment.
#
# Numbered 50- so it runs before spwn's global `90-worktree.sh` removes the worktree —
# repo-scope `session-deleted` hooks run first, inside the worktree, for exactly this
# reason.
#
# Every step is guarded: a failing teardown must never block a delete.
set -euo pipefail

name="spwn-$SPWN_TERMINAL_ID"
docker="$(command -v docker)" || exit 0

# `docker rm -f`, not `stop`: the container was created with `--restart
# unless-stopped`, so a merely-stopped one comes back on the next Docker restart —
# long after the session that owned it is gone.
if "$docker" inspect "$name" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] removing environment $name"
  "$docker" rm -f "$name" >/dev/null 2>&1 || true
fi

echo "[$SPWN_EVENT] teardown complete"
