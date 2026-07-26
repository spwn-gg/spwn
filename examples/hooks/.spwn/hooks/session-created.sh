#!/usr/bin/env bash
# The single entry point for the `session-created` event, run when the session's
# worktree is ready. Hooks run synchronously (the session waits for this to finish), so
# background any long-running work yourself, e.g. `my-server & disown`. This is just a
# normal shell script — orchestrate whatever setup you need by calling other files/code.
set -euo pipefail

echo "[session-created] $SPWN_TERMINAL_ID"
echo "  event       = $SPWN_EVENT"
echo "  project dir = $SPWN_PROJECT_DIR"
echo "  worktree    = $SPWN_WORKTREE"
echo "  branch      = ${SPWN_BRANCH:-<none>}"
echo "  base branch = ${SPWN_BASE_BRANCH:-<none>}"
echo "  session id  = ${SPWN_SESSION_ID:-<not yet known>}"
echo "  cwd         = $(pwd)"

# Orchestrate other files/code — the hook is a plain script, so call anything:
#   npm install
#   docker compose up -d
#   python scripts/seed_db.py
"$SPWN_WORKTREE/.spwn/hooks/setup/install.sh"

# The cwd is the worktree, so relative writes land in the session's files.
printf 'session %s set up on %s\n' "$SPWN_TERMINAL_ID" "${SPWN_BRANCH:-<none>}" > SPWN_SETUP.txt
echo "[session-created] wrote SPWN_SETUP.txt"
