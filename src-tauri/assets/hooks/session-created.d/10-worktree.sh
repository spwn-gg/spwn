#!/bin/sh
# spwn default global hook — session-created / worktree (GLOBAL scope).
#
# spwn owns this file: it may be OVERWRITTEN when spwn updates. Don't edit it to add
# your own steps — drop a separate script (e.g. 50-my-setup.sh) in this same
# `session-created.d/` folder instead; they run in filename order after this one.
# Delete this file to opt out of spwn's worktree creation entirely.
#
# Runs in the PROJECT dir, before the session's worktree exists, and creates it +
# seeds heavy build dirs so an agent can build immediately. Reports the worktree back
# to spwn via `::spwn:set::` lines. spwn passes the intended path/branch/base in
# SPWN_WORKTREE / SPWN_BRANCH / SPWN_BASE_BRANCH.

# Not a git repo → nothing to do; spwn keeps the session in the project dir.
git rev-parse --show-toplevel >/dev/null 2>&1 || exit 0

# Nothing to create without a target path/branch (spwn always sets these for a
# fresh Claude session in a git repo).
[ -n "$SPWN_WORKTREE" ] && [ -n "$SPWN_BRANCH" ] && [ -n "$SPWN_BASE_BRANCH" ] || exit 0

# Already created (e.g. a manual re-run) → just re-report it.
if [ -e "$SPWN_WORKTREE/.git" ]; then
  echo "::spwn:set:: worktree=$SPWN_WORKTREE"
  echo "::spwn:set:: branch=$SPWN_BRANCH"
  echo "::spwn:set:: base=$SPWN_BASE_BRANCH"
  exit 0
fi

mkdir -p "$(dirname "$SPWN_WORKTREE")"
git worktree add -b "$SPWN_BRANCH" "$SPWN_WORKTREE" "$SPWN_BASE_BRANCH" || exit 1

# COW-clone heavy, gitignored build dirs (clonefile on APFS; plain copy elsewhere) so
# the agent doesn't pay a cold install/build. A worktree only checks out tracked files,
# so these are otherwise absent.
for d in node_modules target .venv venv dist build .next .svelte-kit .turbo; do
  if [ -d "$SPWN_PROJECT_DIR/$d" ] && [ ! -e "$SPWN_WORKTREE/$d" ]; then
    cp -cR "$SPWN_PROJECT_DIR/$d" "$SPWN_WORKTREE/$d" 2>/dev/null \
      || cp -R "$SPWN_PROJECT_DIR/$d" "$SPWN_WORKTREE/$d" 2>/dev/null || true
  fi
done

echo "::spwn:set:: worktree=$SPWN_WORKTREE"
echo "::spwn:set:: branch=$SPWN_BRANCH"
echo "::spwn:set:: base=$SPWN_BASE_BRANCH"
