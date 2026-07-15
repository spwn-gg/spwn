# spwn

A desktop GUI for **branching and merging Claude Code conversations** so you can
plan effectively — organized into projects, with first-class terminal support.

## The problem it solves

A single Claude conversation is a straight line. Real planning isn't: you want to
explore an idea down one path, back out when it stalls, try a different angle, and
then bring the good parts of several explorations back together. A linear chat
forces you to either pollute one context with dead ends or lose your work in
scattered sessions.

spwn turns the conversation into something you can **branch** and **merge**:

- **Branch** — fork a session at any point to explore an alternative without
  disturbing the original. **Fork** starts a new session with the Agent SDK's
  fork-session option (grouped under its lineage), and **Rewind** resumes an earlier
  point in the conversation so you can roll back and set off in a new direction.
  Explorations live side by side instead of overwriting each other.
- **Merge** — each project has a **composable context space** (notes + files +
  individual turns picked from *any* of your sessions). **Inject** assembles those
  blocks into a first message and seeds a fresh Claude session with it (`▦` on a
  project → add blocks → Inject). This is how you pull the best reasoning out of
  several branches and start a clean, well-planned session from the synthesis.

The result: you plan by exploring many branches cheaply, then merge what worked
into a focused session — instead of fighting one ever-growing linear chat.

## How it's organized

- **Projects** — each is a name + working directory that **groups terminals** you
  open. spwn owns them (persisted to `app_data_dir/projects.json`);
  they're not derived from Claude's own dirs.
- **Terminals** — open a **shell** (default) or a **Claude** session in a project,
  arranged in tabbed panes.

## What spwn stores on disk

spwn confines its own writes to its **app data dir**
(`~/Library/Application Support/com.markbarta.spwn/`): `projects.json` (the projects
+ terminals store), `settings.json`, `checkpoints/<session_id>/` (APFS copy-on-write
code-undo snapshots), and — for the *App data* worktree layout — `worktrees/`. It
only **reads/watches** (never writes) `~/.claude/projects/` and `~/.claude.json`.

### Session worktrees

A fresh Claude session in a git repo runs in its **own git worktree on a new
`cm/<id>` branch** (forked from the repo's current branch, or the parent session's
branch for a fork), so sessions run concurrently without clobbering each other's
files — and the work merges back with normal git. spwn **COW-clones heavy gitignored
build dirs** (`node_modules`, `target`, `.venv`, `dist`, `build`, `.next`,
`.svelte-kit`, `.turbo`, …) into each new worktree so an agent can build immediately.
Worktrees are removed when the session's terminal is deleted; the branch is kept.

**Worktree location** is configurable in **Settings → Session worktree location**.
It applies to new sessions only — existing worktrees stay where they were created.

| Option | Location | Notes |
|--------|----------|-------|
| **Sibling** (default) | `<repo-parent>/.<repo-name>-worktrees/<id>/` | A dot-prefixed folder *beside* the repo. Outside the working tree, so builds, file watchers, and IDE indexers never recurse into it. |
| **Inside repo** | `<repo>/.spwn/worktrees/<id>/` | Registered in the repo's `.git/info/exclude` (the tracked `.gitignore` is untouched). The `.spwn/` dot-prefix keeps most tooling from scanning it, but tools with explicit include globs (e.g. a `tsc` `include: ["src"]`) may still pick it up. |
| **App data** | `…/com.markbarta.spwn/worktrees/<id>/` | Under the app data dir, away from your repos entirely (the original layout). |

### Per-session services (Docker)

Agents working autonomously on parallel branches often need more than files — a
*running* service and a live test harness. Doing that by hand doesn't scale: one full
stack per branch is wasteful, and the branches collide on ports and databases.

With an optional **`spwn.yaml`** at your repo root, spwn brings up **each session's own
service + test harness** in an isolated **docker-compose** stack. Per-session project
names and ephemeral host ports (surfaced as a clickable live URL) let many sessions run
at once without colliding; heavy backing services (a database, say) run **once, shared**
and ref-counted across sessions; forks reuse their parent's image; and idle stacks stop
themselves to save resources. It's a thin wrapper over your own `docker-compose.yml`
(never mutated) and fully opt-in — no `spwn.yaml`, no change. See the **Per-Session
Services** guide in the docs, or `examples/compose-integration/` for a runnable sample.
