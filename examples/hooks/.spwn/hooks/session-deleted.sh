#!/usr/bin/env bash
# Runs just BEFORE the worktree is removed on delete (synchronously, so it finishes
# before the worktree — and this script — disappear). Use it to tear down anything
# session-created started (containers, servers, data).
set -euo pipefail

echo "[session-deleted] cleaning up session $SPWN_TERMINAL_ID (${SPWN_BRANCH:-<none>})"
rm -f "$SPWN_WORKTREE/SPWN_SETUP.txt" 2>/dev/null || true
