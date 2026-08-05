---
title: Branches & Merging
description: Each session works on its own git branch, so several can run at once without clashing — then merge the results back with one click or plain git.
---

Every Claude session in a git project works on **its own branch, in its own isolated
copy of the files**. Several sessions can run at the same time — even unattended —
without stepping on each other, and you merge each one's work back only when you're happy
with it.

## Each session gets its own branch

When you start a Claude session in a project that's a git repository, spwn creates a
branch for it in **your real repo** (named `spwn/…`) and gives the session its own
checkout to work in. The branch shows up as a small **⎇ chip** on the session in the
sidebar, and it's a normal git branch — you can see it with `git branch` and work with
it like any other.

Your project folder itself stays on whatever branch you had checked out. The session's
work happens on its branch, off to the side, until you choose to bring it in.

## Run several at once

Because each session has its own files, sessions are free to run **in parallel**. You
can kick off a few autonomous sessions on different tasks and let them work at the same
time — one editing and building while another does something else — and none of them
disturbs the others or your project folder.

Switching between session tabs is instant and safe: it just changes what you're
looking at. Nothing on disk moves, so a session running in the background keeps going
untouched while you check on another.

## Ready to work immediately

A fresh checkout normally wouldn't have your installed dependencies or build output, so
a session couldn't build or run right away. spwn seeds each session with a
copy-on-write clone of those heavy folders (like `node_modules` and build output), so
they're there instantly and share disk space until something changes. A session can
build, test, and run from the first moment — no cold reinstall.

## Merge the work back

Each turn's work is committed on the session's branch as it goes, so the branch always
has **real, mergeable history** — nothing to reconstruct at the end. When a session has
produced something you want to keep, open its Inspector and use **⤵ Merge…** (also
reachable as **Merge session**). spwn merges the session's `spwn/…` branch into its
**base branch** — the branch it was forked from — with ordinary git. You can also:

- **Merge** and leave the session in place, or
- **Merge & delete** to merge and then clean up the session's branch and worktree in one
  step.

Prefer to do it yourself? It's your repo and your branch — merge with plain git:

```sh
git merge spwn/<id>
```

Nothing is merged automatically. You decide what lands and when.

## Deleting a session safely

Deleting a session removes **both its worktree and its `spwn/…` branch**, so they don't
pile up in your repo. If the branch still holds commits that aren't in its base — or
uncommitted changes — the confirm dialog **names exactly what you'd lose** before you go
through with it. **Merge first** to keep the work.

**Deleting a project** deletes each of its sessions the same way, so their worktrees and
`spwn/…` branches go too. The confirm lists every session whose work isn't merged, for the
same reason. Your project folder stays where it is, and only the per-session branches are
removed — never the base branch they were cut from.

## Forks mirror the conversation tree

A [fork](/spwn/guides/fork-and-rewind/) starts a new session from an existing one, and
its branch is created from the **parent session's branch**. So a fork begins with the
work its parent had committed, then goes its own way — and merges back into its parent's
branch by default. The code tree mirrors the conversation tree.

## Projects that aren't git repositories

If a project folder isn't a git repository, sessions simply run in the project folder —
there are no branches to isolate or merge. To get per-session branches and merging, make
the project a git repo (`git init`) and start a new session.

## Next

- [Sessions & Shells](/spwn/guides/terminals/) — what you open inside a project.
- [Fork & Timeline](/spwn/guides/fork-and-rewind/) — branch and roll back sessions.
- [Merge Tray](/spwn/guides/context-composer/) — collect the good parts across sessions.
