#!/usr/bin/env bash
# COOKBOOK RECIPE — place in .spwn/hooks/session-created.sh
#
# Keep the base branch fresh: when a new top-level session starts, fast-forward the
# parent repo's base branch so the session's worktree is cut from up-to-date code.
#
# Runs synchronously with the worktree as cwd. Safe to re-run.
set -euo pipefail

base="${SPWN_BASE_BRANCH:-}"

# Child/fork sessions branch off a parent session's `spwn/…` branch, not off a real
# branch. They should inherit the parent's tree as-is, so skip the pull for them.
# (`cm/*` is the legacy prefix — matched too so older sessions keep working.)
case "$base" in
  spwn/*|cm/*|"")
    echo "[$SPWN_EVENT] base is '$base' (child/unknown session); skipping base-branch pull"
    exit 0
    ;;
esac

# Worktrees live off SPWN_PROJECT_DIR (the shared parent checkout). Pull there so every
# new top-level session branches off an up-to-date base.
cd "$SPWN_PROJECT_DIR"
echo "[$SPWN_EVENT] refreshing base branch '$base' in $SPWN_PROJECT_DIR"

git fetch origin "$base"

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$current_branch" = "$base" ]; then
  # On the base branch: fast-forward only so we never create a merge commit or conflict.
  git merge --ff-only "origin/$base"
else
  # Parent checked out elsewhere: update the local base ref to match remote without
  # switching branches. No-op if it can't fast-forward.
  git fetch origin "$base:$base" \
    || echo "[$SPWN_EVENT] could not fast-forward local '$base'; skipping"
fi
