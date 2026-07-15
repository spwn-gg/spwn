<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { composeStatus, composeUp, composeDown, composeLogs, onComposeEvent } from './ipc';
	import type { ComposeStatus } from './types';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import type { UnlistenFn } from '@tauri-apps/api/event';

	let { terminalId, onStatus }: { terminalId: string; onStatus?: (m: string) => void } = $props();

	let status = $state<ComposeStatus | null>(null);
	let loading = $state(true);
	let busy = $state(false);
	let logsFor = $state<string | null>(null);
	let logText = $state('');
	let unlisten: UnlistenFn | undefined;
	let timer: ReturnType<typeof setInterval> | undefined;

	async function refresh() {
		try {
			status = await composeStatus(terminalId);
		} catch (e) {
			onStatus?.(String(e));
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		refresh();
		onComposeEvent(terminalId, refresh).then((u) => (unlisten = u));
		// Light polling so live URLs / states stay fresh while the panel is open.
		timer = setInterval(refresh, 4000);
	});
	onDestroy(() => {
		unlisten?.();
		if (timer) clearInterval(timer);
	});

	async function up() {
		busy = true;
		try {
			await composeUp(terminalId);
			onStatus?.('Services starting…');
			await refresh();
		} catch (e) {
			onStatus?.(String(e));
		} finally {
			busy = false;
		}
	}

	async function down() {
		busy = true;
		try {
			await composeDown(terminalId);
			onStatus?.('Services stopped.');
			await refresh();
		} catch (e) {
			onStatus?.(String(e));
		} finally {
			busy = false;
		}
	}

	async function toggleLogs(svc: string) {
		if (logsFor === svc) {
			logsFor = null;
			return;
		}
		logsFor = svc;
		logText = 'Loading…';
		try {
			logText = (await composeLogs(terminalId, svc)) || '(no output)';
		} catch (e) {
			logText = String(e);
		}
	}

	async function open(url: string) {
		try {
			await openUrl(url);
		} catch (e) {
			onStatus?.(String(e));
		}
	}

	const anyRunning = $derived(!!status?.services.some((s) => s.state === 'running'));
</script>

<div class="services">
	{#if loading}
		<div class="muted">Checking services…</div>
	{:else if !status?.available}
		<div class="muted">Docker isn't available — start Docker Desktop to run this session's services.</div>
	{:else if status.services.length === 0}
		<div class="muted">No services declared in this project's <code>spwn.yaml</code>.</div>
	{:else}
		<div class="rows">
			{#each status.services as s (s.name)}
				<div class="row">
					<span class="dot" class:on={s.state === 'running'} title={s.state}></span>
					<span class="name">{s.name}</span>
					<span class="badge {s.scope}">{s.scope}</span>
					{#if s.role}<span class="role">{s.role}</span>{/if}
					{#if s.url}
						<button class="url" onclick={() => open(s.url!)} title="Open {s.url}">{s.url}</button>
					{:else}
						<span class="state">{s.state}</span>
					{/if}
					<button class="mini" onclick={() => toggleLogs(s.name)} title="Show recent logs"
						>{logsFor === s.name ? 'Hide logs' : 'Logs'}</button
					>
				</div>
				{#if logsFor === s.name}
					<pre class="logs">{logText}</pre>
				{/if}
			{/each}
		</div>

		<div class="foot">
			<button class="btn" disabled={busy} onclick={up}>{anyRunning ? 'Restart / Up' : 'Up'}</button>
			<button class="btn" disabled={busy || !anyRunning} onclick={down}>Down</button>
		</div>
	{/if}
</div>

<style>
	.services {
		border: 1px solid #2c2c2c;
		border-radius: 8px;
		background: #141414;
		padding: 10px 12px;
		margin: 6px 8px;
		font-size: 13px;
	}
	.muted {
		color: #9a9a9a;
	}
	.rows {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: #5a5a5a;
		flex: 0 0 auto;
	}
	.dot.on {
		background: #5cc46a;
		box-shadow: 0 0 6px rgba(92, 196, 106, 0.6);
	}
	.name {
		font-family: ui-monospace, Menlo, monospace;
		color: #e6e6e6;
	}
	.badge {
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.03em;
		padding: 1px 6px;
		border-radius: 4px;
		border: 1px solid #333;
		color: #b9c2d0;
	}
	.badge.shared {
		color: #d8b25a;
		border-color: #5a4a24;
		background: rgba(216, 178, 90, 0.08);
	}
	.badge.session {
		color: #7fb0d8;
		border-color: #274156;
		background: rgba(127, 176, 216, 0.08);
	}
	.role {
		font-size: 11.5px;
		color: #8a8a8a;
	}
	.state {
		font-size: 12px;
		color: #8a8a8a;
		margin-left: auto;
	}
	.url {
		margin-left: auto;
		background: none;
		border: none;
		color: #6ab0ff;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
		cursor: pointer;
		text-decoration: underline;
		padding: 0;
	}
	.url:hover {
		color: #8ec6ff;
	}
	.mini {
		background: #232323;
		border: 1px solid #3a3a3a;
		border-radius: 5px;
		color: #cfcfcf;
		font-size: 11.5px;
		padding: 2px 8px;
		cursor: pointer;
	}
	.mini:hover {
		background: #2b2b2b;
	}
	.logs {
		margin: 0;
		padding: 8px 10px;
		max-height: 200px;
		overflow: auto;
		background: #0d0d0d;
		border: 1px solid #262626;
		border-radius: 6px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11.5px;
		color: #c8c8c8;
		white-space: pre-wrap;
		word-break: break-word;
	}
	.foot {
		display: flex;
		gap: 8px;
		margin-top: 10px;
	}
	.btn {
		background: #232323;
		border: 1px solid #3a3a3a;
		border-radius: 6px;
		color: #e6e6e6;
		padding: 5px 12px;
		font-size: 12.5px;
		cursor: pointer;
	}
	.btn:hover:not(:disabled) {
		background: #2b2b2b;
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
