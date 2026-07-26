#!/usr/bin/env bash
# COOKBOOK RECIPE — place in .spwn/hooks/session-deleted.sh
#
# Tear down whatever session-created started. This runs synchronously JUST BEFORE the
# worktree is removed, so it can still read the worktree's files. Every step is guarded
# (`|| true`) and idempotent — a failing teardown must never block the delete.
set -euo pipefail

run_dir="$SPWN_WORKTREE/.spwn/run"

echo "[$SPWN_EVENT] tearing down session $SPWN_TERMINAL_ID (${SPWN_BRANCH:-<none>})"

# --- Stop the preview server started by preview-env.sh --------------------------------
if [ -f "$run_dir/preview.pid" ]; then
  pid="$(cat "$run_dir/preview.pid" 2>/dev/null || true)"
  if [ -n "${pid:-}" ]; then
    echo "[$SPWN_EVENT] stopping preview server (pid $pid)"
    kill "$pid" 2>/dev/null || true
  fi
fi

# --- Bring down any ephemeral containers ----------------------------------------------
# Namespace by terminal id so you only stop THIS session's stack:
#   docker compose -p "$SPWN_TERMINAL_ID" down -v || true

# --- Drop ephemeral data / release resources ------------------------------------------
#   dropdb "session_${SPWN_TERMINAL_ID}" 2>/dev/null || true
rm -rf "$run_dir" 2>/dev/null || true

echo "[$SPWN_EVENT] teardown complete"
