#!/usr/bin/env bash
# spwn 'session-created' hook. Runs synchronously with the worktree as the working directory.
# Env: SPWN_EVENT SPWN_TERMINAL_ID SPWN_PROJECT_DIR SPWN_WORKTREE SPWN_BRANCH \
#      SPWN_BASE_BRANCH SPWN_SESSION_ID  (SPWN_SESSION_ID is unset on session-created)
set -euo pipefail

# Update the top-level parent repo's main branch when a new session starts.
# Session worktrees live off SPWN_PROJECT_DIR (the shared parent checkout); we pull
# there so every new session branches off an up-to-date main.
cd "$SPWN_PROJECT_DIR"

echo "[$SPWN_EVENT] pulling main in parent repo: $SPWN_PROJECT_DIR"

# Fetch latest refs.
git fetch origin main

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$current_branch" = "main" ]; then
  # On main: fast-forward only so we never create a merge commit or hit conflicts.
  git merge --ff-only origin/main
else
  # Not on main (e.g. parent checked out elsewhere): update the local main ref
  # to match the remote without switching branches. No-op if it can't fast-forward.
  git fetch origin main:main || echo "[$SPWN_EVENT] could not fast-forward local main; skipping"
fi
