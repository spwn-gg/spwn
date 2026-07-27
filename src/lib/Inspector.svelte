<script lang="ts">
	// The session Inspector: one place for everything about a session's state, so the
	// chat toolbar stays down to everyday actions. Houses:
	//   • Overview — where the code lives, branch/base, ahead, changed files, + Merge.
	//   • Timeline — the unified undo (return to a turn ± restore files; file snapshots).
	//   • Hooks    — the session's .spwn lifecycle hooks + last runs.
	import { onMount, onDestroy } from 'svelte';
	import { sessionMergeStatus, hooksStatus, onProjectsChanged, openInVscode } from './ipc';
	import { projects, toggleInspector, alwaysAllowTools, sessionAllowTools, revokeTool } from './stores';
	import MergePanel from './MergePanel.svelte';
	import CheckpointList from './CheckpointList.svelte';
	import HooksPanel from './HooksPanel.svelte';
	import type { MergeStatus, HooksStatus } from './types';

	let {
		projectId,
		terminalId,
		sessionId,
		busy = false
	}: {
		projectId: string;
		terminalId: string | undefined;
		sessionId: string | undefined;
		busy?: boolean;
	} = $props();

	type Section = 'overview' | 'timeline' | 'hooks';
	let section = $state<Section>('overview');
	let showMerge = $state(false);
	let showFiles = $state(false);
	let status = $state('');

	const term = $derived(
		$projects.find((p) => p.id === projectId)?.terminals.find((t) => t.id === terminalId)
	);

	let merge = $state<MergeStatus | null>(null);
	let hooks = $state<HooksStatus | null>(null);
	let unlisten: (() => void) | undefined;
	let debounce: ReturnType<typeof setTimeout> | undefined;

	async function refresh() {
		if (!terminalId) return;
		try {
			[merge, hooks] = await Promise.all([
				sessionMergeStatus(projectId, terminalId),
				hooksStatus(terminalId)
			]);
		} catch {
			/* best-effort */
		}
	}

	$effect(() => {
		void terminalId;
		refresh();
	});

	onMount(async () => {
		unlisten = await onProjectsChanged(() => {
			clearTimeout(debounce);
			debounce = setTimeout(refresh, 1000);
		});
	});
	onDestroy(() => {
		unlisten?.();
		clearTimeout(debounce);
	});

	const hasHooks = $derived(!!hooks?.available);
	const canMerge = $derived(!!term?.branch && !!merge && merge.ahead > 0 && !merge.blocker);

	// Tools the user has auto-allowed (session grants first, then global "always").
	const grants = $derived.by(() => {
		const always = [...$alwaysAllowTools].map((tool) => ({ tool, scope: 'always' as const }));
		const sess = [...($sessionAllowTools.get(terminalId ?? '') ?? [])]
			.filter((t) => !$alwaysAllowTools.has(t))
			.map((tool) => ({ tool, scope: 'session' as const }));
		return [...sess, ...always];
	});
</script>

