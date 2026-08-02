#!/usr/bin/env bash
# Driver for the spwn CLI dev loop: manage the backend web server (cargo) and the
# Vite frontend as two background processes, bridged by Vite's /api + /ws proxy.
#
# Usage: dev.sh <command>
#   start              build + start backend, start frontend, wait until both are up
#   stop               stop both
#   restart            stop then start both
#   backend            (re)build + (re)start ONLY the backend  — after Rust edits
#   frontend           (re)start ONLY the frontend             — rarely needed (HMR)
#   status             show what's running + the URL to open
#   logs [backend|frontend|both]   print the tail of the logs (default: both)
#   check              svelte-check + cargo check
#   build              release build (npm run build:app)
#   open               print the dev URL (frontend, with HMR)
set -uo pipefail

# Repo root = three levels up from .claude/skills/spwn-dev/.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

# cargo is a rustup shim under ~/.cargo/bin, absent from a fresh non-login shell.
export PATH="$HOME/.cargo/bin:$PATH"

BACKEND_PORT="${SPWN_BACKEND_PORT:-4317}"
FRONTEND_PORT="${SPWN_FRONTEND_PORT:-1420}"
BACKEND_URL="http://127.0.0.1:${BACKEND_PORT}"
FRONTEND_URL="http://localhost:${FRONTEND_PORT}"

# Per-worktree run state (pids + logs), keyed by repo path so parallel worktrees
# don't collide.
HASH="$(printf '%s' "$ROOT" | cksum | cut -d' ' -f1)"
STATE="${TMPDIR:-/tmp}/spwn-dev/${HASH}"
mkdir -p "$STATE"
BE_PID="$STATE/backend.pid"; BE_LOG="$STATE/backend.log"; BE_PORTF="$STATE/backend.port"
FE_PID="$STATE/frontend.pid"; FE_LOG="$STATE/frontend.log"; FE_PORTF="$STATE/frontend.port"

# The port a running process was actually started on (falls back to the env/default).
be_port() { [ -f "$BE_PORTF" ] && running "$BE_PID" && cat "$BE_PORTF" || echo "$BACKEND_PORT"; }
fe_port() { [ -f "$FE_PORTF" ] && running "$FE_PID" && cat "$FE_PORTF" || echo "$FRONTEND_PORT"; }

running() { # pidfile -> 0 if the recorded pid is alive
  local pf="$1"
  [ -f "$pf" ] && kill -0 "$(cat "$pf" 2>/dev/null)" 2>/dev/null
}

port_pid() { # port -> pid of a foreign LISTEN on it (empty if free)
  lsof -nP -iTCP:"$1" -sTCP:LISTEN -t 2>/dev/null | head -1
}

# Abort if the target port is held by something that isn't this skill's own process.
guard_port() { # port pidfile label
  running "$2" && return 0
  local other; other="$(port_pid "$1")"
  if [ -n "$other" ]; then
    echo "→ port $1 is already in use (pid $other) by another process."
    echo "  Stop it, or set SPWN_${3}_PORT to a free port and retry."
    return 1
  fi
  return 0
}

stop_pid() { # pidfile -> SIGTERM the process (and its direct children)
  local pf="$1"
  [ -f "$pf" ] || return 0
  local pid; pid="$(cat "$pf" 2>/dev/null)"
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    pkill -P "$pid" 2>/dev/null || true
    kill "$pid" 2>/dev/null || true
  fi
  rm -f "$pf"
}

ensure_deps() {
  if [ ! -d node_modules ]; then
    echo "→ installing frontend deps (npm install)…"
    npm install || { echo "npm install failed"; exit 1; }
  fi
}

