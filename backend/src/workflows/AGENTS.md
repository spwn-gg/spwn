<!-- spwn:workflows-agents — written by spwn and kept up to date. Delete this line to keep your own edits. -->
# Writing spwn workflows

This directory holds **spwn workflows**: scripts that orchestrate AI coding agent sessions
for this project. This file is for an AI agent (or a person) writing one. The exact API is in
`spwn.d.ts` next to this file — read it before writing code; this file explains how to use it
correctly.

spwn is a session manager for coding agents. Each **session** is an agent (e.g. Claude Code)
running in its own git worktree on its own branch, visible in spwn's sidebar. A workflow drives
sessions from code: start them, prompt them, wait for replies, react to events.

## File format

A workflow is one ES module at the top level of `.spwn/workflows/`, named `<name>.js`, `.mjs`,
`.ts` or `.mts`. The file stem is its name.

```ts
import type { Spwn, WorkflowMeta } from "./spwn";

export const meta: WorkflowMeta = {
  name: "Triage",                       // display name (optional)
  description: "Fix new bug reports",   // optional
  keepAlive: false,                     // true: restart whenever main returns or throws
  inputs: {                             // shown as a form when the user clicks Run
    label: { type: "string", default: "bug", description: "Issue label to work" },
    limit: { type: "number", default: 5 },
  },
};

export default async function main(spwn: Spwn, inputs: { label: string; limit: number }) {
  // ...
}
```

- `export default` must be a function. `meta` is optional.
- Input `type` is `"string"` (default), `"number"` or `"boolean"`. Missing inputs get their
  `default`; `required: true` refuses to start without a value.
- Not workflows (importable helpers instead): files starting with `_`, `*.d.ts`, and anything in
  a subdirectory such as `lib/`. Import them with relative paths: `import { x } from "./lib/x.js"`.
- In JavaScript, get editor types with `// @ts-check` and `/** @param {import("./spwn").Spwn} spwn */`.

## The runtime is NOT Node.js

Workflows run in an embedded QuickJS engine. Modern JavaScript works (async/await, classes,
modules, `JSON`, `Map`, regexes, `setTimeout`/`setInterval`, `console.*`). There is:

- **no** `require`, npm packages, or Node built-ins (`fs`, `path`, `child_process`, `process`, `Buffer`)
- **no** browser globals (`fetch`, `window`, `URL` may be missing)
- **no** bare imports — only relative imports of files inside `.spwn/workflows/`

Use the `spwn` API instead:

| Need | Use |
|---|---|
| run a command | `await spwn.exec("git", ["status"])` or `await spwn.sh("npm test")` → `{ code, ok, stdout, stderr }` (non-zero exit does NOT throw; check `ok`) |
| read/write project files | `spwn.fs.read(path)`, `spwn.fs.write(path, text)`, `spwn.fs.exists(path)` (paths relative to the project root) |
| HTTP | `const r = await spwn.fetch(url, { method, headers, json, body })` → `{ status, ok, headers, body }`, `await r.json()` |
| GitHub | `await spwn.github.graphql(query, variables)` → `data`; `await spwn.github.rest("GET", "/repos/o/r/issues")` → parsed JSON. Uses the token saved in spwn's Settings. |
| remember things across runs | `spwn.state.get(key, fallback)`, `spwn.state.set(key, jsonValue)` (synchronous) |
| log | `spwn.log(...)`, `spwn.warn(...)`, `spwn.error(...)` or `console.*` — shown in the Workflows panel |
| wait | `await spwn.sleep(ms)` |

TypeScript is accepted but only has its types removed — it is never type-checked at run time.

## Sessions

```ts
const session = await spwn.sessions.create({
  title: "Fix #42",          // sidebar title
  key: "issue-42",           // YOUR stable id for this session (see "One session per thing")
  prompt: "Fix the bug described in issue #42 ...",  // sent once the agent is ready
  // agent: "claude",        // agent definition id; default is the user's default agent
  // onHookPrompt: (q) => ..., // see "Hooks"
});

const turn = await session.waitForTurn();
if (turn.blocked) {
  // The agent stopped to ask for permission or an answer. turn.screen is its screen text.
  spwn.warn(`needs a human: ${session.title}`);
  return;
}
spwn.log(turn.text); // the agent's reply to the prompt

const next = await session.prompt("Now add a test.");   // send + waitForTurn
```

