# Dev environment for spwn.
#
# Purpose: reproducible Rust/Node builds + tests, AND running the Tauri GUI inside
# a virtual X display exposed to the host browser over noVNC (the `gui` compose
# service). On Apple Silicon, Docker pulls the arm64 variants automatically.
#
# What this image can do:
#   * `cargo build` / `cargo test` the Rust crate (Tauri Linux system deps below)
#   * `npm install` / `npm run build` the SvelteKit frontend
#   * run the windowed app headlessly via Xvfb + x11vnc + noVNC (see scripts/gui.sh)
#   * run the gated M0 pty risk-spike against a Linux `claude`
FROM rust:1-bookworm

# --- Tauri v2 Linux system dependencies (needed to compile the GUI crate) ---
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libxdo-dev \
        libssl-dev \
        pkg-config \
        build-essential \
        curl wget file ca-certificates git xz-utils \
    && rm -rf /var/lib/apt/lists/*

# --- Node.js (SvelteKit frontend build) ---
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# --- Virtual display + noVNC stack (for the `gui` service) ---
# Xvfb (headless X server), fluxbox (a minimal WM so the window maps full-size),
# x11vnc (shares the X display over VNC), and noVNC/websockify (serves VNC to the
# browser). mesa dri provides software GL since there is no GPU in the container.
RUN apt-get update && apt-get install -y --no-install-recommends \
        xvfb x11vnc fluxbox novnc websockify \
        libgl1-mesa-dri mesa-utils \
        dbus-x11 at-spi2-core \
        fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*

# --- Claude Code (Linux native build) ---
# Installs to /root/.local/bin/claude. The container authenticates separately from
# the host (host tokens are not portable into Linux); auth persists in the
# `claude-config` volume after a one-time `make login`.
RUN (curl -fsSL https://claude.ai/install.sh | bash) || \
    echo "WARN: claude install skipped/failed."
ENV PATH="/root/.local/bin:${PATH}"

# WebKitGTK under Xvfb/software rendering: disable the dmabuf renderer and GPU
# compositing (both fail without a real GPU and yield a blank window otherwise).
ENV WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    LIBGL_ALWAYS_SOFTWARE=1

ENV CARGO_TERM_COLOR=always
WORKDIR /work
CMD ["bash"]
