# Context Manager

A desktop GUI for **composing context and seeding Claude Code sessions with it**,
organized into projects, with first-class terminal support.

**Context space:** each project has a composable context (notes + files + turns
picked from other sessions). **Inject** assembles it into a first message and
opens a new Claude session seeded with it (`▦` on a project → add blocks → Inject).

- **Projects** — each is a name + working directory that **groups terminals** you
  open. Context Manager owns them (persisted to `app_data_dir/projects.json`);
  they're not derived from Claude's own dirs.
- **Terminals** — open a **shell** (default) or a **Claude** session in a project.
  Each runs under a named, persistent rmux session and **reattaches across app
  restarts**.
- **Claude terminals = real TUI + chat mirror.** The actual `claude` TUI runs in an
  rmux pty (you type in it — full slash-commands, native tool prompts), with a live
  **chat mirror** panel rendered from the session JSONL alongside it. **Fork** spawns
  a native `--fork-session` pty (grouped under the lineage); **Rewind** opens Claude's
  native `/rewind` picker in the terminal. Shell terminals are the same pty, running a
  shell. (The Agent SDK sidecar from the earlier chat approach is retired/dormant.)

Built with **Tauri v2** (Rust backend), a **SvelteKit** (SPA) frontend, and
**xterm.js**. Sessions run under **[rmux](https://github.com/helvesec/rmux)** (a
programmable Rust terminal multiplexer) via the `rmux-sdk` crate — giving
persistent sessions that survive app restarts and structured snapshots/`wait_for_text`
for the rewind/branch automation. See
`/Users/mark/.claude/plans/i-want-to-create-unified-puppy.md` for the full plan.

## Status

- ✅ **M0** — scaffold, Docker dev env, and the rewind/branch PTY risk-spike harness.
- ✅ **M1** — live xterm.js terminal running `claude` via the Rust PTY manager
  (`src-tauri/src/pty/`, `src/lib/Terminal.svelte`).
- ✅ **M2** — navigation tree of real projects/sessions (`src-tauri/src/projects/`,
  `src/lib/ProjectTree.svelte`) + tabbed panes; click a session to resume it,
  ＋ to start a new one. Tree auto-refreshes via a filesystem watcher.
- ✅ **M3** — transcript panel (`src-tauri/src/transcript/`, `src/lib/TranscriptPanel.svelte`):
  parses a session's JSONL into its active conversation path and renders selectable
  turns with Fork/Branch/Rewind affordances (wired in M4–M6).
- ✅ **M4** — Fork: the ⑂ Fork button spawns `--resume <id> --fork-session` in a new
  tab; the backend discovers the new session id (`pty://session-id/<ptyId>`) so the
  tree and transcript follow the fork.
- ✅ **M5/M6** — Rewind/Branch from an arbitrary turn: the ↺ Rewind here button drives
  the live session's `/rewind` picker (`rewind_restore_to` in `pty/manager.rs`, using
  rmux snapshots to match the target checkpoint row) and selects "Restore conversation"
  — conversation forked, code unchanged. Needs a live-tuning pass against the real menu.

## Native macOS build (on the host)

A native, double-clickable macOS app must be built **on the host** (a Tauri macOS
app links the system WKWebView framework and Apple SDK, which can't be done from a
Linux container). One-time, install Rust; Xcode Command Line Tools and Node are
assumed present:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
npm install
npm run tauri build      # bundles the .app (see tauri.conf.json bundle.targets)
```

The build bundles its runtimes so the app is self-contained:
- **rmux** daemon (sidecar binary; `RMUX_SDK_DAEMON_BINARY` points at it),
- **node** runtime (sidecar binary), and
- the **Claude chat sidecar**, esbuild-bundled to a single `resources/sidecar.mjs`
  (no `node_modules` shipped) via `npm run build:sidecar`.

Binaries live in `src-tauri/binaries/<name>-<target-triple>`; the sidecar bundle is
produced by `beforeBuildCommand` (`npm run build:all`). The only remaining host
dependency is your own authenticated `claude` CLI (the Agent SDK shells out to it,
using your login). App size is ~134 MB (mostly the node runtime).

Output: `src-tauri/target/release/bundle/macos/Context Manager.app`. Run it with:

```sh
open "src-tauri/target/release/bundle/macos/Context Manager.app"
```

The native app uses your host's own `claude` (already authenticated) and real
`~/.claude/projects` directly — no Docker, no auth wiring. `bundle.targets` is set
to `["app"]`; the `.dmg` wrapper is skipped because its Finder/AppleScript step
needs an interactive GUI session.

## Development (Docker)

Everything runs **inside Docker**. The container compiles the Rust backend and
SvelteKit frontend, runs tests, and can also run the windowed GUI on a virtual X
display exposed to your browser via **noVNC** (a Tauri app built in the Linux
container is a Linux binary, so it's shown through noVNC rather than as a native
macOS window).

```sh
make image   # build the dev image (Rust + Node + Tauri deps + noVNC + Linux claude)
make login   # ONE-TIME: authenticate the container's claude (see Auth below)
make gui     # run the app; then open http://localhost:6080/vnc.html
make fe      # npm install + build the SvelteKit frontend (produces ./build)
make build   # compile the Tauri Rust crate
make test    # frontend build + cargo test (the live claude spike stays gated off)
make spike   # run ONLY the gated M0 rewind/branch live spike
make sh      # interactive shell in the container
make clean   # drop the cached volumes (cargo, target, node_modules, claude-config)
```

Cargo registry, the Rust `target/` dir, `node_modules`, and the container's
`~/.claude` are cached in named Docker volumes, so only the first build is slow
and Linux artifacts never land in the host tree.

## Authentication

The container's `claude` authenticates **separately** from your host (host tokens
are not portable into the Linux container). Run `make login` once:

```sh
make login
# follow the printed OAuth URL in your Mac browser, paste the code back
```

The token persists in the `claude-config` Docker volume, so the GUI (`make gui`)
and the spike (`make spike`) are authenticated on subsequent runs. The container's
`~/.claude` is **isolated from your host `~/.claude`** — container sessions never
touch your real Claude Code data. (Surfacing your real host projects in the
navigation tree is a separate, opt-in step planned for M2.)

### Note on `tauri::generate_context!`

The Rust crate embeds the built frontend at compile time, so every cargo step is
preceded by a frontend build (`npm run build` → `./build`). The Makefile targets
already chain this.

## The M0 risk spike (the project's riskiest assumption)

There is **no CLI** to branch from an *arbitrary past message* — `/rewind` is an
interactive checkpoint TUI. `src-tauri/tests/rewind_branch_spike.rs` drives a
**real, authenticated** `claude` over a pty, attempts `/rewind` + `/branch`, and
asserts that a new session JSONL appears with a **truncated** conversation chain.

It is **gated** and no-ops during normal builds. To run it (after `make login`):

```sh
make spike
```

This makes real model calls and requires a one-time `make login`. The spike dumps
every TUI frame to stdout. Its purpose is to **confirm the exact `/rewind`
keystroke choreography** on first live run; tighten
`navigate_rewind_to_earlier_checkpoint()` based on the captured frames.

## Always-on sanity test

`src-tauri/tests/pty_smoke.rs` verifies the `portable-pty` plumbing (open pty,
spawn a process, stream output) independent of `claude`. It runs in every
`make test`.
