<script lang="ts">
	import { onMount } from 'svelte';
	import {
		sessionMergeStatus,
		mergeSession,
		deleteTerminal,
		syncSessionFromBase,
		abortSessionSync,
		verifySessionMerge
	} from './ipc';
	import { refreshProjects, pasteToInput } from './stores';
	import { syncConflictPrompt } from './labels';
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
	let commitFirst = $state(true);
	let syncing = $state(false);
	let syncNote = $state('');
	let verifying = $state(false);
	let verifyNote = $state('');
	let verifyOk = $state<boolean | null>(null);
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

	const nothingToMerge = $derived(!!status && status.ahead === 0 && !status.uncommitted);
	// A running turn is writing the tree right now: committing would catch it
	// half-written, and merging without committing would drop the work. Neither is a
	// choice worth offering, so wait it out.
	const inFlux = $derived(!!status?.uncommitted && !!status?.midTurn);
	// An unresolved sync blocks everything: the tree holds conflict markers, so neither
	// committing nor merging is a sane thing to do next.
	const midSync = $derived((status?.syncConflicts?.length ?? 0) > 0);
	// Worth offering a sync when the base has moved on and the land isn't already a
	// guaranteed fast-forward.
	const canSync = $derived(
		!!status?.branch && !!status?.behind && !status?.willFastForward && !inFlux && !syncing
	);
	const conflicts = $derived(new Set(status?.conflicts ?? []));
	// More than one rung means this merge lands on another session's branch, and the
	// work still has that many hops to go before it reaches a root branch.
	const path = $derived(status?.mergePath ?? []);
	const rungsLeft = $derived(Math.max(0, path.length - 1));
	// "Clean" is only claimable when the trial merge actually ran and found nothing.
	const mergesClean = $derived(
		!!status && !nothingToMerge && !status.previewUnavailable && conflicts.size === 0
	);
	const canMerge = $derived(
		!!status?.branch && !status?.blocker && !nothingToMerge && !merging && !inFlux && !midSync
	);

	async function sync() {
		if (!canSync) return;
		syncing = true;
		syncNote = '';
		try {
			const r = await syncSessionFromBase(terminalId);
			if (r.outcome === 'upToDate') {
				syncNote = 'Already up to date with the base.';
			} else if (r.outcome === 'merged') {
				syncNote = `Synced — landing this is now a fast-forward. ${r.summary}`;
			} else if (r.outcome === 'replayedResolution') {
				syncNote = `Conflicted, but git replayed a resolution you'd recorded before and finished the merge (${r.files.join(
					', '
				)}). Replays are textual, so give ${r.files.length === 1 ? 'it' : 'them'} a look.`;
			} else {
				syncNote = handOff(r.conflicts);
			}
		} catch (e) {
			syncNote = String(e);
		} finally {
			syncing = false;
			await load();
		}
	}

	/** Put the conflict in front of the agent that caused it — see syncConflictPrompt. */
	function handOff(paths: string[]): string {
		pasteToInput.set({
			terminalId,
			text: syncConflictPrompt(status?.baseBranch ?? 'the base branch', paths)
		});
		const n = paths.length;
		return `Sync stopped on ${n} conflict${n === 1 ? '' : 's'}. A note describing ${
			n === 1 ? 'it' : 'them'
		} is waiting in this session's composer — send it to have the agent resolve ${
			n === 1 ? 'it' : 'them'
		}, or abort the sync.`;
	}

	// Both branches can be green on their own and still merge into something broken —
	// only the combined tree proves otherwise, so it has to be built somewhere.
	async function verify() {
		if (verifying) return;
		verifying = true;
		verifyNote = '';
		verifyOk = null;
		try {
			const r = await verifySessionMerge(projectId, terminalId);
			if (r.noScripts) {
				verifyNote =
					'No session-integrate hook is set up, so nothing was checked. Add one (e.g. ~/.spwn/hooks/session-integrate.d/10-test.sh) to build and test the merged result.';
			} else {
				verifyOk = r.ok;
				const failed = r.runs.filter((x) => !x.ok);
				verifyNote = r.ok
					? `The merged result passed ${r.runs.length} check${r.runs.length === 1 ? '' : 's'}.`
					: `${failed.length} of ${r.runs.length} checks failed on the merged result: ${failed
							.map((x) => x.script)
							.join(', ')}. Both branches can pass alone and still break together.`;
			}
		} catch (e) {
			verifyNote = String(e);
		} finally {
			verifying = false;
		}
	}

	async function abort() {
		syncing = true;
		try {
			await abortSessionSync(terminalId);
			syncNote = 'Sync aborted — the branch is back where it was.';
		} catch (e) {
			syncNote = String(e);
		} finally {
			syncing = false;
			await load();
		}
	}

	async function merge() {
		if (!canMerge) return;
		merging = true;
		result = '';
		try {
			const msg = await mergeSession(projectId, terminalId, commitFirst && !!status?.uncommitted);
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

				{#if rungsLeft > 0}
					<div class="note warn">
						This is a fork of another session, so merging lands in
						<code>{path[0]}</code> — not <code>{path[path.length - 1]}</code>.
						Full route: <code>{[status.branch, ...path].join(' → ')}</code>
						({rungsLeft} more merge{rungsLeft === 1 ? '' : 's'} after this one).
						Folding each fork into its parent keeps every step small; going straight to
						<code>{path[path.length - 1]}</code> would skip that.
					</div>
				{/if}

				<div class="stats">
					<span class="stat" class:zero={status.ahead === 0}>
						<strong>{status.ahead}</strong> commit{status.ahead === 1 ? '' : 's'} ahead
					</span>
					<span class="stat">
						<strong>{status.changedFiles.length}</strong>
						file{status.changedFiles.length === 1 ? '' : 's'} changed
					</span>
					{#if status.behind}
						<span class="stat clash"><strong>{status.behind}</strong> behind</span>
					{/if}
					{#if conflicts.size}
						<span class="stat clash">
							<strong>{conflicts.size}</strong>
							conflict{conflicts.size === 1 ? '' : 's'}
						</span>
					{:else if mergesClean}
						<span class="stat clean">merges cleanly</span>
					{/if}
				</div>

				{#if status.changedFiles.length}
					<ul class="files">
						{#each status.changedFiles as f (f)}
							<li class:clash={conflicts.has(f)} title={conflicts.has(f) ? `${f} — conflicts with ${status.baseBranch}` : f}>
								{f}
							</li>
						{/each}
					</ul>
				{/if}

				{#if nothingToMerge}
					<div class="note">
						This session's branch has no new commits and nothing uncommitted — nothing to merge yet.
					</div>
				{/if}
				{#if conflicts.size}
					<div class="note warn">
						{conflicts.size === 1 ? 'One file conflicts' : `${conflicts.size} files conflict`}
						with <code>{status.baseBranch}</code>. Merging will stop on the conflict and
						leave <code>{status.baseBranch}</code> untouched.
					</div>
				{/if}
				{#if status.previewUnavailable}
					<div class="note">
						Couldn't check for conflicts ahead of time: {status.previewUnavailable}
					</div>
				{/if}
				{#if midSync}
					<div class="note warn">
						A sync is part-way through, with conflicts still unresolved in
						{status.syncConflicts.length === 1 ? ' one file' : ` ${status.syncConflicts.length} files`}:
						<code>{status.syncConflicts.join(', ')}</code>. Resolve them in the session, or
						abort the sync.
					</div>
					<div class="btnrow">
						<button class="btn" disabled={syncing} onclick={() => (syncNote = handOff(status?.syncConflicts ?? []))}>
							Hand to the agent
						</button>
						<button class="btn" disabled={syncing} onclick={abort}>Abort sync</button>
					</div>
				{:else if canSync}
					<div class="note">
						<code>{status.baseBranch}</code> has moved on by {status.behind}
						commit{status.behind === 1 ? '' : 's'}. Syncing brings it into this session's
						branch — conflicts surface here, where the agent can resolve them, and the
						merge afterwards becomes a fast-forward that can't fail.
					</div>
					<button class="btn" disabled={!canSync} onclick={sync}>
						{syncing ? 'Syncing…' : `Sync with ${status.baseBranch}`}
					</button>
				{:else if status.willFastForward && !nothingToMerge}
					<div class="note ok">Up to date with the base — this will fast-forward.</div>
				{/if}
				{#if syncNote}
					<div class="note">{syncNote}</div>
				{/if}

				{#if inFlux}
					<div class="note warn">
						This session is mid-turn and has uncommitted changes. Wait for the turn to finish,
						so the merge doesn't take a half-written tree.
					</div>
				{:else if status.uncommitted}
					<label class="del">
						<input type="checkbox" bind:checked={commitFirst} />
						Commit this session's uncommitted changes first
					</label>
					{#if !commitFirst}
						<div class="note warn">
							Uncommitted changes will be left behind — only committed work merges.
						</div>
					{/if}
				{/if}
				{#if status.blocker}
					<div class="blocker">{status.blocker}</div>
				{/if}

				{#if !nothingToMerge && !midSync && !conflicts.size}
					<div class="btnrow">
						<button class="btn" disabled={verifying} onclick={verify}>
							{verifying ? 'Building the merged result…' : 'Verify merged result'}
						</button>
					</div>
				{/if}
				{#if verifyNote}
					<div class="note" class:warn={verifyOk === false} class:ok={verifyOk === true}>
						{verifyNote}
					</div>
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
					{merging
						? 'Merging…'
						: conflicts.size
							? 'Merge anyway'
							: deleteAfter
								? 'Merge & delete'
								: 'Merge'}
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
	.stat.clash strong {
		color: #d8b25a;
	}
	.stat.clean {
		color: #9fd0a6;
	}
	.files li {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		padding: 1px 0;
	}
	.files li.clash {
		color: #d8b25a;
	}
	.files li.clash::before {
		content: '!';
		display: inline-block;
		width: 10px;
		font-weight: 700;
	}
	.files li:not(.clash) {
		padding-left: 10px;
	}
	.note {
		font-size: 12.5px;
		color: #9a9a9a;
		margin-bottom: 10px;
	}
	.note.warn {
		color: #d8b25a;
	}
	.note.ok {
		color: #9fd0a6;
	}
	.btnrow {
		display: flex;
		gap: 8px;
		margin-bottom: 10px;
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
