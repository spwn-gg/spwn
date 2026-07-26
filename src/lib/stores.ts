// Frontend state: the project list, the open terminal tabs, and which is active.

import { writable, derived, get } from 'svelte/store';
import { listProjects } from './ipc';
import type { ProjectRec, SessionStatus, TerminalKind } from './types';

/** A pane is a terminal (shell/claude), the context composer, or the scheduler. */
export type PaneKind = TerminalKind | 'context' | 'schedule';

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
	projectName?: string;
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

/** Text queued to be dropped into a session's input box (keyed by terminal id). */
export const pasteToInput = writable<{ terminalId: string; text: string } | null>(null);

/** Sessions currently mid-turn (an agent may be writing files) — restores gate on this. */
export const busySessions = writable<Set<string>>(new Set());
export function setSessionBusy(sessionId: string, busy: boolean) {
	busySessions.update((s) => {
		const n = new Set(s);
		if (busy) n.add(sessionId);
		else n.delete(sessionId);
		return n;
	});
}

/** Which session's code is currently materialized in each project dir (projectId → sessionId). */
export const activeCodeSession = writable<Record<string, string>>({});

/** Sessions with a `.spwn` hook executing right now (terminalId → event name).
 * Drives the live spinner on tabs and project-tree rows. */
export const hookRunning = writable<Map<string, string>>(new Map());
export function setHookRunning(terminalId: string, event: string | null) {
	hookRunning.update((m) => {
		const n = new Map(m);
		if (event) n.set(terminalId, event);
		else n.delete(terminalId);
		return n;
	});
}

/** Live Claude session status (terminalId → status), fed by the backend `claude://status`
 * event. The single source of truth for the sidebar/tab-bar spinner and attention dots —
 * it tracks background sessions the pane can't see. `idle`/null removes the entry. */
export const claudeStatus = writable<Map<string, SessionStatus>>(new Map());
export function setClaudeStatus(terminalId: string, status: SessionStatus | null) {
	claudeStatus.update((m) => {
		const n = new Map(m);
		if (status && status !== 'idle') n.set(terminalId, status);
		else n.delete(terminalId);
		return n;
	});
}

/** Claude permission/execution mode. Kept in sync with InputBar's local union. */
export type PermMode = 'default' | 'acceptEdits' | 'plan' | 'auto';
const CLAUDE_MODE_KEY = 'cm.claudeMode';
const VALID_MODES: PermMode[] = ['default', 'acceptEdits', 'plan', 'auto'];

function loadClaudeMode(): PermMode {
	if (typeof localStorage === 'undefined') return 'default';
	const v = localStorage.getItem(CLAUDE_MODE_KEY) as PermMode | null;
	return v && VALID_MODES.includes(v) ? v : 'default';
}

/** The last execution mode the user selected — seeds new Claude panes so the
 * choice sticks across panes and app restarts. */
export const claudeMode = writable<PermMode>(loadClaudeMode());
claudeMode.subscribe((m) => {
	if (typeof localStorage !== 'undefined') localStorage.setItem(CLAUDE_MODE_KEY, m);
});

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
	// One context composer / scheduler per project — focus it if already open.
	if (spec.kind === 'context' || spec.kind === 'schedule') {
		const existing = get(openTabs).find(
			(t) => t.kind === spec.kind && t.projectId === spec.projectId
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
	// Remember where the closing tab sat so we can focus its neighbour, not jump
	// to the far end of the bar.
	const idx = get(openTabs).findIndex((t) => t.key === key);
	openTabs.update((ts) => ts.filter((t) => t.key !== key));
	activeTabKey.update((cur) => {
		if (cur !== key) return cur;
		const rest = get(openTabs);
		if (!rest.length) return null;
		// Prefer the tab now at the same index (the one to the right), else the last.
		return rest[Math.min(idx, rest.length - 1)].key;
	});
}

export function setTabTerminalId(key: string, terminalId: string) {
	openTabs.update((ts) => ts.map((t) => (t.key === key ? { ...t, terminalId } : t)));
}

export function setTabSession(key: string, sessionId: string) {
	openTabs.update((ts) => ts.map((t) => (t.key === key ? { ...t, sessionId } : t)));
}
