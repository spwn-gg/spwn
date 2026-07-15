// Typed wrappers over the Tauri command/event interface.
// Tauri auto-converts camelCase JS arg keys to the snake_case Rust params.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type {
	CheckpointMeta,
	ClaudeEvent,
	ComposeStatus,
	MergeStatus,
	ProjectRec,
	ScheduledTask,
	Settings,
	TerminalKind,
	Turn
} from './types';

// --- Settings ---

export function getSettings(): Promise<Settings> {
	return invoke('get_settings');
}

export function setSettings(settings: Settings): Promise<void> {
	return invoke('set_settings', { settings });
}

/** Auto-detected claude path (probe; ignores the configured override). */
export function detectClaude(): Promise<string | null> {
	return invoke('find_claude');
}

/** Native file picker; returns the chosen path or null. */
export async function pickFile(): Promise<string | null> {
	const result = await openDialog({ directory: false, multiple: false });
	return typeof result === 'string' ? result : null;
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

/** Open a session's uncommitted working-tree changes (vs HEAD) in the diff viewer. */
export function openWorkingDiff(cwd: string): Promise<void> {
	return invoke('open_working_diff', { cwd });
}

/** Diff a checkpoint (defaults to the session's most recent) against current files. */
export function openCheckpointDiff(sessionId: string, checkpointId?: string): Promise<void> {
	return invoke('open_checkpoint_diff', { sessionId, checkpointId: checkpointId ?? null });
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

/** Native folder picker; returns the chosen path or null. */
export async function pickDirectory(): Promise<string | null> {
	const result = await openDialog({ directory: true, multiple: false });
	return typeof result === 'string' ? result : null;
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

// --- Per-session docker-compose services ---

/** Current compose status for a session (availability, project, services + URLs). */
export function composeStatus(terminalId: string): Promise<ComposeStatus> {
	return invoke('compose_status', { terminalId });
}

/** Bring a session's service stack up (or resume it if idle-stopped). */
export function composeUp(terminalId: string): Promise<void> {
	return invoke('compose_up', { terminalId });
}

/** Tear a session's service stack down (down -v). */
export function composeDown(terminalId: string): Promise<void> {
	return invoke('compose_down', { terminalId });
}

/** Recent logs for one service in a session's stack. */
export function composeLogs(terminalId: string, service: string, tail = 200): Promise<string> {
	return invoke('compose_logs', { terminalId, service, tail });
}

/** Fires when a session's compose stack changes state (up/down/idle-stop). */
export function onComposeEvent(terminalId: string, cb: () => void): Promise<UnlistenFn> {
	return listen(`compose://event/${terminalId}`, () => cb());
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
