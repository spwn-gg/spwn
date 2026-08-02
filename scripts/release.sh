#!/usr/bin/env bash
# Cut a release of the spwn CLI for GitHub Releases.
#
# Builds the release binary (with the SPA embedded) and packages a flat tarball
# containing everything spwn needs at runtime — the `spwn` binary plus its `rmux`,
# `node`, and `sidecar.mjs` helpers laid out the way the binary discovers them
# (next to the executable). With the `gh` CLI present it also creates the GitHub
# release and uploads the tarball.
#
# There is no self-updater and no code signing — users reinstall to upgrade.
#
# Usage:
#   scripts/release.sh                 # version from backend/Cargo.toml
#   scripts/release.sh --notes "..."   # set release notes
#   NOTES_FILE=notes.md scripts/release.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

OUT="$ROOT/release"

NOTES="See the GitHub release for details."
if [ "${1:-}" = "--notes" ] && [ -n "${2:-}" ]; then NOTES="$2"; fi
if [ -n "${NOTES_FILE:-}" ] && [ -f "$NOTES_FILE" ]; then NOTES="$(cat "$NOTES_FILE")"; fi

# --- version + platform ----------------------------------------------------
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' backend/Cargo.toml | head -1)"
[ -n "$VERSION" ] || { echo "error: could not read version from backend/Cargo.toml" >&2; exit 1; }
TAG="v$VERSION"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"   # darwin
ARCH="$(uname -m)"                              # arm64
PLATFORM="${OS}-${ARCH}"
NAME="spwn-${VERSION}-${PLATFORM}"

# Derive owner/repo from the git remote (single source of truth for URLs).
REMOTE="$(git remote get-url origin 2>/dev/null || true)"
if [ -z "$REMOTE" ]; then
  echo "error: no 'origin' git remote. Add one:" >&2
  echo "  git remote add origin git@github.com:<owner>/<repo>.git" >&2
  exit 1
fi
SLUG="$(echo "$REMOTE" | sed -E 's#^.*github\.com[:/]##; s#\.git$##')"
echo "==> releasing $TAG ($PLATFORM) to github.com/$SLUG"

# --- build -----------------------------------------------------------------
[ -d node_modules ] || npm install
echo "==> npm run build:app (SPA + sidecar + release binary)"
npm run build:app

BIN="backend/target/release/spwn"
SIDECAR="backend/resources/sidecar.mjs"
[ -x "$BIN" ] || { echo "error: $BIN not found — did the build fail?" >&2; exit 1; }
[ -f "$SIDECAR" ] || { echo "error: $SIDECAR not found — run npm run build:sidecar" >&2; exit 1; }

# Runtime helpers: prefer committed bundled binaries, else fall back to PATH.
RMUX="$(ls backend/binaries/rmux 2>/dev/null || command -v rmux || true)"
NODE="$(ls backend/binaries/node 2>/dev/null || command -v node || true)"
[ -n "$RMUX" ] || { echo "error: rmux not found (bundle it in backend/binaries/ or install on PATH)" >&2; exit 1; }
[ -n "$NODE" ] || { echo "error: node not found on PATH" >&2; exit 1; }

# --- stage the flat layout the binary discovers (next to the executable) ---
rm -rf "$OUT"; mkdir -p "$OUT/$NAME/resources"
cp "$BIN"     "$OUT/$NAME/spwn"
cp "$RMUX"    "$OUT/$NAME/rmux"
cp "$NODE"    "$OUT/$NAME/node"
cp "$SIDECAR" "$OUT/$NAME/resources/sidecar.mjs"
chmod +x "$OUT/$NAME/spwn" "$OUT/$NAME/rmux" "$OUT/$NAME/node"

TARBALL="$OUT/${NAME}.tar.gz"
( cd "$OUT" && tar czf "${NAME}.tar.gz" "$NAME" )
rm -rf "$OUT/$NAME"
echo "==> packaged $TARBALL"
ls -1 "$OUT"

# --- publish ---------------------------------------------------------------
if command -v gh >/dev/null 2>&1; then
  echo "==> creating GitHub release $TAG"
  if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "$TARBALL" --clobber
  else
    gh release create "$TAG" "$TARBALL" --title "$TAG" --notes "$NOTES"
  fi
  echo "==> published: https://github.com/$SLUG/releases/tag/$TAG"
else
  echo
  echo "gh CLI not found — upload manually:"
  echo "  1. Create a release tagged $TAG at https://github.com/$SLUG/releases/new"
  echo "  2. Attach: $TARBALL"
  echo "  (or install gh:  brew install gh  then re-run)"
fi
