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
		openWorkingDiff,
		openCheckpointDiff
	} from './ipc';
	import { projects, openTab, closeTab, refreshProjects, openTabs, activeTab } from './stores';
	import { get } from 'svelte/store';
	import type { ProjectRec, TerminalRec } from './types';

	let collapsed = $state(new Set<string>());
	let openMenuId = $state<string | null>(null);
	let menuPos = $state({ x: 0, y: 0 });
	let sessionMenu = $state<TerminalRec | null>(null);
	let sessionMenuPos = $state({ x: 0, y: 0 });
	let unlisten: Array<() => void> = [];

	const closeMenu = () => {
		openMenuId = null;
		sessionMenu = null;
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
		openTab({ projectId: p.id, kind, title: kind, projectName: p.name });
	}

	async function openSessionCode(t: TerminalRec, e: Event) {
		e.stopPropagation();
		try {
			await openInVscode(t.cwd);
		} catch (err) {
			console.error(err);
		}
	}

	function toggleSessionMenu(t: TerminalRec, e: MouseEvent) {
		e.stopPropagation();
		if (sessionMenu?.id === t.id) {
			sessionMenu = null;
			return;
		}
		openMenuId = null;
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const estHeight = 92; // ~2 rows; flip above if it would overflow the viewport
		const y = r.bottom + estHeight > window.innerHeight ? r.top - estHeight : r.bottom + 2;
		sessionMenuPos = { x: Math.min(r.left, window.innerWidth - 220), y: Math.max(8, y) };
		sessionMenu = t;
	}

	async function diffWorking(t: TerminalRec, e: Event) {
		e.stopPropagation();
		sessionMenu = null;
		try {
			await openWorkingDiff(t.cwd);
		} catch (err) {
			alert(String(err));
		}
	}

	async function diffCheckpoint(t: TerminalRec, e: Event) {
		e.stopPropagation();
		sessionMenu = null;
		if (!t.sessionId) return;
		try {
			await openCheckpointDiff(t.sessionId);
		} catch (err) {
			alert(String(err));
		}
	}

	function openContext(p: ProjectRec, e: Event) {
		e.stopPropagation();
		openTab({ projectId: p.id, kind: 'context', title: `Context · ${p.name}`, projectName: p.name });
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
		// Viewing a session clears its persisted attention flag (from a headless run).
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

	async function removeTerminal(p: ProjectRec, t: TerminalRec, e: Event) {
		e.stopPropagation();
		if (!confirm(`Delete terminal “${t.title}”? This kills its session and can't be undone.`)) return;
		// Close any open tab for this terminal first.
		const tab = get(openTabs).find((x) => x.terminalId === t.id);
		if (tab) closeTab(tab.key);
		await deleteTerminal(p.id, t.id);
		await refreshProjects();
	}

	async function removeProject(p: ProjectRec, e: Event) {
		e.stopPropagation();
		const n = p.terminals.length;
		const detail = n ? ` and its ${n} terminal${n === 1 ? '' : 's'}` : '';
		if (!confirm(`Delete project “${p.name}”${detail}? This can't be undone.`)) return;
		for (const tab of get(openTabs).filter((x) => x.projectId === p.id)) closeTab(tab.key);
		await deleteProject(p.id);
		await refreshProjects();
	}

	function shells(p: ProjectRec): TerminalRec[] {
		return p.terminals.filter((t) => t.kind === 'shell');
	}

	// Build the claude sessions into a branch forest: each fork nests under the
	// session it was forked from, so lineage is visible at a glance.
	interface SessionNode {
		t: TerminalRec;
		children: SessionNode[];
	}
	function parentOf(t: TerminalRec, ids: Set<string>): string | null {
		if (t.parentId && ids.has(t.parentId)) return t.parentId;
		// Legacy data: groupId pointed at the lineage root.
		if (t.groupId && t.groupId !== t.id && ids.has(t.groupId)) return t.groupId;
		return null;
	}
	function claudeForest(p: ProjectRec): SessionNode[] {
		const claudes = p.terminals.filter((t) => t.kind === 'claude');
		const ids = new Set(claudes.map((t) => t.id));
		const nodes = new Map<string, SessionNode>();
		for (const t of claudes) nodes.set(t.id, { t, children: [] });
		const roots: SessionNode[] = [];
		for (const t of claudes) {
			const pid = parentOf(t, ids);
			if (pid) nodes.get(pid)!.children.push(nodes.get(t.id)!);
			else roots.push(nodes.get(t.id)!);
		}
		return roots;
	}

	// Branch a new session from an existing one (same as Fork in the chat panel).
	function forkSession(p: ProjectRec, t: TerminalRec, e: Event) {
		e.stopPropagation();
		if (!t.sessionId) return;
		openTab({
			projectId: p.id,
			kind: 'claude',
			title: 'branch',
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
	// A session needs attention: either a background tab flagged it (permission /
	// turn done) or a windowless scheduled run persisted the flag on the record.
	const attnFor = (t: TerminalRec) =>
		t.needsAttention === true ||
		$openTabs.some((tab) => tab.terminalId === t.id && tab.needsAttention);
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
		<button class="icon-btn diff" title="Open working-tree diff" onclick={(e) => diffWorking(t, e)}>⇄</button>
		<button class="icon-btn t-del" title="Delete terminal" onclick={(e) => removeTerminal(p, t, e)}>×</button>
	</div>
{/snippet}

{#snippet sessionNode(p: ProjectRec, node: SessionNode, depth: number)}
	{@const t = node.t}
	{@const open = !collapsed.has('s:' + t.id)}
	<div class="row session" class:active={isActiveTerm(t)} style="--depth: {depth}">
		{#if node.children.length}
			<button class="twisty" title={open ? 'Hide branches' : 'Show branches'} onclick={() => toggle('s:' + t.id)}>{open ? '▾' : '▸'}</button>
		{:else}
			<span class="twisty-spacer"></span>
		{/if}
		<button class="row-main" onclick={() => openExisting(p, t)} title={t.title}>
			<span class="t-icon" class:branch={depth > 0}>{depth > 0 ? '↳' : '✦'}</span>
			<span class="t-title" class:attn={attnFor(t)}>{t.title}</span>
			{#if t.branch}<span class="wt-chip" title="git worktree branch: {t.branch}">⎇ {t.branch.replace(/^cm\//, '')}</span>{/if}
			{#if attnFor(t)}<span class="attn-dot" title="Needs attention"></span>{/if}
			{#if node.children.length}<span class="count" title="{node.children.length} branch(es)">{node.children.length}</span>{/if}
		</button>
		<button class="icon-btn fork" title={t.sessionId ? 'Branch a new session from here' : 'Send a message first to enable branching'} disabled={!t.sessionId} onclick={(e) => forkSession(p, t, e)}>⑂</button>
		<button class="icon-btn code" title="Open project in VS Code" onclick={(e) => openSessionCode(t, e)}>{'</>'}</button>
		<button class="icon-btn diff" title="Diff this session's changes" onclick={(e) => toggleSessionMenu(t, e)}>⇄</button>
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
							<span class="t-title">Context</span>
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
					{#each shells(p) as t (t.id)}
						{@render termRow(p, t, false)}
					{/each}
					{#each claudeForest(p) as node (node.t.id)}
						{@render sessionNode(p, node, 0)}
					{/each}
					<button class="add-session" onclick={(e) => menuClaude(p, e)} title="Start a new Claude session">＋ Claude session</button>
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
			<button onclick={(e) => menuShell(p, e)}>New terminal</button>
			<button onclick={(e) => menuClaude(p, e)}>New Claude session</button>
			<button onclick={(e) => menuVscode(p, e)}>Open in VSCode</button>
			<div class="sep"></div>
			<button class="danger" onclick={(e) => menuDelete(p, e)}>Delete project</button>
		</div>
	{/if}
{/if}

{#if sessionMenu}
	{@const t = sessionMenu}
	<div
		class="menu"
		role="menu"
		tabindex="-1"
		style="left: {sessionMenuPos.x}px; top: {sessionMenuPos.y}px">
		<button onclick={(e) => diffWorking(t, e)}>Diff working changes</button>
		<button
			disabled={!t.sessionId}
			title={t.sessionId ? 'Diff the latest checkpoint against current files' : 'Send a message first to create a checkpoint'}
			onclick={(e) => diffCheckpoint(t, e)}>Diff last checkpoint</button>
	</div>
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
	.diff {
		font-size: 14px;
	}
	.diff:hover {
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
	.attn-dot {
		flex: 0 0 auto;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: #e0a83a;
		box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.22);
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
