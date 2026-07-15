---
title: How it works & your data
description: What spwn touches, how it uses your Claude login, and where your data lives.
---

spwn is a native macOS app that sits on top of your existing Claude Code setup. This
page explains what it touches and where your data lives — the short version is: it
uses **your** Claude and **your** files, and keeps its own data **on your Mac**.

## It uses your own Claude

spwn runs **your** authenticated `claude` command with **your** existing login. There
is no separate account, no sign-in inside spwn, and nothing is proxied through a
server in between. Your conversations go straight to Claude the same way they do when
you run `claude` yourself.

If your `claude` is installed somewhere unusual, point spwn at it in
[Settings](/spwn/guides/settings/).

## It reads your real Claude history

Claude Code already stores your sessions on your machine. spwn reads that history
directly to show your conversations and keep them in sync — it's a view of the same
sessions you'd see in the terminal, not a separate copy kept somewhere else.

## Sessions keep running

Your shell and Claude sessions run in the background and are managed so they **survive
closing the app**. That's why reopening spwn brings everything back exactly where you
left it, including running programs. Fork and Rewind build on this: a fork is a real
branched session, and a rewind rolls a session back to an earlier point.

## Sessions work on their own branch

In a git project, each Claude session works on **its own branch in your real repo**
(named `cm/…`), with its own checkout kept in the app's data folder. This is what lets
sessions run in parallel without clashing, and it keeps their work off to the side
until you merge it back. Your project folder stays on whatever branch you had checked
out; spwn only ever adds `cm/…` branches and never moves your branch or rewrites your
history. Non-git projects run in the project folder directly, with no branches. See
[Parallel Sessions](/spwn/guides/parallel-sessions/).

## Your data stays local

- **Projects, context, and scheduled tasks** you create in spwn are stored **locally
  on your Mac**, in the app's own data folder. They're yours; spwn doesn't upload
  them anywhere.
- **Scheduled tasks run read-only** — they can read your project and its context but
  never modify your files.
- spwn talks to exactly two things: your local **Claude** and your local **files**.
  Nothing else leaves your machine.

## Building it yourself

spwn is open source. If you want to build it from source or contribute, see
[Building from Source](/spwn/reference/building/).
