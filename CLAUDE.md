## Dev loop

- Do work on a feature/topic branch, not `main`.
- **Never merge into `main` (or push to it) without the user's explicit go-ahead.** Open a PR or ask first; wait for an explicit "yes" before merging.

## Local dev loop (CLI + web server)

spwn is a CLI that runs an HTTP + WebSocket web server and serves the SPA in the browser.
Dev is two processes bridged by Vite's proxy:

- **Backend:** `npm run server` (= `cargo run -- serve --no-open`, default `:4317`). Add
  `cargo watch -x 'run -- serve --no-open'` for auto-rebuild on Rust changes.
- **Frontend:** `npm run dev` — Vite on `localhost:1420` with HMR; it proxies `/api` and
  `/ws` to the backend (override the target with `SPWN_BACKEND`). Open `localhost:1420`.
- `npm run check` — svelte-check typecheck before committing.
- `npm run build:app` — build the SPA + sidecar and compile the release binary
  (`backend/target/release/spwn`, which embeds the SPA). Run it with `spwn` (or `spwn serve`).
- Restarting the backend kills legacy Claude sidecars, but **rmux panes persist** —
  shells and agent sessions both survive, and reattach with their processes intact.
- Agent definitions live in `backend/assets/agents/*.toml` (installed to
  `~/.spwn/agents`). Editing one there and hitting **Reload definitions** in Settings
  applies it without a rebuild — that's the intended way to track a TUI change.
- `backend/tests/agent_tui_spike.rs` (`make agent-spike`) drives a real `claude` in a
  real rmux pane and dumps annotated frames. Run it when a detect pattern or the
  rewind choreography stops matching; it is how those values were derived.