start_backend() {
  if running "$BE_PID"; then
    echo "→ backend already running (pid $(cat "$BE_PID"), http://127.0.0.1:$(be_port)/)"
    return 0
  fi
  guard_port "$BACKEND_PORT" "$BE_PID" BACKEND || return 1
  echo "→ building backend (cargo build --bin spwn)…"
  ( cd backend && cargo build --quiet --bin spwn ) || { echo "cargo build failed"; exit 1; }
  echo "→ starting backend on $BACKEND_URL"
  nohup "$ROOT/backend/target/debug/spwn" serve --no-open --port "$BACKEND_PORT" \
    >"$BE_LOG" 2>&1 &
  echo $! >"$BE_PID"; echo "$BACKEND_PORT" >"$BE_PORTF"
  # Wait for the HTTP surface to answer.
  for _ in $(seq 1 50); do
    if curl -fsS "$BACKEND_URL/api/version" >/dev/null 2>&1; then
      echo "  backend up: $(curl -fsS "$BACKEND_URL/api/version")"
      return 0
    fi
    running "$BE_PID" || { echo "  backend exited early — see: dev.sh logs backend"; return 1; }
    sleep 0.2
  done
  echo "  backend didn't answer in time — see: dev.sh logs backend"
  return 1
}

start_frontend() {
  ensure_deps
  if running "$FE_PID"; then
    echo "→ frontend already running (pid $(cat "$FE_PID"), http://localhost:$(fe_port)/)"
    return 0
  fi
  guard_port "$FRONTEND_PORT" "$FE_PID" FRONTEND || return 1
  # Proxy target follows the backend's actual running port.
  local be_url="http://127.0.0.1:$(be_port)"
  echo "→ starting frontend (Vite) on $FRONTEND_URL (proxying → $be_url)"
  # --strictPort: bind exactly $FRONTEND_PORT or fail (never silently drift, which
  # would make the proxy URL we print wrong).
  SPWN_BACKEND="$be_url" nohup node_modules/.bin/vite dev --port "$FRONTEND_PORT" --strictPort \
    >"$FE_LOG" 2>&1 &
  echo $! >"$FE_PID"; echo "$FRONTEND_PORT" >"$FE_PORTF"
  for _ in $(seq 1 50); do
    if curl -fsS -o /dev/null "$FRONTEND_URL" 2>/dev/null; then break; fi
    running "$FE_PID" || { echo "  frontend exited early — see: dev.sh logs frontend"; return 1; }
    sleep 0.2
  done
}

status() {
  local burl="http://127.0.0.1:$(be_port)" furl="http://localhost:$(fe_port)"
  if running "$BE_PID"; then echo "backend  : UP   (pid $(cat "$BE_PID")) $burl"
  else echo "backend  : down"; fi
  if running "$FE_PID"; then echo "frontend : UP   (pid $(cat "$FE_PID")) $furl"
  else echo "frontend : down"; fi
  echo
  if running "$FE_PID"; then
    echo "Open $furl in your browser (Vite HMR, proxies /api + /ws → $burl)."
  else
    echo "Frontend down — run: dev.sh start"
  fi
}

logs() {
  local which="${1:-both}"
  case "$which" in
    backend) tail -n 60 "$BE_LOG" 2>/dev/null || echo "(no backend log yet)";;
    frontend) tail -n 60 "$FE_LOG" 2>/dev/null || echo "(no frontend log yet)";;
    both|*)
      echo "===== backend ($BE_LOG) ====="; tail -n 40 "$BE_LOG" 2>/dev/null || echo "(none)"
      echo; echo "===== frontend ($FE_LOG) ====="; tail -n 40 "$FE_LOG" 2>/dev/null || echo "(none)"
      ;;
  esac
}

case "${1:-status}" in
  start)     start_backend && start_frontend; echo; status;;
  stop)      stop_pid "$FE_PID"; stop_pid "$BE_PID"; rm -f "$BE_PORTF" "$FE_PORTF"; echo "stopped.";;
  restart)   stop_pid "$FE_PID"; stop_pid "$BE_PID"; start_backend && start_frontend; echo; status;;
  backend|restart-backend)   stop_pid "$BE_PID"; rm -f "$BE_PORTF"; start_backend;;
  frontend|restart-frontend) stop_pid "$FE_PID"; rm -f "$FE_PORTF"; start_frontend;;
  status)    status;;
  logs)      logs "${2:-both}";;
  open)      echo "$FRONTEND_URL";;
  check)
    echo "→ npm run check"; npm run check || exit 1
    echo "→ cargo check"; ( cd backend && cargo check ) || exit 1
    ;;
  build)     npm run build:app;;
  *)
    echo "unknown command: ${1:-}"; echo
    sed -n '3,20p' "${BASH_SOURCE[0]}"
    exit 2;;
esac