Key facts:

- `create` starts a full session: new worktree + branch, the project's hooks, a sidebar entry.
  It resolves after the prompt was submitted (or right away with no `prompt`). If the agent is
  blocked before it can take the prompt (e.g. a folder-trust question), `create` throws.
- `waitForTurn()` resolves when the reply is complete AND the turn's commit hooks have run, so
  the work is committed on `session.branch`. It waits forever unless you pass `{ timeoutMs }`.
- **Always handle `turn.blocked`.** It means permission prompt or question. Options: leave it for a
  human (the session shows as waiting in the sidebar), or answer with `session.press("Enter")`,
  `session.press("Escape")`, or `session.send("...")`, then `waitForTurn()` again.
- `send(text)` then `waitForTurn()` waits for the reply to THAT text. Calling `waitForTurn()` twice
  without a new `send` waits for the next turn after the last one returned.
- Other methods: `status()`, `waitForStatus(status)`, `screen()`, `press(key)`, `interrupt()`,
  `lastMessage()`, `transcript()`, `delete()` (removes worktree and branch), `refresh()`.
- Session properties: `id`, `title`, `key`, `branch`, `baseBranch`, `cwd` (the worktree path),
  `agent`, `sessionId`. `status` is a METHOD: `await session.status()`.
- For work that needs no conversation, `await spwn.agents.run({ prompt })` does a one-shot headless
  run in its own worktree and resolves with `{ ok, error, text, sessionId }`.

## One session per thing (idempotence)

Workflows get re-run and restarted, so never assume a clean slate. The standard pattern:

```ts
let session = await spwn.sessions.find(ticket.id);    // survives restarts of spwn itself
if (!session) {
  session = await spwn.sessions.create({ key: ticket.id, title: ticket.title, prompt });
} else if (!session.awaitingTurn) {
  await session.send(prompt);
}
const turn = await session.waitForTurn();
```

`session.awaitingTurn` is true when a prompt was submitted but no `waitForTurn()` has returned its
reply yet — typically because the run was stopped or restarted mid-turn. It is kept across runs and
spwn restarts. Don't send the prompt again: `waitForTurn()` on the found session waits for the reply
to that prompt, and returns at once if the agent already finished while the workflow was stopped.

Track progress with `spwn.state` so work isn't repeated:

```ts
const done = spwn.state.get<Record<string, string>>("done", {});
if (done[ticket.id] === ticket.column) continue;   // already handled this stage
// ... do the work ...
done[ticket.id] = ticket.column;
spwn.state.set("done", done);                       // state values must be JSON
```

## Long-running workflows

A poller loops until stopped and sets `keepAlive: true` so crashes restart it (backoff 5s → 5 min):

```ts
export const meta = { keepAlive: true };

export default async function main(spwn: Spwn) {
  while (!spwn.stopping) {
    try {
      await pollOnce(spwn);
    } catch (e) {
      spwn.error("poll failed:", e);   // log and keep going; don't let one bad poll kill the loop
    }
    await spwn.sleep(60_000);          // throws when the run is stopped, which ends the loop
  }
}
```

- `keepAlive` restarts the workflow when `main` **returns** too, not only when it throws. Don't set
  it on a workflow meant to run once.
- When the user stops a run, every pending `spwn.*` promise rejects with an error whose
  `code === "stopped"`, and a busy loop is interrupted. Don't catch-and-continue in a way that
  ignores this: check `spwn.stopping`, or rethrow errors with `code === "stopped"`.
- To work several items concurrently, start the promises without awaiting each one, and cap the
  count yourself. Always attach `.catch(...)` to promises you don't await.
- A workflow that only reacts to events should `await spwn.untilStopped()` at the end of `main`.

## Events

