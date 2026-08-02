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

spwn talks to two local helpers: **rmux** (shell terminals) and **node** running the
Claude Agent SDK **sidecar** (`resources/sidecar.mjs`, produced by `npm run build:sidecar`).
Discovery prefers paths next to the executable, then env overrides, then `$PATH`:

- `rmux`  → `<exe dir>/rmux`, else `RMUX_SDK_DAEMON_BINARY`, else `rmux` on `$PATH`.
- `node`  → `<exe dir>/node`, else `CM_NODE`, else `node` on `$PATH` / common dirs / nvm.
- sidecar → `CM_SIDECAR`, else `<exe dir>/sidecar.mjs` or `<exe dir>/resources/sidecar.mjs`,
  else the repo source in dev.

So a release tarball is just `spwn`, `rmux`, `node`, and `sidecar.mjs` in one directory;
a Homebrew-style install (rmux/node as formula deps on `$PATH`) needs no bundling at all.
Self-update is dropped — reinstall to upgrade.

## Development (Docker)

Development can run **inside Docker**. The container compiles the Rust backend and
SvelteKit frontend, runs tests, and can run the web server with its port published to
your host browser (the container's `claude` is a Linux build, authenticated separately).

```sh
make image   # build the dev image (Rust + Node + Linux claude)
make login   # ONE-TIME: authenticate the container's claude
make gui     # run `spwn serve`; then open http://localhost:4317 on the host
make fe      # npm install + build the SvelteKit frontend (produces ./build)
make build   # compile the Rust crate
make test    # frontend build + cargo test
make sh      # interactive shell in the container
make clean   # drop the cached volumes
```

Cargo registry, the Rust `target/` dir, `node_modules`, and the container's
`~/.claude` are cached in named Docker volumes, so only the first build is slow and
Linux artifacts never land in the host tree.

### Authentication in Docker

The container's `claude` authenticates **separately** from your host. Run
`make login` once and follow the printed OAuth URL. The token persists in the
`claude-config` Docker volume; the container's `~/.claude` is isolated from your host
`~/.claude`.

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
