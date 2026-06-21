// Mirrors the Rust serde types (camelCase) returned by the backend.

export type TerminalKind = 'shell' | 'claude';

export interface TerminalRec {
	id: string;
	title: string;
	kind: TerminalKind;
	cwd: string;
	sessionId?: string | null;
	groupId?: string | null;
}

export interface ContextBlock {
	id: string;
	kind: 'note' | 'file' | 'session';
	label: string;
	text: string;
}

export interface ProjectRec {
	id: string;
	name: string;
	directory: string;
	terminals: TerminalRec[];
	context: ContextBlock[];
}

export interface Settings {
	claudePath?: string | null;
}

export interface Block {
	kind: 'text' | 'thinking' | 'toolUse' | 'toolResult';
	text?: string | null;
	name?: string | null;
	isError?: boolean | null;
	id?: string | null;
}

export interface Turn {
	uuid: string;
	parentUuid?: string | null;
	role: 'user' | 'assistant';
	timestamp?: string | null;
	model?: string | null;
	blocks: Block[];
}
