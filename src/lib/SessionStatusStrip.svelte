<script lang="ts">
	// Always-visible one-line summary of a session's isolation: where its code lives,
	// what branch it's on, how far ahead of base, whether it has uncommitted work, and
	// its last hook result. Turns the invisible worktree into a legible object so a user
	// never has to guess what spwn did to their repo. Click to open the Inspector.
	import { onMount, onDestroy } from 'svelte';
	import { sessionMergeStatus, hooksStatus, onProjectsChanged } from './ipc';
	import { projects } from './stores';
	import type { MergeStatus, HooksStatus } from './types';

	let {
		projectId,
		terminalId,
		onOpen,
		open = false
	}: {
		projectId: string;
		terminalId: string | undefined;
		onOpen?: () => void;
		open?: boolean;
	} = $props();

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
			/* best-effort; leave prior values */
		}
	}

	// Refetch when this session's id changes, and (debounced) whenever the transcript
	// tree changes — which is what fires after a turn commits onto the branch.
	$effect(() => {
		void terminalId;
		refresh();
	});

	onMount(async () => {
		unlisten = await onProjectsChanged(() => {
			clearTimeout(debounce);
			debounce = setTimeout(refresh, 1200);
		});
	});
	onDestroy(() => {
		unlisten?.();
		clearTimeout(debounce);
	});

	function shortPath(pth: string): string {
		const parts = pth.split('/').filter(Boolean);
		return parts.length > 3 ? '…/' + parts.slice(-3).join('/') : pth;
	}

	const branchShort = $derived((term?.branch ?? '').replace(/^cm\//, ''));
	const lastHook = $derived.by(() => {
		const runs = hooks?.events?.filter((e) => e.lastRun) ?? [];
		if (!runs.length) return null;
		return runs.every((e) => e.lastRun?.ok) ? 'ok' : 'failed';
	});
</script>

<div class="strip">
	{#if !term}
		<div class="row">
			<span class="seg dim">session starting…</span>
			<button class="toggle" onclick={() => onOpen?.()} title="Open the session inspector">
				Inspector {open ? '⌄' : '›'}
			</button>
		</div>
	{:else}
		<div class="row">
			{#if term?.cwd}
				<span class="seg path" title="This session's isolated worktree: {term.cwd}">
					📁 {shortPath(term.cwd)}
				</span>
			{:else}
				<span class="seg dim">📁 —</span>
			{/if}
			<button class="toggle" onclick={() => onOpen?.()} title="Toggle the session inspector">
				Inspector {open ? '⌄' : '›'}
			</button>
		</div>
		<div class="row state">
			{#if term?.branch}
				<span class="seg branch" title="git branch (with the branch it merges back into)">
					⎇ {branchShort}{merge?.baseBranch ? ` → ${merge.baseBranch}` : ''}
				</span>
				{#if merge && merge.ahead > 0}
					<span class="chip ahead" title="Commits on this branch not yet in {merge.baseBranch}">
						{merge.ahead} ahead
					</span>
				{/if}
				{#if merge?.uncommitted}
					<span class="chip warn" title="The worktree has uncommitted changes">uncommitted</span>
				{/if}
				{#if merge && merge.ahead === 0 && !merge.uncommitted}
					<span class="chip ok" title="Nothing to merge back yet">in sync with {merge.baseBranch}</span>
				{/if}
			{:else}
				<span class="seg dim" title="Not a git repo — this session shares the project directory">no worktree</span>
			{/if}
			{#if lastHook}
				<span class="chip" class:ok={lastHook === 'ok'} class:warn={lastHook === 'failed'} title="Most recent .spwn hook result">
					hooks {lastHook === 'ok' ? '✓' : '✗'}
				</span>
			{/if}
		</div>
	{/if}
</div>

<style>
	.strip {
		display: flex;
		flex-direction: column;
		gap: 3px;
		width: 100%;
		box-sizing: border-box;
		background: #14181f;
		border-bottom: 1px solid var(--border);
		color: var(--text-dim);
		padding: 6px 12px;
		font-size: 11px;
		font-family: ui-monospace, Menlo, monospace;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}
	.row.state {
		flex-wrap: wrap;
		row-gap: 4px;
	}
	.seg {
		flex: 0 1 auto;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.seg.path {
		color: #9bb0c8;
	}
	.seg.branch {
		color: var(--text);
		flex-shrink: 1;
	}
	.chip {
		flex: 0 0 auto;
		padding: 1px 7px;
		border-radius: 999px;
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid var(--border);
		white-space: nowrap;
	}
	.ahead {
		color: #e0a83a;
	}
	.warn {
		color: var(--danger);
	}
	.ok {
		color: var(--ok);
	}
	.dim {
		color: var(--text-muted);
	}
	.toggle {
		flex: 0 0 auto;
		margin-left: auto;
		background: none;
		border: none;
		color: var(--accent-text);
		font-family: ui-sans-serif, system-ui, sans-serif;
		font-size: 11px;
		cursor: pointer;
		padding: 0 0 0 8px;
	}
	.toggle:hover {
		color: var(--text);
	}
</style>
