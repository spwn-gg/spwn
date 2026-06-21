<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { readTranscript, onProjectsChanged, writeToPty, addContextBlock } from './ipc';
	import { openTab, refreshProjects } from './stores';
	import type { Turn } from './types';
	import { marked } from 'marked';
	import DOMPurify from 'dompurify';
	import { openUrl } from '@tauri-apps/plugin-opener';

	function renderMarkdown(text: string): string {
		const html = marked.parse(text, { async: false, gfm: true, breaks: true }) as string;
		return DOMPurify.sanitize(html);
	}

	// Open links externally instead of navigating the app's webview.
	function onBodyClick(e: MouseEvent) {
		const a = (e.target as HTMLElement)?.closest?.('a');
		const href = a?.getAttribute('href');
		if (href && /^https?:\/\//.test(href)) {
			e.preventDefault();
			openUrl(href).catch(() => {});
		}
	}

	let {
		projectId,
		terminalId = undefined,
		sessionId = undefined
	}: { projectId: string; terminalId?: string; sessionId?: string } = $props();

	let turns = $state<Turn[]>([]);
	let loadingId: string | null = null;
	let status = $state('');

	async function reload() {
		const sid = sessionId ?? null;
		loadingId = sid;
		if (!sid) {
			turns = [];
			return;
		}
		const loaded = await readTranscript(sid);
		if (loadingId === sid) turns = loaded;
	}

	$effect(() => {
		void sessionId;
		reload();
	});

	// Map tool_use id -> its result, so results nest under their call.
	const resultsById = $derived.by(() => {
		const m = new Map<string, { text: string; isError: boolean }>();
		for (const t of turns)
			for (const b of t.blocks)
				if (b.kind === 'toolResult' && b.id)
					m.set(b.id, { text: b.text ?? '', isError: !!b.isError });
		return m;
	});

	// Which block categories are shown.
	let filters = $state({ text: true, thinking: true, toolCalls: true, toolResults: true });

	const counts = $derived.by(() => {
		const c = { text: 0, thinking: 0, toolCalls: 0, toolResults: 0 };
		for (const t of turns)
			for (const b of t.blocks) {
				if (b.kind === 'text') c.text++;
				else if (b.kind === 'thinking') c.thinking++;
				else if (b.kind === 'toolUse') c.toolCalls++;
				else if (b.kind === 'toolResult') c.toolResults++;
			}
		return c;
	});

	function blockVisible(b: { kind: string }): boolean {
		if (b.kind === 'text') return filters.text;
		if (b.kind === 'thinking') return filters.thinking;
		if (b.kind === 'toolUse') return filters.toolCalls;
		if (b.kind === 'toolResult') return filters.toolResults;
		return true;
	}

	let unlisten: (() => void) | undefined;
	onMount(async () => {
		unlisten = await onProjectsChanged(() => reload());
	});
	onDestroy(() => unlisten?.());

	function fork() {
		if (!sessionId) return;
		openTab({
			projectId,
			kind: 'claude',
			title: 'fork',
			claudeFork: sessionId,
			parentTerminalId: terminalId
		});
		status = 'Forked — opening the new session…';
	}

	function rewind() {
		if (!terminalId) return;
		// Open Claude's native /rewind picker in the live terminal; pick there.
		writeToPty(terminalId, '/rewind\r');
		status = 'Opened /rewind in the terminal — choose a checkpoint there.';
	}

	async function addToContext(t: Turn) {
		const text = t.blocks
			.filter((b) => b.kind === 'text')
			.map((b) => b.text ?? '')
			.join('\n')
			.trim();
		if (!text) return;
		await addContextBlock(projectId, 'session', t.role, text);
		await refreshProjects();
		status = 'Added to this project’s context.';
	}
</script>

