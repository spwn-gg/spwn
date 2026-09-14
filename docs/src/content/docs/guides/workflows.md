---
title: Workflows
description: Scripts in a project's .spwn/workflows that start and drive agent sessions — poll a board, hand tickets to personas, react to session events.
---

A **workflow** is a script in your project's `.spwn/workflows/` folder that orchestrates
agents. It can start sessions, prompt them, wait for their replies, run one-shot headless
agents, call GitHub, and react when sessions change — whatever your process is.

spwn has no opinion about that process. It doesn't know what a ticket, a persona or a
review is: those are just code in your workflow. spwn runs the script and gives it an API.

## Create a workflow

On a project in the sidebar, open **⚙ Workflows** and click **＋ New workflow**. Name it,
pick JavaScript or TypeScript, and spwn writes a starter file plus `spwn.d.ts` (the API's
types, so your editor can complete and check it) into `.spwn/workflows/`.

A workflow is an ES module with a default-exported function:

```js
// .spwn/workflows/explain.js
export const meta = {
  description: "Ask an agent to explain a file",
  inputs: { path: { default: "README.md" } },
};

export default async function main(spwn, inputs) {
  const session = await spwn.sessions.create({
    title: `explain ${inputs.path}`,
    prompt: `Explain ${inputs.path} in five bullet points.`,
  });
  const turn = await session.waitForTurn();
  spwn.log(turn.text);
}
```

- **`main(spwn, inputs)`** is the workflow. When it returns, the run is finished.
- **`meta`** is optional: a display `name`, a `description`, the `inputs` the Run form
  asks for, and `keepAlive`.
- Files starting with `_`, `.d.ts` files, and anything in a subfolder (`lib/`) aren't
  listed as workflows. Use them for shared code: `import { personas } from "./lib/personas.js"`.

## Allow workflows

Workflows are code from the project, and a project can be a repo you just cloned — so
**nothing runs until you allow it**. The Workflows panel asks once per project. Allowed
workflows run with your permissions: they can start sessions, run commands, and use your
saved GitHub token. **Turn off** stops every running workflow in the project.

## Run, stop, keep running

- **Run** starts the workflow (after asking for its inputs, if it has any). Its log streams
  into the panel, with links to every session it started.
- **Stop** ends it right away. Anything it was waiting on — `sleep`, a turn, a command —
  is cancelled, and a command it started is killed. Sessions it created stay.
- **`keepAlive: true`** in `meta` restarts the workflow whenever it returns or throws,
  waiting 5s, then 10s, up to 5 minutes between attempts (reset once it stays up for ten
  minutes). Use it for workflows that loop forever, like a board poller.
- **Start with spwn** starts the workflow whenever spwn starts, with its default inputs.

A workflow runs once at a time: running it again while it's running is refused.

## Driving sessions

`spwn.sessions.create()` starts the same kind of session **＋ New session** does — its own
worktree and branch, the project's hooks, the sidebar entry — and returns a handle:

```js
const session = await spwn.sessions.create({ title: "fix login", key: "ISSUE-42", prompt });
const turn = await session.waitForTurn();

if (turn.blocked) {
  // The agent stopped to ask for permission or an answer. turn.screen shows the question.
  spwn.warn("waiting for a person:", turn.screen);
} else {
  spwn.log(turn.text);                 // the agent's reply
  await session.prompt("Now add a test for it.");
}
```

- **`waitForTurn()`** resolves when the agent's reply is complete *and* the turn's
  `session-turn` hooks (the per-turn commit) have run — so the work is on the branch when
  your workflow acts on it.
- **`key`** files the session under your own id. `spwn.sessions.find(key)` gets it back —
  across runs, restarts, and spwn restarts — so a workflow can keep **one session per
  ticket** instead of starting a new one each time.
