#!/bin/sh
# spwn default global hook — session-turn / checkpoint (GLOBAL scope).
#
# spwn owns this file: it may be OVERWRITTEN when spwn updates. Delete this file to stop
# taking a checkpoint each turn (rewind/undo will then only have earlier snapshots).
#
# Runs in the session worktree after each completed Claude turn. Snapshots the working
# tree for rewind/undo. `spwn checkpoint` reads SPWN_SESSION_ID and SPWN_WORKTREE from
# the environment; it no-ops if the session id isn't known yet.

if [ -n "$SPWN_SESSION_ID" ] && [ -n "$SPWN_BIN" ] && [ -n "$SPWN_TURN_UUID" ]; then
  "$SPWN_BIN" checkpoint "$SPWN_TURN_UUID" >/dev/null 2>&1 || true
fi
