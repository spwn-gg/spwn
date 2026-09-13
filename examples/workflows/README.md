# spwn workflow examples

[Workflows](https://spwn-gg.github.io/spwn/guides/workflows/) are scripts in a project's
`.spwn/workflows/` that orchestrate agent sessions.

## `github-board.ts`

Works a GitHub Projects board with a persona per column:

- **Personas** (architect, engineer, reviewer) are plain objects: a name, an optional
  agent, and a preamble describing how they work.
- **Columns** map to a persona and a prompt — *Ready* → plan, *In progress* → implement,
  *In review* → review — and optionally the column to move the ticket to when done.
- **One session per ticket**, filed under the board item id with `spwn.sessions.find` /
  `create({ key })`, so the same worktree and conversation follow the ticket across the
  board.
- **`spwn.state`** remembers which column each ticket was last worked in, so a ticket is
  worked once per column, across restarts.
- **`keepAlive`** keeps the poller running; errors reading the board are logged and retried.

### Use it

1. In the project's **⚙ Workflows** panel, click **＋ New workflow** once (any name) so spwn
   writes `.spwn/workflows/spwn.d.ts`, then delete that starter file.
2. Copy `github-board.ts` into `.spwn/workflows/`.
3. Save a GitHub token with the `project` and `repo` scopes in spwn's **Settings**.
4. **Allow workflows** for the project, click **Run**, and fill in the owner and project
   number. Tick **Start with spwn** to keep it running.

Then edit `PERSONAS` and `COLUMNS` to match your board and your process.