- **Stopping mid-turn is safe.** If a run stops while a prompt is out, the session found
  in the next run has `awaitingTurn: true`; call `waitForTurn()` to collect the reply
  instead of sending the prompt again:

  ```js
  let session = await spwn.sessions.find(ticket.id);
  if (!session) session = await spwn.sessions.create({ key: ticket.id, prompt });
  else if (!session.awaitingTurn) await session.send(prompt);
  const turn = await session.waitForTurn();
  ```
- You can watch or take over any workflow session: it's an ordinary session in the
  sidebar, marked ⚙.

For work that doesn't need a conversation, `spwn.agents.run({ prompt })` does a one-shot
headless run (like a [scheduled task](/spwn/guides/scheduled-tasks/)) and resolves with
the agent's final message.

## Hooks and workflows

Workflow sessions are ordinary sessions, so your [hooks](/spwn/reference/hooks/) run for
them exactly as they do for sessions you start. Two things are workflow-specific:

- **A hook's question goes to the workflow.** If a hook in a session the workflow owns asks
  something with `spwn prompt`, the workflow's `onHookPrompt` handler answers it. With no
  handler, the question is declined, as it is for a scheduled run.

  ```js
  await spwn.sessions.create({
    title: "with a database",
    onHookPrompt: (q) => (q.question.includes("Seed") ? "Yes" : null), // null declines
  });
  ```

- **Workflows can listen to session events** anywhere in the project, including sessions
  you started by hand:

  ```js
  spwn.on("session-turn", async ({ terminalId }) => {
    const session = await spwn.sessions.get(terminalId);
    spwn.log(`${session.title} finished a turn`);
  });
  await spwn.untilStopped();
  ```

And two hook events fire around each run, for setup and teardown that belongs to the
workflow rather than a session: `workflow-started` and `workflow-stopped`. See
[Events](/spwn/reference/hooks/#events).

## Example: a GitHub board with personas

[`examples/workflows/github-board.ts`](https://github.com/spwn-gg/spwn/blob/main/examples/workflows/github-board.ts)
polls a GitHub Projects board and works each ticket according to the column it's in:

- **Personas** — an architect, an engineer, a reviewer — are plain objects in the script:
  a name, an agent, and how they think.
- **Columns** map to a persona and a prompt: *Ready* → the architect writes a plan,
  *In progress* → the engineer implements it, *In review* → the reviewer checks it.
- **Each ticket gets one session**, found again by its board item id. When the ticket moves
  to the next column, the same session — same worktree, same conversation — is prompted as
  the next persona.
- When a persona finishes, the workflow moves the ticket on. If the agent stops to ask
  something, the ticket waits for you.

Copy it into `.spwn/workflows/`, save a GitHub token with the `project` and `repo` scopes
in [Settings](/spwn/reference/settings/), and change the personas, columns and prompts to
fit how you work.

## TypeScript

Name a workflow `.ts` and write TypeScript. spwn strips the types before running it; it
doesn't type-check, so rely on your editor and the `spwn.d.ts` next to your workflows.

## Limits

- Workflows run in an embedded JavaScript engine (QuickJS), **not Node**: there's no
  `require`, no npm packages and no Node built-ins. Imports must be relative files inside
  `.spwn/workflows/`. Use `spwn.exec`, `spwn.fetch` and `spwn.fs` to reach outside.
- A workflow's top level runs whenever the panel lists workflows (to read `meta`), so keep
  side effects inside `main`.
- `spwn.state` is stored in spwn's data folder, per project and workflow. Logs are kept in
  memory: the last 2,000 lines of the last 50 runs, until spwn restarts.

## Next

- [Workflows API](/spwn/reference/workflows-api/) — everything `spwn` can do.
- [Workflow guide for AI agents](/spwn/workflows-agents.md) — one self-contained file to give an
  agent that's writing a workflow for you. "New workflow" also puts it in `.spwn/workflows/` as
  `AGENTS.md`, where coding agents find it on their own.
- [Hooks](/spwn/reference/hooks/) — per-session setup and teardown.
