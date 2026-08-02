// Typed wrappers over the backend HTTP + WebSocket interface.
//
// `invoke` posts to `POST /api/invoke/:command` with a JSON body of camelCase args
// (the backend renames them to the snake_case Rust params). `listen` subscribes to a
// topic on one shared WebSocket that carries every `{topic, payload}` event — the
// same shape Tauri delivered, so the wrappers below are unchanged.

import { writable } from 'svelte/store';
import type {
	CheckpointMeta,
	ClaudeEvent,
	GitBranches,
	HookPromptCloseEvent,
	HookPromptEvent,
	HooksStatus,
	MergeStatus,
	ProjectRec,
	RepoStatus,
	ScheduledTask,
	SessionStatus,
	Settings,
	TerminalKind,
	Turn
} from './types';

// ---------------------------------------------------------------------------
// Transport: HTTP invoke + a single multiplexed WebSocket
// ---------------------------------------------------------------------------

export type UnlistenFn = () => void;

/** Call a backend command. Rejects with the backend's error text on non-2xx. */
async function invoke<T = void>(command: string, args: Record<string, unknown> = {}): Promise<T> {
	const res = await fetch(`/api/invoke/${command}`, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(args)
	});
	if (!res.ok) {
		throw new Error((await res.text()) || res.statusText);
	}
	const text = await res.text();
	return (text ? JSON.parse(text) : undefined) as T;
}

type EventHandler = (e: { payload: unknown }) => void;

/** One WebSocket for the whole app; dispatches `{topic, payload}` frames to
 *  per-topic subscribers and reconnects with backoff. */
class WsBus {
	private ws: WebSocket | null = null;
	private handlers = new Map<string, Set<EventHandler>>();
	private backoff = 500;

	private connect() {
		if (typeof window === 'undefined') return;
		const proto = location.protocol === 'https:' ? 'wss' : 'ws';
		const ws = new WebSocket(`${proto}://${location.host}/ws`);
		this.ws = ws;
		ws.onopen = () => {
			this.backoff = 500;
		};
		ws.onmessage = (ev) => {
			let msg: { topic: string; payload: unknown };
			try {
				msg = JSON.parse(ev.data);
			} catch {
				return;
			}
			const set = this.handlers.get(msg.topic);
			if (set) for (const cb of [...set]) cb({ payload: msg.payload });
		};
		ws.onclose = () => {
			this.ws = null;
			const delay = this.backoff;
			this.backoff = Math.min(this.backoff * 2, 10000);
			setTimeout(() => this.connect(), delay);
		};
		ws.onerror = () => ws.close();
	}

	on(topic: string, cb: EventHandler): UnlistenFn {
		if (!this.ws) this.connect();
		let set = this.handlers.get(topic);
		if (!set) {
			set = new Set();
			this.handlers.set(topic, set);
		}
		set.add(cb);
		return () => {
			set!.delete(cb);
			if (set!.size === 0) this.handlers.delete(topic);
		};
	}
}

const bus = new WsBus();

/** Subscribe to a backend event topic; resolves to an unsubscribe fn. */
function listen<T>(topic: string, cb: (e: { payload: T }) => void): Promise<UnlistenFn> {
	return Promise.resolve(bus.on(topic, cb as EventHandler));
}

// ---------------------------------------------------------------------------
// Server-side file browser (replaces the native file/folder dialog)
// ---------------------------------------------------------------------------

export interface FsEntry {
	name: string;
	path: string;
	isDir: boolean;
}
export interface FsListing {
	path: string;
	parent: string | null;
	entries: FsEntry[];
}

/** List a directory on the host machine (for the pick-a-path UI). */
export async function fsList(path: string | null, includeFiles: boolean): Promise<FsListing> {
	const q = new URLSearchParams();
	if (path) q.set('path', path);
	if (includeFiles) q.set('files', 'true');
	const res = await fetch(`/api/fs/list?${q.toString()}`);
	if (!res.ok) throw new Error((await res.text()) || res.statusText);
	return res.json();
}

