#!/usr/bin/env bash
# Run the spwn web server inside the container, bound to all interfaces so the host
# browser can reach it. Open http://localhost:4317 on your Mac once it's up.
set -euo pipefail

PORT="${SPWN_PORT:-4317}"

echo "[gui] ============================================================"
echo "[gui]  Open  http://localhost:${PORT}  in your Mac browser"
echo "[gui]  (first run compiles the Rust crate — the server starts after)"
echo "[gui] ============================================================"

cd /work
npm install
npm run build          # build the SPA so the server can embed/serve it
# --host 0.0.0.0: reachable from the host; --no-open: no browser inside the container.
exec cargo run --manifest-path backend/Cargo.toml -- serve --host 0.0.0.0 --port "$PORT" --no-open
