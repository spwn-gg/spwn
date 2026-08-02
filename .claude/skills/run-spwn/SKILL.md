---
name: run-spwn
description: Build the release spwn CLI binary (SvelteKit SPA embedded) and optionally launch it (`spwn serve`) as a smoke check. Use when asked to build, rebuild, compile, package, produce the release binary, make a release build, or run/launch the app from a fresh build. For the iterative dev loop (hot-reload, restart backend), use the spwn-dev skill instead.
---

# Build the release spwn binary

spwn is a **CLI + web server** (Rust backend, axum HTTP + WebSocket) that serves a
SvelteKit/xterm.js SPA in the browser. The deliverable is a single binary with the SPA
embedded (`rust-embed`). The driver for this skill is **`build-app.sh`** — it builds the
frontend, esbuild-bundles the Claude Agent SDK sidecar, compiles the Rust crate in
release mode, and (with `--open`) starts the server and opens the browser as a smoke check.

Paths below are relative to the repo root.

## Prerequisites

- **Rust via rustup** — installed at `~/.cargo/bin` (a rustup shim). If missing:
  `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
- **Node + npm** — for the frontend build and the bundled sidecar.
- **`claude` CLI** — required at *runtime* (spwn shells out to your authenticated host
  `claude`); not needed to build.

## Build

```sh
.claude/skills/run-spwn/build-app.sh
```

It puts `~/.cargo/bin` on `PATH` (cargo is a rustup shim, absent from a fresh non-login
shell), runs `npm install` only if `node_modules` is missing, then runs `npm run build:app`
(SvelteKit build → esbuild sidecar bundle → release `cargo build`), verifies the binary
exists, and prints its mtime so you can confirm it's *this* build.

**Output:**

```
backend/target/release/spwn
```

Cold build is a few minutes (Rust release compile); warm rebuild is seconds.

### Build + launch smoke check

```sh
.claude/skills/run-spwn/build-app.sh --open
```

Adds: start `spwn serve` (which opens your browser), poll `GET /api/version`, and print
`OK: server is serving …` on success. It leaves the server running (prints the pid) so you
can use the app; Ctrl-C or `kill <pid>` to stop. Override the port with `SPWN_PORT`.

### Equivalent raw commands

```sh
export PATH="$HOME/.cargo/bin:$PATH"
npm install                 # only if node_modules is absent
npm run build:app           # SPA + sidecar + release binary
backend/target/release/spwn # = `spwn serve`: binds 127.0.0.1:4317 and opens the browser
```

## Test

```sh
cd backend && cargo test         # unit + integration tests
make test                        # Docker: SvelteKit build + cargo test (the live claude spike stays gated off)
```

## Gotchas

- **`cargo: command not found` in a fresh shell.** cargo is a rustup shim at
  `~/.cargo/bin`, which a non-login shell doesn't add to `PATH`. The driver exports it;
  raw, run `export PATH="$HOME/.cargo/bin:$PATH"` first.
- **`spwn UI not built` when the server serves `/`.** The SPA is embedded at compile time
  from `./build`, so `npm run build` must run before the release `cargo build`. The driver
  (`npm run build:app`) chains them; a bare `cargo build` embeds a stale/empty SPA.
- **`claude` still required at runtime.** The binary shells out to your own authenticated
  host `claude` CLI (path configurable in the app's Settings); the build doesn't provide it.
- **Port already in use.** `spwn serve` fails to bind if `:4317` is taken. Pass a free port
  (`SPWN_PORT=… build-app.sh --open`, or `spwn serve --port …`).
- **Distributing the binary.** For a shippable tarball (spwn + rmux + node + sidecar in a
  flat layout), use `scripts/release.sh`, not this skill.

## Troubleshooting

- **Build "succeeds" but the UI is blank / old** — the embedded SPA is stale. Re-run the
  driver so `npm run build` runs before the release `cargo build`.
- **Server starts but the chat sidecar never streams** — `node` isn't discoverable. spwn
  looks next to the binary, then `CM_NODE`, then `$PATH`. Ensure a `node` is reachable.
