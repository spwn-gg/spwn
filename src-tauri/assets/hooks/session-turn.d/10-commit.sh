#!/bin/sh
# spwn default global hook — session-turn / commit (GLOBAL scope).
#
# spwn owns this file: it may be OVERWRITTEN when spwn updates. Add your own per-turn
# steps as separate scripts in this `session-turn.d/` folder. Delete this file to stop
# auto-committing each turn.
#
# Runs in the session worktree after each completed Claude turn. Commits the turn's
# working changes onto the session branch so it carries mergeable history. Uses a fixed
# identity so it works in repos with no configured user, and skips hooks so an
# autonomous run can't trip pre-commit hooks. `git add -A` respects .gitignore.

git rev-parse --show-toplevel >/dev/null 2>&1 || exit 0

git add -A 2>/dev/null || true
if ! git diff --cached --quiet 2>/dev/null; then
  GIT_AUTHOR_NAME="spwn session" GIT_AUTHOR_EMAIL="spwn@localhost" \
  GIT_COMMITTER_NAME="spwn session" GIT_COMMITTER_EMAIL="spwn@localhost" \
  git commit --no-verify -m "spwn turn $(printf %s "$SPWN_TURN_UUID" | cut -c1-8)" \
    >/dev/null 2>&1 || true
fi
