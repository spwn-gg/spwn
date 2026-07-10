// Mirrors the Rust serde types (camelCase) returned by the backend.

export type TerminalKind = 'shell' | 'claude';

export interface TerminalRec {
	id: string;
	title: string;
	kind: TerminalKind;
	cwd: string;
	sessionId?: string | null;
	groupId?: string | null;
	/** The terminal this was forked from (its parent in the branch tree). */
	parentId?: string | null;
	/** Git branch this session works on in its own worktree (null = no worktree). */
	branch?: string | null;
	/** The branch this session merges back into. */
	baseBranch?: string | null;
	/** Persisted attention flag (set by a windowless scheduled run). */
	needsAttention?: boolean;
}

/** Preview of what merging a session's branch into its base would do. */
export interface MergeStatus {
	branch?: string | null;
	baseBranch?: string | null;
	ahead: number;
	changedFiles: string[];
	uncommitted: boolean;
	blocker?: string | null;
}

export interface ContextBlock {
	id: string;
	kind: 'note' | 'file' | 'session';
	label: string;
	text: string;
}

/** A per-project scheduled task: a headless read-only run on a daily/weekly cadence. */
export interface ScheduledTask {
	id: string;
	name: string;
	prompt: string;
	/** Local "HH:MM" (24h). */
	time: string;
	/** Weekdays it may fire on: 0=Sun..6=Sat. Empty = every day. */
	weekdays: number[];
	enabled: boolean;
	useContext: boolean;
	lastRun?: number | null;
}

export interface ProjectRec {
	id: string;
	name: string;
	directory: string;
	terminals: TerminalRec[];
	context: ContextBlock[];
	scheduledTasks: ScheduledTask[];
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

/** One question in an AskUserQuestion tool call. */
export interface QuestionSpec {
	question: string;
	header?: string;
	multiSelect?: boolean;
	options: { label: string; description?: string }[];
}

/** A pending interactive question awaiting the user's selection. */
export interface PendingQuestion {
	id: string;
	questions: QuestionSpec[];
}

/** A code checkpoint (APFS-clone snapshot of the project dir). */
export interface CheckpointMeta {
	id: string;
	sessionId: string;
	turnUuid: string;
	projectDir: string;
	createdMs: number;
	kind: 'turn' | 'baseline' | 'pre-restore' | 'pre-switch';
}

/** Streamed events from the Claude sidecar (mirrors its stdout JSON-line protocol). */
export type ClaudeEvent =
	| { t: 'init'; sessionId: string }
	| { t: 'delta'; text: string }
	| { t: 'thinking'; text: string }
	| { t: 'tool_use'; id: string; name: string; input: unknown }
	| { t: 'tool_result'; id: string; text: string; isError?: boolean }
	| { t: 'permission'; id: string; tool: string; input: unknown; title?: string }
	| { t: 'question'; id: string; questions: QuestionSpec[] }
	| { t: 'assistant_uuid'; uuid: string }
	| { t: 'result'; subtype: string; sessionId: string }
	| { t: 'error'; message: string };

/** A pending tool-permission request awaiting the user's allow/deny. */
export interface PermissionReq {
	id: string;
	tool: string;
	input: unknown;
	title?: string;
}
