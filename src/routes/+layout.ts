// The Rust web server serves a static SPA (no SSR). With ssr off, prerendering
// writes index.html as an empty shell that boots the app in the browser, with
// relative asset paths so it runs under any path prefix.
// See: https://svelte.dev/docs/kit/single-page-apps
export const ssr = false;
export const prerender = true;
