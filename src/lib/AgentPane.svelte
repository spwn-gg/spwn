<script lang="ts">
	/**
	 * One agent session: the agent's real TUI, live, in an rmux pane.
	 *
	 * Deliberately thin compared to the sidecar-era ClaudePane. There is no event
	 * stream to reassemble, no optimistic overlay, and no permission/question queue —
	 * the TUI renders all of that itself, and keystrokes go straight to it. What's
	 * left is the things the TUI can't know about: which agent this is, and spwn's
	 * own "paste this into the composer" channel.
	 */
	import { onDestroy } from 'svelte';
	import Terminal from './Terminal.svelte';
	import AgentBar from './AgentBar.svelte';
	import { agentSend, listAgents } from './ipc';
	import { pasteToInput } from './stores';
	import type { AgentSummary } from './types';

	let {
		tabKey,
		projectId,
		agent = undefined,
		terminalId = undefined,
		claudeResume = undefined,
		claudeFork = undefined,
		parentTerminalId = undefined,
		initialPrompt = undefined
	}: {
		tabKey: string;
		projectId: string;
		agent?: string;
		terminalId?: string;
		claudeResume?: string;
		claudeFork?: string;
		parentTerminalId?: string;
		initialPrompt?: string;
	} = $props();

	let liveId = $state<string | null>(terminalId ?? null);
	let defs = $state<AgentSummary[]>([]);
	let def = $derived(defs.find((d) => d.id === agent) ?? defs.find((d) => d.id === 'claude'));

	listAgents()
		.then((a) => (defs = a))
		.catch(() => {});

	function onOpened(id: string) {
		liveId = id;
		// The initial prompt is pasted, NOT submitted: a seeded session should open
		// with its context in the composer so the human can read and edit it before
		// spending a turn. Same contract as "→ parent".
		if (initialPrompt) {
			agentSend(id, initialPrompt, false).catch(() => {});
		}
	}

	// "Bring work back" / context injection drop text into this session's composer.
	const stopPaste = pasteToInput.subscribe((p) => {
		if (!p || !liveId || p.terminalId !== liveId) return;
		agentSend(liveId, p.text, false).catch(() => {});
		pasteToInput.set(null);
	});
	onDestroy(stopPaste);
</script>

<div class="pane">
	<AgentBar terminalId={liveId ?? undefined} {def} />
	<div class="term">
		<Terminal
			{tabKey}
			{projectId}
			kind="agent"
			{agent}
			{terminalId}
			{claudeResume}
			{claudeFork}
			{parentTerminalId}
			{onOpened} />
	</div>
</div>

<style>
	.pane {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.term {
		flex: 1;
		min-height: 0;
	}
</style>
