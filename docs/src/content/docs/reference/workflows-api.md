---
title: Workflows API
description: The spwn object a workflow's main function receives — sessions, headless runs, state, commands, HTTP, GitHub and events.
---

A [workflow](/spwn/guides/workflows/) receives `spwn` as the first argument of `main`. The
same API, with full types, is in `spwn.d.ts` — spwn writes it next to your workflows when
you create one from the Workflows panel.

Having an AI agent write the workflow? Point it at the
[workflow guide for AI agents](/spwn/workflows-agents.md) — the same file "New workflow" writes
to `.spwn/workflows/AGENTS.md`.

Every method that returns a promise **rejects as soon as the run is stopped**, with an
error whose `code` is `"stopped"`. Other failures reject with a plain `Error`.

## The run

| | |
|---|---|
| `spwn.project` | `{ id, name, dir }` of the project. |
| `spwn.workflow` | `{ name, runId }`. |
| `spwn.log(...args)`, `spwn.warn(...)`, `spwn.error(...)` | Write to the run's log. `console.log` and friends do the same. |
| `spwn.sleep(ms)` | Wait. `setTimeout` and `setInterval` also work. |
| `spwn.stopping` | `true` once the run has been asked to stop. |
| `spwn.untilStopped()` | Resolves when the run is stopped — for workflows that only react to [events](#events). |

## State

`spwn.state` keeps JSON values per workflow, across runs, restarts and spwn restarts.

| | |
|---|---|
| `get(key, fallback?)` | The stored value, or `fallback`. |
| `set(key, value)` | Store a value. Setting `null` or `undefined` deletes the key. |
| `delete(key)` | |
| `all()` | Every key and value. |

## Sessions

### `spwn.sessions.create(options?)` → `Session`

Starts an interactive agent session: its own worktree and branch, the project's hooks, an
entry in the sidebar.

| Option | |
|---|---|
| `title` | Sidebar title. Default: the workflow's name. |
| `key` | Your id for the session, for [`find`](#spwnsessionsfindkey-options--session--null). |
| `agent` | Agent definition id. Default: the default agent from Settings. |
| `prompt` | Sent once the agent is ready. `waitForTurn()` then waits for the reply. |
| `permissionMode` | A permission mode from the agent's definition, applied at launch. |
| `readyTimeoutMs` | How long to wait for the agent to be ready for `prompt`. Default 90 000. |
| `onHookPrompt(prompt)` | Answers [hook questions](#hook-questions) from this session. |

If the agent stops to ask something before it can take `prompt` — a folder-trust check,
say — `create` rejects with the question in the message.

### `spwn.sessions.find(key, options?)` → `Session | null`

The session this workflow created with `key`. Accepts `onHookPrompt`.

### `spwn.sessions.get(id, options?)` → `Session | null`

Any session in the project, by its id. The run then owns it: its hook questions come to
`onHookPrompt`, and it's listed with the run's sessions.

### `spwn.sessions.list()` → `Session[]`

The sessions this workflow created.

### `Session`

| Property | |
|---|---|
| `id` | spwn's id for the session. |
| `title`, `kind`, `agent` | |
| `workflow`, `key` | The workflow that created it and the key it was filed under. |
| `branch`, `baseBranch`, `cwd` | Its branch, the branch it merges into, and its worktree. |
| `sessionId` | The agent's own conversation id. |

| Method | |
|---|---|
| `send(text, { submit = true })` | Type into the agent and submit. |
| `waitForTurn({ timeoutMs })` | Wait for the reply to the last `send` (or the `create` prompt). See below. |
| `prompt(text, { timeoutMs })` | `send`, then `waitForTurn`. |
| `status()` | `"thinking"`, `"blockedPermission"`, `"blockedQuestion"`, `"done"`, `"error"` or `"idle"`. |
| `waitForStatus(status \| status[], { timeoutMs })` | Resolves with the status once it matches. |
| `screen()` | The agent's screen, as text. |
| `press(key)` | Press a key: `Enter`, `Escape`, `Up`, `Down`, `C-c`, `BTab`, … |
| `interrupt()` | Interrupt the running turn. |
| `lastMessage()` | The agent's reply to the latest prompt. |
| `transcript()` | The conversation, as turns with text, thinking, tool use and tool result blocks. |
| `delete()` | Delete the session, its worktree and its branch. |
| `refresh()` | Re-read the properties. |

`waitForTurn` resolves with one of:

- `{ turnUuid, text, status }` — the turn finished and its `session-turn` hooks (the commit)
  have run. `text` is the agent's reply.
- `{ blocked: true, status, screen }` — the agent stopped to ask for permission or input.
  Answer it with `press`/`send`, or leave it for a person.

With no `timeoutMs` it waits indefinitely.

### Hook questions

When a hook in a session the run owns asks a question with `spwn prompt`, `onHookPrompt` is
called with:

| | |
|---|---|
| `event` | The hook event (`session-created`, …). |
| `session` | The session's id. |
| `question`, `header` | |
| `options` | `[{ label, description }]`. |
| `multiSelect` | |

Return an option's label (an array of labels for `multiSelect`), or `null` to decline.
Without a handler the question is declined. Unanswered questions are declined after five
minutes.

## Headless runs

### `spwn.agents.run(prompt | options)` → `{ ok, error, sessionId, text }`

A one-shot, non-interactive run in its own worktree, like a scheduled task. Resolves when
the agent finishes; `text` is its final message, and `sessionId` is the run's session,
which stays in the sidebar. Options: `prompt`, `agent`, `title`, `key`, `onHookPrompt`.

### `spwn.agents.list()`

The installed agent definitions.

## Commands and files

### `spwn.exec(cmd, args?, options?)` → `{ code, ok, stdout, stderr }`

Runs a program directly (no shell) in the project dir. A non-zero exit doesn't reject —
check `ok`. Stopping the run kills the process.

| Option | |
|---|---|
| `cwd` | Relative to the project dir. |
| `env` | Extra environment variables. |
| `input` | Written to stdin. |
| `timeoutMs` | Default 10 minutes. |

### `spwn.sh(script, options?)`

`spwn.exec("sh", ["-c", script], options)`.

### `spwn.fs`

`read(path)`, `write(path, content)` and `exists(path)`, with paths relative to the project
dir. Paths outside the project are refused.

## HTTP and GitHub

### `spwn.fetch(url, options?)` → `{ status, ok, headers, body, text(), json() }`

Options: `method`, `headers`, `body` (a string), `json` (sent as a JSON body), `timeoutMs`
(default 60 000).

### `spwn.github.graphql(query, variables?)`

Calls GitHub's GraphQL API with the token saved in [Settings](/spwn/reference/settings/),
and resolves with `data`. Rejects if GitHub returns errors and no data; partial errors are
logged. The token itself never reaches the script.

### `spwn.github.rest(method, path, body?)`

Calls `https://api.github.com` + `path` (e.g. `/repos/owner/repo/issues`) and resolves
with the parsed body.

## Events

### `spwn.on(event, handler)` → unsubscribe

Calls `handler` for events anywhere in the project — including sessions the workflow
didn't start. `spwn.off(event, handler)` removes it.

| Event | Payload |
|---|---|
| `session-created` | `{ terminalId, branch, worktree, workflow, … }` — after the session's worktree and `session-created` hooks. |
| `session-ready` | The agent's conversation id is known (`sessionId`). |
| `session-turn` | A turn finished and its hooks ran (`turnUuid`). |
| `session-deleted` | The session was deleted. |
| `status` | `{ terminalId, status }` — a session's live status changed. |

Session events carry `event`, `projectId`, `terminalId` (the session's `id`), `sessionId`,
`turnUuid`, `branch`, `worktree` and `workflow` (`{ name, key }` if a workflow created it).
They fire whether or not any hook scripts exist.

## `meta`

| Field | |
|---|---|
| `name` | Shown instead of the file name. |
| `description` | |
| `inputs` | `{ [key]: { type, default, description, required } }`. `type` is `"string"` (default), `"number"` or `"boolean"`. |
| `keepAlive` | Restart whenever `main` returns or throws, with backoff, until stopped. |
