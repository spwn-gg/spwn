#!/usr/bin/env bash
# Cut a self-update release of spwn.app for GitHub Releases.
#
# Produces a Tauri updater bundle (`.app.tar.gz`) signed with the local minisign
# key, plus the `latest.json` manifest the in-app updater fetches. With the `gh`
# CLI present it also creates the GitHub release and uploads the assets.
#
# Prerequisites (one-time):
#   - Signing key at ~/.tauri/spwn.key (generated with `tauri signer
#     generate`). Its public key is baked into src-tauri/tauri.conf.json.
#   - A GitHub remote: `git remote add origin git@github.com:<owner>/<repo>.git`.
#     The release URLs are derived from it.
#
# Usage:
#   scripts/release.sh                 # version from tauri.conf.json
#   scripts/release.sh --notes "..."   # set release notes
#   NOTES_FILE=notes.md scripts/release.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

KEY="$HOME/.tauri/spwn.key"
CONF="src-tauri/tauri.conf.json"
APP_DIR="src-tauri/target/release/bundle/macos"
OUT="$ROOT/release"

NOTES="See the GitHub release for details."
if [ "${1:-}" = "--notes" ] && [ -n "${2:-}" ]; then NOTES="$2"; fi
if [ -n "${NOTES_FILE:-}" ] && [ -f "$NOTES_FILE" ]; then NOTES="$(cat "$NOTES_FILE")"; fi

# --- preconditions ---------------------------------------------------------
if [ ! -f "$KEY" ]; then
  echo "error: signing key not found at $KEY" >&2
  echo "  generate it: npx @tauri-apps/cli signer generate --ci -p '' -w $KEY" >&2
  exit 1
fi

VERSION="$(node -p "require('./$CONF').version")"
[ -n "$VERSION" ] || { echo "error: could not read version from $CONF" >&2; exit 1; }
TAG="v$VERSION"

# Derive owner/repo from the git remote (single source of truth for URLs).
REMOTE="$(git remote get-url origin 2>/dev/null || true)"
if [ -z "$REMOTE" ]; then
  echo "error: no 'origin' git remote. Add one:" >&2
  echo "  git remote add origin git@github.com:<owner>/<repo>.git" >&2
  exit 1
fi
# Normalise git@github.com:owner/repo.git OR https://github.com/owner/repo(.git)
SLUG="$(echo "$REMOTE" | sed -E 's#^.*github\.com[:/]##; s#\.git$##')"
echo "==> releasing $TAG to github.com/$SLUG"

# Keep the baked updater endpoint in sync with the actual repo.
ENDPOINT="https://github.com/$SLUG/releases/latest/download/latest.json"
node -e "
  const fs='fs', f='$CONF';
  const c=require('./'+f);
  c.plugins=c.plugins||{}; c.plugins.updater=c.plugins.updater||{};
  c.plugins.updater.endpoints=['$ENDPOINT'];
  require(fs).writeFileSync(f, JSON.stringify(c,null,2)+'\n');
"

# --- build (signed updater artifacts) --------------------------------------
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

[ -d node_modules ] || npm install
echo "==> npm run tauri build (signed)"
npm run tauri build

# --- collect artifacts -----------------------------------------------------
TARBALL="$(ls "$APP_DIR"/*.app.tar.gz 2>/dev/null | head -1 || true)"
SIG="$(ls "$APP_DIR"/*.app.tar.gz.sig 2>/dev/null | head -1 || true)"
if [ -z "$TARBALL" ] || [ -z "$SIG" ]; then
  echo "error: updater artifacts not found in $APP_DIR" >&2
  echo "  (is bundle.createUpdaterArtifacts true and the signing key valid?)" >&2
  exit 1
fi

rm -rf "$OUT"; mkdir -p "$OUT"
# GitHub turns spaces in asset names into dots; use a space-free name so the
# URL in latest.json is unambiguous.
ASSET="spwn.app.tar.gz"
cp "$TARBALL" "$OUT/$ASSET"
cp "$SIG" "$OUT/$ASSET.sig"

ASSET_URL="https://github.com/$SLUG/releases/download/$TAG/$ASSET"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SIGNATURE="$(cat "$OUT/$ASSET.sig")"

# --- latest.json (static endpoint the updater reads) -----------------------
SIGNATURE="$SIGNATURE" URL="$ASSET_URL" VERSION="$VERSION" \
NOTES="$NOTES" PUB_DATE="$PUB_DATE" node -e '
  const m = {
    version: process.env.VERSION,
    notes: process.env.NOTES,
    pub_date: process.env.PUB_DATE,
    platforms: {
      "darwin-aarch64": { signature: process.env.SIGNATURE, url: process.env.URL }
    }
  };
  require("fs").writeFileSync(process.env.OUT_JSON, JSON.stringify(m, null, 2) + "\n");
' OUT_JSON="$OUT/latest.json"

echo "==> artifacts in $OUT:"
ls -1 "$OUT"

# --- publish ---------------------------------------------------------------
if command -v gh >/dev/null 2>&1; then
  echo "==> creating GitHub release $TAG"
  if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "$OUT/$ASSET" "$OUT/$ASSET.sig" "$OUT/latest.json" --clobber
  else
    gh release create "$TAG" "$OUT/$ASSET" "$OUT/$ASSET.sig" "$OUT/latest.json" \
      --title "$TAG" --notes "$NOTES"
  fi
  echo "==> published: https://github.com/$SLUG/releases/tag/$TAG"
else
  echo
  echo "gh CLI not found — upload manually:"
  echo "  1. Create a release tagged $TAG at https://github.com/$SLUG/releases/new"
  echo "  2. Attach: $OUT/$ASSET, $OUT/$ASSET.sig, $OUT/latest.json"
  echo "  (or install gh:  brew install gh  then re-run)"
fi
