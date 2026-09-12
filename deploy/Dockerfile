# Runtime image for running spwn as a server (e.g. on Kubernetes, see
# deploy/kubernetes). Not the dev environment: that is the root Dockerfile.
#
# Build from the repo root:
#   docker build -f deploy/Dockerfile -t spwn .
#
# The SPA uses relative asset paths and resolves its base in the browser, so one
# image serves at `/` or under any path prefix a reverse proxy strips.

# --- SPA ---
FROM node:22-bookworm AS web
WORKDIR /src
COPY package.json package-lock.json ./
RUN npm ci
COPY svelte.config.js vite.config.js tsconfig.json ./
COPY src src
COPY static static
RUN npm run build

# --- Backend (embeds ./build) + rmux ---
FROM rust:1-bookworm AS backend
WORKDIR /src
# Pinned: rmux-sdk accepts exactly one wire version, so the daemon's minor must track
# the `rmux-sdk` dep in backend/Cargo.toml. See the root Dockerfile.
RUN cargo install rmux --version 0.6.1 --locked
COPY backend backend
COPY --from=web /src/build build
RUN cargo build --release --manifest-path backend/Cargo.toml

# --- Runtime ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        bash ca-certificates curl git openssh-client less procps tini \
    && rm -rf /var/lib/apt/lists/*

# Claude Code, installed outside $HOME: deployments mount a volume over the home dir,
# which would hide anything the image put there.
RUN (curl -fsSL https://claude.ai/install.sh | bash) \
    && cp -L /root/.local/bin/claude /usr/local/bin/claude \
    && rm -rf /root/.local /root/.claude /root/.claude.json

COPY --from=backend /usr/local/cargo/bin/rmux /usr/local/bin/rmux
COPY --from=backend /src/backend/target/release/spwn /usr/local/bin/spwn

RUN useradd --uid 1000 --create-home --home-dir /home/spwn --shell /bin/bash spwn
USER spwn
# Shell panes run $SHELL, falling back to /bin/zsh, which this image doesn't have.
ENV HOME=/home/spwn SHELL=/bin/bash
WORKDIR /home/spwn

EXPOSE 4317
# tini reaps the shells and agents rmux leaves behind as they exit.
ENTRYPOINT ["tini", "--", "spwn"]
CMD ["serve", "--host", "0.0.0.0", "--no-open"]
