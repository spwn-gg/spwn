#!/usr/bin/env bash
# Build the release spwn CLI binary (with the SPA embedded), and optionally smoke-check
# that it serves. This is the driver for the run-spwn skill: it puts cargo on PATH,
# builds the frontend + sidecar + release Rust binary, and (with --open) starts the
# server, confirms it answers, and opens the browser.
#
# Usage:
#   .claude/skills/run-spwn/build-app.sh           # build only
#   .claude/skills/run-spwn/build-app.sh --open    # build, then serve + open browser
set -euo pipefail

# Repo root = three levels up from this script (.claude/skills/run-spwn/).
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

BIN="backend/target/release/spwn"
PORT="${SPWN_PORT:-4317}"

# cargo is installed via rustup but is NOT on a fresh non-login shell's PATH here.
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found. Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y" >&2
  exit 1
fi

if [ ! -d node_modules ]; then
  echo "==> npm install (node_modules missing)"
  npm install
fi

echo "==> npm run build:app  (SPA build + sidecar bundle + release cargo build)"
npm run build:app

if [ ! -x "$BIN" ]; then
  echo "error: build reported success but $BIN is missing" >&2
  exit 1
fi

echo
echo "==> built: $ROOT/$BIN"
# Show the freshly-compiled binary's mtime so you can confirm it's THIS build.
stat -f '    %Sm  %N' "$BIN"

if [ "${1:-}" = "--open" ]; then
  echo "==> starting server for a smoke check on http://127.0.0.1:$PORT"
  "$ROOT/$BIN" serve --port "$PORT" &   # opens the browser itself (no --no-open)
  SRV=$!
  ok=0
  for _ in $(seq 1 50); do
    if curl -fsS "http://127.0.0.1:$PORT/api/version" >/dev/null 2>&1; then ok=1; break; fi
    kill -0 "$SRV" 2>/dev/null || break
    sleep 0.2
  done
  if [ "$ok" = 1 ]; then
    echo "    OK: server is serving ($(curl -fsS "http://127.0.0.1:$PORT/api/version"))"
    echo "    left running (pid $SRV) — open http://127.0.0.1:$PORT; Ctrl-C or kill $SRV to stop."
    wait "$SRV"
  else
    echo "    WARN: server did not answer" >&2
    kill "$SRV" 2>/dev/null || true
    exit 1
  fi
fi
