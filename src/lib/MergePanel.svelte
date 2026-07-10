<script lang="ts">
	import { onMount } from 'svelte';
	import { sessionMergeStatus, mergeSession, deleteTerminal } from './ipc';
	import { refreshProjects } from './stores';
	import type { MergeStatus } from './types';

	let { projectId, terminalId, onClose }: {
		projectId: string;
		terminalId: string;
		onClose: () => void;
	} = $props();

	let status = $state<MergeStatus | null>(null);
	let loading = $state(true);
	let loadError = $state('');
	let merging = $state(false);
	let deleteAfter = $state(false);
	let result = $state('');
	let merged = $state(false);

	async function load() {
		loading = true;
		loadError = '';
		try {
			status = await sessionMergeStatus(projectId, terminalId);
		} catch (e) {
			loadError = String(e);
		} finally {
			loading = false;
		}
	}

	onMount(load);

	const nothingToMerge = $derived(!!status && status.ahead === 0);
	const canMerge = $derived(
		!!status?.branch && !status?.blocker && !nothingToMerge && !merging
	);

	async function merge() {
		if (!canMerge) return;
		merging = true;
		result = '';
		try {
			const msg = await mergeSession(projectId, terminalId);
			result = msg;
			merged = true;
			await refreshProjects();
			if (deleteAfter) {
				await deleteTerminal(projectId, terminalId);
				onClose();
				return;
			}
			await load();
		} catch (e) {
			result = String(e);
		} finally {
			merging = false;
		}
	}
</script>

<div class="overlay" onclick={onClose} role="presentation">
	<div class="panel" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
		<div class="head">
			<span>Merge session</span>
			<button class="x" onclick={onClose} title="Close">×</button>
		</div>

		<div class="body">
			{#if loading}
				<div class="muted">Checking merge status…</div>
			{:else if loadError}
				<div class="blocker">{loadError}</div>
			{:else if !status?.branch}
				<div class="muted">This session has no git branch, so there's nothing to merge.</div>
			{:else}
				<div class="target">
					<code class="branch">{status.branch}</code>
					<span class="arrow">→</span>
					<code class="branch base">{status.baseBranch}</code>
				</div>

				<div class="stats">
					<span class="stat" class:zero={status.ahead === 0}>
						<strong>{status.ahead}</strong> commit{status.ahead === 1 ? '' : 's'} ahead
					</span>
					<span class="stat">
						<strong>{status.changedFiles.length}</strong>
						file{status.changedFiles.length === 1 ? '' : 's'} changed
					</span>
				</div>

				{#if status.changedFiles.length}
					<ul class="files">
						{#each status.changedFiles as f (f)}
							<li title={f}>{f}</li>
						{/each}
					</ul>
				{/if}

				{#if nothingToMerge}
					<div class="note">This session's branch has no new commits — nothing to merge yet.</div>
				{/if}
				{#if status.uncommitted}
					<div class="note warn">
						This session has uncommitted changes that won't be included until its next turn commits them.
					</div>
				{/if}
				{#if status.blocker}
					<div class="blocker">{status.blocker}</div>
				{/if}

				<label class="del">
					<input type="checkbox" bind:checked={deleteAfter} />
					Delete this session after merging
				</label>

				{#if result}
					<div class="result" class:ok={merged}>{result}</div>
				{/if}
			{/if}
		</div>

		<div class="foot">
			<button class="btn" onclick={onClose}>{merged ? 'Close' : 'Cancel'}</button>
			{#if status?.branch}
				<button class="btn primary" disabled={!canMerge} onclick={merge}>
					{merging ? 'Merging…' : deleteAfter ? 'Merge & delete' : 'Merge'}
				</button>
			{/if}
		</div>
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
	}
	.panel {
		width: 520px;
		max-width: 90vw;
		background: var(--bg);
		border: 1px solid var(--border-strong);
		border-radius: 10px;
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
		display: flex;
		flex-direction: column;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid #2c2c2c;
		font-weight: 600;
		color: #e6e6e6;
	}
	.x {
		background: none;
		border: none;
		color: #999;
		font-size: 18px;
		cursor: pointer;
	}
	.x:hover {
		color: #fff;
	}
	.body {
		padding: 16px;
	}
	.muted {
		color: #9a9a9a;
		font-size: 13px;
	}
	.target {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-bottom: 12px;
	}
	.branch {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
		color: #cdd6e6;
		background: #1b2230;
		border: 1px solid #2a3344;
		border-radius: 5px;
		padding: 2px 7px;
		max-width: 200px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.branch.base {
		color: #9fd0a6;
	}
	.arrow {
		color: #777;
	}
	.stats {
		display: flex;
		gap: 16px;
		font-size: 13px;
		color: #cfcfcf;
		margin-bottom: 10px;
	}
	.stat strong {
		color: #fff;
	}
	.stat.zero strong {
		color: #9a9a9a;
	}
	.files {
		list-style: none;
		margin: 0 0 12px;
		padding: 8px 10px;
		max-height: 180px;
		overflow: auto;
		background: #141414;
		border: 1px solid #2c2c2c;
		border-radius: 6px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
		color: #c8c8c8;
	}
	.files li {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		padding: 1px 0;
	}
	.note {
		font-size: 12.5px;
		color: #9a9a9a;
		margin-bottom: 10px;
	}
	.note.warn {
		color: #d8b25a;
	}
	.blocker {
		font-size: 12.5px;
		color: #e08a8a;
		background: rgba(224, 138, 138, 0.08);
		border: 1px solid rgba(224, 138, 138, 0.3);
		border-radius: 6px;
		padding: 8px 10px;
		margin-bottom: 10px;
	}
	.del {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		color: #cfcfcf;
		margin-top: 4px;
	}
	.result {
		margin-top: 12px;
		font-size: 12.5px;
		color: #cfcfcf;
		border-top: 1px solid #2c2c2c;
		padding-top: 10px;
	}
	.result.ok {
		color: #9fd0a6;
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding: 12px 16px;
		border-top: 1px solid #2c2c2c;
	}
	.btn {
		background: #232323;
		border: 1px solid #3a3a3a;
		border-radius: 6px;
		color: #e6e6e6;
		padding: 7px 14px;
		font-size: 13px;
		cursor: pointer;
	}
	.btn:hover {
		background: #2b2b2b;
	}
	.btn.primary {
		background: #2d5a34;
		border-color: #397043;
		color: #eafbee;
	}
	.btn.primary:hover:not(:disabled) {
		background: #356b3e;
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
