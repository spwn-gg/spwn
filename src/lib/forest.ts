// Shared fork-lineage helpers. A project's agent sessions form a forest: each
// forked session nests under the session it was forked from, so lineage is visible
// in the sidebar tree (ProjectTree).

import type { ProjectRec, TerminalRec } from './types';

/**
 * Does this terminal hold a conversation (as opposed to a plain shell)?
 *
 * Covers BOTH transports: `agent` (the agent's TUI in an rmux pane) and the legacy
 * `claude` sidecar. Filtering on `kind === 'claude'` alone leaves TUI sessions out
 * of the sidebar entirely — they'd still exist, still hold a worktree and a branch,
 * but be unreachable once their tab is closed.
 */
export function isSessionTerminal(t: TerminalRec): boolean {
	return t.kind === 'claude' || t.kind === 'agent';
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
