---
title: Installation
description: Install a spwn release on macOS, or build it from source.
---

spwn is a native macOS app. You can install a prebuilt release or
[build it from source](/spwn/reference/building/).

## Requirements

- **macOS** (Apple Silicon or Intel).
- An authenticated **`claude` CLI** on your `PATH`. spwn uses your existing Claude
  login — it doesn't handle Claude authentication itself, and nothing is re-uploaded
  or proxied.

## Install a release

1. Download the latest `spwn.app` from the
   [GitHub Releases](https://github.com/spwn-gg/spwn/releases) page.
2. Move it to `/Applications`.
3. Launch it (see the first-launch note below).

### First launch (Gatekeeper)

Releases are **ad-hoc signed but not notarized** by Apple, so a copy downloaded from
GitHub is quarantined by macOS and needs a one-time approval on first launch. The
in-app auto-updater is unaffected — updates it installs are never quarantined.

- **Recommended:** double-click the app, and when blocked open **System Settings →
  Privacy & Security** and click **Open Anyway**. On older macOS, right-click the
  app → **Open** → **Open**.
- **Or** clear the quarantine flag from a terminal:

  ```sh
  xattr -dr com.apple.quarantine "/Applications/spwn.app"
  ```

This is only required once per download.

## Updates

spwn ships with an in-app auto-updater. When a new release is available you'll see
an update banner; updates it installs are not quarantined, so no Gatekeeper prompt
is needed for them.

## Next

- [Quick Start](/spwn/getting-started/quick-start/) — create your first project.
