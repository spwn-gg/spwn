#!/usr/bin/env bash
# Launch the Tauri GUI inside the container on a virtual X display, exposed to the
# host browser via noVNC. Open http://localhost:6080/vnc.html once it's up.
set -euo pipefail

export DISPLAY=:99
SCREEN_GEOMETRY="${SCREEN_GEOMETRY:-1600x1000x24}"

echo "[gui] starting Xvfb on $DISPLAY ($SCREEN_GEOMETRY)"
Xvfb "$DISPLAY" -screen 0 "$SCREEN_GEOMETRY" -ac +extension GLX +render -noreset \
    >/tmp/xvfb.log 2>&1 &
# Wait for the X server socket to come up.
for _ in $(seq 1 50); do
    [ -e /tmp/.X11-unix/X99 ] && break
    sleep 0.1
done

echo "[gui] starting dbus session"
eval "$(dbus-launch --sh-syntax)"

echo "[gui] starting fluxbox window manager"
fluxbox >/tmp/fluxbox.log 2>&1 &

echo "[gui] starting x11vnc on :5900"
x11vnc -display "$DISPLAY" -nopw -forever -shared -rfbport 5900 -quiet \
    >/tmp/x11vnc.log 2>&1 &

echo "[gui] starting noVNC/websockify on :6080"
websockify --web=/usr/share/novnc 6080 localhost:5900 \
    >/tmp/novnc.log 2>&1 &

echo "[gui] ============================================================"
echo "[gui]  Open  http://localhost:6080/vnc.html  in your Mac browser"
echo "[gui]  (first run compiles the Rust app — the window appears after)"
echo "[gui] ============================================================"

cd /work
npm install
# tauri dev: starts the Vite dev server (beforeDevCommand) then builds & runs the
# app, whose window maps onto the Xvfb display that noVNC is sharing.
exec npm run tauri dev
