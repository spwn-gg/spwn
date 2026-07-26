<script lang="ts">
	import { onMount } from 'svelte';
	import {
		gitRepoStatus,
		gitBranches,
		gitCheckout,
		gitCreateBranch,
		gitFetch,
		gitPull,
		gitPush,
		gitSync
	} from './ipc';
	import type { RepoStatus, GitBranches } from './types';

	let { projectId, onChanged }: { projectId: string; onChanged?: () => void } = $props();

	let status = $state<RepoStatus | null>(null);
	let branches = $state<GitBranches | null>(null);
	let busy = $state(false);
	let error = $state('');
	let info = $state('');
	let showBranches = $state(false);
	let newBranch = $state('');

	async function load() {
		try {
			[status, branches] = await Promise.all([gitRepoStatus(projectId), gitBranches(projectId)]);
			error = '';
		} catch (e) {
			error = String(e);
		}
	}

	onMount(load);

	/** Run a mutating git action with shared busy/error handling, then reload. */
	async function run(fn: () => Promise<string | void>, closeList = true) {
		if (busy) return;
		busy = true;
		error = '';
		info = '';
		try {
			const msg = await fn();
			if (typeof msg === 'string' && msg.trim()) info = lastLine(msg);
			if (closeList) showBranches = false;
			await load();
			onChanged?.();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	function lastLine(s: string): string {
		const lines = s.split('\n').map((l) => l.trim()).filter(Boolean);
		return lines[lines.length - 1] ?? '';
	}

	/** Remote entries like "origin/feature" → check out the local tracking name. */
	function checkout(branch: string, remote = false) {
		const target = remote ? branch.slice(branch.indexOf('/') + 1) : branch;
		if (target === status?.branch) {
			showBranches = false;
			return;
		}
		run(() => gitCheckout(projectId, target));
	}

	function createBranch() {
		const name = newBranch.trim();
		if (!name) return;
		newBranch = '';
		run(() => gitCreateBranch(projectId, name));
	}
</script>

{#if status?.isRepo}
	<div class="scm">
		<div class="scm-head">
			<button
				class="branch-btn"
				title={status.upstream ? `tracking ${status.upstream}` : 'no upstream'}
				onclick={() => (showBranches = !showBranches)}>
				<span class="glyph">⎇</span>
				<span class="bname">{status.branch ?? 'detached'}</span>
				<span class="caret">{showBranches ? '▾' : '▸'}</span>
			</button>
			{#if status.behind > 0 || status.ahead > 0}
				<span class="ab" title="{status.behind} behind · {status.ahead} ahead of upstream">
					{#if status.behind > 0}<span class="down">↓{status.behind}</span>{/if}
					{#if status.ahead > 0}<span class="up">↑{status.ahead}</span>{/if}
				</span>
			{/if}
			{#if status.dirty}<span class="dirty" title="Uncommitted changes">●</span>{/if}
			<span class="spacer"></span>
			<button
				class="sync"
				disabled={busy}
				title="Sync (fetch, fast-forward pull, push)"
				onclick={() => run(() => gitSync(projectId), false)}>
				<span class:spin={busy}>⟳</span>
			</button>
		</div>

		{#if showBranches}
			<div class="branches">
				<input
					class="new-branch"
					placeholder="Create branch…"
					bind:value={newBranch}
					disabled={busy}
					onkeydown={(e) => e.key === 'Enter' && createBranch()} />
				{#each branches?.local ?? [] as b (b)}
					<button class="branch-row" class:current={b === status.branch} onclick={() => checkout(b)}>
						<span class="tick">{b === status.branch ? '✓' : ''}</span>
						<span class="rname">{b}</span>
					</button>
				{/each}
				{#if branches?.remote?.length}
					<div class="sub">Remote</div>
					{#each branches.remote as b (b)}
						<button class="branch-row remote" onclick={() => checkout(b, true)}>
							<span class="tick"></span>
							<span class="rname">{b}</span>
						</button>
					{/each}
				{/if}
			</div>
		{/if}

		<div class="sync-row">
			<button disabled={busy} onclick={() => run(() => gitFetch(projectId), false)}>Fetch</button>
			<button disabled={busy} onclick={() => run(() => gitPull(projectId), false)}>Pull</button>
			<button disabled={busy} onclick={() => run(() => gitPush(projectId), false)}>Push</button>
		</div>

		{#if error}
			<div class="msg err" title={error}>{error}</div>
		{:else if info}
			<div class="msg ok" title={info}>{info}</div>
		{/if}
	</div>
{/if}

<style>
	.scm {
		padding: 4px 8px 8px 26px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.scm-head {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.branch-btn {
		display: flex;
		align-items: center;
		gap: 4px;
		background: var(--bg-elevated);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text);
		padding: 2px 6px;
		font-size: 12px;
		cursor: pointer;
		max-width: 160px;
	}
	.branch-btn:hover {
		background: var(--bg-hover);
	}
	.glyph {
		color: var(--accent-text);
	}
	.bname {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: ui-monospace, Menlo, monospace;
	}
	.caret {
		color: var(--text-muted);
		font-size: 10px;
	}
	.ab {
		display: flex;
		gap: 4px;
		font-size: 11px;
		font-family: ui-monospace, Menlo, monospace;
	}
	.down {
		color: var(--accent-text);
	}
	.up {
		color: var(--ok);
	}
	.dirty {
		color: var(--danger);
		font-size: 10px;
	}
	.spacer {
		flex: 1;
	}
	.sync {
		background: none;
		border: none;
		color: var(--text-dim);
		cursor: pointer;
		font-size: 14px;
		padding: 2px 4px;
	}
	.sync:hover:not(:disabled) {
		color: var(--text);
	}
	.sync:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.spin {
		display: inline-block;
		animation: spin 0.9s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	.branches {
		display: flex;
		flex-direction: column;
		gap: 1px;
		max-height: 220px;
		overflow-y: auto;
		background: var(--bg-input);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: 4px;
	}
	.new-branch {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text);
		padding: 3px 6px;
		font-size: 12px;
		margin-bottom: 3px;
	}
	.new-branch:focus {
		outline: none;
		border-color: var(--accent-border);
	}
	.branch-row {
		display: flex;
		align-items: center;
		gap: 4px;
		background: none;
		border: none;
		color: var(--text);
		padding: 3px 4px;
		font-size: 12px;
		cursor: pointer;
		text-align: left;
		border-radius: 4px;
		font-family: ui-monospace, Menlo, monospace;
	}
	.branch-row:hover {
		background: var(--bg-hover);
	}
	.branch-row.current {
		color: var(--accent-text);
	}
	.branch-row.remote .rname {
		color: var(--text-dim);
	}
	.tick {
		width: 10px;
		color: var(--accent-text);
	}
	.rname {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.sub {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-muted);
		padding: 4px 4px 2px;
	}
	.sync-row {
		display: flex;
		gap: 4px;
	}
	.sync-row button {
		flex: 1;
		background: var(--bg-elevated);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text-dim);
		padding: 3px 0;
		font-size: 11px;
		cursor: pointer;
	}
	.sync-row button:hover:not(:disabled) {
		background: var(--bg-hover);
		color: var(--text);
	}
	.sync-row button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.msg {
		font-size: 11px;
		line-height: 1.3;
		max-height: 48px;
		overflow: hidden;
		word-break: break-word;
	}
	.msg.err {
		color: var(--danger);
	}
	.msg.ok {
		color: var(--text-dim);
	}
</style>
