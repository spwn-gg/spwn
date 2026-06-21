// Frontend state: the project list, the open terminal tabs, and which is active.

import { writable, derived, get } from 'svelte/store';
import { listProjects } from './ipc';
import type { ProjectRec, TerminalKind } from './types';

/** A pane is a terminal (shell/claude) or the project's context composer. */
export type PaneKind = TerminalKind | 'context';

export const projects = writable<ProjectRec[]>([]);

/** Reload the project list from the backend store, and sync open tab titles to
 * the (possibly Claude-renamed) terminal records. */
export async function refreshProjects() {
	const ps = await listProjects();
	projects.set(ps);
	const titleById = new Map<string, string>();
	for (const p of ps) for (const t of p.terminals) titleById.set(t.id, t.title);
	openTabs.update((tabs) =>
		tabs.map((tab) =>
			tab.terminalId && titleById.has(tab.terminalId)
				? { ...tab, title: titleById.get(tab.terminalId)! }
				: tab
		)
	);
}

/** An open terminal tab. `terminalId` is filled once the backend opens it (new
 * terminals) or is known upfront (reattaching an existing terminal). */
export interface OpenTab {
	key: string;
	projectId: string;
	kind: PaneKind;
	title: string;
	terminalId?: string;
	cwd?: string;
	sessionId?: string;
	claudeResume?: string;
	claudeFork?: string;
	parentTerminalId?: string;
	initialPrompt?: string;
}

export const openTabs = writable<OpenTab[]>([]);
export const activeTabKey = writable<string | null>(null);

/** Whether the settings panel is shown. */
export const showSettings = writable(false);

export const activeTab = derived(
	[openTabs, activeTabKey],
	([$tabs, $key]) => $tabs.find((t) => t.key === $key) ?? null
);

export const showTranscript = writable(true);

let counter = 0;
function tabKey() {
	return `tab-${++counter}`;
}

/** Open a tab. Reattaching a known terminal focuses an existing tab if present. */
export function openTab(spec: Omit<OpenTab, 'key'>) {
	if (spec.terminalId) {
		const existing = get(openTabs).find((t) => t.terminalId === spec.terminalId);
		if (existing) {
			activeTabKey.set(existing.key);
			return;
		}
	}
	// One context composer per project — focus it if already open.
	if (spec.kind === 'context') {
		const existing = get(openTabs).find(
			(t) => t.kind === 'context' && t.projectId === spec.projectId
		);
		if (existing) {
			activeTabKey.set(existing.key);
			return;
		}
	}
	const key = tabKey();
	openTabs.update((ts) => [...ts, { key, ...spec }]);
	activeTabKey.set(key);
}

export function closeTab(key: string) {
	openTabs.update((ts) => ts.filter((t) => t.key !== key));
	activeTabKey.update((cur) => {
		if (cur !== key) return cur;
		const rest = get(openTabs);
		return rest.length ? rest[rest.length - 1].key : null;
	});
}

export function setTabTerminalId(key: string, terminalId: string) {
	openTabs.update((ts) => ts.map((t) => (t.key === key ? { ...t, terminalId } : t)));
}

export function setTabSession(key: string, sessionId: string) {
	openTabs.update((ts) => ts.map((t) => (t.key === key ? { ...t, sessionId } : t)));
}
