// Mirrors the Rust serde types (camelCase) returned by the backend.

/**
 * `shell` — a login shell in an rmux pane.
 * `agent` — a coding agent driven as its real TUI in an rmux pane.
 * `claude` — LEGACY: a Claude session on the Agent-SDK sidecar. Kept while both
 *   transports run side by side so existing sessions aren't stranded.
 */
export type TerminalKind = 'shell' | 'agent' | 'claude';

export interface TerminalRec {
	id: string;
	title: string;
	kind: TerminalKind;
	/** Which agent definition drives this session (kind === 'agent'). */
	agent?: string | null;
	cwd: string;
	sessionId?: string | null;
	groupId?: string | null;
	/** The terminal this was forked from (its parent in the branch tree). */
	parentId?: string | null;
	/** Git branch this session works on in its own worktree (null = no worktree). */
	branch?: string | null;
	/** The branch this session merges back into. */
	baseBranch?: string | null;
	/** Persisted attention flag: a turn finished / hit a prompt / failed unseen. */
	needsAttention?: boolean;
	/** Why attention is needed — 'blocked' | 'done' | 'error' — for restart rendering. */
	attentionReason?: string | null;
}

/** Live status of a Claude session (mirrors Rust `SessionStatus`). Drives the
 * sidebar/tab-bar spinner and attention dots. */
export type SessionStatus =
	| 'thinking'
	| 'blockedPermission'
	| 'blockedQuestion'
	| 'done'
	| 'error'
	| 'idle';

/** Which layer a hook came from: shared `~/.spwn/hooks` (global) or repo `.spwn/hooks`. */
export type HookScope = 'global' | 'repo';

/** One hook script's most recent run (mirrors hooks::HookRun). */
export interface HookRun {
	event: string;
	scope: HookScope;
	/** Script basename (e.g. "10-install.sh"). */
	script: string;
	exitCode?: number | null;
	ok: boolean;
	/** Combined stdout+stderr tail. */
	output: string;
	/** Epoch seconds when the run finished. */
	at: number;
}

/** One discovered hook script (scope + file) + its last run (mirrors hooks::HookScriptInfo). */
export interface HookScriptInfo {
	scope: HookScope;
	/** Hook file basename (e.g. "session-created.sh"). */
	script: string;
	lastRun?: HookRun | null;
}

/** The discovered hook scripts (0–2, global first) for one lifecycle event
 * (mirrors hooks::HookEventInfo). */
export interface HookEventInfo {
	event: string;
	scripts: HookScriptInfo[];
}

/** A session's hooks status (mirrors hooks::HooksStatus). */
export interface HooksStatus {
	available: boolean;
	events: HookEventInfo[];
	/** The event whose hook is executing right now, if any. */
	running?: string | null;
}

/** A blocking multiple-choice prompt raised by a running hook (mirrors the flattened
 * `hooks://prompt` payload). The user's pick is written back to the script's stdin. */
export interface HookPromptEvent {
	terminalId: string;
	/** Correlation id echoed back with the answer. */
	id: string;
	/** The lifecycle event whose hook raised this (e.g. "session-created"). */
	event: string;
	question: string;
	header?: string;
	multiSelect?: boolean;
	options: { label: string; description?: string }[];
}

/** A hook prompt that's no longer answerable (answered / timed out / hook died). */
export interface HookPromptCloseEvent {
	terminalId: string;
	id: string;
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

/** A project's main-checkout git status (mirrors commands::RepoStatus). */
export interface RepoStatus {
	/** Whether the project directory is inside a git repository. */
	isRepo: boolean;
	/** Current branch (null on detached HEAD or non-repo). */
	branch?: string | null;
	/** Upstream tracking branch, e.g. "origin/main" (null if none). */
	upstream?: string | null;
	/** Commits ahead of upstream. */
	ahead: number;
	/** Commits behind upstream. */
	behind: number;
	/** Working tree has staged/unstaged changes. */
	dirty: boolean;
}

/** Local + remote branches for a project's repo (mirrors commands::GitBranches). */
export interface GitBranches {
	current?: string | null;
	local: string[];
	remote: string[];
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

export type WorktreeLocation = 'sibling' | 'internal' | 'appData';

export interface Settings {
	/** @deprecated Migrated into `agentPaths.claude` on load; kept so an old settings.json still parses. */
	claudePath?: string | null;
	/** Per-agent binary overrides, keyed by agent id. Absent/empty ⇒ auto-detect. */
	agentPaths?: Record<string, string>;
	/** Agent id for new sessions when none is picked. Null ⇒ first installed agent. */
	defaultAgent?: string | null;
	worktreeLocation?: WorktreeLocation;
	/** Whether the shared global hooks in ~/.spwn/hooks run (default true). */
	globalHooksEnabled?: boolean;
}

/** Where an agent definition came from; later scopes override earlier ones by id. */
export type AgentScope = 'builtIn' | 'global' | 'repo';

/**
 * What an agent can actually do, derived from its definition. The UI hides
 * affordances rather than offering ones that will fail — an agent with no
 * transcript adapter genuinely cannot be rewound.
 */
export interface AgentCapabilities {
	transcript: boolean;
	rewind: boolean;
	headless: boolean;
	/** Has real status rules, vs. generic activity detection only. */
	status: boolean;
}

/** One agent definition, as the picker and Settings need it. */
export interface AgentSummary {
	id: string;
	name: string;
	icon?: string | null;
	/** Ships with spwn but has never been driven against the real binary. */
	untested: boolean;
	scope: AgentScope;
	/** Resolved executable, or null when it isn't installed. */
	binary?: string | null;
	capabilities: AgentCapabilities;
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
