// Typed wrappers over the Tauri command/event interface.
// Tauri auto-converts camelCase JS arg keys to the snake_case Rust params.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import type { ProjectRec, Settings, TerminalKind, Turn } from './types';

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

export function clearContext(projectId: string): Promise<void> {
	return invoke('clear_context', { projectId });
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

// --- Claude chat ---

/** Send a user message to a Claude terminal's sidecar. */
export function claudeSend(terminalId: string, text: string): Promise<void> {
	return invoke('claude_send', { terminalId, text });
}

/** Answer a Claude tool-permission request. */
export function claudePermission(
	terminalId: string,
	id: string,
	allow: boolean,
	message?: string
): Promise<void> {
	return invoke('claude_permission', { terminalId, id, allow, message });
}

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

/** Claude sidecar events (JSON lines: init/delta/thinking/tool_use/.../result). */
export function onClaudeEvent(terminalId: string, cb: (ev: any) => void): Promise<UnlistenFn> {
	return listen<string>(`claude://event/${terminalId}`, (e) => {
		try {
			cb(JSON.parse(e.payload));
		} catch {
			/* ignore malformed line */
		}
	});
}

export function onClaudeExit(terminalId: string, cb: () => void): Promise<UnlistenFn> {
	return listen(`claude://exit/${terminalId}`, () => cb());
}

/** Fires (debounced) whenever ~/.claude/projects changes (live transcript). */
export function onProjectsChanged(cb: () => void): Promise<UnlistenFn> {
	return listen('projects://changed', () => cb());
}

function base64ToBytes(b64: string): Uint8Array {
	const bin = atob(b64);
	const out = new Uint8Array(bin.length);
	for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
	return out;
}