/** An open request for the global `FileBrowser` modal; it resolves the promise. */
export type FileBrowserRequest = {
	/** Pick a directory (true) or a file (false). */
	directory: boolean;
	resolve: (path: string | null) => void;
};
export const fileBrowserRequest = writable<FileBrowserRequest | null>(null);

function pickPath(directory: boolean): Promise<string | null> {
	return new Promise((resolve) => fileBrowserRequest.set({ directory, resolve }));
}

// --- Settings ---

export function getSettings(): Promise<Settings> {
	return invoke('get_settings');
}

export function setSettings(settings: Settings): Promise<void> {
	return invoke('set_settings', { settings });
}

/** Reveal the shared global hooks folder (~/.spwn/hooks) in Finder (creates it if needed). */
export function openGlobalHooksDir(): Promise<void> {
	return invoke('open_global_hooks_dir');
}

/** Auto-detected claude path (probe; ignores the configured override). */
export function detectClaude(): Promise<string | null> {
	return invoke('find_claude');
}

/** File picker (server-side browser); returns the chosen host path or null. */
export function pickFile(): Promise<string | null> {
	return pickPath(false);
}

// --- Projects ---

export function listProjects(): Promise<ProjectRec[]> {
	return invoke('list_projects');
}

export function createProject(name: string, directory: string): Promise<ProjectRec> {
	return invoke('create_project', { name, directory });
}

export function deleteProject(projectId: string): Promise<void> {
	return invoke('delete_project', { projectId });
}

export function openInVscode(path: string): Promise<void> {
	return invoke('open_in_vscode', { path });
}

// --- Context space ---

export function addContextBlock(
	projectId: string,
	kind: 'note' | 'session',
	label: string,
	text: string
): Promise<void> {
	return invoke('add_context_block', { projectId, kind, label, text });
}

export function addContextFile(projectId: string, path: string): Promise<void> {
	return invoke('add_context_file', { projectId, path });
}

export function removeContextBlock(projectId: string, blockId: string): Promise<void> {
	return invoke('remove_context_block', { projectId, blockId });
}

export function updateContextBlock(projectId: string, blockId: string, text: string): Promise<void> {
	return invoke('update_context_block', { projectId, blockId, text });
}

/** Persist a new ordering of a project's context blocks (by block id). */
export function reorderContext(projectId: string, order: string[]): Promise<void> {
	return invoke('reorder_context', { projectId, order });
}

export function clearContext(projectId: string): Promise<void> {
	return invoke('clear_context', { projectId });
}

// --- Scheduled tasks ---

export function addScheduledTask(
	projectId: string,
	name: string,
	prompt: string,
	time: string,
	weekdays: number[],
	useContext: boolean
): Promise<ScheduledTask> {
	return invoke('add_scheduled_task', { projectId, name, prompt, time, weekdays, useContext });
}

export function updateScheduledTask(
	projectId: string,
	taskId: string,
	name: string,
	prompt: string,
	time: string,
	weekdays: number[],
	useContext: boolean,
	enabled: boolean
): Promise<void> {
	return invoke('update_scheduled_task', {
		projectId,
		taskId,
		name,
		prompt,
		time,
		weekdays,
		useContext,
		enabled
	});
}

export function setScheduledTaskEnabled(
	projectId: string,
	taskId: string,
	enabled: boolean
): Promise<void> {
	return invoke('set_scheduled_task_enabled', { projectId, taskId, enabled });
}

export function removeScheduledTask(projectId: string, taskId: string): Promise<void> {
	return invoke('remove_scheduled_task', { projectId, taskId });
}

export function runScheduledTaskNow(projectId: string, taskId: string): Promise<void> {
	return invoke('run_scheduled_task_now', { projectId, taskId });
}

export function clearTerminalAttention(terminalId: string): Promise<void> {
	return invoke('clear_terminal_attention', { terminalId });
}

/** Folder picker (server-side browser); returns the chosen host path or null. */
export function pickDirectory(): Promise<string | null> {
	return pickPath(true);
}

// --- Terminals ---

