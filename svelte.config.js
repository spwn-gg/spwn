// The Rust web server serves a static SPA (no SSR), so we use adapter-static with an
// index.html fallback to put the site in SPA mode.
// See: https://svelte.dev/docs/kit/single-page-apps
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // The one route is prerendered as an empty shell (see src/routes/+layout.ts)
    // instead of emitted as an SPA fallback, because fallback pages always use
    // absolute asset paths. Prerendered pages use relative ones, so the same build
    // works at `/` and under any path prefix a reverse proxy strips.
    adapter: adapter(),
  },
};

export default config;
