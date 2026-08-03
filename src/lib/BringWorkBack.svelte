<script lang="ts">
	// P2.2 — one surface for "move this session's work somewhere useful", unifying the
	// three previously-disconnected mechanisms: git Merge (branch → base), Send (the
	// generalized "→ parent" — into ANY live session), and Add to the Merge tray.
	import { onMount } from 'svelte';
	import {
		sessionMergeStatus,
		mergeSession,
		addContextBlock,
		readTranscript
	} from './ipc';
	import { projects, openTab, pasteToInput, refreshProjects } from './stores';
	import { isSessionTerminal } from './forest';
	import type { MergeStatus, Turn } from './types';

	let {
		projectId,
		terminalId,
		sessionId,
		title,
		onClose
	}: {
		projectId: string;
		terminalId: string;
		sessionId?: string;
		title: string;
		onClose: () => void;
	} = $props();

	// Sibling sessions we could hand work to (any other session with a live id),
	// on either transport.
	const targets = $derived(
		($projects.find((p) => p.id === projectId)?.terminals ?? []).filter(
			(t) => isSessionTerminal(t) && t.id !== terminalId && t.sessionId
		)
	);
	const projectName = $derived($projects.find((p) => p.id === projectId)?.name);

	let status = $state<MergeStatus | null>(null);
	let lastResponse = $state('');
	let loading = $state(true);
	let busy = $state(false);
	let result = $state('');
	let error = $state('');
	let sendTarget = $state('');

	const nothingToMerge = $derived(!!status && status.ahead === 0 && !status.uncommitted);
	const canMerge = $derived(!!status?.branch && !status?.blocker && !nothingToMerge && !busy);

	onMount(async () => {
		try {
			const [s, turns] = await Promise.all([
				sessionMergeStatus(projectId, terminalId).catch(() => null),
				sessionId ? readTranscript(sessionId) : Promise.resolve([] as Turn[])
			]);
			status = s;
			lastResponse = lastAssistantText(turns);
		} catch (e) {
			error = String(e);
		} finally {
			loading = false;
		}
	});

	function lastAssistantText(turns: Turn[]): string {
		for (let i = turns.length - 1; i >= 0; i--) {
			const t = turns[i];
			if (t.role !== 'assistant') continue;
			const text = t.blocks
				.filter((b) => b.kind === 'text')
				.map((b) => b.text ?? '')
				.join('\n')
				.trim();
			if (text) return text;
		}
		return '';
	}

	async function doMerge() {
		if (!canMerge) return;
		busy = true;
		error = '';
		result = '';
		try {
			result = await mergeSession(projectId, terminalId);
			status = await sessionMergeStatus(projectId, terminalId).catch(() => status);
			await refreshProjects();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	async function doSend() {
		const target = targets.find((t) => t.id === sendTarget);
		if (!target || !lastResponse) return;
		openTab({
			projectId,
			kind: 'claude',
			terminalId: target.id,
			sessionId: target.sessionId ?? undefined,
			title: target.title,
			projectName
		});
		pasteToInput.set({ terminalId: target.id, text: lastResponse });
		result = `Sent this session's last response to “${target.title}”.`;
	}

	async function doAddToTray() {
		if (!lastResponse) return;
		busy = true;
		try {
			await addContextBlock(projectId, 'session', title || 'session', lastResponse);
			await refreshProjects();
			result = 'Added this session’s last response to the Merge tray.';
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	function onKey(e: KeyboardEvent) {
		if (e.key === 'Escape') onClose();
	}
</script>

<svelte:window onkeydown={onKey} />

<div class="overlay" onclick={onClose} role="presentation">
	<div class="panel" role="dialog" aria-modal="true" aria-label="Bring work back" onclick={(e) => e.stopPropagation()}>
		<div class="head">
			<span>Bring work back — <span class="dim">{title}</span></span>
			<button class="x" onclick={onClose} title="Close (Esc)">×</button>
		</div>

		<div class="body">
			<p class="lede">
				This session is an isolated copy of your code and conversation. Choose how its
				work travels back.
			</p>

			{#if loading}
				<div class="hint">Loading this session’s state…</div>
			{:else}
				<!-- 1. Merge branch → base -->
				<section class="opt">
					<div class="opt-head">
						<span class="opt-title">⤵ Merge into <code>{status?.baseBranch ?? 'base'}</code></span>
						{#if status?.branch}
							<span class="meta">
								{status.ahead} commit{status.ahead === 1 ? '' : 's'} ahead{status.uncommitted ? ' · uncommitted changes' : ''}
							</span>
						{/if}
					</div>
					<p class="opt-desc">Fold this session’s git branch back into the branch it was forked from.</p>
					{#if status?.blocker}
						<div class="warn">Can’t merge yet: {status.blocker}</div>
					{:else if nothingToMerge}
						<div class="hint">Nothing to merge — no commits ahead of base.</div>
					{/if}
					<button class="primary" disabled={!canMerge} onclick={doMerge}>Merge</button>
				</section>

				<!-- 2. Send last response into another session -->
				<section class="opt">
					<div class="opt-head"><span class="opt-title">→ Send to another session</span></div>
					<p class="opt-desc">Drop this session’s last response into another session’s input, ready to send.</p>
					{#if !lastResponse}
						<div class="hint">No assistant response to send yet.</div>
					{:else if targets.length === 0}
						<div class="hint">No other sessions in this project to send to.</div>
					{:else}
						<div class="row">
							<select bind:value={sendTarget}>
								<option value="" disabled>Choose a session…</option>
								{#each targets as t (t.id)}
									<option value={t.id}>{t.title}</option>
								{/each}
							</select>
							<button class="secondary" disabled={!sendTarget} onclick={doSend}>Send</button>
						</div>
					{/if}
				</section>

				<!-- 3. Add to the Merge tray -->
				<section class="opt">
					<div class="opt-head"><span class="opt-title">▦ Add to the Merge tray</span></div>
					<p class="opt-desc">Save the last response as a reusable block to seed a future session.</p>
					<button class="secondary" disabled={!lastResponse || busy} onclick={doAddToTray}>Add to tray</button>
				</section>

				{#if result}<div class="ok">{result}</div>{/if}
				{#if error}<div class="warn">{error}</div>{/if}
			{/if}
		</div>

		<div class="foot">
			<button onclick={onClose}>Close</button>
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
		max-height: 85vh;
		overflow: auto;
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
		border-bottom: 1px solid var(--border);
		font-weight: 600;
	}
	.head .dim {
		color: var(--text-dim);
		font-weight: 500;
	}
	.x {
		background: none;
		border: none;
		color: var(--text-dim);
		font-size: 18px;
		cursor: pointer;
	}
	.x:hover {
		color: #fff;
	}
	.body {
		padding: 16px;
		display: flex;
		flex-direction: column;
		gap: 14px;
	}
	.lede {
		margin: 0;
		font-size: 12px;
		color: var(--text-dim);
	}
	.opt {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 12px;
		background: var(--surface);
	}
	.opt-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px;
	}
	.opt-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--text);
	}
	.opt-title code {
		font-family: ui-monospace, Menlo, monospace;
		color: var(--accent-text);
	}
	.meta {
		font-size: 11px;
		color: var(--text-dim);
	}
	.opt-desc {
		margin: 6px 0 10px;
		font-size: 12px;
		color: var(--text-dim);
	}
	.row {
		display: flex;
		gap: 8px;
	}
	.row select {
		flex: 1 1 auto;
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		color: var(--text);
		padding: 6px 10px;
		font-size: 13px;
		cursor: pointer;
	}
	button {
		border-radius: var(--radius);
		padding: 6px 14px;
		font-size: 13px;
		cursor: pointer;
		border: 1px solid var(--border-strong);
		background: var(--bg-elevated);
		color: var(--text);
	}
	button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.primary {
		background: var(--accent);
		border-color: var(--accent-border);
		color: #fff;
	}
	.secondary:hover:not(:disabled) {
		filter: brightness(1.2);
	}
	.hint {
		font-size: 12px;
		color: var(--text-muted);
	}
	.ok {
		font-size: 12px;
		color: var(--ok);
	}
	.warn {
		font-size: 12px;
		color: var(--danger);
		background: var(--danger-bg);
		border-radius: var(--radius);
		padding: 6px 10px;
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		padding: 12px 16px;
		border-top: 1px solid var(--border);
	}
</style>