export interface OpenTerminalArgs {
	projectId: string;
	terminalId?: string;
	kind: TerminalKind;
	cols: number;
	rows: number;
	claudeResume?: string;
	claudeFork?: string;
	/** Terminal a fork/branch came from (to inherit its group). */
	parentTerminalId?: string;
	/** Seed a new Claude session: pasted into the input box (not auto-submitted). */
	initialPrompt?: string;
	/** Initial permission/execution mode for a new Claude session. */
	permissionMode?: string;
}

/** Open or reattach a terminal; resolves to its terminal id. */
export function openTerminal(spec: OpenTerminalArgs): Promise<string> {
	return invoke('open_terminal', { spec });
}

/** Detach a terminal tab (keeps the rmux session alive for reattach). */
export function closeTerminal(terminalId: string): Promise<void> {
	return invoke('close_terminal', { terminalId });
}

/** Permanently delete a terminal (kills its rmux session). */
export function deleteTerminal(projectId: string, terminalId: string): Promise<void> {
	return invoke('delete_terminal', { projectId, terminalId });
}

/** Merge a session's worktree branch back into its base branch; resolves to a summary. */
export function mergeSession(projectId: string, terminalId: string): Promise<string> {
	return invoke('merge_session', { projectId, terminalId });
}

/** Preview what merging a session's branch into its base would do. */
export function sessionMergeStatus(projectId: string, terminalId: string): Promise<MergeStatus> {
	return invoke('session_merge_status', { projectId, terminalId });
}

/** Commit a session's changes onto its worktree branch (no-op if it has no branch). */
export function commitSessionTurn(terminalId: string, message: string): Promise<void> {
	return invoke('commit_session_turn', { terminalId, message });
}

// --- Source Control (git for a project's main checkout) ---

/** Git status of a project's main checkout (safe on non-repos → isRepo:false). */
export function gitRepoStatus(projectId: string): Promise<RepoStatus> {
	return invoke('git_repo_status', { projectId });
}

/** Local + remote branches for a project's repo. */
export function gitBranches(projectId: string): Promise<GitBranches> {
	return invoke('git_branches', { projectId });
}

/** Check out an existing branch in the project's main checkout. */
export function gitCheckout(projectId: string, branch: string): Promise<void> {
	return invoke('git_checkout', { projectId, branch });
}

/** Create a new branch off HEAD and switch to it. */
export function gitCreateBranch(projectId: string, name: string): Promise<void> {
	return invoke('git_create_branch', { projectId, name });
}

/** Fetch all remotes (resolves to a summary line). */
export function gitFetch(projectId: string): Promise<string> {
	return invoke('git_fetch', { projectId });
}

/** Fast-forward-only pull (resolves to a summary line). */
export function gitPull(projectId: string): Promise<string> {
	return invoke('git_pull', { projectId });
}

/** Push the current branch, setting upstream if it has none (resolves to a summary). */
export function gitPush(projectId: string): Promise<string> {
	return invoke('git_push', { projectId });
}

/** VS Code "Sync": fetch → ff pull → push. */
export function gitSync(projectId: string): Promise<string> {
	return invoke('git_sync', { projectId });
}

/** Persist a discovered claude session id onto a terminal record. */
export function setTerminalSession(
	projectId: string,
	terminalId: string,
	sessionId: string
): Promise<void> {
	return invoke('set_terminal_session', { projectId, terminalId, sessionId });
}

export function writeToPty(terminalId: string, data: string): Promise<void> {
	return invoke('write_to_pty', { ptyId: terminalId, data });
}

export function resizePty(terminalId: string, cols: number, rows: number): Promise<void> {
	return invoke('resize_pty', { ptyId: terminalId, cols, rows });
}

// --- Claude chat (Agent SDK sidecar) ---

/** Send a user turn to a Claude session's sidecar. */
export function claudeSend(terminalId: string, text: string): Promise<void> {
	return invoke('claude_send', { terminalId, text });
}

