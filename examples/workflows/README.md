# spwn workflow examples

[Workflows](https://spwn-gg.github.io/spwn/guides/workflows/) are scripts in a project's
`.spwn/workflows/` that orchestrate agent sessions.

## `merge-queue.ts`

Lands finished sessions one at a time, so no conflict is ever more than two-way:

- **One land per pass.** Parallel sessions each work against the base as it was when they
  forked; merging them all at once is an N-way pile-up. Merging one at a time means every
  conflict is against a base that moved by exactly one branch.
- **Conflicts go to the session that caused them.** `session.sync()` brings the base into
  the session's *own* worktree, so a conflict surfaces where its agent is still live and
  still holds the conversation explaining the code. The workflow hands it back with
  `prompt()` — and then re-reads `mergeStatus()`, because an agent believing it resolved
  everything is not evidence.
- **Verified against the merged result**, not the branch. `session.verifyMerge()` builds
  and tests the combination via `session-integrate` hooks. Two green branches say nothing
  about the two of them together.
- **Sessions mid-turn are left alone.** A running turn owns its worktree.
- **Policy is all in the file** — which sessions qualify, whether conflicts are delegated,
  whether an unverified merge may land.

This is the design test for everything under it: the queue is a *script*, not a spwn
feature. spwn supplies the git; the file supplies the order and the judgement.

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
