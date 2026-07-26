#!/usr/bin/env bash
# COOKBOOK RECIPE — place in .spwn/hooks/session-created.sh
#
# Bring untracked, gitignored config into the session. A worktree is a fresh checkout, so
# files that aren't committed (.env, local certs, service-account JSON) are NOT present.
# Copy them from the shared project dir into the worktree.
#
# Safer alternative: fetch from a secrets manager instead of copying plaintext, e.g.
#   op read "op://vault/app/.env" > .env      # 1Password CLI
#   vault kv get -field=env secret/app > .env # HashiCorp Vault
set -euo pipefail

# Files to bring across (gitignored, so absent from the checkout). Adjust to taste.
files=(".env" ".env.local" ".envrc")

for f in "${files[@]}"; do
  src="$SPWN_PROJECT_DIR/$f"
  dst="$SPWN_WORKTREE/$f"

  if [ -e "$dst" ]; then
    echo "[$SPWN_EVENT] $f already present; leaving as-is"
    continue
  fi
  if [ -e "$src" ]; then
    cp "$src" "$dst"
    echo "[$SPWN_EVENT] copied $f from project dir"
  else
    echo "[$SPWN_EVENT] no $f in project dir; skipping"
  fi
done
