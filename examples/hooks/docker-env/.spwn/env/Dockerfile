# Image for a per-session spwn environment.
#
# Two requirements, both load-bearing:
#   1. the agent CLI must be on PATH — that is what spwn's `{bin}` resolves against,
#      and the host's macOS binary cannot execute here;
#   2. `env`, `git` and a shell must exist (rules out distroless/scratch).
#
# Everything else is yours: pin the toolchain this project actually needs. That is the
# point of the feature — the session gets this environment instead of your laptop's.
FROM node:22-bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        git ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Claude Code, Linux build. Installs to /root/.local/bin/claude.
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/root/.local/bin:${PATH}"

# The session's worktree is bind-mounted at its real host path at run time, so there
# is deliberately no WORKDIR/COPY here — the image carries tools, never source.
CMD ["sleep", "infinity"]
