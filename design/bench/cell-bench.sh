#!/bin/sh
# Reproduces the measurements in design/001-cell-architecture.md §3.
# Usage: cell-bench.sh <path-to-a-git-repo> [N]
set -eu
SRC=${1:?usage: cell-bench.sh <repo> [N]}
N=${2:-50}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

git clone -q --no-hardlinks "$SRC" "$WORK/cell"
cd "$WORK/cell"
git config user.email bench@local && git config user.name bench
git checkout -q -B main "$(git rev-parse HEAD)"

# N session branches, one per-turn commit each.
i=1
while [ "$i" -le "$N" ]; do
  git checkout -q -B "spwn/s$i" main
  echo "session $i" > "sess$i.txt"
  git add -A >/dev/null && git commit -qm "turn $i"
  i=$((i + 1))
done
git checkout -q main

t() { _t0=$(date +%s%N); eval "$1"; _t1=$(date +%s%N); echo "  $(( (_t1 - _t0) / 1000000 )) ms"; }

echo "refs: $(git for-each-ref refs/heads | wc -l)"
echo "one session's overlap pass (capped at 12):"
t 'for b in $(seq 1 12); do git diff --name-only main...spwn/s$b >/dev/null; done'
echo "full cycle, N=$N, capped at 12:"
t 'for a in $(seq 1 '"$N"'); do for b in $(seq 1 12); do git diff --name-only main...spwn/s$b >/dev/null; done; done'
echo "full cycle, N=$N, uncapped (true O(N^2)):"
t 'for a in $(seq 1 '"$N"'); do for b in $(seq 1 '"$N"'); do git diff --name-only main...spwn/s$b >/dev/null; done; done'
echo "one shared index pass (N diffs):"
t 'for b in $(seq 1 '"$N"'); do git diff --name-only main...spwn/s$b >/dev/null; done'
echo "loose objects from $N per-turn commits:"
git count-objects -vH | grep -E "^count:|^size:"
