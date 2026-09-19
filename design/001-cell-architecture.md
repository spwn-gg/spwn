# 001 — The cell: N agents on one repo

**Status:** draft · **Context:** moving spwn from local orchestration to hosted infrastructure

## Summary

The unit of hosting is not a session container. It is a **cell**: one repository's object
store and ref authority, with K session worktrees sharing it, on one machine.

Sessions are disposable inside a cell. The cell is rebuildable from durable storage. You
scale by adding cells (repos), not by splitting a repo across machines.

This document argues why the cell boundary is a *correctness* boundary rather than a
performance optimisation, then pressure-tests it at N=50 agents against measurements
taken on this repository.

**Headline findings:**

- At N=50, the current status path costs **~1265 ms per refresh cycle** against a
  1200 ms debounce — it stops converging right about there — and the answer it produces
  is *wrong*, because `MAX_OVERLAP_SIBLINGS = 12` silently truncates. Restructuring the
  same work as one shared index pass costs **~104 ms** and is correct. This is the first
  thing to fix.
- Git is not the storage problem. This repo's `.git` is **4.1 MB**; its build artifacts
  are **3.5 GB** — 850× larger. Disk is the binding constraint on cell size, and it has
  nothing to do with git.
- The global `Mutex<ProjectStore>` + whole-file `projects.json` write is the first thing
  to break under concurrency, before any git-level contention appears.

## 1. Why the cell boundary is about correctness

Co-location is not a latency optimisation we could trade away. Several behaviours we
have already shipped depend on sessions sharing one git directory:

