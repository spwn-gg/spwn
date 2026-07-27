<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import {
		createProject,
		deleteProject,
		deleteTerminal,
		pickDirectory,
		onProjectsChanged,
		onScheduledTaskFired,
		clearTerminalAttention,
		openInVscode,
		sessionMergeStatus,
		gitRepoStatus
	} from './ipc';
	import SourceControl from './SourceControl.svelte';
	import {
		projects,
		openTab,
		closeTab,
		refreshProjects,
		openTabs,
		activeTab,
		hookRunning,
		claudeStatus,
		setClaudeStatus,
		confirmDialog,
		type ConfirmRow
	} from './stores';
	import { get } from 'svelte/store';
	import { ACTIONS } from './labels';
	import { claudeForest, type SessionNode } from './forest';
	import type { ProjectRec, TerminalRec } from './types';

	let collapsed = $state(new Set<string>());
	let openMenuId = $state<string | null>(null);
	let menuPos = $state({ x: 0, y: 0 });
	let unlisten: Array<() => void> = [];

	// Which projects are git repos (lazily probed when a project is expanded), so
	// the Source Control section only shows for repos. `checkedRepos` dedupes the probe.
	let repoIsGit = $state<Record<string, boolean>>({});
	const checkedRepos = new Set<string>();
	// Which projects have their Source Control section expanded (collapsed by default).
	let scmOpen = $state(new Set<string>());

	$effect(() => {
		for (const p of $projects) {
			if (collapsed.has(p.id) || checkedRepos.has(p.id)) continue;
			checkedRepos.add(p.id);
			gitRepoStatus(p.id)
				.then((s) => (repoIsGit = { ...repoIsGit, [p.id]: s.isRepo }))
				.catch(() => {});
		}
	});

	function toggleScm(id: string) {
		const next = new Set(scmOpen);
		next.has(id) ? next.delete(id) : next.add(id);
		scmOpen = next;
	}

	const closeMenu = () => {
		openMenuId = null;
	};

	onMount(async () => {
		await refreshProjects();
		// Claude's ai-title evolves as a session runs; refresh names live.
		unlisten.push(await onProjectsChanged(() => refreshProjects()));
		// A scheduled run finished (or bound its session) — surface it in the tree.
		unlisten.push(await onScheduledTaskFired(() => refreshProjects()));
		window.addEventListener('click', closeMenu);
		window.addEventListener('keydown', onKey);
	});
	onDestroy(() => {
		unlisten.forEach((u) => u());
		window.removeEventListener('click', closeMenu);
		window.removeEventListener('keydown', onKey);
	});

	function onKey(e: KeyboardEvent) {
		if (e.key === 'Escape') closeMenu();
	}

	function toggleMenu(p: ProjectRec, e: MouseEvent) {
		e.stopPropagation();
		if (openMenuId === p.id) {
			openMenuId = null;
			return;
		}
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const estHeight = 196; // ~5 rows; flip above if it would overflow the viewport
		const y = r.bottom + estHeight > window.innerHeight ? r.top - estHeight : r.bottom + 2;
		menuPos = { x: Math.min(r.left, window.innerWidth - 210), y: Math.max(8, y) };
		openMenuId = p.id;
	}
	function menuShell(p: ProjectRec, e: Event) {
		openMenuId = null;
		addTerminal(p, 'shell', e);
	}
	function menuClaude(p: ProjectRec, e: Event) {
		openMenuId = null;
		addTerminal(p, 'claude', e);
	}
	async function menuVscode(p: ProjectRec, e: Event) {
		e.stopPropagation();
		openMenuId = null;
		try {
			await openInVscode(p.directory);
		} catch (err) {
			console.error(err);
		}
	}
	function menuDelete(p: ProjectRec, e: Event) {
		openMenuId = null;
		removeProject(p, e);
	}

	function toggle(id: string) {
		const next = new Set(collapsed);
		next.has(id) ? next.delete(id) : next.add(id);
		collapsed = next;
	}

	async function newProject() {
		const dir = await pickDirectory();
		if (!dir) return;
		const name = dir.split('/').filter(Boolean).pop() ?? dir;
		await createProject(name, dir);
		await refreshProjects();
	}

	function addTerminal(p: ProjectRec, kind: 'shell' | 'claude', e: Event) {
		e.stopPropagation();
		openTab({ projectId: p.id, kind, title: kind === 'claude' ? 'session' : 'shell', projectName: p.name });
	}

	async function openSessionCode(t: TerminalRec, e: Event) {
		e.stopPropagation();
		try {
			await openInVscode(t.cwd);
		} catch (err) {
			console.error(err);
		}
	}

	function openContext(p: ProjectRec, e: Event) {
		e.stopPropagation();
		openTab({ projectId: p.id, kind: 'context', title: `Merge tray · ${p.name}`, projectName: p.name });
	}

	function openSchedule(p: ProjectRec, e: Event) {
		e.stopPropagation();
		openTab({
			projectId: p.id,
			kind: 'schedule',
			title: `Schedule · ${p.name}`,
			projectName: p.name
		});
	}

	function openExisting(p: ProjectRec, t: TerminalRec) {
		// Viewing a session clears its attention. Drop the live "needs you" status now so
		// the dot clears immediately (the still-alive sidecar won't re-emit until its next
		// event); keep a live "thinking" spinner. Also clear the persisted flag.
		if ($claudeStatus.get(t.id) !== 'thinking') setClaudeStatus(t.id, 'idle');
		if (t.needsAttention) {
			clearTerminalAttention(t.id).then(() => refreshProjects());
		}
		openTab({
			projectId: p.id,
			kind: t.kind,
			terminalId: t.id,
			title: t.title,
			projectName: p.name,
			sessionId: t.sessionId ?? undefined
		});
	}

	/** Name the work a delete would destroy, so the confirm can spell it out. Deleting a
	 * session drops its worktree *and* branch, so unmerged commits are gone for good. */
	async function stakeRows(p: ProjectRec, t: TerminalRec): Promise<{ rows: ConfirmRow[]; atRisk: boolean }> {
		const rows: ConfirmRow[] = [];
		if (t.cwd) rows.push({ label: 'Code lives at', value: shortPath(t.cwd) });
		let atRisk = false;
		try {
			const s = await sessionMergeStatus(p.id, t.id);
			if (s.branch) {
				rows.push({ label: 'On branch', value: s.branch });
				if (s.ahead > 0) {
					rows.push({
						label: 'Unmerged commits',
						value: `${s.ahead} not in “${s.baseBranch}”`,
						danger: true
					});
					atRisk = true;
				}
				if (s.uncommitted) {
					rows.push({ label: 'Uncommitted changes', value: 'yes', danger: true });
					atRisk = true;
				}
			}
		} catch {
			// Best-effort: a status failure must never block deletion.
		}
		return { rows, atRisk };
	}

	/** Shorten a long absolute worktree path for display (keep the tail). */
	function shortPath(pth: string): string {
		const parts = pth.split('/').filter(Boolean);
		return parts.length > 3 ? '…/' + parts.slice(-3).join('/') : pth;
	}

	async function removeTerminal(p: ProjectRec, t: TerminalRec, e: Event) {
		e.stopPropagation();
		const isSession = t.kind === 'claude';
		const { rows, atRisk } = isSession
			? await stakeRows(p, t)
			: { rows: [] as ConfirmRow[], atRisk: false };
		const res = await confirmDialog({
			title: isSession ? `Delete session “${t.title}”?` : `Delete shell “${t.title}”?`,
			body: isSession
				? "This throws away the session's isolated copy of your code — its worktree and its git branch — and can't be undone."
				: "This ends the shell and can't be undone.",
			rows,
			confirmLabel: 'Delete',
			// Offer a way out when there's work that a delete would discard.
			secondaryLabel: isSession && atRisk ? 'Open to merge first' : undefined
		});
		if (res === 'cancel') return;
		if (res === 'secondary') {
			openExisting(p, t); // let the user merge from the session's toolbar
			return;
		}
		// Close any open tab for this terminal first.
		const tab = get(openTabs).find((x) => x.terminalId === t.id);
		if (tab) closeTab(tab.key);
		await deleteTerminal(p.id, t.id);
		await refreshProjects();
	}

	async function removeProject(p: ProjectRec, e: Event) {
		e.stopPropagation();
		const n = p.terminals.length;
		const detail = n ? ` and its ${n} session${n === 1 ? '' : 's'}/shell${n === 1 ? '' : 's'}` : '';
		const res = await confirmDialog({
			title: `Delete project “${p.name}”?`,
			body: `This removes the project${detail} from spwn and can't be undone.`,
			confirmLabel: 'Delete'
		});
		if (res !== 'confirm') return;
		for (const tab of get(openTabs).filter((x) => x.projectId === p.id)) closeTab(tab.key);
		await deleteProject(p.id);
		await refreshProjects();
	}

	function shells(p: ProjectRec): TerminalRec[] {
		return p.terminals.filter((t) => t.kind === 'shell');
	}

	// Claude sessions form a branch forest (each fork nests under its parent); the
	// forest-building lives in forest.ts.

	// Fork a new session from an existing one (same as Fork in the chat panel).
	function forkSession(p: ProjectRec, t: TerminalRec, e: Event) {
		e.stopPropagation();
		if (!t.sessionId) return;
		openTab({
			projectId: p.id,
			kind: 'claude',
			title: 'fork',
			projectName: p.name,
			claudeFork: t.sessionId,
			parentTerminalId: t.id,
			// Show the parent's history immediately; once the branch sends its first
			// message it rebinds to its own (history-carrying) forked session id.
			sessionId: t.sessionId
		});
	}

	// Highlight the row backing the currently-focused tab.
	const isActiveTerm = (t: TerminalRec) => $activeTab?.terminalId === t.id;

	/** The status token to render on a session row: a live spinner while it works, or an
	 * attention state when it needs you. Prefers the live `claude://status` feed; falls
	 * back to the persisted flag (+reason) when no sidecar is attached (e.g. after
	 * restart). Attention states are hidden for the session you're already viewing. */
	type RowStatus = 'thinking' | 'blocked' | 'done' | 'error' | null;
	function statusFor(t: TerminalRec): RowStatus {
		const live = $claudeStatus.get(t.id);
		let s: RowStatus = null;
		if (live === 'thinking') s = 'thinking';
		else if (live === 'blockedPermission' || live === 'blockedQuestion') s = 'blocked';
		else if (live === 'done') s = 'done';
		else if (live === 'error') s = 'error';
		else if (t.needsAttention) {
			// No live sidecar → fall back to the persisted reason.
			s = t.attentionReason === 'blocked' ? 'blocked' : t.attentionReason === 'error' ? 'error' : 'done';
		}
		// You're looking at it — don't nag with an attention dot (a spinner still shows).
		if (s && s !== 'thinking' && isActiveTerm(t)) return null;
		return s;
	}
	const isActiveCtx = (p: ProjectRec) =>
		$activeTab?.kind === 'context' && $activeTab?.projectId === p.id;
	const isActiveSchedule = (p: ProjectRec) =>
		$activeTab?.kind === 'schedule' && $activeTab?.projectId === p.id;
