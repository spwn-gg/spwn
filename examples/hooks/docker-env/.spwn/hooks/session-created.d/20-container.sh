#!/usr/bin/env bash
# Per-session Docker environment — stands up one container per session and scopes the
# agent to it by reporting an exec prefix back to spwn.
#
# REPO scope, and it must run AFTER the worktree exists (spwn's global
# `10-worktree.sh` creates it), which is exactly when repo hooks run.
#
# What spwn does with this: it prepends the reported `exec` prefix to the argv of
# every pane it opens for this session, so the agent's TUI and any shell you open on
# the session run *inside* the container. spwn itself knows nothing about Docker.
set -euo pipefail

name="spwn-$SPWN_TERMINAL_ID"
image="${SPWN_ENV_IMAGE:-spwn-session-env}"

# Absolute path, deliberately. spwn launches panes exec-style against the long-lived
# rmux daemon's PATH — not this script's — so a bare `docker` can resolve here and
# still be missing there.
docker="$(command -v docker)" || {
  echo "[$SPWN_EVENT] docker not found; leaving this session on the host"
  exit 0
}

# Fail fast rather than blocking the session on a cold pull: hooks are synchronous and
# have no timeout, so a multi-minute `docker pull` here is a session that appears hung.
if ! "$docker" image inspect "$image" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] image '$image' not present locally — build or pull it first:"
  echo "[$SPWN_EVENT]   docker build -t $image .spwn/env"
  echo "[$SPWN_EVENT] leaving this session on the host"
  exit 0
fi

# --- Paths that MUST be identical inside and outside -------------------------------
#
# Two things break silently otherwise:
#   * the transcript. spwn locates a session's JSONL by a slug of its cwd. A different
#     in-container path means a different slug, and session binding, the Timeline,
#     turn detection and rewind all go dark.
#   * git. A worktree's `.git` is a FILE holding an absolute `gitdir:` pointer into the
#     main repo, so both paths have to resolve in the container.
#
# Ask git for the repo dir rather than assuming `$SPWN_PROJECT_DIR/.git`: the project
# may be a subdirectory of the repo, or itself a linked worktree, and spwn's worktrees
# can live outside the project entirely (see the `worktree_location` setting).
gitdir="$(git -C "$SPWN_WORKTREE" rev-parse --path-format=absolute --git-common-dir)"

if ! "$docker" inspect "$name" >/dev/null 2>&1; then
  echo "[$SPWN_EVENT] creating environment $name from $image"
  "$docker" run -d --name "$name" \
    --restart unless-stopped \
    -v "$SPWN_WORKTREE:$SPWN_WORKTREE" \
    -v "$gitdir:$gitdir" \
    -v "$HOME/.claude:$HOME/.claude" \
    -e "HOME=$HOME" \
    -w "$SPWN_WORKTREE" \
    "$image" sleep infinity >/dev/null
fi

# Idempotent: the Hooks panel's Run button re-fires this to rebuild a lost container.
"$docker" start "$name" >/dev/null 2>&1 || true

# git refuses to operate on a tree it thinks belongs to someone else, which is the
# default once uid mapping differs between host and container.
"$docker" exec "$name" git config --global --add safe.directory "$SPWN_WORKTREE" || true
"$docker" exec "$name" git config --global --add safe.directory "$gitdir" || true

# --- Hand the environment back to spwn ---------------------------------------------
#
# `-it` is not optional for the interactive prefix: `-t` allocates the tty the agent's
# TUI needs to render at all (without it every `detect` rule in the agent definition
# misses and the session looks permanently idle), and it's what forwards terminal
# resizes. `-i` keeps stdin attached so keys, Escape/C-c and bracketed paste arrive.
echo "::spwn:set:: exec=$docker exec -it -w $SPWN_WORKTREE $name"

# Headless (scheduled) runs get the SAME container without a tty: that path parses
# line-delimited JSON, which a tty corrupts with interleaved spinner output.
echo "::spwn:set:: execHeadless=$docker exec -i -w $SPWN_WORKTREE $name"

# The image ships its own Linux agent CLI; the host's macOS binary can't exec here.
echo "::spwn:set:: execShell=/bin/bash"

echo "[$SPWN_EVENT] session scoped to $name"
