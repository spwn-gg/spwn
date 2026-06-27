#!/usr/bin/env bash
# Build the native macOS spwn.app on the host (NOT Docker).
#
# A macOS Tauri app links the system WKWebView + Apple SDK, so the bundle can
# only be produced on macOS. This script is the driver for the run-spwn
# skill: it puts cargo on PATH, builds the frontend + sidecar + Rust crate, and
# bundles the .app. Pass --open to launch the result for a smoke check.
#
# Usage:
#   .claude/skills/run-spwn/build-app.sh           # build only
#   .claude/skills/run-spwn/build-app.sh --open    # build, then launch
set -euo pipefail

# Repo root = three levels up from this script (.claude/skills/run-spwn/).
UNIT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$UNIT"

APP="src-tauri/target/release/bundle/macos/spwn.app"

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

# createUpdaterArtifacts is on, so the bundler signs the updater tarball — it needs
# the local signing key in the env (no password). Missing key => build fails with a
# clear tauri error; generate one with scripts/release.sh's instructions.
KEY="$HOME/.tauri/spwn.key"
if [ -f "$KEY" ]; then
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
else
  echo "warn: signing key $KEY not found — updater artifacts can't be signed" >&2
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
stat -f '    %Sm  %N' "$APP/Contents/MacOS/spwn"

if [ "${1:-}" = "--open" ]; then
  echo "==> launching (ad-hoc signed, not notarized; a locally built app runs fine — a"
  echo "    downloaded copy needs: xattr -dr com.apple.quarantine \"$APP\")"
  open "$APP"
  sleep 4
  if pgrep -f "$APP/Contents/MacOS/spwn" >/dev/null; then
    echo "    OK: app process is running"
  else
    echo "    WARN: app process not found after launch" >&2
  fi
fi