</script>

{#snippet termRow(p: ProjectRec, t: TerminalRec, nested: boolean)}
	<div class="row terminal" class:nested class:active={isActiveTerm(t)}>
		<button class="row-main" onclick={() => openExisting(p, t)} title={t.title}>
			<span class="t-icon">{t.kind === 'claude' ? '✦' : '$'}</span>
			<span class="t-title">{t.title}</span>
		</button>
		<button class="icon-btn t-del" title="Delete shell" onclick={(e) => removeTerminal(p, t, e)}>×</button>
	</div>
{/snippet}

{#snippet sessionNode(p: ProjectRec, node: SessionNode, depth: number)}
	{@const t = node.t}
	{@const open = !collapsed.has('s:' + t.id)}
	{@const status = statusFor(t)}
	<div class="row session" class:active={isActiveTerm(t)} style="--depth: {depth}">
		{#if node.children.length}
			<button class="twisty" title={open ? 'Hide forks' : 'Show forks'} onclick={() => toggle('s:' + t.id)}>{open ? '▾' : '▸'}</button>
		{:else}
			<span class="twisty-spacer"></span>
		{/if}
		<button class="row-main" onclick={() => openExisting(p, t)} title={t.title}>
			<span class="t-icon" class:branch={depth > 0}>{depth > 0 ? '↳' : '✦'}</span>
			<span class="t-title" class:attn={status === 'blocked' || status === 'done'} class:err={status === 'error'}>{t.title}</span>
			{#if $hookRunning.has(t.id)}<span class="hook-spin" title="Running {$hookRunning.get(t.id)} hook…"></span>{/if}
			{#if t.branch}<span class="wt-chip" title="git branch (this session's worktree): {t.branch}">⎇ {t.branch.replace(/^cm\//, '')}</span>{/if}
			{#if status === 'thinking'}<span class="think-spin" title="Working…"></span>
			{:else if status === 'blocked'}<span class="attn-dot blocked" title="Waiting for you"></span>
			{:else if status === 'done'}<span class="attn-dot" title="Turn finished — awaiting you"></span>
			{:else if status === 'error'}<span class="attn-dot error" title="Session error"></span>{/if}
			{#if node.children.length}<span class="count" title="{node.children.length} fork(s)">{node.children.length}</span>{/if}
		</button>
		<button class="icon-btn fork" title={t.sessionId ? ACTIONS.fork : ACTIONS.forkDisabled} disabled={!t.sessionId} onclick={(e) => forkSession(p, t, e)}>⑂</button>
		<button class="icon-btn code" title="Open in VS Code" onclick={(e) => openSessionCode(t, e)}>{'</>'}</button>
		<button class="icon-btn t-del" title="Delete session" onclick={(e) => removeTerminal(p, t, e)}>×</button>
	</div>
	{#if node.children.length && open}
		{#each node.children as c (c.t.id)}
			{@render sessionNode(p, c, depth + 1)}
		{/each}
	{/if}
{/snippet}

<div class="tree">
	<button class="new-project" onclick={newProject}>＋ New Project</button>
	{#if $projects.length === 0}
		<div class="empty">No projects yet. Click “New Project” to pick a folder.</div>
	{/if}
	{#each $projects as p (p.id)}
		<div class="project">
			<div class="row project-header">
				<button class="row-main" onclick={() => toggle(p.id)} title={p.directory}>
					<span class="chevron">{collapsed.has(p.id) ? '▸' : '▾'}</span>
					<span class="proj-folder">▪</span>
					<span class="project-name">{p.name}</span>
				</button>
				<button class="icon-btn act" title="Actions" onclick={(e) => toggleMenu(p, e)}>⋯</button>
			</div>
			{#if !collapsed.has(p.id)}
				<div class="terminals">
					<div class="row ctx-row" class:active={isActiveCtx(p)}>
						<button class="row-main" onclick={(e) => openContext(p, e)}>
							<span class="t-icon ctx">▦</span>
							<span class="t-title">Merge tray</span>
							{#if p.context?.length}<span class="count">{p.context.length}</span>{/if}
						</button>
					</div>
					<div class="row ctx-row" class:active={isActiveSchedule(p)}>
						<button class="row-main" onclick={(e) => openSchedule(p, e)}>
							<span class="t-icon ctx">◷</span>
							<span class="t-title">Scheduled Tasks</span>
							{#if p.scheduledTasks?.length}<span class="count">{p.scheduledTasks.length}</span>{/if}
						</button>
					</div>
					{#if repoIsGit[p.id]}
						<div class="row ctx-row">
							<button class="row-main" onclick={() => toggleScm(p.id)}>
								<span class="t-icon ctx">{scmOpen.has(p.id) ? '▾' : '▸'}</span>
								<span class="t-title">Source Control</span>
							</button>
						</div>
						{#if scmOpen.has(p.id)}
							<SourceControl projectId={p.id} onChanged={() => refreshProjects()} />
						{/if}
					{/if}
					{#each shells(p) as t (t.id)}
						{@render termRow(p, t, false)}
					{/each}
					{#each claudeForest(p) as node (node.t.id)}
						{@render sessionNode(p, node, 0)}
					{/each}
					<button class="add-session" onclick={(e) => menuClaude(p, e)} title="Start a new session (isolated worktree + conversation)">＋ New session</button>
					{#if p.terminals.length === 0}
						<div class="t-empty">No sessions yet — start one above, or use the ⋯ menu.</div>
					{/if}
				</div>
			{/if}
		</div>
	{/each}
</div>

{#if openMenuId}
	{@const p = $projects.find((x) => x.id === openMenuId)}
	{#if p}
		<div
			class="menu"
			role="menu"
			tabindex="-1"
			style="left: {menuPos.x}px; top: {menuPos.y}px">
			<button onclick={(e) => menuClaude(p, e)}>New session</button>
			<button onclick={(e) => menuShell(p, e)}>New shell</button>
			<button onclick={(e) => menuVscode(p, e)}>Open in VS Code</button>
			<div class="sep"></div>
			<button class="danger" onclick={(e) => menuDelete(p, e)}>Delete project</button>
		</div>
	{/if}
{/if}

<style>
	.tree {
		overflow-y: auto;
		flex: 1 1 auto;
		font-size: 13px;
	}
	.empty {
		padding: 14px;
		color: var(--text-muted);
	}
	.new-project {
		display: block;
		width: calc(100% - 16px);
		margin: 8px;
		padding: 6px 8px;
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #cfcfcf;
		border-radius: var(--radius);
		font-size: 12px;
		cursor: pointer;
	}
	.new-project:hover {
		background: #333;
		color: #fff;
	}

	/* A row is a flex container holding a primary button + optional action buttons,
	   so interactive elements are siblings (never nested). */
	.row {
		display: flex;
		align-items: center;
		border-left: 2px solid transparent;
	}
	.row-main {
		display: flex;
		align-items: center;
		gap: 6px;
		flex: 1 1 auto;
		min-width: 0;
		background: none;
		border: none;
		color: #cfcfcf;
		cursor: pointer;
		text-align: left;
		padding: 6px 6px 6px 10px;
	}
	.icon-btn {
		background: none;
		border: none;
		color: var(--text-muted);
		cursor: pointer;
		padding: 0 6px;
		border-radius: 4px;
		font-size: 16px;
		line-height: 1;
	}
	.icon-btn:hover {
		color: #fff;
		background: #333;
	}

	/* Projects are sticky section headers so they stay anchored over their own
	   sessions and read clearly apart from the next project. */
	.project {
		position: relative;
		border-top: 1px solid #000;
	}
	.tree .project:first-of-type {
		border-top: none;
	}
	.project-header {
		position: sticky;
		top: 0;
		z-index: 5;
		background: var(--bg-elevated);
		border-bottom: 1px solid var(--border);
		border-left-color: #3b475e;
	}
	.project-header .row-main {
		padding-top: 9px;
		padding-bottom: 9px;
	}
	.project-header:hover {
		background: #2c2c2c;
	}
	.chevron {
		width: 12px;
		color: #9a9a9a;
		font-size: 12px;
		flex: 0 0 auto;
	}
	.proj-folder {
		flex: 0 0 auto;
		color: var(--accent-text);
		font-size: 11px;
	}
	.project-name {
		flex: 1 1 auto;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-weight: 700;
		font-size: 13px;
		letter-spacing: 0.01em;
		color: #efefef;
	}

	.menu {
		position: fixed;
		z-index: 200;
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
		padding: 4px;
		min-width: 190px;
		display: flex;
		flex-direction: column;
	}
	.menu button {
		background: none;
		border: none;
		color: #d0d0d0;
		text-align: left;
		padding: 7px 10px;
		border-radius: 5px;
		cursor: pointer;
		font-size: 13px;
	}
	.menu button:hover:not(:disabled) {
		background: var(--accent-soft);
		color: #fff;
	}
	.menu button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.menu button.danger {
		color: var(--danger);
	}
	.menu button.danger:hover {
		background: var(--danger-bg);
		color: #fff;
	}
	.menu .sep {
		height: 1px;
		background: #333;
		margin: 4px 6px;
	}

	.terminals {
		display: flex;
		flex-direction: column;
	}
	.terminal .row-main {
		padding-left: 26px;
		color: #d0d0d0;
	}
	.terminal:hover {
		background: #1f1f1f;
		border-left-color: var(--accent-line);
	}
	.terminal.nested .row-main {
		padding-left: 40px;
	}
	.row.active {
		background: #20262f;
		border-left-color: var(--accent-line);
	}
	.row.active .row-main {
		color: #fff;
	}

	.ctx-row .row-main {
		padding-left: 26px;
		color: #b8a9d8;
	}
	.ctx-row:hover {
		background: #1f1f1f;
		border-left-color: #8a7fb0;
	}
	.ctx-row.active {
		border-left-color: #8a7fb0;
	}

	/* Claude session branch tree: indentation = fork depth. */
	.row.session {
		padding-left: calc(var(--depth) * 16px);
	}
	.twisty {
		flex: 0 0 auto;
		width: 16px;
		background: none;
		border: none;
		color: #888;
		font-size: 11px;
		cursor: pointer;
		padding: 0;
	}
	.twisty:hover {
		color: #fff;
	}
	.twisty-spacer {
		flex: 0 0 auto;
		width: 16px;
	}
	.session .row-main {
		gap: 7px;
		color: #d0d0d0;
		padding-left: 2px;
	}
	.session:hover {
		background: #1f1f1f;
		border-left-color: var(--accent-line);
	}
	.t-icon.branch {
		color: #b88fd8;
		font-size: 13px;
	}
	.fork {
		font-size: 13px;
	}
	.code {
		font-size: 11px;
		font-family: ui-monospace, Menlo, monospace;
	}
	.code:hover {
		color: #9bbce0;
		background: #1b2230;
	}
	.fork:hover:not(:disabled) {
		color: #d8b8f0;
		background: #2f2640;
	}
	.fork:disabled {
		opacity: 0.25;
		cursor: default;
	}
	/* Keep row actions out of the way until hover / active, to cut clutter. */
	.terminals .row .icon-btn {
		opacity: 0;
		transition: opacity 0.1s;
	}
	.terminals .row:hover .icon-btn,
	.terminals .row.active .icon-btn,
	.terminals .row .icon-btn:focus-visible {
		opacity: 1;
	}
	.add-session {
		display: flex;
		align-items: center;
		gap: 6px;
		width: calc(100% - 16px);
		margin: 4px 8px 8px 24px;
		padding: 4px 8px;
		background: none;
		border: 1px dashed var(--border-strong);
		border-radius: var(--radius);
		color: var(--text-muted);
		font-size: 12px;
		cursor: pointer;
		text-align: left;
	}
	.add-session:hover {
		color: var(--accent-text);
		border-color: var(--accent-line);
		background: #1b2230;
	}
	.count {
		color: #777;
		font-size: 11px;
	}
	.wt-chip {
		flex: 0 0 auto;
		font-size: 10px;
		font-family: ui-monospace, Menlo, monospace;
		color: #7a8aa0;
		background: #1b2230;
		border: 1px solid #2a3344;
		border-radius: 4px;
		padding: 0 4px;
		max-width: 90px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.t-title.attn {
		color: #f0c674;
	}
	.t-title.err {
		color: #e06c6c;
	}
	.attn-dot {
		flex: 0 0 auto;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: #e0a83a;
		box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.22);
	}
	/* Blocked-on-you pulses to read as "act now"; done is a steady dot. */
	.attn-dot.blocked {
		animation: attn-pulse 1.4s ease-in-out infinite;
	}
	.attn-dot.error {
		background: #e06c6c;
		box-shadow: 0 0 0 2px rgba(224, 108, 108, 0.22);
	}
	@keyframes attn-pulse {
		0%,
		100% {
			opacity: 1;
			box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.22);
		}
		50% {
			opacity: 0.55;
			box-shadow: 0 0 0 4px rgba(224, 168, 58, 0.12);
		}
	}
	/* A working session: a cool-toned spinner, distinct from the accent hook spinner. */
	.think-spin {
		flex: 0 0 auto;
		width: 10px;
		height: 10px;
		border-radius: 50%;
		border: 2px solid rgba(240, 198, 116, 0.25);
		border-top-color: #e0a83a;
		animation: hook-spin 0.7s linear infinite;
	}
	.hook-spin {
		flex: 0 0 auto;
		width: 10px;
		height: 10px;
		border-radius: 50%;
		border: 2px solid rgba(127, 163, 223, 0.3);
		border-top-color: var(--accent-text);
		animation: hook-spin 0.7s linear infinite;
	}
	@keyframes hook-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.t-icon {
		color: var(--accent-text);
		font-size: 15px;
		flex: 0 0 auto;
	}
	.t-icon.ctx {
		color: #b8a9d8;
	}
	.t-title {
		flex: 1 1 auto;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.t-del {
		font-size: 15px;
	}
	.t-del:hover {
		color: #fff;
		background: var(--danger-bg);
	}
	.t-empty {
		padding: 5px 10px 8px 26px;
		font-size: 12px;
		color: #666;
	}
</style>
