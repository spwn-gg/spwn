## Dev loop

- Do work on a feature/topic branch, not `main`.
- **Never merge into `main` (or push to it) without the user's explicit go-ahead.** Open a PR or ask first; wait for an explicit "yes" before merging.

## Local dev loop (native macOS)

- `npm run tauri dev` — run the app natively with the frontend hot-reloading (Vite on `localhost:1420`). Primary dev loop.
- `npm run check` — svelte-check typecheck before committing.
- `npm run tauri build` — bundle the release `spwn.app` (output: `src-tauri/target/release/bundle/macos/spwn.app`).
- Docker (`make gui`, etc.) is the Linux/CI alternative; native macOS builds must run on the host. See `BUILD.md`.