<aside class="inspector">
	<div class="head">
		<div class="tabs">
			<button class:on={section === 'overview'} onclick={() => (section = 'overview')}>Overview</button>
			<button class:on={section === 'timeline'} onclick={() => (section = 'timeline')}>Timeline</button>
			<button class:on={section === 'hooks'} onclick={() => (section = 'hooks')} disabled={!hasHooks}>Hooks</button>
		</div>
		<button class="close" title="Close inspector" onclick={() => terminalId && toggleInspector(terminalId, false)}>×</button>
	</div>

	<div class="content">
		{#if section === 'overview'}
			{#if term?.branch}
				<div class="field">
					<div class="k">Code lives at</div>
					<div class="v mono wrap">{term.cwd}</div>
				</div>
				<div class="field">
					<div class="k">Branch</div>
					<div class="v mono">{term.branch} <span class="from">→ {merge?.baseBranch ?? term.baseBranch ?? '—'}</span></div>
				</div>
				<div class="field row3">
					<div>
						<div class="k">Ahead of base</div>
						<div class="v" class:accent={(merge?.ahead ?? 0) > 0}>{merge?.ahead ?? '—'} commit{(merge?.ahead ?? 0) === 1 ? '' : 's'}</div>
					</div>
					<div>
						<div class="k">Changed files</div>
						<div class="v">{merge?.changedFiles.length ?? '—'}</div>
					</div>
					<div>
						<div class="k">Working tree</div>
						<div class="v" class:warn={merge?.uncommitted}>{merge?.uncommitted ? 'uncommitted' : 'clean'}</div>
					</div>
				</div>
				{#if merge?.changedFiles.length}
					<button class="disclose" onclick={() => (showFiles = !showFiles)}>
						{showFiles ? '▾' : '▸'} {merge.changedFiles.length} changed file{merge.changedFiles.length === 1 ? '' : 's'}
					</button>
					{#if showFiles}
						<ul class="files">
							{#each merge.changedFiles as f (f)}<li title={f}>{f}</li>{/each}
						</ul>
					{/if}
				{/if}
				{#if merge?.blocker}<div class="blocker">{merge.blocker}</div>{/if}
				<div class="actions">
					<button class="act primary" disabled={!canMerge} title={canMerge ? 'Merge this branch into its base' : 'Nothing to merge yet'} onclick={() => (showMerge = true)}>⤵ Merge…</button>
					{#if term?.cwd}
						<button class="act" onclick={() => openInVscode(term!.cwd).catch(() => {})}>Open in VS Code</button>
					{/if}
				</div>
			{:else}
				<div class="empty">This session has no git worktree (the project isn't a git repo), so there's nothing to merge and no branch state to show.</div>
			{/if}
			<div class="grants">
				<div class="k">Auto-allowed tools</div>
				{#if grants.length}
					{#each grants as g (g.tool)}
						<div class="grant">
							<span class="mono gtool">{g.tool}</span>
							<span class="gscope">{g.scope}</span>
							<button class="revoke" title="Ask again next time" onclick={() => terminalId && revokeTool(terminalId, g.tool)}>revoke</button>
						</div>
					{/each}
				{:else}
					<div class="empty-inline">None — you're asked before each tool runs. Choose “This session” or “Always” on a prompt to auto-allow.</div>
				{/if}
			</div>
		{:else if section === 'timeline'}
			<div class="explain">
				Return to an earlier point from the <strong>↺ Return here</strong> button on any message —
				choose <em>conversation only</em> or <em>conversation + files</em>. Below are the file
				snapshots captured after each turn; restore one on its own without rewinding the chat.
			</div>
			<CheckpointList {projectId} {sessionId} disabled={busy} onStatus={(m) => (status = m)} />
			{#if status}<div class="status">{status}</div>{/if}
		{:else if section === 'hooks'}
			{#if terminalId}
				<HooksPanel {terminalId} onStatus={(m) => (status = m)} />
				{#if status}<div class="status">{status}</div>{/if}
			{/if}
		{/if}
	</div>
</aside>

{#if showMerge && term?.branch && terminalId}
	<MergePanel {projectId} {terminalId} onClose={() => { showMerge = false; refresh(); }} />
{/if}

<style>
	.inspector {
		flex: 0 0 340px;
		max-width: 46%;
		display: flex;
		flex-direction: column;
		min-height: 0;
		background: var(--bg-sidebar);
		border-left: 1px solid var(--border);
	}
	.head {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 6px 8px 6px 10px;
		border-bottom: 1px solid var(--border);
	}
	.tabs {
		display: flex;
		gap: 4px;
		flex: 1 1 auto;
	}
	.tabs button {
		background: none;
		border: 1px solid transparent;
		color: var(--text-dim);
		border-radius: var(--radius);
		padding: 4px 9px;
		font-size: 12px;
		cursor: pointer;
	}
	.tabs button.on {
		background: var(--bg-elevated);
		border-color: var(--border-strong);
		color: var(--text);
	}
	.tabs button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.close {
		flex: 0 0 auto;
		background: none;
		border: none;
		color: var(--text-dim);
		font-size: 17px;
		line-height: 1;
		cursor: pointer;
		padding: 0 4px;
	}
	.close:hover {
		color: #fff;
	}
	.content {
		flex: 1 1 auto;
		overflow-y: auto;
		min-height: 0;
	}
	.field {
		padding: 9px 12px;
		border-bottom: 1px solid var(--border);
	}
	.field.row3 {
		display: flex;
		gap: 12px;
		justify-content: space-between;
	}
	.k {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-muted);
		margin-bottom: 3px;
	}
	.v {
		font-size: 12px;
		color: var(--text);
	}
	.v.mono,
	.mono {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11.5px;
	}
	.wrap {
		word-break: break-all;
		color: #9bb0c8;
	}
	.from {
		color: var(--text-muted);
	}
	.accent {
		color: #e0a83a;
	}
	.warn {
		color: var(--danger);
	}
	.disclose {
		width: 100%;
		text-align: left;
		background: none;
		border: none;
		border-bottom: 1px solid var(--border);
		color: var(--text-dim);
		padding: 8px 12px;
		font-size: 11px;
		cursor: pointer;
	}
	.files {
		margin: 0;
		padding: 6px 12px 10px 26px;
		border-bottom: 1px solid var(--border);
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11px;
		color: var(--text-dim);
		max-height: 180px;
		overflow-y: auto;
	}
	.files li {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.blocker {
		margin: 10px 12px;
		padding: 8px 10px;
		background: var(--danger-bg);
		border: 1px solid #7a3a3a;
		border-radius: var(--radius);
		color: #fff;
		font-size: 12px;
	}
	.actions {
		display: flex;
		gap: 8px;
		padding: 12px;
	}
	.act {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: var(--text);
		border-radius: var(--radius);
		padding: 6px 12px;
		font-size: 12px;
		cursor: pointer;
	}
	.act.primary {
		background: var(--accent);
		border-color: var(--accent-border);
		color: #fff;
	}
	.act:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.explain {
		padding: 11px 12px;
		font-size: 11.5px;
		line-height: 1.5;
		color: var(--text-dim);
		border-bottom: 1px solid var(--border);
		background: #14181f;
	}
	.explain strong {
		color: #d8b8f0;
	}
	.empty {
		padding: 16px 12px;
		font-size: 12px;
		color: var(--text-muted);
		line-height: 1.5;
	}
	.status {
		padding: 6px 12px;
		font-size: 11px;
		color: #c89a4a;
	}
	.grants {
		padding: 10px 12px;
	}
	.grant {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 5px 0;
		font-size: 12px;
	}
	.gtool {
		flex: 1 1 auto;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		color: #9bbf8a;
	}
	.gscope {
		flex: 0 0 auto;
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-muted);
	}
	.revoke {
		flex: 0 0 auto;
		background: none;
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
		border-radius: 4px;
		padding: 2px 8px;
		font-size: 11px;
		cursor: pointer;
	}
	.revoke:hover {
		color: var(--danger);
		border-color: #5a3a3a;
	}
	.empty-inline {
		font-size: 11px;
		color: var(--text-muted);
		line-height: 1.5;
		margin-top: 4px;
	}
</style>
