# 002 — Workers, priority and bounded lag

**Status:** draft · **Supersedes parts of:** [001](./001-cell-architecture.md)

## Summary

RFC 001 modelled two kinds of participant: a human (local, privileged, special-cased)
and agents (local, in the cell). That split doesn't survive hosting — agents are remote
too, and the cell is only the authority for filesystem and git state, not the place work
happens.

The generalisation: **one kind of participant, a worker**, carrying a priority. The human
is a worker with high priority. The test that this is the right abstraction is that the
old behaviour falls out without a special case — `human_blockers` becomes
`blockers_above(my_priority)`, and #74 is the instance where that priority is the top of
the order.

Two decisions this RFC records:

- **Conservative staging.** The lowest priority class stages by default and promotes only
  when no higher-priority worker claims those paths, so prioritised workers never pay a
  merge caused by work beneath them.
- **Bounded lag.** Staging carries a deadline. Conservative staging without one has
  unbounded staleness; with one, the guarantee becomes two-sided and statable.

## 1. The worker

```
worker = (id, priority, working_set, lease)
```

| Field | Notes |
|---|---|
| `id` | stable across reconnects |
| `priority` | **assigned by the control plane, never claimed by the worker** |
| `working_set` | paths it currently holds |
| `lease` | TTL, renewed by heartbeat; expiry empties the working set |

Priority being control-plane-assigned is a trust boundary, not a detail. A self-declared
priority is decorative: every agent declares itself urgent. It is also painful to
retrofit, because by then clients depend on setting it.

`working_set` is obtained differently depending on where the worker runs, and the
difference is load-bearing:

| Worker | Working set | Freshness |
|---|---|---|
| Cell-resident | cell computes `dirty_paths` | current |
| Remote agent | reported on heartbeat | stale by ≤ 1 interval |
| Remote human (IDE) | client reports open/modified | stale by ≤ 1 interval |

## 2. The landing rule

```
blockers = paths(my landing) ∩ ⋃ { working_set(w) : priority(w) > priority(me) }

blockers empty  → land on base
otherwise       → queue on staging
```

This is #74 with one substitution. Under conservative staging (§4) the lowest class
inverts its default: it stages unless it is the highest-priority claimant of those paths.

## 3. What the guarantee actually is

Worth stating precisely, because the strong version is easy to read in and we would be
overclaiming.

- **Cell-resident prioritised worker:** *we will not overwrite your files.* Enforceable —
  the cell holds the tree and git refuses the write. This is what #74 delivers today.
- **Remote prioritised worker:** the cell cannot touch their files, so that promise is
  vacuous. What it becomes is *we will not move the base under you* → **your next pull
  will be clean.** A best-effort scheduling property, not an enforced invariant.

Both are valuable. They are not the same promise, and product copy should not blur them.

## 4. Conservative staging

The lowest priority class stages by default. Promotion happens when

```
paths(staged work) ∩ ⋃ { working_set(w) : priority(w) > staged.priority } = ∅
```

Rationale: a prioritised worker should never inherit a merge caused by work beneath it.
The cost is that the base lags and something must promote — which §5 bounds.

Above the lowest class, landing stays optimistic: land unless currently blocked. Strict
staging everywhere would make the base permanently stale for no benefit.

## 5. Bounded lag

Staging carries a deadline. **This is what makes conservative safe to choose** — without
it, conservative's staleness is unbounded and the base can drift indefinitely behind
what has actually been built.

**One knob, two guarantees.** Bounding how long staged work may wait also bounds how long
any holder may block, because a hold that outlasts the deadline stops mattering. Configure
the lag; the hold budget follows.

Graduated, not abrupt:

| Stage | Behaviour |
|---|---|
| `T_soft` | Notify the holder: "N changes have waited T, touching files you have open." Converts a silent hold into a decision. |
| `T_hard` | Promote regardless. |

Silently overriding a priority is worse than asking, and most holds end the moment
someone is told they are holding.

**The deadline behaves differently by locality**, following §3:

- **Cell-resident holder:** at `T_hard` the merge is attempted and *git itself refuses*
  if the holder's tree would be overwritten. The deadline cannot force it. Escalate to the
  human instead — this is the one case that genuinely needs a person.
- **Remote holder:** nothing physical blocks the merge, so it lands. Their next pull
  carries a conflict. That is the bounded cost of remote priority, and it should be said
  out loud rather than discovered.

## 6. Invariants

The model cannot deadlock, and it is worth recording *why*, because the reasons are
one-line changes away from being false.