<div class="mirror">
	<div class="bar">
		<span class="title">Conversation</span>
		<button class="act" disabled={!sessionId} onclick={fork} title="Fork this whole session">⑂ Fork</button>
		<button class="act" disabled={!terminalId} onclick={rewind} title="Open /rewind in the terminal">↺ Rewind</button>
	</div>
	<div class="filters">
		<button class="chip" class:off={!filters.text} onclick={() => (filters.text = !filters.text)}>
			Text <span class="n">{counts.text}</span>
		</button>
		<button class="chip" class:off={!filters.thinking} onclick={() => (filters.thinking = !filters.thinking)}>
			Thinking <span class="n">{counts.thinking}</span>
		</button>
		<button class="chip" class:off={!filters.toolCalls} onclick={() => (filters.toolCalls = !filters.toolCalls)}>
			Tool calls <span class="n">{counts.toolCalls}</span>
		</button>
		<button class="chip" class:off={!filters.toolResults} onclick={() => (filters.toolResults = !filters.toolResults)}>
			Tool results <span class="n">{counts.toolResults}</span>
		</button>
	</div>
	{#if status}<div class="status">{status}</div>{/if}
	<div class="body" role="presentation" onclick={onBodyClick}>
		{#if !sessionId}
			<div class="hint">Waiting for the Claude session to start…</div>
		{:else if turns.length === 0}
			<div class="hint">No messages yet.</div>
		{:else}
			{#each turns as t (t.uuid)}
				{@const toolOnly = t.blocks.length > 0 && t.blocks.every((b) => b.kind === 'toolResult')}
				{@const visible = t.blocks.filter(blockVisible)}
				{#if !toolOnly && visible.length > 0}
					<div class="turn {t.role}">
						<div class="who">
							<span>{t.role}</span>
							<button class="addctx" title="Add to project context" onclick={() => addToContext(t)}>＋ ctx</button>
						</div>
						{#each t.blocks as b}
							{#if b.kind === 'text' && filters.text}
								<div class="text md">{@html renderMarkdown(b.text ?? '')}</div>
							{:else if b.kind === 'thinking' && filters.thinking}
								<details class="thinking"><summary>thinking</summary><div class="pre">{b.text}</div></details>
							{:else if b.kind === 'toolUse' && filters.toolCalls}
								{@const res = b.id ? resultsById.get(b.id) : undefined}
								<details class="toolcall">
									<summary>▸ {b.name} <span class="dim">{b.text}</span></summary>
									{#if res && filters.toolResults}
										<div class="tool result" class:err={res.isError}>{res.text}</div>
									{/if}
								</details>
							{:else if b.kind === 'toolResult' && filters.toolResults}
								<div class="tool result" class:err={b.isError}>⮑ <span class="dim">{b.text}</span></div>
							{/if}
						{/each}
					</div>
				{/if}
			{/each}
		{/if}
	</div>
</div>

<style>
	.mirror {
		display: flex;
		flex-direction: column;
		height: 100%;
		background: #161616;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 12px;
		border-bottom: 1px solid #2c2c2c;
	}
	.title {
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: #9a9a9a;
		flex: 1 1 auto;
	}
	.act {
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 5px;
		padding: 3px 9px;
		font-size: 13px;
		cursor: pointer;
	}
	.act:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.filters {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		padding: 8px 12px;
		border-bottom: 1px solid #2c2c2c;
	}
	.chip {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		background: #243044;
		border: 1px solid #34507a;
		color: #cfe0f5;
		border-radius: 12px;
		padding: 2px 10px;
		font-size: 11px;
		cursor: pointer;
	}
	.chip .n {
		color: #8aa0bf;
		font-size: 10px;
	}
	.chip.off {
		background: #232323;
		border-color: #3a3a3a;
		color: #777;
	}
	.chip.off .n {
		color: #555;
	}
	.status {
		padding: 6px 12px;
		font-size: 11px;
		color: #c89a4a;
		border-bottom: 1px solid #2c2c2c;
	}
	.body {
		flex: 1 1 auto;
		overflow-y: auto;
		padding: 10px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.hint {
		color: #6a6a6a;
		font-size: 13px;
		padding: 8px;
	}
	.who {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 11px;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: #7fa3df;
		margin-bottom: 3px;
	}
	.addctx {
		background: none;
		border: 1px solid #3a3a3a;
		color: #888;
		border-radius: 4px;
		font-size: 10px;
		padding: 1px 6px;
		cursor: pointer;
		text-transform: none;
		letter-spacing: 0;
	}
	.addctx:hover {
		color: #fff;
		background: #2a3344;
	}
	.turn.user {
		align-self: flex-end;
		max-width: 85%;
		background: #20262f;
		padding: 8px 12px;
		border-radius: 10px;
	}
	.turn.user .who {
		color: #9a9a9a;
	}
	.turn.assistant {
		max-width: 95%;
	}
	.turn.toolturn {
		max-width: 100%;
	}
	.text {
		white-space: pre-wrap;
		word-break: break-word;
		color: #e6e6e6;
		font-size: 14px;
		line-height: 1.5;
	}
	.text.md {
		white-space: normal;
	}
	.text.md :global(p) {
		margin: 0 0 8px;
	}
	.text.md :global(h1),
	.text.md :global(h2),
	.text.md :global(h3),
	.text.md :global(h4) {
		margin: 10px 0 6px;
		line-height: 1.3;
	}
	.text.md :global(ul),
	.text.md :global(ol) {
		margin: 6px 0;
		padding-left: 22px;
	}
	.text.md :global(li) {
		margin: 2px 0;
	}
	.text.md :global(code) {
		background: #0d0d0d;
		padding: 1px 4px;
		border-radius: 4px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
	}
	.text.md :global(pre) {
		background: #0d0d0d;
		padding: 10px;
		border-radius: 6px;
		overflow: auto;
		margin: 8px 0;
	}
	.text.md :global(pre code) {
		background: none;
		padding: 0;
	}
	.text.md :global(a) {
		color: #7fa3df;
	}
	.text.md :global(blockquote) {
		border-left: 3px solid #3a3a3a;
		margin: 8px 0;
		padding-left: 10px;
		color: #aaa;
	}
	.text.md :global(table) {
		border-collapse: collapse;
		margin: 8px 0;
	}
	.text.md :global(th),
	.text.md :global(td) {
		border: 1px solid #3a3a3a;
		padding: 3px 8px;
	}
	.text.md :global(> :first-child) {
		margin-top: 0;
	}
	.text.md :global(> :last-child) {
		margin-bottom: 0;
	}
	.thinking summary {
		cursor: pointer;
		color: #8a7fb0;
		font-size: 11px;
	}
	.thinking .pre {
		white-space: pre-wrap;
		word-break: break-word;
		color: #a99fc8;
		font-size: 12px;
		margin-top: 4px;
	}
	.tool {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11px;
		color: #9bbf8a;
		white-space: pre-wrap;
		word-break: break-word;
		margin-top: 3px;
	}
	.tool.result {
		color: #8aa0bf;
	}
	.tool.result.err {
		color: #cf7a7a;
	}
	.toolcall {
		margin-top: 3px;
	}
	.toolcall summary {
		cursor: pointer;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11px;
		color: #9bbf8a;
		white-space: pre-wrap;
		word-break: break-word;
	}
	.toolcall .tool.result {
		margin-top: 4px;
		max-height: 280px;
		overflow: auto;
	}
	.dim {
		color: #888;
	}
</style>
