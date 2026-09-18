---
title: Installation
description: Run spwn on your own machine, or as a server on a cluster.
---

spwn is a CLI that runs a web server and opens its UI in your browser. Run it on your
own machine, or deploy it to a cluster and open it from anywhere.

## Requirements

- **macOS or Linux.**
- The **`claude` CLI**. spwn runs it for you — it doesn't reimplement Claude Code, and
  nothing is re-uploaded or proxied. You can sign in from spwn's own setup screen the
  first time you open it.

## Install a release

Releases are published from a maintainer's machine, so the platforms available depend on
what was built for a given version — check the
[Releases page](https://github.com/spwn-gg/spwn/releases/latest).

1. Download `spwn-<version>-<os>-<arch>.tar.gz` and unpack it.
2. It contains two binaries, `spwn` and `rmux`. **Keep them in the same directory** — spwn
   looks for `rmux` next to itself first — and put that directory on your `PATH`.
3. Run `spwn`.

Your browser opens on the UI. `spwn serve --port 4317 --host 127.0.0.1 --no-open` gives
you the same server without the browser.

## Build from source

See [Building from Source](/spwn/reference/building/) — `npm run build:app` produces
`backend/target/release/spwn` with the UI embedded.

## Run it as a server

The published image carries spwn, `rmux`, `claude` and `git`:

```sh
docker run -p 4317:4317 -v spwn-home:/home/spwn ghcr.io/spwn-gg/spwn:latest
```

On Kubernetes, one command:

```sh
helm install spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn --create-namespace
```

See [Kubernetes](/spwn/deploy/kubernetes/) for ingress, storage and the rest.

:::caution
**spwn has no authentication.** Anyone who can reach its port can open shells and run
agents on that machine, read and change everything in its home directory, and use its
Claude login. Bind it to `127.0.0.1` (the default), or put an auth proxy in front of it.
:::

## First run

Open spwn and it walks you through the three things it needs: signing in to Claude, an
optional GitHub token for private repos, and your first repository. All of it happens in
the browser — there is nothing to set up on disk beforehand, on your laptop or in a pod.

If you already have code on the machine, skip the clone and use **＋ New Project** in the
sidebar to point spwn at a folder.

## Updates

There is no auto-updater. Download the new release and replace the binaries, or pull a
newer image and `helm upgrade`.

## Next

- [Quick Start](/spwn/getting-started/quick-start/) — create your first project.
