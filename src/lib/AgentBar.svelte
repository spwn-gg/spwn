<script lang="ts">
	import { agentInterrupt, agentSetMode } from './ipc';
	import type { AgentSummary } from './types';

	let {
		terminalId = undefined,
		def = undefined
	}: {
		terminalId?: string;
		def?: AgentSummary;
	} = $props();

	/**
	 * Modes spwn offers. Deliberately excludes anything that bypasses permission
	 * checks: with the TUI as the surface, the agent's own Shift-Tab still reaches
	 * every mode it supports, so there's no need for spwn to make that easy.
	 */
	const MODES = ['default', 'acceptEdits', 'plan'];
	const LABEL: Record<string, string> = {
		default: 'Ask',
		acceptEdits: 'Accept edits',
		plan: 'Plan'
	};

	let busy = $state(false);
	let error = $state<string | null>(null);

	async function setMode(m: string) {
		if (!terminalId) return;
		busy = true;
		error = null;
		try {
			await agentSetMode(terminalId, m);
		} catch (e) {
			// Mode cycling reads the screen back; if the agent's footer changes shape
			// this fails rather than blindly pressing keys until it lands somewhere
			// arbitrary. Say so instead of pretending it worked.
			error = String(e);
		} finally {
			busy = false;
		}
	}

	async function stop() {
		if (!terminalId) return;
		try {
			await agentInterrupt(terminalId);
		} catch (e) {
			error = String(e);
		}
	}
</script>

<div class="bar">
	{#if def}
		<span class="agent" title={def.binary ?? 'not found'}>
			{def.icon ?? '✦'}
			{def.name}
		</span>
		{#if def.untested}
			<span class="chip warn" title="Ships with spwn but has not been verified against the real CLI">
				experimental
			</span>
		{/if}
	{/if}

	<span class="spacer"></span>

	{#if error}
		<span class="chip err" title={error}>mode?</span>
	{/if}

	<div class="modes">
		{#each MODES as m (m)}
			<button class="mode" disabled={busy || !terminalId} onclick={() => setMode(m)}>
				{LABEL[m] ?? m}
			</button>
		{/each}
	</div>

	<button class="stop" disabled={!terminalId} onclick={stop} title="Interrupt the current turn (Esc)">
		Stop
	</button>
</div>

<style>
	.bar {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 4px 8px;
		border-bottom: 1px solid var(--border, #2a2a2a);
		background: var(--panel, #191919);
		font-size: 12px;
		color: var(--fg-dim, #9a9a9a);
		flex: 0 0 auto;
	}
	.agent {
		display: inline-flex;
		gap: 5px;
		align-items: center;
		color: var(--fg, #e6e6e6);
	}
	.spacer {
		flex: 1;
	}
	.chip {
		padding: 1px 6px;
		border-radius: 999px;
		font-size: 10px;
		border: 1px solid var(--border, #2a2a2a);
	}
	.warn {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.err {
		color: #e06c75;
		border-color: #5c2a2a;
	}
	.modes {
		display: inline-flex;
		gap: 2px;
	}
	.mode,
	.stop {
		background: transparent;
		border: 1px solid var(--border, #2a2a2a);
		color: inherit;
		border-radius: 4px;
		padding: 2px 7px;
		font-size: 11px;
		cursor: pointer;
	}
	.mode:hover:not(:disabled),
	.stop:hover:not(:disabled) {
		background: var(--hover, #232323);
		color: var(--fg, #e6e6e6);
	}
	.mode:disabled,
	.stop:disabled {
		opacity: 0.45;
		cursor: default;
	}
</style>
