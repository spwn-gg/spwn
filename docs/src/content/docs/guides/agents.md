---
title: Agents
description: Which coding agents spwn can drive, and how to teach it a new one.
---

A spwn session runs a coding agent's **own terminal UI**, live, in a pane that
survives closing the app. You interact with the real CLI — its own prompts, its own
keyboard shortcuts, its own slash commands — and spwn wraps it with the things a
terminal can't do for itself: an isolated worktree and branch, per-turn commits and
checkpoints, live status in the sidebar, and a browsable conversation with fork and
rewind.

## Which agents are available

Open **Settings → Agents**. Each one shows where its definition came from, whether
the binary was found, and what it can actually do:

| Capability | Meaning |
|---|---|
| **transcript** | spwn can read the conversation, so the Transcript tab and per-turn actions work |
| **status** | Real "thinking / waiting for you / done" detection, rather than just activity |
| **rewind** | The conversation can be returned to an earlier turn |
| **scheduled** | The agent can run non-interactively for scheduled tasks |

**Claude Code** ships fully configured. **Codex** and **Gemini** ship marked
*experimental*: their command lines are from the published CLI docs, but nobody has
driven them against the real binary, so they carry no invented detect patterns and
advertise no capabilities they can't honour. They'll run as a terminal session and
get worktrees, commits and activity-based status — but no transcript, no rewind.

If an agent shows **not found**, either install its CLI or point spwn at the binary
in the same panel.

## Teaching spwn a new agent

An agent is a TOML file, not code. Copy an existing one and edit it:

```sh
cp ~/.spwn/agents/claude.toml ~/.spwn/agents/myagent.toml
```

Then **Settings → Agents → Reload definitions**. No restart, no rebuild. Parse errors
are reported with the file and line rather than the agent silently vanishing, and a
broken override falls back to the bundled definition.

Definitions load in three scopes, each overriding the last by `id`:

1. built into spwn,
2. `~/.spwn/agents/*.toml` — yours, everywhere,
3. `<project>/.spwn/agents/*.toml` — committed with a repo.

So you can pin a project to a patched definition without touching your global setup.

### What a definition contains

```toml
id   = "myagent"
name = "My Agent"
icon = "◆"

[binary]                      # how to find the executable
env  = "MYAGENT_BIN"          # checked first
name = "myagent"              # then $PATH
candidates = [".local/bin/myagent"]   # then these, relative to $HOME

[env]                         # extra environment for the pane
TERM = "xterm-256color"

[argv]                        # how to start it
new    = ["{bin}", "--session-id", "{sessionId}"]
resume = ["{bin}", "--resume", "{sessionId}"]

[keys]                        # tmux key tokens spwn sends
submit    = ["Enter"]
interrupt = ["Escape"]
clear     = ["C-u"]

[[detect.rules]]              # how to read status off the screen
status = "thinking"
any    = ["esc to interrupt"]
```

Two details that are easy to get wrong:

- **`[[ ... ]]` in argv is an optional group.** It expands to one or more arguments
  and vanishes entirely if a placeholder inside is empty. Keep a flag and its value
  in the *same* group — split apart, the flag survives without its value and the
  agent exits with a usage error the moment the pane opens.
- **`clear` is not `interrupt`.** Interrupting a turn usually leaves the composer's
  text intact, and spwn types into that composer. If `clear` doesn't genuinely empty
  it, commands get appended to whatever was there and submitted as ordinary prompts.

### Status rules

Rules are evaluated in order and the first match wins, so put blocking states before
"thinking". Each rule can use `any` (substrings, OR'd), `regex`, and `all`
(substrings that must *also* be present — useful for pinning a pattern that would
otherwise be ambiguous). `rows = { last = N }` restricts a rule to the bottom of the
screen.

With no rules at all, an agent still gets activity-based status: output means
thinking, silence means done. It can never report *waiting for you*, which is honest
— without patterns there's no way to know.

## Rewind

spwn never edits an agent's stored history. It drives the agent's own rewind UI:
opens the menu, walks it while reading the highlighted row back after every
keypress, and checks the confirmation screen names the same message before
committing.

If it can't positively identify the turn you asked for — say the agent has since
pruned that checkpoint — it stops and tells you, changing nothing. That's
deliberate: a rewind can be paired with a file restore, so landing on the wrong
point doesn't just show the wrong conversation, it reverts your files to the wrong
place.

One consequence worth knowing: some agents fork the conversation *in memory* and
only write the new branch when you send the next message. Until then the Transcript
tab may still show the old history even though the rewind succeeded.
