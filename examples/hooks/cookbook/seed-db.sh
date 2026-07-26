#!/usr/bin/env bash
# COOKBOOK RECIPE — place in .spwn/hooks/session-created.sh
#
# Give the session a database to work against: run migrations and load fixtures. Keep it
# idempotent so a manual re-run (the ▸ Hooks panel "Run" button) is harmless.
#
# Needs the Claude session id? Move this to session-ready.sh instead — SPWN_SESSION_ID is
# NOT set during session-created.
set -euo pipefail

# Namespace the database per session so parallel sessions don't clobber each other.
db="session_${SPWN_TERMINAL_ID}"
echo "[$SPWN_EVENT] preparing database '$db'"

# --- Create (idempotent) --------------------------------------------------------------
#   createdb "$db" 2>/dev/null || true

# --- Migrate --------------------------------------------------------------------------
#   DATABASE_URL="postgres://localhost/$db" npm run migrate
#   php artisan migrate --force
#   rails db:migrate

# --- Seed / load fixtures -------------------------------------------------------------
#   DATABASE_URL="postgres://localhost/$db" npm run seed
#   python manage.py loaddata fixtures/dev.json

echo "[$SPWN_EVENT] database ready (fill in the commented migrate/seed steps)"
