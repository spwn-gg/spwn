# Building spwn

For **installing** the app, see the [Installation guide](https://spwn-gg.github.io/spwn/getting-started/installation/).
This document is for **contributors** building spwn from source.

## Native macOS build (on the host)

A native, double-clickable macOS app must be built **on the host** — a Tauri macOS
app links the system WKWebView framework and the Apple SDK, which can't be done from
a Linux container.

One-time, install Rust. Xcode Command Line Tools and Node are assumed present:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
npm install
npm run tauri build      # bundles the .app (see tauri.conf.json bundle.targets)
```

The build bundles the **rmux** daemon as a sidecar binary so sessions are
self-contained. The only host dependency at runtime is your own authenticated
`claude` CLI — the bundled app runs it using your login.

Output:

```
src-tauri/target/release/bundle/macos/spwn.app
```

Run it with:

```sh
open "src-tauri/target/release/bundle/macos/spwn.app"
```

`bundle.targets` is set to `["app"]`; the `.dmg` wrapper is skipped because its
Finder/AppleScript step needs an interactive GUI session. Builds are **ad-hoc
signed** (`signingIdentity: "-"`) but not notarized — which is why a downloaded copy
needs a one-time Gatekeeper approval (see the Installation guide).

## Development (Docker)

Everything for development runs **inside Docker**. The container compiles the Rust
backend and SvelteKit frontend, runs tests, and can run the windowed GUI on a
virtual X display exposed to your browser via **noVNC** (a Tauri app built in the
Linux container is a Linux binary, shown through noVNC rather than as a native macOS
window).

```sh
make image   # build the dev image (Rust + Node + Tauri deps + noVNC + Linux claude)
make login   # ONE-TIME: authenticate the container's claude
make gui     # run the app; then open http://localhost:6080/vnc.html
make fe      # npm install + build the SvelteKit frontend (produces ./build)
make build   # compile the Tauri Rust crate
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

### `tauri::generate_context!`

The Rust crate embeds the built frontend at compile time, so every cargo step is
preceded by a frontend build (`npm run build` → `./build`). The Makefile targets
already chain this.

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
