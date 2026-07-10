---
title: Parallel Sessions
description: Each session works on its own git branch, so several can run at once without clashing — then merge the results back.
---

Every Claude session in a git project works on **its own branch, in its own isolated
copy of the files**. That means several sessions can run at the same time — even
unattended — without stepping on each other, and you merge the results back when
you're happy with them.

## Each session gets its own branch

When you start a Claude session in a project that's a git repository, spwn creates a
branch for it in **your real repo** (named `cm/…`) and gives the session its own
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

When a session has produced something you want to keep, bring it into your main branch:

- Use the **⤵ Merge** button in the conversation view to merge the session's branch
  back into the branch it started from, or
- Merge it yourself with normal git (`git merge cm/…`) — it's your repo and your
  branch.

Each turn's work is committed on the session's branch as it goes, so the branch always
has real, mergeable history. Nothing is merged automatically; you decide what lands and
when.

## Forks branch from their parent

A [fork](/spwn/guides/fork-and-rewind/) starts a new session from an existing one, and
its branch is created from the **parent session's branch** — so the code tree mirrors
the conversation tree. A fork begins with the work its parent had committed and then
goes its own way.

## Projects that aren't git repositories

If a project folder isn't a git repository, sessions simply run in the project folder
as before — there are no branches to isolate or merge. To get per-session branches,
make the project a git repo (`git init`) and start a new session.

## Next

- [Fork & Rewind](/spwn/guides/fork-and-rewind/)
- [Claude Sessions](/spwn/guides/claude-sessions/)
