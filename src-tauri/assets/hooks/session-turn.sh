#!/bin/sh
# spwn default global hook — session-turn (GLOBAL scope).
#
# Runs in the session worktree after each completed Claude turn. Commits the turn's
# working changes onto the session branch (so it carries mergeable history), then takes
# a code checkpoint for rewind/undo. spwn passes SPWN_TURN_UUID, SPWN_SESSION_ID and
# SPWN_BIN (the path to the spwn binary).
#
# This is spwn's built-in per-turn commit + checkpoint, exposed as an editable hook.
# Delete this file to disable per-turn commit/checkpoint.

# Commit onto the session branch (no-op if not a git repo or nothing changed). Uses a
# fixed identity so it works in repos with no configured user, and skips hooks so an
# autonomous run can't trip pre-commit hooks. `git add -A` respects .gitignore.
if git rev-parse --show-toplevel >/dev/null 2>&1; then
  git add -A 2>/dev/null || true
  if ! git diff --cached --quiet 2>/dev/null; then
    GIT_AUTHOR_NAME="spwn session" GIT_AUTHOR_EMAIL="spwn@localhost" \
    GIT_COMMITTER_NAME="spwn session" GIT_COMMITTER_EMAIL="spwn@localhost" \
    git commit --no-verify -m "spwn turn $(printf %s "$SPWN_TURN_UUID" | cut -c1-8)" \
      >/dev/null 2>&1 || true
  fi
fi

# Snapshot the working tree for rewind/undo. `spwn checkpoint` reads SPWN_SESSION_ID and
# SPWN_WORKTREE from the environment; it no-ops if the session id isn't known yet.
if [ -n "$SPWN_SESSION_ID" ] && [ -n "$SPWN_BIN" ] && [ -n "$SPWN_TURN_UUID" ]; then
  "$SPWN_BIN" checkpoint "$SPWN_TURN_UUID" >/dev/null 2>&1 || true
fi