```ts
spwn.on("session-turn", async (e) => {
  const s = await spwn.sessions.get(e.terminalId);   // e.terminalId is the Session id
  if (s) spwn.log(`${s.title} finished a turn`);
});
await spwn.untilStopped();
```

Events: `session-created`, `session-ready`, `session-turn`, `session-deleted` (payload includes
`terminalId`, `branch`, `worktree`, `turnUuid` for turns, and `workflow` if a workflow created
the session) and `status` (`{ terminalId, status }`). They fire for every session in the project,
including ones the user started. `spwn.on` returns an unsubscribe function.

## Hooks

The project may have shell hooks (`.spwn/hooks/`) that run when sessions are created, finish a
turn, or are deleted. They run for workflow sessions too. If a hook asks a question
(`spwn prompt`), your `onHookPrompt` handler answers it; without a handler it's declined:

```ts
await spwn.sessions.create({
  title: "with db",
  onHookPrompt: (q) => {          // { event, session, question, header, options: [{label}], multiSelect }
    if (q.question.includes("Seed the database")) return "Yes";   // return an option label
    return null;                                                  // decline
  },
});
```

## Rules that bite

1. **Top-level code runs whenever spwn lists workflows** (to read `meta`), with no `spwn` API and a
   2-second limit. Keep the top level to imports, constants, `meta`, and function definitions.
   Never `await` or cause side effects at the top level.
2. **Relative imports only**, and only within `.spwn/workflows/`.
3. **Handle `turn.blocked`** every time you wait for a turn.
4. **Use `key` + `sessions.find`** instead of creating a new session on every run.
5. **`spwn.exec` doesn't throw on failure**; check `result.ok`.
6. **`spwn.state` is synchronous and JSON-only.** Don't store functions, `Date`s (store ISO strings),
   or sessions (store `session.id` and use `sessions.get(id)`).
7. **Don't put secrets in workflows.** GitHub calls use the user's saved token automatically.
8. Workflows only run after the user allows workflows for the project in the Workflows panel.

## Checking your work

1. Type-check (if Node/TypeScript are available): `npx tsc --noEmit --strict --target es2022
   --module esnext --moduleResolution bundler <name>.ts` in this directory.
2. In spwn, open the project's **⚙ Workflows** panel: a load error (syntax, bad import, missing
   default export) shows on the workflow's card.
3. Click **Run**, watch the log, and open the sessions it links to.

## Example: a GitHub board, one session per ticket, a persona per column

```ts
import type { Spwn } from "./spwn";

export const meta = {
  keepAlive: true,
  inputs: { owner: { required: true }, project: { type: "number", required: true } },
};

const PERSONAS = {
  engineer: "You are a careful senior engineer. Keep changes focused and tested.",
  reviewer: "You are a demanding reviewer. Fix problems you find.",
};
const COLUMNS: Record<string, { persona: keyof typeof PERSONAS; ask: string }> = {
  "In progress": { persona: "engineer", ask: "Implement this ticket." },
  "In review": { persona: "reviewer", ask: "Review the work on this branch against the ticket." },
};

export default async function main(spwn: Spwn, inputs: { owner: string; project: number }) {
  const handled = spwn.state.get<Record<string, string>>("handled", {});
  while (!spwn.stopping) {
    for (const t of await readTickets(spwn, inputs)) {   // your GraphQL query
      const col = COLUMNS[t.column];
      if (!col || handled[t.id] === t.column) continue;
      const prompt = `${PERSONAS[col.persona]}\n\n${col.ask}\n\n# ${t.title}\n${t.body}`;
      let s = await spwn.sessions.find(t.id);
      if (s) await s.send(prompt);
      else s = await spwn.sessions.create({ key: t.id, title: t.title, prompt });
      const turn = await s.waitForTurn();
      if (turn.blocked) spwn.warn(`${t.title} needs a human`);
      handled[t.id] = t.column;
      spwn.state.set("handled", handled);
    }
    await spwn.sleep(60_000);
  }
}
```

A complete version (pagination, moving tickets between columns, concurrency) is in spwn's repo at
`examples/workflows/github-board.ts`.
