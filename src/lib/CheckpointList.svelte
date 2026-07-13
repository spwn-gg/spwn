<script lang="ts">
	import { onMount } from 'svelte';
	import { listCheckpoints, restoreCheckpoint, openCheckpointDiff } from './ipc';
	import { refreshProjects } from './stores';
	import type { CheckpointMeta } from './types';

	let {
		projectId,
		sessionId,
		disabled = false,
		onStatus = () => {}
	}: {
		projectId: string;
		sessionId: string | undefined;
		disabled?: boolean;
		onStatus?: (msg: string) => void;
	} = $props();

	let checkpoints = $state<CheckpointMeta[]>([]);
	let restoring = $state(false);

	async function refresh() {
		checkpoints = sessionId ? await listCheckpoints(sessionId) : [];
	}
	onMount(refresh);
	$effect(() => {
		void sessionId;
		refresh();
	});

	function label(c: CheckpointMeta): string {
		if (c.kind === 'baseline') return 'Session start';
		if (c.kind === 'pre-restore') return 'Safety snapshot (before a restore)';
		if (c.kind === 'pre-switch') return 'Auto-saved (before switching away)';
		return 'After a turn';
	}
	function ago(ms: number): string {
		const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
		if (s < 60) return `${s}s ago`;
		if (s < 3600) return `${Math.round(s / 60)}m ago`;
		if (s < 86400) return `${Math.round(s / 3600)}h ago`;
		return `${Math.round(s / 86400)}d ago`;
	}

	async function restore(c: CheckpointMeta) {
		if (!sessionId || disabled || restoring) return;
		if (
			!confirm(
				'Restore the project files to this checkpoint?\n\nThis reverts working files and deletes files created since. Git history is kept, and a safety snapshot is saved first.'
			)
		)
			return;
		restoring = true;
		onStatus('Restoring files…');
		try {
			await restoreCheckpoint(projectId, sessionId, c.id, true);
			onStatus('Files restored. A safety snapshot was saved — undo it from this list.');
			await refresh();
			await refreshProjects();
		} catch (e) {
			onStatus(String(e));
		} finally {
			restoring = false;
		}
	}

	async function diff(c: CheckpointMeta) {
		if (!sessionId) return;
		try {
			await openCheckpointDiff(sessionId, c.id);
		} catch (e) {
			onStatus(String(e));
		}
	}

	// "Undo last change" = restore the turn checkpoint before the most recent one.
	const undoTarget = $derived.by(() => {
		const turns = checkpoints.filter((c) => c.kind === 'turn');
		return turns[1] ?? checkpoints.find((c) => c.kind === 'baseline') ?? null;
	});
</script>

<div class="panel">
	<div class="head">
		<span>Code checkpoints</span>
		{#if undoTarget}
			<button class="undo" disabled={disabled || restoring} onclick={() => restore(undoTarget!)}
				>⟲ Undo last change</button>
		{/if}
	</div>
	{#if !sessionId}
		<div class="empty">Send a message to start capturing checkpoints.</div>
	{:else if checkpoints.length === 0}
		<div class="empty">No checkpoints yet — they're captured after each turn.</div>
	{:else}
		<div class="list">
			{#each checkpoints as c (c.id)}
				<div class="row" class:synthetic={c.kind !== 'turn' && c.kind !== 'baseline'}>
					<div class="meta">
						<span class="lbl">{label(c)}</span>
						<span class="time">{ago(c.createdMs)}</span>
					</div>
					<button class="diff" title="Open this checkpoint's diff vs current files" onclick={() => diff(c)}
						>Diff</button>
					<button class="restore" disabled={disabled || restoring} onclick={() => restore(c)}
						>Restore</button>
				</div>
			{/each}
		</div>
	{/if}
	{#if disabled}<div class="note">Finish the current turn to restore.</div>{/if}
	<div class="note">Snapshots share storage and are near-free until files change.</div>
</div>

<style>
	.panel {
		border-bottom: 1px solid var(--border);
		background: #14181f;
		max-height: 240px;
		overflow-y: auto;
		font-size: 12px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px 12px;
		font-weight: 600;
		color: var(--text-dim);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		font-size: 11px;
		border-bottom: 1px solid var(--border);
	}
	.undo {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #d8b8f0;
		border-radius: 5px;
		padding: 2px 8px;
		font-size: 11px;
		cursor: pointer;
		text-transform: none;
		letter-spacing: 0;
	}
	.undo:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.empty,
	.note {
		padding: 8px 12px;
		color: var(--text-muted);
	}
	.note {
		font-size: 10px;
	}
	.list {
		display: flex;
		flex-direction: column;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 12px;
		border-top: 1px solid #1c2128;
	}
	.row.synthetic {
		opacity: 0.7;
	}
	.meta {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.lbl {
		color: #cfcfcf;
	}
	.time {
		color: var(--text-muted);
		font-size: 10px;
	}
	.diff {
		flex: 0 0 auto;
		background: none;
		border: 1px solid var(--border-strong);
		color: #9bbce0;
		border-radius: 5px;
		padding: 3px 10px;
		cursor: pointer;
	}
	.diff:hover {
		border-color: var(--accent-line);
		background: #1b2230;
	}
	.restore {
		flex: 0 0 auto;
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #cfe0f5;
		border-radius: 5px;
		padding: 3px 10px;
		cursor: pointer;
	}
	.restore:hover:not(:disabled) {
		border-color: var(--accent-line);
		background: #1b2230;
	}
	.restore:disabled {
		opacity: 0.4;
		cursor: default;
	}
</style>
