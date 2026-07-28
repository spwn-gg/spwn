---
title: Merging Work
description: Bring a session's committed work back into its base branch — with one click or with plain git — and delete sessions safely.
---

Each Claude session works on its own `spwn/…` branch, and every turn's work is committed on
that branch as it goes. So a session's branch always has **real, mergeable history** —
nothing to reconstruct at the end. When a session has produced something you want to
keep, you **merge** its branch back into the branch it started from.

## Merge a session

Open the session's Inspector and use **⤵ Merge…** (also reachable as **Merge session**).
spwn merges the session's `spwn/…` branch into its **base branch** — the branch it was
forked from — with ordinary git. You can also:

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

## Forks merge into their parent

A [fork](/spwn/guides/fork-and-rewind/) starts from an existing session, and its branch
is created from the **parent session's branch**. So a fork merges back into its parent's
branch by default — the code tree mirrors the conversation tree.

## Non-git projects

If a project folder isn't a git repository, sessions run in the folder directly — there
are no `spwn/…` branches to merge. To get per-session branches and merging, make the
project a git repo (`git init`) and start a new session.

## Next

- [Parallel Sessions](/spwn/guides/parallel-sessions/) — how sessions get their branches.
- [Fork & Timeline](/spwn/guides/fork-and-rewind/)
- [Merge Tray](/spwn/guides/context-composer/) — collect the good parts across sessions.
