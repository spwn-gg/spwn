---
title: Glossary
description: The words spwn uses, and the one concept each maps to.
---

spwn uses one word per concept. If two words seem to mean the same thing, this page
is the tiebreaker — the UI, tooltips, and these docs all follow it.

## Project

A name plus a working directory. A project **groups** your sessions and shells. spwn
owns projects itself (it doesn't derive them from Claude's folders).

## Session

An **isolated Claude workspace**: its own conversation *and* its own copy of your
code. Each session runs on its own git **branch** in its own **worktree** (a separate
checkout on disk), so parallel sessions never clobber each other. Switching between
sessions is a pure focus change — nothing is copied in or out of your main checkout.

The whole point: explore many sessions cheaply, then **merge** the ones that worked.

## Shell

A plain terminal inside a project. No conversation, no worktree — just a shell in the
project directory.

## Fork *(verb)*

To **fork** is to start a **new session** from a point in an existing one. The new
session carries the parent's conversation history *and* branches its code, then goes
its own way. "Fork" is the only word spwn uses for branching a conversation — so that
"branch" can mean exactly one thing:

## branch *(noun)*

The **git branch** a session's worktree lives on (spwn names them `cm/<id>`). It's a
**property** of a session — shown as a `⎇` chip — never an action you take. To create
a new line of work you *fork a session*; the git branch comes with it.

## Merge tray

A per-project space where you collect reusable **blocks** — notes, files, and
individual turns picked from any session — and then **Inject** them as the first
message of a fresh session. It's how you bring the good parts of several explorations
back together. (Formerly called "Context"; renamed so it isn't confused with the
model's *context window*.)

## Merge

Bringing a session's committed work back into its base branch with ordinary git. The
session's worktree branch (`cm/<id>`) merges into the branch it was forked from.

## Checkpoint / Timeline

A **checkpoint** is a copy-on-write snapshot of a session's files, captured after each
turn. The **Timeline** is the single place you return to an earlier point — choosing
whether to roll back the *conversation*, the *files*, or both.
