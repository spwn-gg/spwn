// Shared fork-lineage helpers. A project's agent sessions form a forest: each
// forked session nests under the session it was forked from, so lineage is visible
// in the sidebar tree (ProjectTree).

import type { ProjectRec, TerminalRec } from './types';

/**
 * Does this terminal hold a conversation (as opposed to a plain shell)?
 *
 * A session holds a conversation and owns a worktree; a shell is just a terminal.
 * Getting this wrong once left agent sessions out of the sidebar entirely — they
 * still existed and still held a branch, but became unreachable when their tab
 * closed — and made the delete confirm treat one as a shell, skipping the
 * at-risk warning.
 */
export function isSessionTerminal(t: TerminalRec): boolean {
	return t.kind === 'agent';
}

export interface SessionNode {
	t: TerminalRec;
	children: SessionNode[];
}

/** The parent session id for a terminal, if its parent is itself a session. */
export function parentOf(t: TerminalRec, ids: Set<string>): string | null {
	if (t.parentId && ids.has(t.parentId)) return t.parentId;
	// Legacy data: groupId pointed at the lineage root.
	if (t.groupId && t.groupId !== t.id && ids.has(t.groupId)) return t.groupId;
	return null;
}

/** Build a project's agent sessions into a branch forest (roots + nested forks). */
export function claudeForest(p: ProjectRec): SessionNode[] {
	const claudes = p.terminals.filter(isSessionTerminal);
	const ids = new Set(claudes.map((t) => t.id));
	const nodes = new Map<string, SessionNode>();
	for (const t of claudes) nodes.set(t.id, { t, children: [] });
	const roots: SessionNode[] = [];
	for (const t of claudes) {
		const pid = parentOf(t, ids);
		if (pid) nodes.get(pid)!.children.push(nodes.get(t.id)!);
		else roots.push(nodes.get(t.id)!);
	}
	return roots;
}
