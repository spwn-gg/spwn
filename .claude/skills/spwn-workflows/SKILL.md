---
name: spwn-workflows
description: Write, run, debug, or change spwn workflows — the JS/TS scripts in a project's .spwn/workflows that orchestrate agent sessions (spwn.sessions, spwn.agents.run, spwn.github, spwn.on, keepAlive). Use when asked to create or edit a workflow, automate agents over a board/issues/tickets, run or stop a workflow against the dev server, read a run's log, or change the workflow runtime or its spwn API.
---

# spwn workflows

A workflow is an ES module in `<project>/.spwn/workflows/` (top-level `.js`/`.ts`/`.mjs`/
`.mts`; `_*`, `*.d.ts` and subfolders are importable modules, not workflows):

```js
export const meta = { name, description, keepAlive, inputs: { key: { type, default, description, required } } };
export default async function main(spwn, inputs) { … }
```

## Sources of truth

- **API surface:** `backend/src/workflows/spwn.d.ts` (types, also written into projects by
  "New workflow"), implemented by `backend/src/workflows/prelude.js` (JS wrapper) over
  `backend/src/workflows/host.rs` (ops: `__host(op, json)` / `__hostSync(op, json)`).
- **Runtime:** `backend/src/workflows/runtime.rs` (QuickJS via rquickjs, module loader,
  meta reading), `ts.rs` (oxc type stripping).
- **Runs, discovery, trust, autostart, keep-alive, workflow hooks:** `backend/src/workflows/mod.rs`.
- **Tests:** `backend/src/workflows/tests.rs` (real runtime, no rmux).
- **UI:** `src/lib/Workflows.svelte`; docs `docs/src/content/docs/guides/workflows.md` and
  `reference/workflows-api.md`; example `examples/workflows/github-board.ts`.

**Changing the API means changing four places together:** `host.rs` (op), `prelude.js`
(wrapper), `spwn.d.ts` (types) and `reference/workflows-api.md`. Add a test in `tests.rs`.

## Run one against the dev server

With the dev loop up (`spwn-dev` skill), everything is `POST /api/invoke/<command>` on
`localhost:4317` (camelCase JSON):

```sh
api() { curl -s -X POST "localhost:4317/api/invoke/$1" -H 'content-type: application/json' -d "${2:-{\}}"; }
api list_projects | jq '.[] | {id, name, directory}'
api list_workflows '{"projectId":"<id>"}' | jq                    # meta + load errors + latest run
api set_workflows_enabled '{"projectId":"<id>","enabled":true}'    # the per-project trust gate
api new_workflow '{"projectId":"<id>","name":"hello","typescript":false}'
api run_workflow '{"projectId":"<id>","name":"hello","inputs":{}}' | jq -r .id
api workflow_log '{"runId":"<run>"}' | jq -r '.[] | "\(.level) \(.msg)"'
api workflow_runs '{"projectId":"<id>"}' | jq '.[] | {workflow, status, error, restarts}'
api stop_workflow '{"runId":"<run>"}'
```

Live events on the WebSocket: `workflow://runs` (run info) and `workflow://log/<runId>`.

## Writing workflows — what bites

- **QuickJS, not Node.** No `require`, npm packages, or Node built-ins; only relative
  imports inside `.spwn/workflows`. Reach out with `spwn.exec` / `spwn.sh`, `spwn.fetch`,
  `spwn.fs`, `spwn.github`.
- **The top level runs when the panel lists workflows** (to read `meta`, 2s limit, no
  `spwn`). Keep side effects and awaits inside `main`.
- **Stop rejects everything.** Every pending `spwn.*` promise rejects with `code: "stopped"`;
  a busy loop is interrupted. A `try/catch` around `spwn.sleep` in a loop should check
  `spwn.stopping`.
- **`keepAlive` restarts on return too**, not only on throw. A finite workflow shouldn't set it.
- **Idempotence is your job.** Use `sessions.create({ key })` + `sessions.find(key)` for
  one-session-per-thing, and `spwn.state` to remember what's been handled.
- **`waitForTurn()` can return `{ blocked: true, screen }`** (permission prompt / question)
  instead of a turn. Handle it. `create({ prompt })` rejects if the agent is blocked before
  it can take the prompt (e.g. a folder-trust gate).
- **Hook questions** in a workflow's sessions go to `onHookPrompt` (on `create`/`find`/`get`
  /`agents.run`); without one they're declined. `workflow-started`/`workflow-stopped` hooks
  run in the project dir and never prompt.
- **TypeScript is stripped, not checked.** Check with `tsc --noEmit` against `spwn.d.ts`.
- **Logs are in memory** (2,000 lines × 50 runs) and gone after a backend restart;
  `spwn.state` persists (`<app data>/workflows/<projectId>/<name>.state.json`).
