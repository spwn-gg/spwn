// Land every finished session, one at a time, without anyone babysitting it.
//
// Parallel sessions each work against the base as it was when they forked. Merge them
// all at once and you get an N-way pile-up; merge them one at a time and every conflict
// is a two-way one, against a base that just moved by exactly one branch — and the
// session that wrote the code is still alive to resolve it.
//
// That is the whole trick here. After each land, the remaining sessions sync (which is
// where their conflicts surface, in their OWN worktrees), re-verify, and queue up
// again. Nothing is resolved by a bystander.
//
// You are not in this queue. You own the base branch, so your work is already ahead of
// every agent's by construction — there is nothing to schedule. What the queue owes you
// is the other half: never touching your working copy, and never stalling because you
// happen to have a file open. A landing that would overwrite something you're editing
// goes to a staging branch instead, and waits for you.
//
// spwn supplies the git; this file supplies the policy. Which sessions qualify, whether
// a conflict is handed back to its agent or left for you, whether an unverified merge
// may land — all of it is below, and all of it is yours to change.
//
// Use it: copy this file and `spwn.d.ts` (the Workflows panel's "New workflow" writes
// one) into your project's `.spwn/workflows/`, allow workflows for the project, and
// Run it. Set up a `session-integrate` hook first if you want `requireVerified`.

import type { Session, Spwn, WorkflowMeta } from "./spwn";

export const meta: WorkflowMeta = {
  name: "Merge queue",
  description: "Land finished sessions one at a time, syncing the rest after each",
  keepAlive: true,
  inputs: {
    pollSeconds: { type: "number", default: 120 },
    requireVerified: {
      type: "boolean",
      default: true,
      description: "Only land a session whose MERGED result passes session-integrate hooks",
    },
    resolveConflicts: {
      type: "boolean",
      default: true,
      description: "Hand a sync conflict back to the session's own agent to resolve",
    },
    deleteAfterMerge: { type: "boolean", default: false },
  },
};

interface Inputs {
  pollSeconds: number;
  requireVerified: boolean;
  resolveConflicts: boolean;
  deleteAfterMerge: boolean;
}

/** What to say to an agent whose sync stopped on conflicts. */
function conflictPrompt(base: string, files: string[]): string {
  return [
    `I merged \`${base}\` into this session's branch and it stopped on conflicts.`,
    "",
    `Conflicted ${files.length === 1 ? "file" : "files"}:`,
    ...files.map((f) => `- ${f}`),
    "",
    "The merge is still open in this worktree. Please resolve each conflict — you have",
    "the context for our side of it, so keep both intents where they do not actually",
    "disagree — then stage the files and commit the merge.",
    "",
    "Do not run `git merge --abort`: that would throw the sync away.",
  ].join("\n");
}

export default async function main(spwn: Spwn, inputs: Inputs) {
  while (!spwn.stopping) {
    await runQueue(spwn, inputs);
    await spwn.sleep(inputs.pollSeconds * 1000);
  }
}

async function runQueue(spwn: Spwn, inputs: Inputs) {
  const sessions = await spwn.sessions.list();

  // One pass = one land. After a land the base has moved, so every other session's
  // status is stale — recompute rather than trusting what we gathered a moment ago.
  for (const session of sessions) {
    if (spwn.stopping) return;
    const landed = await tryLand(spwn, session, inputs);
    if (landed) {
      spwn.log(`Landed ${session.title}. Re-syncing the rest against the new base.`);
      await resyncOthers(spwn, session.id, inputs);
      return; // next poll takes the next one, against a base that just moved
    }
  }
}

/** Walk one session as far towards landing as it can go. Returns true if it landed. */
async function tryLand(spwn: Spwn, session: Session, inputs: Inputs): Promise<boolean> {
  const status = await session.mergeStatus();
  if (!status.branch || status.ahead === 0) return false;

  // Never touch a session that's mid-turn: its worktree is being written right now.
  if (status.midTurn) {
    spwn.log(`${session.title}: working, leaving it alone.`);
    return false;
  }
  if (status.syncConflicts.length) {
    spwn.log(`${session.title}: a sync is still unresolved (${status.syncConflicts.join(", ")}).`);
    return false;
  }
  // Still a real obstacle (nowhere checked out to land into); note that a dirty base
  // checkout is NOT one — that case queues rather than blocks.
  if (status.blocker) {
    spwn.log(`${session.title}: ${status.blocker}`);
    return false;
  }

  // Bring the base in first, so landing is a fast-forward and any conflict surfaces
  // HERE, in this session's worktree, where its own agent can answer for it.
  if (!status.willFastForward) {
    const synced = await session.sync();
    if (synced.outcome === "conflicted") {
      if (!inputs.resolveConflicts) {
        spwn.warn(`${session.title}: conflicts in ${synced.conflicts.join(", ")} — left for you.`);
        return false;
      }
      spwn.log(`${session.title}: conflicts in ${synced.conflicts.join(", ")} — asking its agent.`);
      const turn = await session.prompt(
        conflictPrompt(status.baseBranch ?? "the base branch", synced.conflicts),
      );
      if (turn.blocked) {
        spwn.warn(`${session.title}: its agent stopped to ask something. Needs you.`);
        return false;
      }
      // Whether it actually finished the merge is a question for git, not the agent's
      // say-so: an agent that believes it resolved everything is not evidence.
      const after = await session.mergeStatus();
      if (after.syncConflicts.length) {
        spwn.warn(`${session.title}: still unresolved after the attempt. Needs you.`);
        return false;
      }
    } else if (synced.outcome === "replayedResolution") {
      spwn.log(`${session.title}: replayed an earlier resolution for ${synced.files.join(", ")}.`);
    }
  }

  // Both branches being green proves nothing about the two of them together.
  if (inputs.requireVerified) {
    const verified = await session.verifyMerge();
    if (verified.noScripts) {
      spwn.warn(
        `${session.title}: no session-integrate hook, so nothing was checked. ` +
          "Add one, or set requireVerified false to land unchecked.",
      );
      return false;
    }
    if (!verified.ok) {
      const failed = verified.runs.filter((r) => !r.ok).map((r) => r.script);
      spwn.warn(`${session.title}: merged result fails ${failed.join(", ")}. Not landing it.`);
      return false;
    }
  }

  // Landing decides its own destination: straight onto the base when that disturbs
  // nobody, onto staging when it would overwrite something the human has open. Either
  // way the agent is done and the queue moves on.
  if (status.humanBlockers.length) {
    spwn.log(
      `${session.title}: you have ${status.humanBlockers.join(", ")} open, so this queues ` +
        "on staging instead of touching your working copy.",
    );
  }
  spwn.log(await session.merge());
  if (inputs.deleteAfterMerge) await session.delete();
  return true;
}

/** After a land, pull the new base into everyone else so the next conflict is small. */
async function resyncOthers(spwn: Spwn, landedId: string, inputs: Inputs) {
  for (const other of await spwn.sessions.list()) {
    if (spwn.stopping || other.id === landedId) continue;
    const status = await other.mergeStatus();
    // Same rule as above: a running turn owns its worktree.
    if (!status.branch || status.midTurn || status.syncConflicts.length) continue;
    if (status.willFastForward) continue;

    const synced = await other.sync();
    if (synced.outcome === "conflicted") {
      spwn.log(`${other.title}: now conflicts in ${synced.conflicts.join(", ")}.`);
      if (inputs.resolveConflicts) {
        await other.send(conflictPrompt(status.baseBranch ?? "the base branch", synced.conflicts));
      }
    }
  }
}
