---
name: run-context-manager
description: Build the native macOS Context Manager.app locally (release bundle), and launch it. Use when asked to build, rebuild, bundle, package, make a release, produce the .app, or run/launch the desktop app.
---

# Build the Context Manager macOS .app

Context Manager is a **Tauri v2** desktop app (Rust backend + SvelteKit/xterm.js
frontend). The deliverable is a native `.app` bundle. The driver for this skill is
**`build-app.sh`** — it builds the frontend, esbuild-bundles the sidecar, compiles
the Rust crate in release mode, bundles the `.app`, and (with `--open`) launches it
as a smoke check.

**This must run on the macOS host, NOT in Docker.** A macOS Tauri app links the
system WKWebView and Apple SDK, which the Linux dev container cannot do. (`make
build` only does a Linux *debug* compile, and currently can't even bundle — only
the `-apple-darwin` sidecar binaries are committed, not Linux-arch ones.)

Paths below are relative to the repo root (`<unit>/`).

## Prerequisites

Host tools (all already present on the dev machine; versions this was built with):

- **Xcode Command Line Tools** — `xcode-select -p` → `/Library/Developer/CommandLineTools`
- **Rust via rustup** — `cargo 1.96.0`. Installed at `~/.cargo/bin` (a rustup shim).
  If missing: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
- **Node + npm** — `node v24.3.0`, `npm 11.4.2`

No `apt-get` — this is a macOS host build, not the container.

## Build (agent path)

Run the driver from the repo root:

```sh
.claude/skills/run-context-manager/build-app.sh
```

It handles the two things that trip people up automatically:
- puts `~/.cargo/bin` on `PATH` (cargo is a rustup shim, absent from a fresh
  non-login shell — otherwise you get `cargo: command not found`),
- runs `npm install` only if `node_modules` is missing.

Then it runs `npm run tauri build` (which chains `beforeBuildCommand` =
`npm run build:all` = SvelteKit build + esbuild sidecar bundle → release
`cargo build` → bundle), verifies the bundle exists, and prints the freshly-built
main binary's mtime so you can confirm it's *this* build, not a stale one.

**Output:**

```
src-tauri/target/release/bundle/macos/Context Manager.app
```

Cold build is a few minutes (Rust release compile); warm rebuild ~25s.

### Build + launch smoke check

```sh
.claude/skills/run-context-manager/build-app.sh --open
```

Adds: `open` the bundle, wait, and assert the app process is running. On success
prints `OK: app process is running`. The window opens on the host display; verify
it visually (the app shows a project tree / context composer — it reads your real
`~/.claude/projects`).

### Equivalent raw commands

What the driver runs, if you need to do it by hand:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
npm install                 # only needed if node_modules is absent
npm run tauri build
open "src-tauri/target/release/bundle/macos/Context Manager.app"
```

## Test

```sh
make test     # Docker: SvelteKit build + cargo test (the live claude spike stays gated off)
```

Note: `make test` / `make build` run in Docker and exercise the **Linux** crate,
not the macOS bundle. They're a code sanity check, not a `.app` build.

## Gotchas

- **`cargo: command not found` in a fresh shell.** cargo is a rustup shim at
  `~/.cargo/bin`, which a non-login shell doesn't add to `PATH`. The driver exports
  it; if running raw, `export PATH="$HOME/.cargo/bin:$PATH"` first.
- **Docker can't build the `.app`.** `make build` targets `aarch64-unknown-linux-gnu`
  and fails at resource bundling with `resource path binaries/rmux-aarch64-unknown-linux-gnu
  doesn't exist` — only `rmux-aarch64-apple-darwin` / `node-aarch64-apple-darwin` are
  committed. The native host build uses those `-apple-darwin` binaries and works.
- **Unsigned bundle → Gatekeeper.** First launch is blocked ("damaged"/"unverified
  developer"). Right-click → Open, or `xattr -dr com.apple.quarantine "src-tauri/target/release/bundle/macos/Context Manager.app"`.
- **No `.dmg`.** `bundle.targets` is `["app"]` only; the dmg wrapper's Finder/AppleScript
  step needs an interactive GUI session, so it's intentionally skipped.
- **Stale `.app` after a code change.** Nothing rebuilds the bundle automatically —
  editing Rust/Svelte and re-running the existing `.app` runs the OLD code. Re-run
  the driver. Confirm via the printed `context_manager` binary mtime (the bundled
  `node`/`rmux` sidecars keep older dates — that's expected, they're copied in
  as-is, not recompiled).
- **`claude` still required at runtime.** The bundled app shells out to your own
  authenticated host `claude` CLI (path set in the app's Settings); the build does
  not provide it.

## Troubleshooting

- **`failed to run custom build command for context_manager` →
  `resource path binaries/rmux-…-linux-gnu doesn't exist`** — you're building in
  Docker / for Linux. Build on the host instead (the driver does). The committed
  sidecars are macOS-only.
- **`Built application at … target/release/context_manager` but no `.app`** — the
  raw `cargo build` was run instead of `npm run tauri build`; only `tauri build`
  invokes the bundler. Use the driver.
- **Build "succeeds" but the app shows old behavior** — you launched a stale bundle.
  Re-run the driver and check the binary mtime it prints.
