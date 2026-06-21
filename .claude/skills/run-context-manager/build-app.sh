#!/usr/bin/env bash
# Build the native macOS Context Manager.app on the host (NOT Docker).
#
# A macOS Tauri app links the system WKWebView + Apple SDK, so the bundle can
# only be produced on macOS. This script is the driver for the run-context-manager
# skill: it puts cargo on PATH, builds the frontend + sidecar + Rust crate, and
# bundles the .app. Pass --open to launch the result for a smoke check.
#
# Usage:
#   .claude/skills/run-context-manager/build-app.sh           # build only
#   .claude/skills/run-context-manager/build-app.sh --open    # build, then launch
set -euo pipefail

# Repo root = three levels up from this script (.claude/skills/run-context-manager/).
UNIT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$UNIT"

APP="src-tauri/target/release/bundle/macos/Context Manager.app"

# cargo is installed via rustup but is NOT on a fresh non-login shell's PATH here.
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found. Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y" >&2
  exit 1
fi

# The frontend build (beforeBuildCommand) needs node_modules; tauri does not install.
if [ ! -d node_modules ]; then
  echo "==> npm install (node_modules missing)"
  npm install
fi

echo "==> npm run tauri build  (frontend + sidecar + release cargo build + bundle)"
npm run tauri build

if [ ! -d "$APP" ]; then
  echo "error: build reported success but $APP is missing" >&2
  exit 1
fi

echo
echo "==> built: $UNIT/$APP"
# Show the freshly-compiled main binary's mtime so you can confirm it's THIS build,
# not a stale bundle (the node/rmux sidecars keep their older dates — expected).
stat -f '    %Sm  %N' "$APP/Contents/MacOS/context_manager"

if [ "${1:-}" = "--open" ]; then
  echo "==> launching (unsigned: if Gatekeeper blocks, run: xattr -dr com.apple.quarantine \"$APP\")"
  open "$APP"
  sleep 4
  if pgrep -f "$APP/Contents/MacOS/context_manager" >/dev/null; then
    echo "    OK: app process is running"
  else
    echo "    WARN: app process not found after launch" >&2
  fi
fi
