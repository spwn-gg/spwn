# spwn docs

The documentation site for [spwn](https://github.com/spwn-gg/spwn), built with
[Starlight](https://starlight.astro.build/) (Astro). It deploys to GitHub Pages at
<https://spwn-gg.github.io/spwn/> via `.github/workflows/docs.yml`.

## Develop

```sh
npm install
npm run dev      # local preview at http://localhost:4321/spwn
npm run build    # static output in ./dist
npm run preview  # serve the built site locally
```

## Structure

```
src/
├── assets/screenshots/   # app screenshots (see "Screenshots" below)
├── content/docs/         # the pages (Markdown / MDX), one route per file
│   ├── getting-started/
│   ├── guides/
│   └── reference/
└── content.config.ts
astro.config.mjs          # title, sidebar, site/base (base = /spwn)
```

Pages live in `src/content/docs/`. Sidebar and site config are in
`astro.config.mjs`.

## Screenshots

`src/assets/screenshots/` currently holds **labeled placeholders**. Replace each
with a real screenshot of the app, keeping the same filename — the pages reference
them by name and Astro re-optimizes on the next build. To capture a window cleanly
on macOS: **Shift+Cmd+4**, then **Space**, then click the window.

| File | Shows |
| :--- | :--- |
| `app-main.png` | The whole app (used on the home page hero) |
| `project-tree.png` | The project / session tree |
| `claude-session.png` | A Claude session: real TUI + chat mirror |
| `context-composer.png` | The context space with blocks before Inject |
| `fork-rewind.png` | Fork / Rewind affordances on a session |
| `settings.png` | The Settings panel |