/** Answer a tool-permission request. */
export function claudePermission(
	terminalId: string,
	id: string,
	allow: boolean,
	message?: string
): Promise<void> {
	return invoke('claude_permission', { terminalId, id, allow, message });
}

/** Change the permission mode live (Shift-Tab): default | acceptEdits | plan | bypassPermissions. */
export function claudeSetMode(terminalId: string, mode: string): Promise<void> {
	return invoke('claude_set_mode', { terminalId, mode });
}

/** Interrupt the in-flight turn (Esc). */
export function claudeInterrupt(terminalId: string): Promise<void> {
	return invoke('claude_interrupt', { terminalId });
}

/** Answer an AskUserQuestion picker (id = the question event's id). */
export function claudeAnswer(terminalId: string, id: string, text: string): Promise<void> {
	return invoke('claude_answer', { terminalId, id, text });
}

/** Rewind a session to an earlier turn (anchorUuid = the turn's uuid). */
export function claudeRewind(terminalId: string, anchorUuid: string): Promise<void> {
	return invoke('claude_rewind', { terminalId, anchorUuid });
}

/** Rewind AND restore the project files to that turn's checkpoint. */
export function claudeRewindRestore(
	terminalId: string,
	anchorUuid: string,
	restore: boolean
): Promise<void> {
	return invoke('claude_rewind_restore', { terminalId, anchorUuid, restore });
}

// --- Project hooks (discovered shell scripts run on session lifecycle events) ---

/** Discovered hooks + last-run results for a session's worktree. */
export function hooksStatus(terminalId: string): Promise<HooksStatus> {
	return invoke('hooks_status', { terminalId });
}

/** Manually re-run one event's hook for a session (resolves when the script finishes). */
export function hooksRun(terminalId: string, event: string): Promise<void> {
	return invoke('hooks_run', { terminalId, event });
}

/** Fire the `session-turn` hooks after a completed Claude turn (default: commit the
 * turn onto the session branch + snapshot a checkpoint). No-op without a worktree. */
export function hooksRunTurn(terminalId: string, turnUuid: string): Promise<void> {
	return invoke('hooks_run_turn', { terminalId, turnUuid });
}

/** Fires when a session's hooks finish running (created/ready/deleted/manual re-run). */
export function onHooksEvent(terminalId: string, cb: () => void): Promise<UnlistenFn> {
	return listen(`hooks://event/${terminalId}`, () => cb());
}

/** A hook starting (`event` set) or finishing (`event` null) for a session. */
export interface HookRunningEvent {
	terminalId: string;
	event: string | null;
}

/** Fires (globally) whenever any session's hook starts or finishes — drives the
 * live "running" spinner on tabs and the project tree. */
export function onHookRunning(cb: (e: HookRunningEvent) => void): Promise<UnlistenFn> {
	return listen<HookRunningEvent>('hooks://running', (e) => cb(e.payload));
}

/** A session's live status changed (thinking / blocked / done / error / idle). */
export interface ClaudeStatusEvent {
	terminalId: string;
	status: SessionStatus;
}

/** Fires (globally) whenever any Claude session's live status changes — drives the
 * sidebar/tab-bar spinner and attention dots without opening the session. */
export function onClaudeStatus(cb: (e: ClaudeStatusEvent) => void): Promise<UnlistenFn> {
	return listen<ClaudeStatusEvent>('claude://status', (e) => cb(e.payload));
}

/** One streamed output line from a session's currently-running hook. */
export interface HookOutputEvent {
	event: string;
	line: string;
}

/** Streams each output line of a session's running hook, live. */
export function onHookOutput(
	terminalId: string,
	cb: (e: HookOutputEvent) => void
): Promise<UnlistenFn> {
	return listen<HookOutputEvent>(`hooks://output/${terminalId}`, (e) => cb(e.payload));
}

/** Answer a blocking hook prompt with the user's chosen label(s), unblocking the hook. */
export function hooksPromptAnswer(id: string, answer: string): Promise<void> {
	return invoke('hooks_prompt_answer', { id, answer });
}

/** Fires (globally) when any running hook raises a blocking multiple-choice prompt.
 * Global because hooks fire on session create/delete when no session pane is mounted. */
