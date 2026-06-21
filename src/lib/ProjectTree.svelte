<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import {
		createProject,
		deleteProject,
		deleteTerminal,
		pickDirectory,
		onProjectsChanged,
		openInVscode
	} from './ipc';
	import { projects, openTab, closeTab, refreshProjects, openTabs } from './stores';
	import { get } from 'svelte/store';
	import type { ProjectRec, TerminalRec } from './types';

	let collapsed = $state(new Set<string>());
	let openMenuId = $state<string | null>(null);
	let unlisten: (() => void) | undefined;

	const closeMenu = () => (openMenuId = null);

	onMount(async () => {
		await refreshProjects();
		// Claude's ai-title evolves as a session runs; refresh names live.
		unlisten = await onProjectsChanged(() => refreshProjects());
		window.addEventListener('click', closeMenu);
	});
	onDestroy(() => {
		unlisten?.();
		window.removeEventListener('click', closeMenu);
	});

	function toggleMenu(p: ProjectRec, e: Event) {
		e.stopPropagation();
		openMenuId = openMenuId === p.id ? null : p.id;
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
		openTab({ projectId: p.id, kind, title: kind });
	}

	function openContext(p: ProjectRec, e: Event) {
		e.stopPropagation();
		openTab({ projectId: p.id, kind: 'context', title: `Context · ${p.name}` });
	}

	function openExisting(p: ProjectRec, t: TerminalRec) {
		openTab({
			projectId: p.id,
			kind: t.kind,
			terminalId: t.id,
			title: t.title,
			sessionId: t.sessionId ?? undefined
		});
	}

	async function removeTerminal(p: ProjectRec, t: TerminalRec, e: Event) {
		e.stopPropagation();
		// Close any open tab for this terminal first.
		const tab = get(openTabs).find((x) => x.terminalId === t.id);
		if (tab) closeTab(tab.key);
		await deleteTerminal(p.id, t.id);
		await refreshProjects();
	}

	async function removeProject(p: ProjectRec, e: Event) {
		e.stopPropagation();
		for (const tab of get(openTabs).filter((x) => x.projectId === p.id)) closeTab(tab.key);
		await deleteProject(p.id);
		await refreshProjects();
	}

	function shells(p: ProjectRec): TerminalRec[] {
		return p.terminals.filter((t) => t.kind === 'shell');
	}

	// Group claude terminals by their group key (groupId, else their own id). A
	// fork/branch shares its source's key, so lineages cluster together.
	function claudeGroups(p: ProjectRec): { key: string; root: TerminalRec; members: TerminalRec[] }[] {
		const map = new Map<string, TerminalRec[]>();
		for (const t of p.terminals.filter((t) => t.kind === 'claude')) {
			const key = t.groupId ?? t.id;
			if (!map.has(key)) map.set(key, []);
			map.get(key)!.push(t);
		}
		return [...map.entries()].map(([key, members]) => ({
			key,
			members,
			root: members.find((m) => m.id === key) ?? members[0]
		}));
	}
</script>

