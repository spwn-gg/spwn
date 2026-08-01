#!/bin/sh
# spwn default global hook — session-deleted / worktree teardown (GLOBAL scope).
#
# spwn owns this file: it may be OVERWRITTEN when spwn updates. To add your own teardown
# steps, drop a separate script in this `session-deleted.d/` folder (name it to run
# before this one, e.g. 10-my-teardown.sh). Delete this file to opt out.
#
# Runs in the PROJECT dir, AFTER any repo-scope session-deleted hooks (which run inside
# the worktree). Removes the session's git worktree and deletes its branch. spwn passes
# SPWN_WORKTREE and SPWN_BRANCH.

git -C "$SPWN_PROJECT_DIR" rev-parse --show-toplevel >/dev/null 2>&1 || exit 0

# Remove the worktree (force, so uncommitted changes don't block it).
[ -n "$SPWN_WORKTREE" ] && git -C "$SPWN_PROJECT_DIR" worktree remove --force "$SPWN_WORKTREE" 2>/dev/null

# Then the branch (strictly after removal — git won't delete a checked-out branch).
[ -n "$SPWN_BRANCH" ] && git -C "$SPWN_PROJECT_DIR" branch -D "$SPWN_BRANCH" 2>/dev/null

exit 0
