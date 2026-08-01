<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { hooksStatus, hooksRun, onHooksEvent, onHookOutput, onHookRunning } from './ipc';
	import type { HooksStatus, HookEventInfo } from './types';
	import type { UnlistenFn } from '@tauri-apps/api/event';

	let { terminalId, onStatus }: { terminalId: string; onStatus?: (m: string) => void } = $props();

	let status = $state<HooksStatus | null>(null);
	let loading = $state(true);
	let busy = $state(false);
	let openLog = $state<string | null>(null);
	let unlisten: UnlistenFn[] = [];
	let timer: ReturnType<typeof setInterval> | undefined;

	// The event whose hook is executing right now, and its live-streamed output lines
	// (per event), so the panel shows progress as a hook runs — not just the final tail.
	let runningEvent = $state<string | null>(null);
	let liveLines = $state<Record<string, string[]>>({});

	async function refresh() {
		try {
			status = await hooksStatus(terminalId);
			runningEvent = status.running ?? null;
		} catch (e) {
			onStatus?.(String(e));
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		refresh();
		onHooksEvent(terminalId, refresh).then((u) => unlisten.push(u));
		// A hook for this session started / finished → toggle the live spinner + log.
		onHookRunning((e) => {
			if (e.terminalId !== terminalId) return;
			runningEvent = e.event;
			if (e.event) {
				liveLines = { ...liveLines, [e.event]: [] };
				openLog = e.event; // auto-open the log so output is visible live
			}
		}).then((u) => unlisten.push(u));
		// Live output lines from the running hook.
		onHookOutput(terminalId, (e) => {
			liveLines = { ...liveLines, [e.event]: [...(liveLines[e.event] ?? []), e.line] };
		}).then((u) => unlisten.push(u));
		// Light polling so late-finishing hooks / statuses stay fresh while open.
		timer = setInterval(refresh, 4000);
	});
	onDestroy(() => {
		unlisten.forEach((u) => u());
		if (timer) clearInterval(timer);
	});

	async function run(event: string) {
		busy = true;
		try {
			await hooksRun(terminalId, event);
			onStatus?.(`Ran ${event} hook.`);
			await refresh();
		} catch (e) {
			onStatus?.(String(e));
		} finally {
			busy = false;
		}
	}

	function toggleLog(event: string) {
		openLog = openLog === event ? null : event;
	}

	/** Status dot for an event: 'run' while executing, else worst-of the scripts' last
	 * runs (red if any failed, green if any succeeded, grey if none ran yet). */
	function dot(ev: HookEventInfo): '' | 'ok' | 'fail' | 'run' {
		if (runningEvent === ev.event) return 'run';
		const runs = ev.scripts.map((s) => s.lastRun).filter(Boolean) as { ok: boolean }[];
		if (runs.length === 0) return '';
		return runs.every((r) => r.ok) ? 'ok' : 'fail';
	}

	/** The discovered scripts for an event, labelled by scope (global first). */
	function scriptsLabel(ev: HookEventInfo): string {
		if (ev.scripts.length === 0) return '';
		return ev.scripts.map((s) => `${s.scope}: ${s.script}`).join(', ');
	}

	function logText(ev: HookEventInfo): string {
		// While running, show the live-streamed output as it arrives (tagged by event,
		// not scope, so global + repo output share one live view).
		if (runningEvent === ev.event) {
			const lines = liveLines[ev.event] ?? [];
			return `$ ${ev.event} (running…)\n${lines.join('\n') || '…'}`;
		}
		const parts = ev.scripts
			.filter((s) => s.lastRun)
			.map((s) => {
				const r = s.lastRun!;
				const head = `[${s.scope}] $ ${r.script}${r.ok ? '' : ` (exit ${r.exitCode ?? '?'})`}`;
				return `${head}\n${r.output || '(no output)'}`;
			});
		return parts.length ? parts.join('\n\n') : '(not run yet)';
	}
</script>

<div class="hooks">
	{#if loading}
		<div class="muted">Checking hooks…</div>
	{:else if !status?.available}
		<div class="muted">
			Hooks run only for sessions with their own worktree. Add a
			<code>~/.spwn/hooks/&lt;event&gt;.sh</code> (shared) or
			<code>.spwn/hooks/&lt;event&gt;.sh</code> (this repo) file to use them.
		</div>
	{:else}
		<div class="rows">
			{#each status.events as ev (ev.event)}
				<div class="row">
					<span class="d {dot(ev)}" title={dot(ev) === 'run' ? 'running…' : dot(ev) || 'not run'}></span>
					<span class="name">{ev.event}</span>
					<span class="scripts">
						{#if ev.scripts.length}
							{scriptsLabel(ev)}
						{:else}
							<span class="none">no hook</span>
						{/if}
					</span>
					<button
						class="mini"
						disabled={ev.scripts.length === 0 && dot(ev) === ''}
						onclick={() => toggleLog(ev.event)}
						title="Show last output">{openLog === ev.event ? 'Hide' : 'Output'}</button
					>
					<button
						class="mini"
						disabled={busy || !!runningEvent || ev.scripts.length === 0}
						onclick={() => run(ev.event)}
						title="Run this event's hook now">Run</button
					>
				</div>
				{#if openLog === ev.event}
					<pre class="log">{logText(ev)}</pre>
				{/if}
			{/each}
		</div>
		<div class="foot muted">
			Layered from <code>~/.spwn/hooks/&lt;event&gt;.sh</code> (shared, runs first) then
			<code>.spwn/hooks/&lt;event&gt;.sh</code> (this repo).
		</div>
	{/if}
</div>

<style>
	.hooks {
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
	.d {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: #5a5a5a;
		flex: 0 0 auto;
	}
	.d.ok {
		background: #5cc46a;
		box-shadow: 0 0 6px rgba(92, 196, 106, 0.6);
	}
	.d.fail {
		background: #d85a5a;
		box-shadow: 0 0 6px rgba(216, 90, 90, 0.6);
	}
	.d.run {
		background: transparent;
		border: 2px solid rgba(127, 163, 223, 0.3);
		border-top-color: #7fa3df;
		box-shadow: none;
		animation: hook-spin 0.7s linear infinite;
	}
	@keyframes hook-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.name {
		font-family: ui-monospace, Menlo, monospace;
		color: #e6e6e6;
	}
	.scripts {
		flex: 1 1 auto;
		color: #8a8a8a;
		font-size: 12px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.none {
		color: #6a6a6a;
		font-style: italic;
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
	.mini:hover:not(:disabled) {
		background: #2b2b2b;
	}
	.mini:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.log {
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
		margin-top: 10px;
		font-size: 11.5px;
	}
	code {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11.5px;
		color: #b9c2d0;
	}
</style>
