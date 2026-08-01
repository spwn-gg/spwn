#!/bin/sh
# spwn default global hook — session-deleted (GLOBAL scope).
#
# Runs in the PROJECT dir, AFTER any repo-scope session-deleted hook (which runs inside
# the worktree). Removes the session's git worktree and deletes its branch. spwn passes
# SPWN_WORKTREE and SPWN_BRANCH.
#
# This is spwn's built-in teardown, exposed as an editable hook. Delete this file to
# fall back to spwn's native worktree/branch removal.

git -C "$SPWN_PROJECT_DIR" rev-parse --show-toplevel >/dev/null 2>&1 || exit 0

# Remove the worktree (force, so uncommitted changes don't block it).
[ -n "$SPWN_WORKTREE" ] && git -C "$SPWN_PROJECT_DIR" worktree remove --force "$SPWN_WORKTREE" 2>/dev/null

# Then the branch (strictly after removal — git won't delete a checked-out branch).
[ -n "$SPWN_BRANCH" ] && git -C "$SPWN_PROJECT_DIR" branch -D "$SPWN_BRANCH" 2>/dev/null

exit 0
