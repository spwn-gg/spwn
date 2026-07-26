#!/usr/bin/env bash
# Runs the first time the Claude session id is known. Unlike session-created,
# SPWN_SESSION_ID is populated here. Like all hooks, it runs synchronously.
set -euo pipefail

echo "[session-ready] session $SPWN_SESSION_ID is live on ${SPWN_BRANCH:-<none>}"
