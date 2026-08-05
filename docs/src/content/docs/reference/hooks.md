---
title: Hooks
description: Run shell scripts on session lifecycle events — from a shared global folder and per repo, as a single script or a folder of numbered scripts, with the session's details in the environment.
---

Each Claude session works on [its own branch in its own worktree](/spwn/guides/branches-and-merging/).
**Hooks** run your shell scripts when a session's lifecycle changes — to set up (start a
dev server, seed a database, install extras) and tear down per session.

spwn just discovers scripts and runs them with the session's details in the environment.
It has no opinion about what they do — Docker, plain shell, anything. In fact spwn's own
built-in behavior — creating the session's worktree, committing each turn, taking
checkpoints — ships as **default hooks** you can read, edit, or replace (see
[The built-in default hooks](#the-built-in-default-hooks)).

## Where hooks live

Hooks are discovered in two places, and both run for a session — **global first, then
repo**:

| Scope | Location | Applies to |
|-------|----------|------------|
| **Global** | `~/.spwn/hooks/` | Every session in every project (shared across all your projects). |
| **Repo** | `<repo>/.spwn/hooks/` | Just that repo. Because a session works in a checkout of the repo, a **committed** repo hook travels into every session automatically. |

Within either location, an event can be **a single script or a folder of scripts**:

```
~/.spwn/hooks/                 # global (shared)
  session-created.sh           #   a single script, OR…
  session-created.d/           #   …a folder of scripts, run in filename order
    10-worktree.sh
    50-my-setup.sh

<repo>/.spwn/hooks/            # repo (committed)
  session-ready.sh
```

For an event, spwn runs — in order — the bare `<event>.sh` first (if present), then every
script in `<event>.d/` **sorted by filename**. Numeric prefixes (`10-`, `20-`, …) are the
convention for ordering independent steps. This is done for the global folder first, then
the repo folder, so the full run order for one event is:

```
~/.spwn/hooks/<event>.sh
~/.spwn/hooks/<event>.d/*        (sorted)
<repo>/.spwn/hooks/<event>.sh
<repo>/.spwn/hooks/<event>.d/*   (sorted)
```

A script runs directly if it's executable (`chmod +x`, honoring its shebang), otherwise
via `sh`. Inside an `<event>.d/` folder, files that aren't executable and don't end in
`.sh` (a `README`, `notes.txt`) are ignored, so you can keep helpers there too.

This is entirely **opt-in**: no scripts, nothing happens. The whole feature activates only
for sessions that have their own worktree.

## Add a hook

The simplest hook is a single script for one event. To run something for **every**
session, drop it in the global folder:

```sh
#!/usr/bin/env bash
# ~/.spwn/hooks/session-created.sh
set -euo pipefail
echo "Setting up $SPWN_BRANCH in $SPWN_WORKTREE"
```

To run something only for one repo, commit it under that repo — it travels into every
session via the git checkout:

```sh
#!/usr/bin/env bash
# <repo>/.spwn/hooks/session-created.sh
set -euo pipefail
npm install
```

To compose several independent steps, use a folder and number the files:

```
~/.spwn/hooks/session-created.d/
  20-copy-secrets.sh
  30-seed-db.sh
```

A folder is the better choice when you want to **add** to spwn's shipped defaults without
editing them — see below.

## The built-in default hooks

spwn's built-in per-session behavior isn't hardcoded — it ships as default **global**
hooks that spwn writes into `~/.spwn/hooks/` the first time it runs:

| File | What it does |
|------|--------------|
| `session-created.d/10-worktree.sh` | Creates the session's git worktree and COW-seeds heavy build dirs (`node_modules`, `target`, …). Reports the worktree back to spwn (see [Reporting values back](#reporting-values-back-to-spwn)). |
| `session-deleted.d/90-worktree.sh` | Removes the worktree and deletes its branch. |
| `session-turn.d/10-commit.sh` | Commits the turn's changes onto the session branch. |
| `session-turn.d/20-checkpoint.sh` | Snapshots a checkpoint for [rewind/undo](/spwn/guides/fork-and-rewind/). |

Because these are just scripts in a folder, you can:

- **Add alongside them** — drop your own `50-*.sh` in the same `<event>.d/` folder. spwn
  runs it in order and never touches it.
- **Replace one** — edit or delete it. (spwn *owns* the numbered files above and may
  update them on a new version, so prefer adding your own file to editing spwn's. If you
  delete one, spwn won't recreate it.)
- **Turn them all off** — see [enabling/disabling global hooks](/spwn/reference/settings/#global-hooks).

:::caution[Worktrees come from a hook]
Creating and removing a session's worktree is done by the default `10-worktree.sh` /
`90-worktree.sh` scripts. If you disable global hooks or delete those files, **new
sessions no longer get an isolated worktree or branch** — they run in the project folder,
and per-session isolation, per-turn commits, and checkpoints don't apply. That's the
intended "opt all the way out" behavior; just know it's a package deal.
:::

## Events

| Event | When it fires | Working directory |
|-------|---------------|-------------------|
| `session-created` | When a session starts. | The **project dir** for global scripts (the worktree doesn't exist yet — this is where it gets created); the **worktree** for repo scripts (which run after it exists). |
| `session-ready` | The first time the Claude session id is known (after the sidecar starts). | The worktree. |
| `session-turn` | After each completed Claude turn. | The worktree. |
| `session-deleted` | On delete — deleting a session, or deleting the project that contains it (which deletes each of its sessions in turn). Repo scripts run **first**, inside the worktree; global scripts run **last** (that's where the worktree gets removed). | The **worktree** for repo scripts; the **project dir** for global scripts. |

Hooks run only for sessions that have their own worktree. A session that falls back to the
plain project directory doesn't fire lifecycle hooks.

### Hooks are synchronous

Every hook runs **synchronously** — the session waits for each script to finish before
proceeding (and `session-deleted` completes before the worktree is removed). This keeps
the model simple and predictable. To run something in the background, detach it in your
script:

```sh
# ~/.spwn/hooks/session-created.d/50-dev-server.sh
my-dev-server & disown     # returns immediately; server keeps running
```

## Environment

Each script runs with these variables (the working directory is per the table above):

| Variable | Value |
|----------|-------|
| `SPWN_EVENT` | The event name (`session-created`, …). |
| `SPWN_TERMINAL_ID` | The session's stable id. |
| `SPWN_PROJECT_DIR` | The project's root directory (the main checkout). |
| `SPWN_WORKTREE` | The session's worktree path. |
| `SPWN_BRANCH` | The session's branch (`spwn/<short>`). |
| `SPWN_BASE_BRANCH` | The branch it will merge back into. |
| `SPWN_SESSION_ID` | The Claude session id — set for `session-ready` / `session-turn` / `session-deleted`; absent on `session-created` (not known yet). |
| `SPWN_TURN_UUID` | The turn's id — set for `session-turn` only. |
| `SPWN_BIN` | Path to the spwn binary — run `"$SPWN_BIN" prompt …` to [ask the user](#ask-the-user) or `"$SPWN_BIN" checkpoint "$SPWN_TURN_UUID"` to snapshot. |
| `SPWN_EXEC` | The prefix reaching this session's [environment](#running-a-session-somewhere-else), if a hook made one — so a later hook can run commands inside it, or tear it down on delete. |

## Reporting values back to spwn

A hook can hand a value back to spwn by printing a line of the form:

```
::spwn:set:: key=value
```

spwn parses these lines out of the output (they don't appear in the captured log) — one
`key=value` per line, and the value may contain spaces. The default `session-created`
worktree hook uses this to tell spwn which worktree it made:

```sh
echo "::spwn:set:: worktree=$SPWN_WORKTREE"
echo "::spwn:set:: branch=$SPWN_BRANCH"
echo "::spwn:set:: base=$SPWN_BASE_BRANCH"
```

Recognized keys on `session-created` are `worktree`, `branch`, and `base` — this is how
worktree creation lives in a script instead of being hardcoded. Most hooks never need
this.

### Running a session somewhere else

The other `session-created` keys point spwn at an **environment** your hook created — a
container, a VM, a remote host — so the session's processes run *there* instead of on
your machine:

| Key | Value |
|-----|-------|
| `exec` | A command prefix spwn prepends to every interactive pane's argv, e.g. `docker exec -it -w "$SPWN_WORKTREE" my-container`. |
| `execHeadless` | The same for [scheduled](/spwn/guides/scheduled-tasks/) runs. Absent ⇒ scheduled runs stay on the host. |
| `execBin` | The agent's binary inside the environment. Default: the agent definition's bare `binary.name`, resolved by the environment's own `PATH`. |
| `execShell` | The shell for shell panes inside it. Default `/bin/sh`. |

```sh
# .spwn/hooks/session-created.d/20-container.sh
docker run -d --name "spwn-$SPWN_TERMINAL_ID" \
  -v "$SPWN_WORKTREE:$SPWN_WORKTREE" -w "$SPWN_WORKTREE" my-image sleep infinity

echo "::spwn:set:: exec=docker exec -it -w $SPWN_WORKTREE spwn-$SPWN_TERMINAL_ID"
```

The agent's TUI and any shell you open on that session now run inside the container.
spwn never inspects the prefix beyond splitting it into arguments — it has no idea
whether it names Docker or anything else.

Three things to know:

- **The interactive prefix must allocate a tty** (`-t` under Docker). Without one an
  agent's TUI renders nothing, so none of its detect rules match and the session looks
  permanently idle. Headless runs are the opposite — they parse line-delimited JSON,
  which a tty corrupts — which is why `execHeadless` is separate.
- **Mount the worktree at the same absolute path.** spwn locates a session's transcript
  by a slug of its working directory, and a worktree's `.git` holds an absolute pointer
  into the main repo. A different in-container path breaks the Timeline, rewind and git
  with no error.
- **Hooks always run on the host**, never inside the environment. Per-turn commits,
  checkpoints and `spwn prompt` are unaffected. A later hook can reach into it with
  `SPWN_EXEC`.

A complete, runnable setup — image, create and teardown hooks — is in
[`examples/hooks/docker-env/`](https://github.com/spwn-gg/spwn/tree/main/examples/hooks/docker-env).

## Ask the user

A hook can pause and **ask you a question** — spwn shows a picker in the app and blocks the
script until you answer. Use the injected **`spwn prompt`** helper (`SPWN_BIN` points at
the spwn binary); it prints the chosen label to stdout, so a hook can **gate** what it does:

```sh
# ~/.spwn/hooks/session-created.d/30-seed.sh
# Yes/No confirm (no options ⇒ Yes/No):
if [ "$("$SPWN_BIN" prompt 'Seed the database?')" = Yes ]; then
  ./scripts/seed-db.sh
fi

# Multiple choice — the chosen label is printed to stdout:
profile=$("$SPWN_BIN" prompt --header env 'Which services?' none web 'web+worker')
```

- **Flags:** `--header TEXT` (a short label above the question) and `--multi`
  (multi-select — labels come back comma-joined). With no options it's a `Yes`/`No` confirm.
- **Exit codes:** `0` = answered (label on stdout), `2` = declined (no window, or the
  ~5-minute timeout elapsed), `3` = usage error / not run inside a hook. The code never
  encodes *which* option, so `answer=$("$SPWN_BIN" prompt …)` is safe under `set -e` —
  branch on the string.
- **Headless runs auto-decline.** A [scheduled](/spwn/guides/scheduled-tasks/) or
  background session has no window, so every prompt returns "declined" at once — always
  handle that branch with a sensible default.

## The Hooks panel

When a session has a worktree, a **Hooks** tab appears in the session's Inspector. It
lists each event with the scripts discovered for it (global and repo), each showing:

- a status dot (green = the last run passed, red = it exited non-zero),
- the script's name and scope,
- an **Output** toggle showing the last run's combined stdout/stderr,
- a **Run** button to fire that event's hooks manually.

If a hook exits non-zero, spwn shows a one-line advisory — the session still opens.

## Requirements

- Global hooks live under `~/.spwn/hooks/`; repo hooks under `<repo>/.spwn/hooks/`
  (**commit** them so they reach a session's checkout).
- Each event runs a bare `<event>.sh` and/or the scripts in `<event>.d/` (sorted). A
  script runs directly if executable, otherwise via `sh`.
- The feature activates only when a session has its own worktree.
- Global hooks can be turned off in [Settings](/spwn/reference/settings/#global-hooks).

## Next

- [Hooks Cookbook](/spwn/cookbook/hooks/) — copy-paste recipes.
- [Settings → Global hooks](/spwn/reference/settings/#global-hooks)
- [Branches & Merging](/spwn/guides/branches-and-merging/)
