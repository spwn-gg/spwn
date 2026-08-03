<script lang="ts">
	/**
	 * One agent session: the agent's real TUI, live, in an rmux pane.
	 *
	 * There is no event stream to reassemble, no optimistic overlay, and no
	 * permission/question queue — the TUI renders all of that itself, and keystrokes
	 * go straight to it.
	 *
	 * What the terminal cannot do is talk about "a turn": it has no notion of one, so
	 * ↺ Return here, → parent, ＋ ctx and ⑂ Fork have nowhere to live in the pane.
	 * Those move into the Inspector's Transcript tab, one click away, rendered from
	 * the session's own on-disk history.
	 */
	import { onDestroy } from 'svelte';
	import Terminal from './Terminal.svelte';
	import AgentBar from './AgentBar.svelte';
	import SessionStatusStrip from './SessionStatusStrip.svelte';
	import Inspector from './Inspector.svelte';
	import { agentSend, listAgents } from './ipc';
	import { pasteToInput, inspectorOpen, toggleInspector, projects } from './stores';
	import type { AgentSummary } from './types';

	let {
		tabKey,
		projectId,
		agent = undefined,
		terminalId = undefined,
		sessionId = undefined,
		claudeResume = undefined,
		claudeFork = undefined,
		parentTerminalId = undefined,
		initialPrompt = undefined
	}: {
		tabKey: string;
		projectId: string;
		agent?: string;
		terminalId?: string;
		sessionId?: string;
		claudeResume?: string;
		claudeFork?: string;
		parentTerminalId?: string;
		initialPrompt?: string;
	} = $props();

	let liveId = $state<string | null>(terminalId ?? null);
	let defs = $state<AgentSummary[]>([]);
	let def = $derived(defs.find((d) => d.id === agent) ?? defs.find((d) => d.id === 'claude'));

	// The session id is assigned at launch, so it's on the record as soon as the
	// backend has opened the pane — no init event to wait for.
	const term = $derived(
		$projects.find((p) => p.id === projectId)?.terminals.find((t) => t.id === liveId)
	);
	const liveSession = $derived(term?.sessionId ?? sessionId);

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
	<SessionStatusStrip
		{projectId}
		terminalId={liveId ?? undefined}
		open={!!(liveId && $inspectorOpen.has(liveId))}
		onOpen={() => liveId && toggleInspector(liveId)} />
	<AgentBar
		terminalId={liveId ?? undefined}
		{def}
		onTranscript={() => liveId && toggleInspector(liveId)} />
	<div class="body">
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
		{#if liveId && $inspectorOpen.has(liveId)}
			<Inspector {projectId} terminalId={liveId} sessionId={liveSession} kind="agent" />
		{/if}
	</div>
</div>

<style>
	.pane {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.body {
		flex: 1;
		display: flex;
		min-height: 0;
	}
	.term {
		flex: 1;
		min-width: 0;
		min-height: 0;
	}
</style>