export function onHookPrompt(cb: (e: HookPromptEvent) => void): Promise<UnlistenFn> {
	return listen<HookPromptEvent>('hooks://prompt', (e) => cb(e.payload));
}

/** Fires (globally) when a hook prompt should be dismissed (answered / timed out). */
export function onHookPromptClose(cb: (e: HookPromptCloseEvent) => void): Promise<UnlistenFn> {
	return listen<HookPromptCloseEvent>('hooks://prompt-close', (e) => cb(e.payload));
}

// --- Code checkpoints ---

/** Snapshot the project directory (kind: 'turn' | 'baseline' | …). */
export function checkpointProject(
	projectId: string,
	sessionId: string,
	turnUuid: string,
	kind: string
): Promise<CheckpointMeta> {
	return invoke('checkpoint_project', { projectId, sessionId, turnUuid, kind });
}

/** Restore the project's files to a checkpoint; resolves to the safety snapshot taken first. */
export function restoreCheckpoint(
	projectId: string,
	sessionId: string,
	checkpointId: string,
	preRestore = true
): Promise<CheckpointMeta | null> {
	return invoke('restore_checkpoint', { projectId, sessionId, checkpointId, preRestore });
}

export function listCheckpoints(sessionId: string): Promise<CheckpointMeta[]> {
	return invoke('list_checkpoints', { sessionId });
}

// --- Claude transcript ---

/** Prior conversation turns for a saved claude session (history on reattach). */
export function readTranscript(sessionId: string): Promise<Turn[]> {
	return invoke('read_transcript', { sessionId });
}

// --- Events ---

export function onPtyOutput(terminalId: string, cb: (bytes: Uint8Array) => void): Promise<UnlistenFn> {
	return listen<string>(`pty://output/${terminalId}`, (e) => cb(base64ToBytes(e.payload)));
}

export function onPtyExit(terminalId: string, cb: () => void): Promise<UnlistenFn> {
	return listen(`pty://exit/${terminalId}`, () => cb());
}

/** Fires once a new/forked Claude pty session's id is discovered on disk. */
export function onPtySessionId(terminalId: string, cb: (sessionId: string) => void): Promise<UnlistenFn> {
	return listen<string>(`pty://session-id/${terminalId}`, (e) => cb(e.payload));
}

/** Streamed events from a Claude session's sidecar (init/delta/thinking/tool_use/…). */
export function onClaudeEvent(terminalId: string, cb: (ev: ClaudeEvent) => void): Promise<UnlistenFn> {
	return listen<string>(`claude://event/${terminalId}`, (e) => {
		try {
			cb(JSON.parse(e.payload) as ClaudeEvent);
		} catch {
			/* ignore a malformed line */
		}
	});
}

/** Fires when a Claude session's sidecar process exits. */
export function onClaudeExit(terminalId: string, cb: () => void): Promise<UnlistenFn> {
	return listen(`claude://exit/${terminalId}`, () => cb());
}

/** Fires (debounced) whenever ~/.claude/projects changes; the payload is the list
 *  of changed session ids (empty when the affected sessions can't be determined). */
export function onProjectsChanged(cb: (changed: string[]) => void): Promise<UnlistenFn> {
	return listen<string[]>('projects://changed', (e) => cb(e.payload ?? []));
}

/** Fires when the backend fails to persist the project store to disk. */
export function onStoreError(cb: (message: string) => void): Promise<UnlistenFn> {
	return listen<string>('store://error', (e) => cb(e.payload));
}

/** Fires when a scheduled task's headless run finishes (ok=false on failure). */
export interface ScheduleFired {
	projectId: string;
	terminalId: string;
	ok: boolean;
}
export function onScheduledTaskFired(cb: (e: ScheduleFired) => void): Promise<UnlistenFn> {
	return listen<ScheduleFired>('schedule://fired', (e) => cb(e.payload));
}

function base64ToBytes(b64: string): Uint8Array {
	const bin = atob(b64);
	const out = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
	return out;
}
