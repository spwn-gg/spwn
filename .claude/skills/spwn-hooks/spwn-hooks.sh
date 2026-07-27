#!/usr/bin/env bash
# Manage spwn project hooks — the `.spwn/hooks/<event>.sh` scripts spwn runs on session
# lifecycle events. Mirrors the discovery + env contract in src-tauri/src/hooks.rs so
# `test` reproduces exactly how spwn invokes a hook (worktree cwd, SPWN_* env, run
# directly if executable else via `sh`).
set -euo pipefail

# Keep in sync with hooks::EVENTS in src-tauri/src/hooks.rs.
EVENTS="session-created session-ready session-deleted"

repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || pwd
}

hooks_dir() {
  printf '%s/.spwn/hooks' "$(repo_root)"
}

hook_file() {
  printf '%s/%s.sh' "$(hooks_dir)" "$1"
}

valid_event() {
  case " $EVENTS " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

require_event() {
  if [ -z "${1:-}" ]; then
    echo "error: missing <event>. One of: $EVENTS" >&2
    exit 2
  fi
  if ! valid_event "$1"; then
    echo "error: unknown event '$1'. spwn only fires: $EVENTS" >&2
    exit 2
  fi
}

usage() {
  cat <<'EOF'
Manage spwn project hooks (.spwn/hooks/<event>.sh)

Usage: spwn-hooks.sh <command> [args]

Commands:
  list                 Show each lifecycle event and its hook (present? executable?)
  new <event>          Scaffold .spwn/hooks/<event>.sh (executable) with a template
  path <event>         Print the hook's path (creating it from the template if missing)
  test <event>         Run the hook exactly as spwn would (worktree cwd + SPWN_* env)
  rm <event>           Remove .spwn/hooks/<event>.sh

Events: session-created  session-ready  session-deleted
EOF
}

scaffold() {
  event="$1"
  f="$(hook_file "$event")"
  if [ -e "$f" ]; then
    return 0
  fi
  mkdir -p "$(dirname "$f")"
  cat > "$f" <<EOF
#!/usr/bin/env bash
# spwn '$event' hook. Runs synchronously with the worktree as the working directory.
# Env: SPWN_EVENT SPWN_TERMINAL_ID SPWN_PROJECT_DIR SPWN_WORKTREE SPWN_BRANCH \\
#      SPWN_BASE_BRANCH SPWN_SESSION_ID  (SPWN_SESSION_ID is unset on session-created)
# It's a plain script — orchestrate other files/code as you like. Background any
# long-running work yourself so the session doesn't wait, e.g.: my-server & disown
set -euo pipefail

echo "[\$SPWN_EVENT] \${SPWN_BRANCH:-<no-branch>} in \$SPWN_WORKTREE"

# Ask the user (blocks on the answer; prints the chosen label). Exit 0=answered,
# 2=declined/no-UI, 3=error. Always handle the non-zero branch (headless runs decline).
#   if [ "\$("\$SPWN_BIN" prompt 'Seed the database?')" = Yes ]; then ./scripts/seed.sh; fi

# TODO: your setup/teardown here.
EOF
  chmod +x "$f"
}

cmd_list() {
  dir="$(hooks_dir)"
  echo "hooks dir: $dir"
  for e in $EVENTS; do
    f="$dir/$e.sh"
    if [ -f "$f" ]; then
      if [ -x "$f" ]; then run="executable"; else run="via sh (not +x)"; fi
      printf '  %-16s present   (%s)\n' "$e" "$run"
    else
      printf '  %-16s —\n' "$e"
    fi
  done
}

cmd_new() {
  require_event "${1:-}"
  f="$(hook_file "$1")"
  existed=0; [ -e "$f" ] && existed=1
  scaffold "$1"
  if [ "$existed" = 1 ]; then
    echo "exists: $f (left unchanged)"
  else
    echo "created: $f"
  fi
  echo "next: edit it, run 'spwn-hooks.sh test $1', then commit it."
}

cmd_path() {
  require_event "${1:-}"
  scaffold "$1"
  hook_file "$1"
}

cmd_rm() {
  require_event "${1:-}"
  f="$(hook_file "$1")"
  if [ -e "$f" ]; then
    rm -f "$f"
    echo "removed: $f"
  else
    echo "no hook to remove: $f"
  fi
}

cmd_test() {
  require_event "${1:-}"
  event="$1"
  f="$(hook_file "$event")"
  if [ ! -f "$f" ]; then
    echo "error: no hook at $f (create one with: spwn-hooks.sh new $event)" >&2
    exit 1
  fi
  root="$(repo_root)"

  # Reproduce spwn's injected environment (see hooks::run_one). In a plain checkout the
  # worktree and project dir are the same; a real session's worktree differs.
  export SPWN_EVENT="$event"
  export SPWN_TERMINAL_ID="test-terminal"
  export SPWN_PROJECT_DIR="$root"
  export SPWN_WORKTREE="$root"
  export SPWN_BRANCH="$(git -C "$root" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
  export SPWN_BASE_BRANCH=""
  # SPWN_SESSION_ID is set for ready/deleted, absent on created (matches the real fire).
  case "$event" in
    session-created) unset SPWN_SESSION_ID 2>/dev/null || true ;;
    *) export SPWN_SESSION_ID="test-session" ;;
  esac

  # No UI under `test`, so stub SPWN_BIN: `spwn prompt …` auto-answers with the first
  # option (mirrors a user always picking the first choice). Cleaned up on exit.
  stub="$(mktemp "${TMPDIR:-/tmp}/spwn-prompt-stub.XXXXXX")"
  cat > "$stub" <<'STUB'
#!/bin/sh
# test stub for `spwn prompt [--multi] [--header H] "Question" [option ...]`:
# echo the first option (or "Yes" for a bare confirm) and exit 0.
[ "$1" = prompt ] || exit 0
shift
q_seen=0; first_opt=
while [ $# -gt 0 ]; do
  case "$1" in
    --multi) : ;;
    --header) shift ;;
    --header=*) : ;;
    *) if [ "$q_seen" = 0 ]; then q_seen=1; elif [ -z "$first_opt" ]; then first_opt="$1"; fi ;;
  esac
  shift
done
[ -n "$first_opt" ] || first_opt=Yes
printf '%s\n' "$first_opt"
STUB
  chmod +x "$stub"
  export SPWN_BIN="$stub"
  trap 'rm -f "$stub"' EXIT

  echo ">> running $f as spwn would (cwd=$root)"
  echo "---"
  # Run directly if executable, else via sh — exactly like hooks::run_one.
  if [ -x "$f" ]; then
    ( cd "$root" && "$f" ); rc=$?
  else
    ( cd "$root" && sh "$f" ); rc=$?
  fi
  echo "---"
  echo ">> exit $rc"
  return "$rc"
}

main() {
  cmd="${1:-}"
  shift || true
  case "$cmd" in
    list) cmd_list ;;
    new) cmd_new "${1:-}" ;;
    path|edit) cmd_path "${1:-}" ;;
    test) cmd_test "${1:-}" ;;
    rm|remove|delete) cmd_rm "${1:-}" ;;
    ""|-h|--help|help) usage ;;
    *) echo "error: unknown command '$cmd'" >&2; echo >&2; usage >&2; exit 2 ;;
  esac
}

main "$@"
