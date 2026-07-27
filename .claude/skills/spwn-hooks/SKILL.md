---
name: spwn-hooks
description: Create, list, test, or remove spwn project hooks — the .spwn/hooks/<event>.sh scripts spwn runs on session lifecycle events (session-created, session-ready, session-deleted). Use when asked to add/edit/scaffold/inspect/test/delete a spwn hook, wire up per-session setup or teardown (start a dev server, seed a DB, bring up containers, clean up on delete), or work with .spwn/hooks.
---

# Manage spwn project hooks

spwn runs a user shell script on each session lifecycle event. A session works in its
own git worktree on a `cm/<id>` branch; if the repo has a hook file for an event, spwn
runs it. This skill scaffolds, inspects, tests, and removes those hooks.

**Source of truth:** `src-tauri/src/hooks.rs` (discovery + env injection) and
`src-tauri/src/commands.rs` (fire sites). The docs live at
`docs/src/content/docs/guides/hooks.md`; a runnable sample is in `examples/hooks/`.

## The contract (what spwn actually does)

- **Discovery:** one file per event at `<worktree>/.spwn/hooks/<event>.sh`.
- **Events:** `session-created` (worktree ready), `session-ready` (Claude session id
  bound), `session-deleted` (just before the worktree is removed).
- **Execution:** the file runs **directly if executable** (honoring its shebang),
  otherwise via `sh <file>`. Working directory is the **worktree**.
- **Synchronous:** the session waits for the script to finish. Background long-running
  work yourself, e.g. `my-server & disown`.
- **Environment:** `SPWN_EVENT`, `SPWN_TERMINAL_ID`, `SPWN_PROJECT_DIR`,
  `SPWN_WORKTREE`, `SPWN_BRANCH`, `SPWN_BASE_BRANCH`, `SPWN_SESSION_ID` (the last is
  absent on `session-created` — the id isn't known yet), plus `SPWN_BIN` (path to the
  spwn binary, for the `spwn prompt` helper below).
- **Interactive:** a hook can ask the user and block on the answer via `spwn prompt`
  (see below) — it can gate what a `session-created` hook does.
- **Opt-in + git:** no file, no hook. Hooks must be **committed** so they check out
  into each session's worktree. Hooks only run for sessions that have a worktree (a git
  repo); a session that falls back to the plain project dir doesn't fire them.

## Driver

Use **`.claude/skills/spwn-hooks/spwn-hooks.sh`**. It operates on the current git repo
(`git rev-parse --show-toplevel`) and mirrors the runner's discovery + env contract, so
`test` reproduces exactly how spwn would invoke a hook.

```sh
.claude/skills/spwn-hooks/spwn-hooks.sh list            # each event: hook present? executable?
.claude/skills/spwn-hooks/spwn-hooks.sh new session-created   # scaffold + chmod +x, prints the path
.claude/skills/spwn-hooks/spwn-hooks.sh path session-ready    # print the file path (create if missing)
.claude/skills/spwn-hooks/spwn-hooks.sh test session-created  # run it as spwn would (worktree cwd + SPWN_* env)
.claude/skills/spwn-hooks/spwn-hooks.sh rm session-deleted    # delete the hook file
```

## Typical flows

**Add a hook.** `new <event>` writes a template (executable), then edit it. Example —
install deps + start a dev server on create:

```sh
.claude/skills/spwn-hooks/spwn-hooks.sh new session-created
# edit .spwn/hooks/session-created.sh, e.g.:
#   npm install
#   npm run dev & disown        # background: the session must not wait on a server
.claude/skills/spwn-hooks/spwn-hooks.sh test session-created   # verify before relying on it
git add .spwn/hooks/session-created.sh && git commit -m "Add session-created hook"
```

**Orchestrate other code.** The `<event>.sh` file is a plain script — call anything
(`./.spwn/hooks/setup/seed-db.sh`, `docker compose up -d`, `python scripts/x.py`).
Keep helpers alongside (e.g. `.spwn/hooks/setup/`) and commit them too.

**Teardown on delete.** Put cleanup in `session-deleted.sh` — it runs (synchronously)
before the worktree is removed, so stop anything `session-created` started
(`docker compose down`, kill the dev server, remove temp data).

## Prompt the user (interactive)

A hook can ask a multiple-choice question and **block on the answer** via the
`spwn prompt` helper — no raw protocol, no reading stdin. spwn shows a picker in the UI
and the helper prints the chosen label to stdout.

```sh
# N-way choice — chosen label on stdout:
if color=$("$SPWN_BIN" prompt --header setup "Pick a color" Red Blue Green); then
  echo "chose $color"
else
  echo "declined / no UI — using a default"
fi

# Confirm (no options ⇒ Yes/No):
if [ "$("$SPWN_BIN" prompt 'Seed the database?')" = Yes ]; then ./scripts/seed.sh; fi
```

- **Invoke as `"$SPWN_BIN" prompt …`** (spwn injects `SPWN_BIN`; bare `spwn prompt` also
  works if it's on `PATH`). Flags: `--header TEXT`, `--multi` (multi-select — labels come
  back comma-joined). No options ⇒ a `Yes`/`No` confirm.
- **Exit codes:** `0` = answered (label on stdout), `2` = declined (no UI / ~5-min
  timeout), `3` = usage error / not inside a hook. Exit codes don't encode *which* option,
  so `answer=$(…)` stays safe under `set -e` — branch on the string, not the status.
- **Gating:** the hook decides what to do with the answer (seed or not; `exit` to fail),
  which is how a `session-created` hook gates per-session setup.
- **Headless:** scheduled/headless runs have no window, so prompts auto-decline (exit 2)
  immediately — always handle that branch.

## Gotchas

- **Commit the hook** — an uncommitted `.spwn/hooks/*.sh` won't be in the session's
  worktree checkout, so it never runs.
- **Long-running processes must be backgrounded** in-script (`& disown`); otherwise the
  session blocks until the script returns.
- **`session-created` has no `SPWN_SESSION_ID`** — use `session-ready` for anything that
  needs the Claude session id.
- **Only the three known events run** — a file named anything else is never fired.
- **Failures are non-fatal**: a non-zero exit surfaces a one-line notice and shows red
  in the session's **▸ Hooks** panel; the session still opens. Check output there or via
  `test`.
- **`spwn prompt` under `test`**: `spwn-hooks.sh test` has no UI, so it auto-answers every
  `spwn prompt` with the **first option** (via a stub `SPWN_BIN`). To exercise the decline
  branch, run `spwn prompt` yourself with `SPWN_PROMPT_SOCK` unset (exits 2/3).
