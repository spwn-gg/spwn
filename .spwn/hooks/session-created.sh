#!/usr/bin/env bash
# spwn 'session-created' hook. Runs synchronously with the worktree as the working directory.
# Env: SPWN_EVENT SPWN_TERMINAL_ID SPWN_PROJECT_DIR SPWN_WORKTREE SPWN_BRANCH \
#      SPWN_BASE_BRANCH SPWN_SESSION_ID  (SPWN_SESSION_ID is unset on session-created)
set -euo pipefail

# Only pull for TOP-LEVEL sessions. A child/fork session branches off its parent
# session's `cm/…` branch (SPWN_BASE_BRANCH), not off main — those should inherit the
# parent's tree as-is, so we skip them. A top-level session is cut from the repo's
# real branch (e.g. `main`), so its base is not a `cm/…` session branch.
case "${SPWN_BASE_BRANCH:-}" in
  cm/*)
    echo "[$SPWN_EVENT] child session (base=$SPWN_BASE_BRANCH); skipping main pull"
    exit 0
    ;;
esac

# Update the top-level parent repo's main branch when a new session starts.
# Session worktrees live off SPWN_PROJECT_DIR (the shared parent checkout); we pull
# there so every new top-level session branches off an up-to-date main.
cd "$SPWN_PROJECT_DIR"

echo "[$SPWN_EVENT] pulling main in parent repo: $SPWN_PROJECT_DIR"

# Fetch latest refs.
git fetch origin main

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$current_branch" = "main" ]; then
  # On main: fast-forward only so we never create a merge commit or hit conflicts.
  # Fail soft: if local main has diverged from origin/main (e.g. a local commit
  # that later landed upstream as a squash-merged PR, giving it a new hash), the
  # ff would fail and abort the whole hook under `set -e`. Warn and skip instead.
  git merge --ff-only origin/main || echo "[$SPWN_EVENT] local main diverged from origin/main; skipping ff"
else
  # Not on main (e.g. parent checked out elsewhere): update the local main ref
  # to match the remote without switching branches. No-op if it can't fast-forward.
  git fetch origin main:main || echo "[$SPWN_EVENT] could not fast-forward local main; skipping"
fi
