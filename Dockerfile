# Dev environment for spwn.
#
# Purpose: reproducible Rust/Node builds + tests, and running the web server headless
# with its port exposed to the host browser (the `gui` compose service). On Apple
# Silicon, Docker pulls the arm64 variants automatically.
#
# What this image can do:
#   * `cargo build` / `cargo test` the Rust crate
#   * `npm install` / `npm run build` the SvelteKit frontend
#   * run `spwn serve` and reach the UI at http://localhost:4317 from the host
#   * run the gated M0 pty risk-spike against a Linux `claude`
FROM rust:1-bookworm

# --- Build tooling ---
RUN apt-get update && apt-get install -y --no-install-recommends \
        libssl-dev \
        pkg-config \
        build-essential \
        curl wget file ca-certificates git xz-utils \
    && rm -rf /var/lib/apt/lists/*

# --- Node.js (SvelteKit frontend build + the Claude Agent SDK sidecar) ---
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# --- Claude Code (Linux native build) ---
# Installs to /root/.local/bin/claude. The container authenticates separately from
# the host (host tokens are not portable into Linux); auth persists in the
# `claude-config` volume after a one-time `make login`.
RUN (curl -fsSL https://claude.ai/install.sh | bash) || \
    echo "WARN: claude install skipped/failed."
ENV PATH="/root/.local/bin:${PATH}"

ENV CARGO_TERM_COLOR=always
WORKDIR /work
CMD ["bash"]
