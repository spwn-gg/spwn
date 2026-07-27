#!/usr/bin/env bash
# COOKBOOK RECIPE — place in .spwn/hooks/session-created.sh
#
# Ask before doing expensive/optional per-session setup. `spwn prompt` shows a picker in
# the spwn UI and blocks until you answer, printing the chosen label to stdout — so a
# session-created hook can GATE what it does on your choice.
#
# Invoke it as "$SPWN_BIN" prompt … (spwn injects SPWN_BIN; bare `spwn prompt` also works
# if it's on PATH). Exit codes:
#   0 = answered (chosen label on stdout)
#   2 = declined (no UI window, or the ~5-minute timeout elapsed)
#   3 = usage error / not run inside a hook
# The code never encodes WHICH option was picked (0 for every answer), so `answer=$(…)`
# stays safe under `set -e` — branch on the string. Headless/scheduled runs have no
# window and auto-decline, so always handle the non-answered branch with a sane default.
set -euo pipefail

# --- Yes/No confirm (no options ⇒ Yes/No) ---------------------------------------------
if [ "$("$SPWN_BIN" prompt --header setup 'Seed the database for this session?')" = Yes ]; then
  echo "[$SPWN_EVENT] seeding…"
  #   ./.spwn/hooks/setup/seed-db.sh
else
  echo "[$SPWN_EVENT] skipping seed"
fi

# --- Multiple choice ------------------------------------------------------------------
# Give explicit options; the chosen label comes back on stdout. `--multi` allows several
# (returned comma-joined). Wrap in `if` to catch a decline / headless run.
if profile="$("$SPWN_BIN" prompt --header env 'Which services to start?' none web 'web+worker')"; then
  echo "[$SPWN_EVENT] starting profile: $profile"
  #   case "$profile" in
  #     web)        my-dev-server & disown ;;
  #     web+worker) my-dev-server & disown; my-worker & disown ;;
  #     none)       : ;;
  #   esac
else
  echo "[$SPWN_EVENT] no selection (declined/headless); starting nothing"
fi
