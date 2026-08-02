# Convenience targets — everything runs inside the Docker container.
#
# The Rust crate embeds the built frontend (rust-embed reads ./build), so a release
# build is preceded by a frontend build (`fe`). Frontend build is cheap.

# Use a non-login shell (`bash -c`): the rust image exposes cargo/rustc and the
# claude install via the image's PATH env, which a login shell (`-l`) would reset.
COMPOSE = docker compose
RUN     = $(COMPOSE) run --rm dev bash -c

.PHONY: image sh login gui fe build test spike agent-spike clean

## Build the dev image.
image:
	$(COMPOSE) build

## Open an interactive shell in the container.
sh:
	$(COMPOSE) run --rm dev bash

## One-time: authenticate the container's claude (persists in the claude-config
## volume). Follow the printed OAuth URL in your Mac browser, paste the code back.
login:
	$(COMPOSE) run --rm dev claude

## Run the web server in the container; open http://localhost:4317 on the host.
gui:
	$(COMPOSE) run --rm --service-ports gui

## Install deps and build the SvelteKit frontend (produces ./build).
fe:
	$(RUN) "npm install && npm run build"

## Compile the Rust crate (frontend first, so the embed picks it up).
build:
	$(RUN) "npm install && npm run build && cd backend && cargo build"

## Run the test suite. The live claude spike stays gated/skipped here.
test:
	$(RUN) "npm install && npm run build && cd backend && cargo test -- --nocapture"

## Run ONLY the gated M0 rewind/branch pty spike against a real, authed claude.
## Requires a one-time `make login` first (auth persists in the claude-config volume).
spike:
	$(COMPOSE) run --rm -e RUN_CLAUDE_PTY_SPIKE=1 dev bash -c \
		"npm install && npm run build && cd backend && cargo test --test rewind_branch_spike -- --nocapture --test-threads=1"

## Run ONLY the gated M0 agent-TUI spike: drives a real `claude` TUI through a real
## rmux pane and dumps annotated frames for every detect pattern the agent TOML needs.
## Runs on the HOST, not in Docker — it needs the host's rmux daemon and the host's
## authenticated claude. Makes real model calls.
agent-spike:
	cd backend && RUN_CLAUDE_PTY_SPIKE=1 cargo test --test agent_tui_spike -- --nocapture --test-threads=1

## Remove the cached volumes (cargo registry, target, node_modules).
clean:
	$(COMPOSE) down -v
