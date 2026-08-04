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
# Every pane (shell or agent TUI) is backed by rmux. Without the daemon binary the
# server still boots and the UI still loads — terminals just silently never open — so
# fail loudly here rather than leaving that to be discovered in the browser.
command -v rmux >/dev/null || {
  echo "[gui] ERROR: rmux is not on PATH; panes would not work. Rebuild: make image" >&2
  exit 1
}

npm install
npm run build          # build the SPA so the server can embed/serve it
# --host 0.0.0.0: reachable from the host; --no-open: no browser inside the container.
exec cargo run --manifest-path backend/Cargo.toml -- serve --host 0.0.0.0 --port "$PORT" --no-open