{#snippet termRow(p: ProjectRec, t: TerminalRec, nested: boolean)}
	<button class="terminal" class:nested onclick={() => openExisting(p, t)}>
		<span class="t-icon">{t.kind === 'claude' ? '✦' : '$'}</span>
		<span class="t-title">{t.title}</span>
		<span class="t-del" role="button" tabindex="0" title="Delete terminal"
			onclick={(e) => removeTerminal(p, t, e)}
			onkeydown={(e) => e.key === 'Enter' && removeTerminal(p, t, e)}>×</span>
	</button>
{/snippet}

<div class="tree">
	<button class="new-project" onclick={newProject}>＋ New Project</button>
	{#if $projects.length === 0}
		<div class="empty">No projects yet. Click “New Project” to pick a folder.</div>
	{/if}
	{#each $projects as p (p.id)}
		<div class="project">
			<button class="project-header" onclick={() => toggle(p.id)} title={p.directory}>
				<span class="chevron">{collapsed.has(p.id) ? '▸' : '▾'}</span>
				<span class="project-name">{p.name}</span>
				<span class="act plus" role="button" tabindex="0" title="Actions"
					onclick={(e) => toggleMenu(p, e)}
					onkeydown={(e) => e.key === 'Enter' && toggleMenu(p, e)}>＋</span>
			</button>
			{#if openMenuId === p.id}
				<div class="menu" role="menu">
					<button onclick={(e) => menuShell(p, e)}>New terminal</button>
					<button onclick={(e) => menuClaude(p, e)}>New Claude session</button>
					<button onclick={(e) => menuVscode(p, e)}>Open in VSCode</button>
					<div class="sep"></div>
					<button class="danger" onclick={(e) => menuDelete(p, e)}>Delete project</button>
				</div>
			{/if}
			{#if !collapsed.has(p.id)}
				<div class="terminals">
					<button class="ctx-row" onclick={(e) => openContext(p, e)}>
						<span class="t-icon">▦</span>
						<span class="t-title">Context</span>
						{#if p.context?.length}<span class="count">{p.context.length}</span>{/if}
					</button>
					{#each shells(p) as t (t.id)}
						{@render termRow(p, t, false)}
					{/each}
					{#each claudeGroups(p) as g (g.key)}
						{#if g.members.length === 1}
							{@render termRow(p, g.members[0], false)}
						{:else}
							<button class="group-header" onclick={() => toggle('g:' + g.key)} title={g.root.title}>
								<span class="chevron">{collapsed.has('g:' + g.key) ? '▸' : '▾'}</span>
								<span class="t-icon">✦</span>
								<span class="t-title">{g.root.title}</span>
								<span class="count">{g.members.length}</span>
							</button>
							{#if !collapsed.has('g:' + g.key)}
								{#each g.members as t (t.id)}
									{@render termRow(p, t, true)}
								{/each}
							{/if}
						{/if}
					{/each}
					{#if p.terminals.length === 0}
						<div class="t-empty">No terminals — use $ or ✦ above.</div>
					{/if}
				</div>
			{/if}
		</div>
	{/each}
</div>

<style>
	.tree {
		overflow-y: auto;
		flex: 1 1 auto;
		font-size: 13px;
	}
	.empty {
		padding: 14px;
		color: #6a6a6a;
	}
	.new-project {
		display: block;
		width: calc(100% - 16px);
		margin: 8px;
		padding: 6px 8px;
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 6px;
		font-size: 12px;
		cursor: pointer;
	}
	.new-project:hover {
		background: #333;
		color: #fff;
	}
	.project-header {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		padding: 6px 10px;
		background: none;
		border: none;
		color: #cfcfcf;
		cursor: pointer;
		text-align: left;
	}
	.project-header:hover {
		background: #222;
	}
	.chevron {
		width: 12px;
		color: #888;
		font-size: 12px;
	}
	.project-name {
		flex: 1 1 auto;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-weight: 600;
	}
	.actions {
		display: flex;
		gap: 2px;
	}
	.act {
		color: #888;
		padding: 0 5px;
		border-radius: 4px;
		font-size: 16px;
		line-height: 1;
	}
	.act:hover {
		color: #fff;
		background: #333;
	}
	.act.del:hover {
		background: #5a2a2a;
	}
	.project {
		position: relative;
	}
	.menu {
		position: absolute;
		right: 8px;
		top: 28px;
		z-index: 20;
		background: #232323;
		border: 1px solid #3a3a3a;
		border-radius: 8px;
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
	.menu button:hover {
		background: #2f3a4a;
		color: #fff;
	}
	.menu button.danger {
		color: #cf9a9a;
	}
	.menu button.danger:hover {
		background: #5a2a2a;
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
	.terminal {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		text-align: left;
		background: none;
		border: none;
		border-left: 2px solid transparent;
		padding: 5px 10px 5px 26px;
		color: #d0d0d0;
		cursor: pointer;
	}
	.terminal:hover {
		background: #1f1f1f;
		border-left-color: #4a78c8;
	}
	.terminal.nested {
		padding-left: 40px;
	}
	.ctx-row {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		text-align: left;
		background: none;
		border: none;
		border-left: 2px solid transparent;
		padding: 5px 10px 5px 26px;
		color: #b8a9d8;
		cursor: pointer;
	}
	.ctx-row:hover {
		background: #1f1f1f;
		border-left-color: #8a7fb0;
	}
	.group-header {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		text-align: left;
		background: none;
		border: none;
		padding: 5px 10px 5px 22px;
		color: #cfcfcf;
		cursor: pointer;
	}
	.group-header:hover {
		background: #1f1f1f;
	}
	.group-header .chevron {
		width: 12px;
		color: #888;
		font-size: 12px;
	}
	.count {
		color: #777;
		font-size: 11px;
	}
	.t-icon {
		color: #7fa3df;
		font-size: 15px;
	}
	.t-title {
		flex: 1 1 auto;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.t-del {
		color: #777;
		padding: 0 4px;
		border-radius: 3px;
		font-size: 15px;
		line-height: 1;
	}
	.t-del:hover {
		color: #fff;
		background: #5a2a2a;
	}
	.t-empty {
		padding: 5px 10px 8px 26px;
		font-size: 12px;
		color: #666;
	}
</style>
