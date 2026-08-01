#!/usr/bin/env bash
# First step of the `session-created` event (repo scope, directory-based).
# Files in `.spwn/hooks/session-created.d/` run in filename order — this `10-` step runs
# before `20-install.sh`. Hooks run synchronously (the session waits), so background any
# long-running work yourself, e.g. `my-server & disown`.
set -euo pipefail

echo "[session-created] $SPWN_TERMINAL_ID"
echo "  event       = $SPWN_EVENT"
echo "  project dir = $SPWN_PROJECT_DIR"
echo "  worktree    = $SPWN_WORKTREE"
echo "  branch      = ${SPWN_BRANCH:-<none>}"
echo "  base branch = ${SPWN_BASE_BRANCH:-<none>}"
echo "  session id  = ${SPWN_SESSION_ID:-<not yet known>}"
echo "  cwd         = $(pwd)"