Checked against Coffman: **mutual exclusion** — no, claims are advisory and two workers
may edit the same path; **hold and wait** — no, a deferred worker does not wait, its work
goes to staging and it moves on; **no preemption** — no, leases expire; **circular wait**
— no, deferral follows a total order, so the waits-for graph is a DAG. A cycle would
require `priority(W) > priority(W)`.

More simply: *blocked* here means **undelivered**, not **stalled**. The worker already
finished.

Two changes reintroduce cycles, and both are plausible feature requests:

1. **Path-dependent priority.** "The platform team owns `infra/`, the app team owns
   `app/`." Priority becomes a function of `(worker, path)`, and A can defer to B on one
   path while B defers to A on another. Keep priority a **global total order over
   workers**.
2. **The comparison operator.** Defer on `priority(other) > mine` and equal-priority
   workers race, which git arbitrates. Defer on `>=` and two equal workers each defer to
   the other. **`>` is load-bearing** — it deserves an assertion and a test, because it is
   a one-character change someone will make while trying to be fair to ties.

Leases exist for **crashed holders**, not for deadlock. A worker that dies holding a claim
would otherwise hold it forever. TTL should scale with priority: a human is trusted to go
to lunch mid-edit; a batch agent is not.

The residual risk is **starvation**, bounded by §5.

## 7. Implementation gap this exposes — **fixed**

Staging as shipped in #74 **does not track the base**. `land_session` opens staging at the
base tip and advances it to session branches; nothing brings later base commits in. So
once the human commits, staging is behind, and integrating is a three-way merge that can
conflict — verified:

```
staging ahead of main by: 1
after the human commits, is main an ancestor of staging?
  NO — staging is behind; deadline merge is a real three-way merge
```

A deadline that fires into a conflicting three-way merge is the opposite of what §5
promises. Staging must track the base so the deadline merge is always a fast-forward.

The fix needs no worktree, reusing #69's machinery — verified:

```sh
TREE=$(git merge-tree --write-tree main spwn/staging/main | head -1)
C=$(git commit-tree "$TREE" -p spwn/staging/main -p main -m "spwn: track base")
git update-ref refs/heads/spwn/staging/main "$C"
# → staging now contains main; human's checkout untouched; both sides' content carried
```

When that merge-tree conflicts, the conflict belongs to the staged sessions, not the
human — hand it back via #67's handoff, which is what that mechanism is for.

**Shipped** as `gitwt::track_base`, called before a session syncs, before a landing
queues, and before the human integrates. Writing the test first showed the bug was worse
than described above: sessions sync from `landing_target`, which *is* staging once a
queue is open, so every commit the human made after the queue opened was **invisible to
every agent indefinitely**. Not just an awkward integration later — agents quietly
working against a base that had moved, which is the human-priority story failing in its
least visible direction. `integrate_staging` now names whose conflict it is rather than
handing a person a raw merge failure for a disagreement between queued sessions and
their own commits.

## 8. Corrections to RFC 001

- **§1 (co-location is a correctness boundary)** — weaker than written. The argument
  leaned hardest on rerere, but the cell can **train its rr-cache from landed merge
  commits**: replay the merge from its parents, apply the merge's tree, `git rerere`.
  Verified end to end with a worker that had rerere disabled, and a third worker then got
  the conflict auto-resolved. Co-location is convenient, not load-bearing.
- **§3.2 (disk is the binding constraint)** — mostly dissolves. If workers are remote and
  hold their own checkouts, the 3.5 GB of build artifacts lives on the worker, where it is
  already paid. The cell holds base, staging and transient trial worktrees only.
- **§3.5 (verify storm)** — dissolves. A verify is "here is a merged tree, check it",
  which is dispatchable to a worker. It stops being a cell resource problem and becomes
  scheduling, like everything else here.
- **§8.1 (the remote human)** — resolved. A remote human is a worker that reports a
  working set, same as any other.

## 9. Open questions

1. **Deadline defaults.** `T_soft` and `T_hard` are guesses without usage data. They
   should be per-priority-class, and the class that matters is the top one — how long may
   a human hold a file before agent work lands anyway?
2. **Working-set reporting for IDEs.** Open buffers, modified buffers, or the whole dirty
   tree? Open-but-unmodified is a weaker claim than dirty, and treating them alike will
   over-block.
3. **Promotion granularity.** Promote the whole staging branch, or the subset of staged
   work whose paths are now clear? The latter is better behaviour and needs staging to be
   splittable, which it currently is not.
4. **Does a tie ever need breaking?** §6 says equal priority races. If that turns out to
   be unacceptable, the fix is a finer total order (e.g. by queue age), never `>=`.
