import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// The Rust web server (run `cargo run -- serve --no-open`, default :4317). In dev the
// SPA is served by Vite (:1420) with HMR and proxies /api + /ws to the backend; in
// production the backend embeds and serves the built SPA itself (no proxy).
// @ts-expect-error process is a nodejs global
const backend = process.env.SPWN_BACKEND || "http://127.0.0.1:4317";

// https://vite.dev/config/
export default defineConfig({
  plugins: [sveltekit()],
  clearScreen: false,
  server: {
    port: 1420,
    proxy: {
      "/api": backend,
      "/ws": { target: backend.replace(/^http/, "ws"), ws: true },
    },
    watch: {
      ignored: ["**/backend/**"],
    },
  },
});
