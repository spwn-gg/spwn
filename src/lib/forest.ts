// Shared fork-lineage helpers. A project's Claude sessions form a forest: each
// forked session nests under the session it was forked from, so lineage is visible
// in the sidebar tree (ProjectTree).

import type { ProjectRec, TerminalRec } from './types';

export interface SessionNode {
	t: TerminalRec;
	children: SessionNode[];
}

/** The parent session id for a terminal, if its parent is itself a Claude session. */
export function parentOf(t: TerminalRec, ids: Set<string>): string | null {
	if (t.parentId && ids.has(t.parentId)) return t.parentId;
	// Legacy data: groupId pointed at the lineage root.
	if (t.groupId && t.groupId !== t.id && ids.has(t.groupId)) return t.groupId;
	return null;
}

/** Build a project's Claude sessions into a branch forest (roots + nested forks). */
export function claudeForest(p: ProjectRec): SessionNode[] {
	const claudes = p.terminals.filter((t) => t.kind === 'claude');
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
