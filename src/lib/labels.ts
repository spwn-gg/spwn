// Canonical UI vocabulary for spwn. One noun per concept — import these instead
// of hardcoding user-facing strings so the language stays consistent everywhere.
// See docs/src/content/docs/reference/glossary.md for the model these words map to.
//
// The rules that matter most for clarity:
//   • "Session"  — an isolated Claude workspace (its own code copy + conversation).
//   • "Shell"    — a plain terminal (no worktree, no conversation).
//   • "Fork"     — the VERB for making a new session from a point in an existing one.
//   • "branch"   — a NOUN only: the git branch a session's worktree lives on. Never a verb.
//   • "Merge tray" — the reusable-context space you assemble to seed a new session.

export const TERMS = {
	project: 'Project',
	session: 'Session',
	shell: 'Shell',
	/** The action of branching a conversation (+ its code) into a new session. */
	fork: 'Fork',
	/** A git branch — always shown as a property of a session, never an action. */
	branch: 'branch',
	/** Renamed from "Context" to avoid collision with the model's context window. */
	mergeTray: 'Merge tray'
} as const;

/** Icons/glyphs, kept 1:1 with concepts. */
export const GLYPHS = {
	session: '✦',
	shell: '$',
	fork: '⑂',
	/** Lineage marker: "forked from the session above". */
	lineage: '↳',
	/** The git branch property chip. */
	branch: '⎇',
	mergeTray: '▦',
	schedule: '◷',
	/** The per-project exploration map (fork lineage as a canvas). */
	map: '⋔'
} as const;

/** Common action labels, so buttons/menus/tooltips read identically across views. */
export const ACTIONS = {
	newProject: '＋ New Project',
	newSession: '＋ New session',
	newShell: '＋ New shell',
	deleteSession: 'Delete session',
	deleteShell: 'Delete shell',
	deleteProject: 'Delete project',
	/** Fork a new session from a point in this one (conversation + code together). */
	fork: 'Fork a new session from here',
	/** Shown when a session has no id yet and can't be forked. */
	forkDisabled: 'Send a message first to enable forking',
	merge: 'Merge',
	openInVscode: 'Open in VS Code',
	explorationMap: 'Exploration Map',
	/** Unified "move this session's work somewhere useful" flow. */
	bringWorkBack: 'Bring work back'
} as const;
