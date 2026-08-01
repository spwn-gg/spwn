#!/usr/bin/env bash
# Second step of `session-created` — runs after `10-info.sh`. Put real setup here:
# install deps, bring up services, seed data. Each numbered file is independent, so you
# can add/remove steps without editing the others.
set -euo pipefail

echo "  [20-install] (put real setup here, e.g. npm install / docker compose up -d)"

# The cwd is the worktree, so relative writes land in the session's files.
printf 'session %s set up on %s\n' "$SPWN_TERMINAL_ID" "${SPWN_BRANCH:-<none>}" > SPWN_SETUP.txt
echo "  [20-install] wrote SPWN_SETUP.txt"
