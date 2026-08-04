# Building spwn

For **installing** the app, see the [Installation guide](https://spwn-gg.github.io/spwn/getting-started/installation/).
This document is for **contributors** building spwn from source.

## Build (CLI + web server)

spwn is a single CLI binary that runs an HTTP + WebSocket web server and serves the SPA
in your browser — there is no desktop shell to bundle or sign. The built SPA is embedded
into the binary at compile time (via `rust-embed`, reading `./build`).

One-time, install Rust. Xcode Command Line Tools and Node are assumed present:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
npm install
npm run build:app        # build:all (SPA + sidecar) then cargo build --release
```

Output:

```
backend/target/release/spwn
```

Run it:

```sh
spwn                 # = `spwn serve`: binds 127.0.0.1:4317 and opens your browser
spwn serve --port 4317 --host 127.0.0.1 [--no-open]
```

### Dev loop

Two processes bridged by Vite's dev proxy (see `CLAUDE.md`):

```sh
npm run server       # cargo run -- serve --no-open   (backend on :4317)
npm run dev          # Vite on :1420 with HMR, proxies /api + /ws → backend
```

### Distribution (flat layout)

Since agents became TUIs in rmux panes, spwn talks to one local helper: **rmux**, which
backs every pane — shell and agent alike. Discovery prefers a binary next to the
executable, then the env override, then `$PATH`:

- `rmux` → `<exe dir>/rmux`, else `RMUX_SDK_DAEMON_BINARY`, else `rmux` on `$PATH`,
  else the usual install dirs (`/opt/homebrew/bin`, `/usr/local/bin`, `~/.cargo/bin`).

Its version is not free-floating: `rmux-sdk` accepts a single wire version, so the
daemon's minor has to track the dep in `backend/Cargo.toml` (0.6.x today).

Agent binaries (`claude`, …) are resolved separately, from the agent definitions in
`~/.spwn/agents`. So a release tarball is just `spwn` and `rmux` in one directory; a
Homebrew-style install (rmux as a formula dep on `$PATH`) needs no bundling at all.
Self-update is dropped — reinstall to upgrade.

## Development (Docker)

Development can run **inside Docker**. The container compiles the Rust backend and
SvelteKit frontend, runs tests, and can run the web server with its port published to
your host browser. The image ships a Linux `claude` and a version-pinned `rmux` (the
daemon behind every pane — its minor must match the `rmux-sdk` dep, since the SDK
accepts exactly one wire version).

```sh
make image   # build the dev image (Rust + Node + Linux claude + rmux)
make login   # ONE-TIME: authenticate the container's claude
make gui     # run `spwn serve`; then open http://localhost:4317 on the host
make fe      # npm install + build the SvelteKit frontend (produces ./build)
make build   # compile the Rust crate
make test    # frontend build + cargo test
make sh      # interactive shell in the container
make clean   # drop the cached volumes
```

The cargo registry, the Rust `target/` dir, and `node_modules` are cached in named
Docker volumes, so only the first build is slow and Linux artifacts never land in the
host tree.

### Home directory and auth in Docker

The compose file mounts your **real host home at the same path** and sets `HOME` to it,
so the container's `claude` reads the same `~/.claude.json` + `~/.claude/` you use on
the host, and every session's original working dir resolves at its real path (which is
what makes resume/branch work in the container at all). The consequence worth knowing:
the container runs as root with read-write access to everything under `$HOME`. On
Docker Desktop for Mac the file sharing layer maps writes back to your own uid, so
container-created files are not root-owned; on a Linux host with a plain bind mount
they would be.

The container's `claude` is a Linux build, so it may still need its own OAuth pass even
when the host is signed in — `make login` once, follow the printed URL. Those
credentials go into your **shared host** `~/.claude.json`, not a throwaway volume.

### Panes in Docker

The container starts its own rmux daemon, on a socket under the container's `$TMPDIR`
rather than `$HOME` — so it never collides with the rmux running on your Mac. It also
means panes die with the container: `make gui`/`make sh` use `--rm`, and pane
persistence across restarts is a property of host `spwn`, not of this image.

### Embedded frontend

The Rust crate embeds the built frontend at compile time (`rust-embed`, `#[folder =
"../build"]`), so every release cargo build must be preceded by a frontend build
(`npm run build` → `./build`). `npm run build:app` chains both.

## Building the docs

The documentation site lives in `docs/` and is built with
[Starlight](https://starlight.astro.build/):

```sh
cd docs
npm install
npm run dev      # local preview at http://localhost:4321/spwn
npm run build    # static output in docs/dist/
```

It deploys to GitHub Pages via `.github/workflows/docs.yml`.