| Behaviour | Depends on |
|---|---|
| `rerere` replay across sessions (#68) | the shared `rr-cache` in the **common git dir** |
| `merge_preview` / collision detection (#60) | shared refs, `merge-tree` against a local object store |
| `overlaps` (#71) | diffing sibling branches from one worktree |
| Staging landings (#74) | ref moves being local and atomic |
| COW-seeded worktrees | one filesystem, `cp -c` / reflink |

Split sessions across machines and `rerere` stops replaying **silently** — one agent's
resolution never reaches another, and nothing reports an error. That is the clearest
case: the distributed version of this system is not a slower version, it is a system
that quietly loses a feature.

So the cell is drawn where it is because that is where the invariants hold.

## 2. The cell, defined

```
┌─ cell (one machine, one repo) ────────────────────┐
│                                                   │
│   object store + refs  ← the shared invariant     │
│   rr-cache                                        │
│                                                   │
│   ├── worktree: session A ─┐                      │
│   ├── worktree: session B  │ K sessions,          │
│   ├── worktree: session C  │ disposable           │
│   └── worktree: trial      ┘ (#69, transient)     │
│                                                   │
└───────────────────────────────────────────────────┘
                     ↕
        durable storage (S3): cell rebuildable
```

- **In the cell:** object store, refs, rerere cache, session worktrees, trial worktrees,
  the hooks that run in them.
- **Not in the cell:** durable storage, tenancy/auth, scheduling across cells, the
  human's own checkout (see §6).
- **Cell identity:** one repo, one tenant. A repo with 50 concurrent agents is one large
  cell, not fifty small ones.

## 3. Pressure test at N=50

Measured on this repository (git 2.43, 52 refs, one commit per session branch).
Reproduce with `design/bench/cell-bench.sh`.

### 3.1 The status storm — O(N²), and a wall

`session_merge_status` runs per session. It calls `overlaps_with`, which diffs each
sibling session's branch. With every session observed — which is the hosted case, since
agents run headless and the backend computes status for all of them — the work is O(N²).

| Work | Diffs | N=20 | N=50 |
|---|---:|---:|---:|
| One session's overlap pass (capped at 12) | 12 | 28 ms | 27 ms |
| Full cycle, capped at 12 | 12N | 565 ms | **1 265 ms** |
| Full cycle, uncapped (true O(N²)) | N² | 933 ms | **5 223 ms** |
| **One shared index pass** | N | 46 ms | **104 ms** |

Run-to-run variance is ±25%; an earlier manual run measured 1665 / 6511 / 130 ms at
N=50. The ratios are stable: the shared index is ~12× the capped cycle and ~50× the
uncapped one.

Two separate failures here, and the second is worse than the first:

1. **It stops converging.** The debounce in `SessionStatusStrip` is 1200 ms. At N=50 a
   cycle takes ~1265 ms, so the crossover lands around N≈45–50 and refreshes begin
   queueing behind each other. The margin is thin enough that repo size or a slower
   disk moves it well below 50.
2. **It is wrong.** `MAX_OVERLAP_SIBLINGS = 12` was a guess made without usage data
   (flagged at the time). Past N=12 it silently reports *some* overlaps and misses the
   rest. An advisory that quietly stops being complete is worse than one that is absent,
   because people calibrate on it.

**Fix:** invert the computation. Build one `path → [session]` index per cell per cycle —
50 diffs, 130 ms — and answer every session's overlap query from it. That is ~12×
faster than the capped version, ~50× faster than uncapped, **and** it removes the cap,
so the answer becomes correct. This is the single highest-value change in this document.

### 3.2 Disk — the actual binding constraint

| | Size | Files |
|---|---:|---:|
| `.git` | **4.1 MB** | — |
| Checkout (tracked files) | 5.3 MB | — |
| `node_modules` | 75 MB | 1 823 |
| `backend/target` | **3.5 GB** | 5 550 |

Build artifacts are **850× the size of the git data**. Any architecture that carefully
decomposes git storage while leaving the build cache alone has optimised 0.1% of the
footprint.

COW seeding makes a new worktree's copy free *at creation* (`cp -c`, reflink). But
copies diverge as each agent builds, and diverged blocks materialise. Worst case at
N=50 is 50 × 3.5 GB = 175 GB of divergence on one cell.

**Consequence:** cells are sized by **disk**, not CPU or git. And the dependency/build
cache — not git — is where storage engineering should go.

### 3.3 Loose objects and repack contention

50 per-turn commits produced **150 loose objects** (3 per commit — a commit, a tree, a
blob — ~600 KB). Extrapolating: 50 agents × 20 turns/hour ≈ 1000 commits/hour ≈
**3000 loose objects/hour**.

That requires regular repacking — and repacking is the most contention-prone operation
in a shared object store, because it rewrites packs while N agents are writing. Git's
`gc.pruneExpire` grace period handles the single-writer case; N concurrent writers plus
prune is where the sharp edges are.

**Tension to resolve:** loose-object pressure wants frequent repacks; concurrency wants
rare ones.

### 3.4 Staging serialisation

Since #74, once a queue is open every landing goes through staging, and each landing
requires the session to have synced onto staging's tip. So landings **serialise**:

```
sync(A) → land(A) → sync(B) → land(B) → sync(C) → land(C) → …
```

Each cycle contains a real merge in a worktree. This is inherent to keeping one line of
work, not an implementation artifact — but at N=50 the queue's throughput becomes the
system's throughput.

**Open:** batch M ready sessions per cycle instead of one, or accept serialisation and
make each cycle fast.

### 3.5 Verify storm

`session-integrate` (#69) runs a project's full test suite in a trial worktree. N
concurrent verifies is N concurrent test suites on one machine.

This dwarfs every git cost in this document by orders of magnitude and is not a git
problem at all. It needs a scheduler with a concurrency limit, not a faster merge.

### 3.6 The global mutex

Every command takes `state.store.lock()` on a single `Mutex<ProjectStore>`, and
`persist()` rewrites the whole `projects.json` on every change — O(total state) per
write, for all tenants, all repos, all sessions.

This is the shape of a single-user desktop app, and it is almost certainly what breaks
first in practice, before any git-level contention appears.

## 4. What breaks first

Ordered by when you'd actually hit it:

1. **Global mutex + whole-file persist** — concurrency, any N > a handful
2. **Overlap O(N²) + wrong answers past N=12** — measured wall at N≈45–50, and the
   correctness failure starts at 13
3. **Verify concurrency** — the moment more than a few agents verify at once
4. **Disk divergence** — grows with agent-hours, not agent count
5. **Repack contention** — hours to days of sustained load
6. **Ref contention** — not before several hundred refs; `reftable` is the answer when it
   arrives (default in Git 3.0)

Note that 1, 3 and 4 are not git problems. Only 5 and 6 are, and they are the furthest
out.

## 5. Design responses

| Problem | Response |
|---|---|
| Overlap O(N²) | One `path → [session]` index per cell per cycle; delete the sibling cap |
| Status storm | Push, don't poll — the cell knows when a ref moves; emit events instead of N sessions re-diffing |
| Global mutex | Per-cell state, not global. SQLite or an event log per cell; never a whole-file rewrite |
| Disk divergence | Size cells by disk; dedupe below git (overlayfs/btrfs) rather than per-worktree copies; evict idle worktrees, keep branches |
| Repack contention | Schedule repacks in a quiesce window, or record them as log entries (Continuity's approach) so state stays reconstructible |
| Verify storm | A queue with a concurrency limit, sized to the cell's cores |
| Staging throughput | Batch ready sessions per cycle |

## 6. Sizing

On present evidence a cell targets **~25–50 active sessions**, bounded by disk first and
verify concurrency second — *after* the overlap index and mutex fixes, without which the
practical ceiling is closer to 12.

Beyond that, shard by repo. Do not shard a repo across cells: §1 explains what that
costs.

## 7. Out of scope: storage

Cursor's [Git at any scale](https://cursor.com/blog/git-at-any-scale) (Aug 2026) covers
git-on-object-storage thoroughly — a write-ahead log in S3 as source of truth, on-disk
repos as warm cache, one replica to accept a transaction instead of a quorum.

We should not build this. Our differentiation is integration semantics (where a conflict
surfaces, who resolves it, what gets verified), not bytes. Keep storage behind an
interface, pick the cheapest thing that works, and revisit only when cost or a capability
gap forces it. §3.2 is the supporting argument: git is 0.1% of our footprint.

## 8. Open questions

1. **The remote human.** `human_blockers` (#74) reads dirty paths from the project
   checkout on local disk. Hosted, the human is on their laptop with their own clone and
   that signal does not exist. Human-priority needs a different input — the IDE
   reporting open files, or inference from their unpushed branch. This is a redesign,
   not a port, and it is the part of #74 that does not survive the move.
2. **Hook isolation.** `session-integrate` executes customer shell scripts as a
   first-class part of the merge path. Locally that is the best feature; hosted it is the
   entire security model, and it needs microVM/gVisor-class isolation, not containers.
3. **Cell migration.** What happens to running sessions when a cell must move? rmux panes
   are stateful and survive restarts locally; they do not survive a machine.
4. **Does a cell span tenants?** Assumed no. Worth confirming — it has large cost
   implications for small repos.
