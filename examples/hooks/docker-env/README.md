# Per-session Docker environments

Give every spwn session its **own container**, and run the agent *inside* it — so its
builds, tests and installed packages happen there instead of on your machine, and two
sessions can want two different toolchains without a fight.

spwn has no Docker code. A hook creates the container and reports one line back:

```sh
echo "::spwn:set:: exec=docker exec -it -w $SPWN_WORKTREE spwn-$SPWN_TERMINAL_ID"
```

spwn prepends that prefix to the argv of every pane it opens for the session. The same
mechanism that lets a hook create the session's worktree (`::spwn:set:: worktree=…`)
lets it say where the session's processes should run.

## Install

```sh
mkdir -p .spwn
cp -R examples/hooks/docker-env/.spwn/hooks .spwn/
cp -R examples/hooks/docker-env/.spwn/env   .spwn/
chmod +x .spwn/hooks/*/*.sh
docker build -t spwn-session-env .spwn/env    # the hook won't pull; build it first
git add .spwn && git commit -m "Per-session Docker environments"
```

**Commit them.** Repo hooks reach a session through its git checkout, so an uncommitted
hook never runs.

Set `SPWN_ENV_IMAGE` to use a different image.

## What ends up where

|                    | Runs on the host | Runs in the container |
|--------------------|------------------|-----------------------|
| The agent's TUI    |                  | ✓ |
| Shells opened on the session | | ✓ |
| Builds, tests, package installs | | ✓ |
| spwn's hooks       | ✓ | |
| Per-turn commits, checkpoints, worktree create/remove | ✓ | |
| Your editor, `git` in your own terminal | ✓ | |

The container isolates the **environment**, not the files: the worktree is bind-mounted,
so your editor and host-side git see the agent's edits immediately, and spwn's per-turn
commits and checkpoints keep working untouched.

## Why the paths are identical inside and out

The mounts deliberately use the *same absolute path* on both sides. Two things depend
on it, and both fail silently if it's broken:

- **The transcript.** spwn finds a session's JSONL by a slug of its working directory.
  A different in-container path is a different slug — session binding, the Timeline,
  turn detection and rewind all go dark, with no error.
- **git.** A worktree's `.git` is a *file* holding an absolute `gitdir:` pointer into
  the main repo, so that path must resolve in the container too. The hook asks
  `git rev-parse --git-common-dir` for it rather than guessing `$SPWN_PROJECT_DIR/.git`,
  which is wrong when the project is a subdirectory of the repo, when the project dir
  is itself a linked worktree, or when spwn's worktrees live outside the project.

Mounting `~/.claude` at the same path means the container reuses your existing login
and writes its transcript straight to where spwn already reads it.

## `-it` vs `-i`

The hook reports two prefixes, and the difference matters:

- `exec` uses **`-it`**. `-t` allocates the tty the agent's TUI needs to render at all
  — without one, every `detect` rule in the agent definition misses and the session
  looks permanently idle — and it's what forwards terminal resizes into the container.
- `execHeadless` uses **`-i`** only. Scheduled runs parse line-delimited JSON, and a
  tty corrupts that stream with interleaved spinner output. Omit `execHeadless` and
  scheduled runs simply stay on the host.

## Requirements and caveats

- **The image must carry the agent CLI on its PATH.** That's what spwn's `{bin}`
  resolves against inside the environment; your host's macOS binary can't run there.
  It also needs `env`, `git` and a shell — so no distroless/scratch base.
- **`docker` must be on the PATH spwn launches panes with**, which is the long-lived
  rmux daemon's, not the hook's. The hook reports an absolute path for this reason.
- **Cold images are not pulled.** Hooks are synchronous with no timeout, so a
  multi-minute pull would look like a hung session. Build or pull first; the hook says
  so and leaves the session on the host.
- **File ownership.** The container runs as root. Docker Desktop on macOS maps that
  through so it's invisible; on a Linux host add `--user "$(id -u):$(id -g)"` or the
  agent leaves root-owned files that host-side commits and checkpoints trip over.
- **Speed.** spwn COW-clones `node_modules`, `target`, `.venv` into each worktree so a
  session can build immediately. Reaching those through a bind mount is slower than
  native — noticeably so on macOS. A named volume would be faster but invisible to
  host-side git and checkpoints; this example keeps the bind mount.
- **The container shares your `~/.claude`**, including credentials. It is an
  environment boundary, not a security sandbox.

## Recovery

If the container is removed (`docker rm`, a Docker reset), the session's stored prefix
points at nothing and its pane won't start. Rebuild it from the session's **Hooks**
panel: **Run** on `session-created` re-creates the container and re-reports the prefix.

`--restart unless-stopped` means it survives Docker restarts and reboots on its own.

## Files

```
.spwn/
  env/Dockerfile                            # the session environment image
  hooks/
    session-created.d/20-container.sh       # create + report the exec prefix
    session-deleted.d/50-container.sh       # remove it (before the worktree goes)
```

## Need more than one container?

[`../dev-env-services/`](../dev-env-services) builds on this with a database: the app
container stays per-session, the database *server* is shared across sessions, and each
session gets its own database inside it — so ten parallel sessions don't mean ten postgres
processes.
