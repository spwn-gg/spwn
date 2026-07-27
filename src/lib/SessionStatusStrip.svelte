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
		onOpen
	}: {
		projectId: string;
		terminalId: string | undefined;
		onOpen?: () => void;
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

<button class="strip" onclick={() => onOpen?.()} title="Open the session inspector">
	{#if !term}
		<span class="seg dim">session starting…</span>
		<span class="spacer"></span>
		<span class="details">Inspector ›</span>
	{:else}
	{#if term?.cwd}
		<span class="seg path" title="This session's isolated worktree: {term.cwd}">
			📁 {shortPath(term.cwd)}
		</span>
	{/if}
	{#if term?.branch}
		<span class="sep">·</span>
		<span class="seg" title="git branch (with the branch it merges back into)">
			⎇ {branchShort}{merge?.baseBranch ? ` (from ${merge.baseBranch})` : ''}
		</span>
		{#if merge && merge.ahead > 0}
			<span class="sep">·</span>
			<span class="seg ahead" title="Commits on this branch not yet in {merge.baseBranch}">
				{merge.ahead} ahead
			</span>
		{/if}
		{#if merge?.uncommitted}
			<span class="sep">·</span>
			<span class="seg warn" title="The worktree has uncommitted changes">uncommitted</span>
		{/if}
		{#if merge && merge.ahead === 0 && !merge.uncommitted}
			<span class="sep">·</span>
			<span class="seg ok" title="Nothing to merge back yet">in sync with {merge.baseBranch}</span>
		{/if}
	{:else}
		<span class="sep">·</span>
		<span class="seg dim" title="Not a git repo — this session shares the project directory">no worktree</span>
	{/if}
	{#if lastHook}
		<span class="sep">·</span>
		<span class="seg" class:ok={lastHook === 'ok'} class:warn={lastHook === 'failed'} title="Most recent .spwn hook result">
			hooks {lastHook === 'ok' ? '✓' : '✗'}
		</span>
	{/if}
	<span class="spacer"></span>
	<span class="details">Inspector ›</span>
	{/if}
</button>

<style>
	.strip {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		box-sizing: border-box;
		background: #14181f;
		border: none;
		border-bottom: 1px solid var(--border);
		color: var(--text-dim);
		padding: 5px 12px;
		font-size: 11px;
		font-family: ui-monospace, Menlo, monospace;
		cursor: pointer;
		text-align: left;
		overflow: hidden;
		white-space: nowrap;
	}
	.strip:hover {
		background: #171c24;
		color: var(--text);
	}
	.seg {
		flex: 0 1 auto;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.seg.path {
		color: #9bb0c8;
		flex-shrink: 1;
	}
	.sep {
		color: var(--text-muted);
		flex: 0 0 auto;
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
	.spacer {
		flex: 1 1 auto;
	}
	.details {
		flex: 0 0 auto;
		color: var(--accent-text);
		font-family: ui-sans-serif, system-ui, sans-serif;
	}
</style>
