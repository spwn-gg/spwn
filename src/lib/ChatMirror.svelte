<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import {
		readTranscript,
		onProjectsChanged,
		addContextBlock,
		claudeRewind,
		claudeRewindRestore,
		listCheckpoints
	} from './ipc';
	import MergePanel from './MergePanel.svelte';
	import { openTab, refreshProjects, projects, pasteToInput } from './stores';
	import CheckpointList from './CheckpointList.svelte';
	import type { Turn, QuestionSpec } from './types';
	import { marked } from 'marked';
	import DOMPurify from 'dompurify';
	import { openUrl } from '@tauri-apps/plugin-opener';

	function renderMarkdown(text: string): string {
		const html = marked.parse(text, { async: false, gfm: true, breaks: true }) as string;
		const clean = DOMPurify.sanitize(html);
		// Add a copy affordance to each fenced code block (handled via delegation).
		return clean.replaceAll('<pre>', '<pre><button class="copy-btn" type="button">Copy</button>');
	}

	// Handle in-message clicks: copy buttons, and external links.
	function onBodyClick(e: MouseEvent) {
		const target = e.target as HTMLElement;
		const copy = target?.closest?.('.copy-btn');
		if (copy) {
			e.preventDefault();
			const code = copy.closest('pre')?.querySelector('code')?.textContent ?? '';
			navigator.clipboard.writeText(code).then(() => {
				const btn = copy as HTMLButtonElement;
				btn.textContent = 'Copied';
				setTimeout(() => (btn.textContent = 'Copy'), 1200);
			});
			return;
		}
		const a = target?.closest?.('a');
		const href = a?.getAttribute('href');
		if (href && /^https?:\/\//.test(href)) {
			e.preventDefault();
			openUrl(href).catch(() => {});
		}
	}

	let {
		projectId,
		terminalId = undefined,
		sessionId = undefined,
		busy = false,
		streamingText = '',
		streamingThinking = '',
		liveTools = [],
		pendingUserText = null,
		onReload = () => {},
		onRewound = () => {}
	}: {
		projectId: string;
		terminalId?: string;
		sessionId?: string;
		busy?: boolean;
		streamingText?: string;
		streamingThinking?: string;
		liveTools?: { id: string; name: string }[];
		pendingUserText?: string | null;
		onReload?: (turns: Turn[]) => void;
		onRewound?: () => void;
	} = $props();

	const showOverlay = $derived(
		busy || streamingText.length > 0 || streamingThinking.length > 0 || liveTools.length > 0
	);

	let turns = $state<Turn[]>([]);
	// Optimistic rewind: truncate the view to the anchor until the next message
	// lands and the parser's active-path takes over.
	let rewindAnchor = $state<string | null>(null);
	let rewindBaseline: string | null = null;
	const visibleTurns = $derived.by(() => {
		if (!rewindAnchor) return turns;
		const i = turns.findIndex((t) => t.uuid === rewindAnchor);
		return i >= 0 ? turns.slice(0, i + 1) : turns;
	});
	let loadingId: string | null = null;
	let status = $state('');
	let statusTimer: ReturnType<typeof setTimeout> | undefined;
	let bodyEl: HTMLDivElement | undefined;
	let stick = true; // pinned to the bottom unless the user scrolls up

	// Transient status line that clears itself.
	function setStatus(msg: string) {
		status = msg;
		clearTimeout(statusTimer);
		statusTimer = setTimeout(() => (status = ''), 4000);
	}

	function onScroll() {
		if (!bodyEl) return;
		stick = bodyEl.scrollHeight - bodyEl.scrollTop - bodyEl.clientHeight < 40;
	}
	function scrollToBottom() {
		if (bodyEl) bodyEl.scrollTop = bodyEl.scrollHeight;
	}

	async function reload() {
		const sid = sessionId ?? null;
		loadingId = sid;
		if (!sid) {
			turns = [];
			return;
		}
		const loaded = await readTranscript(sid);
		if (loadingId === sid) {
			turns = loaded;
			onReload(loaded);
			// Once a new message lands past the rewind, the active-path parser is
			// authoritative — drop the optimistic truncation.
			const leaf = loaded.length ? loaded[loaded.length - 1].uuid : null;
			if (rewindAnchor && leaf !== rewindBaseline) rewindAnchor = null;
			loadCheckpoints();
		}
	}

	// Which turn uuids have a code checkpoint (enables the "+ restore files" option).
	let turnCheckpoints = $state<Set<string>>(new Set());
	// The rewind choice popover: anchored at a turn.
	let rewindMenu = $state<{ uuid: string; x: number; y: number } | null>(null);
	let showCheckpoints = $state(false);

	async function loadCheckpoints() {
		if (!sessionId) {
			turnCheckpoints = new Set();
			return;
		}
		const cps = await listCheckpoints(sessionId);
		turnCheckpoints = new Set(cps.filter((c) => c.kind === 'turn').map((c) => c.turnUuid));
	}

	function openRewindMenu(uuid: string, e: MouseEvent) {
		e.stopPropagation();
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		rewindMenu = { uuid, x: Math.min(r.left, window.innerWidth - 230), y: r.bottom + 2 };
	}

	function rewindTo(uuid: string, restore: boolean) {
		rewindMenu = null;
		if (!terminalId) return;
		if (restore && !confirm('Also restore the project files to this point?\n\nReverts working files and deletes files created since (git history kept; a safety snapshot is saved first).')) return;
		rewindBaseline = turns.length ? turns[turns.length - 1].uuid : null;
		rewindAnchor = uuid;
		const p = restore
			? claudeRewindRestore(terminalId, uuid, true)
			: claudeRewind(terminalId, uuid);
		p.catch((e) => setStatus(String(e)));
		setStatus(
			restore
				? 'Rewound + restored files to this point. A safety snapshot was saved.'
				: 'Rewound here — your next message continues from this point; later turns drop.'
		);
		onRewound();
	}

	$effect(() => {
		void sessionId;
		stick = true;
		reload();
		loadCheckpoints();
	});

	// Follow new turns / streaming text when pinned to the bottom (live mirror).
	$effect(() => {
		void turns;
		void streamingText;
		void streamingThinking;
		void pendingUserText;
		if (stick) requestAnimationFrame(scrollToBottom);
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

	// Parse an AskUserQuestion tool_use input into its questions (null if it isn't one).
	function parseQuestions(text?: string | null): QuestionSpec[] | null {
		if (!text) return null;
		try {
			const v = JSON.parse(text);
			return Array.isArray(v?.questions) && v.questions.length ? (v.questions as QuestionSpec[]) : null;
		} catch {
			return null;
		}
	}
	// An option was chosen if its label appears in the answer (our "→ label" form
	// and Claude's native «"Q"="label"» form both contain the label).
	function wasChosen(label: string, result?: string): boolean {
		return !!result && result.includes(label);
	}

	let unlisten: (() => void) | undefined;
	const closeRewindMenu = () => (rewindMenu = null);
	onMount(async () => {
		// Reload only when our own session's transcript changed (or when the set of
		// changed sessions is unknown), not on every filesystem event in the tree.
		unlisten = await onProjectsChanged((changed) => {
			if (!sessionId || changed.length === 0 || changed.includes(sessionId)) reload();
		});
		window.addEventListener('click', closeRewindMenu);
	});
	onDestroy(() => {
		unlisten?.();
		clearTimeout(statusTimer);
		window.removeEventListener('click', closeRewindMenu);
	});

	// If this is a forked (child) session, find its parent terminal so responses
	// can be pasted back into the parent's input.
	const parentTerm = $derived.by(() => {
		const terms = $projects.find((p) => p.id === projectId)?.terminals;
		const me = terms?.find((t) => t.id === terminalId);
		return me?.parentId ? terms?.find((t) => t.id === me.parentId) : undefined;
	});

	function turnText(t: Turn): string {
		return t.blocks
			.filter((b) => b.kind === 'text')
			.map((b) => b.text ?? '')
			.join('\n')
			.trim();
	}

	function pasteToParent(t: Turn) {
		const parent = parentTerm;
		const text = turnText(t);
		if (!parent || !text) return;
		// Open/focus the parent session, then drop the response into its input.
		openTab({
			projectId,
			kind: 'claude',
			terminalId: parent.id,
			sessionId: parent.sessionId ?? undefined,
			title: parent.title,
			projectName: $projects.find((p) => p.id === projectId)?.name
		});
		pasteToInput.set({ terminalId: parent.id, text });
		setStatus('Pasted into the parent session’s input.');
	}

	// This session's terminal record (for its worktree branch chip + merge panel).
	const term = $derived(
		$projects.find((p) => p.id === projectId)?.terminals.find((t) => t.id === terminalId)
	);
	let showMerge = $state(false);

	function fork() {
		if (!sessionId) return;
		openTab({
			projectId,
			kind: 'claude',
			title: 'fork',
			claudeFork: sessionId,
			parentTerminalId: terminalId,
			// Show the parent's history right away; rebinds to the fork's own
			// (history-carrying) session id after its first message.
			sessionId
		});
		setStatus('Forked — opening the new session…');
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
		setStatus('Added to this project’s context.');
	}
</script>

{#snippet askCard(questions: QuestionSpec[], resultText?: string)}
	<div class="askcard">
		{#each questions as q (q.question)}
			<div class="ask-q">
				<div class="ask-qhead">
					{#if q.header}<span class="ask-tag">{q.header}</span>{/if}
					<span class="ask-qtext">{q.question}</span>
				</div>
				<div class="ask-opts">
					{#each q.options as o (o.label)}
						<div class="ask-opt" class:chosen={wasChosen(o.label, resultText)}>
							<span class="ask-mark">{wasChosen(o.label, resultText) ? '◉' : '○'}</span>
							<span class="ask-olabel">{o.label}</span>
						</div>
					{/each}
				</div>
			</div>
		{/each}
	</div>
{/snippet}

<div class="mirror">
	<div class="bar">
		<span class="title">Conversation</span>
		<button class="act" class:on={showCheckpoints} disabled={!sessionId} onclick={() => (showCheckpoints = !showCheckpoints)} title="Code checkpoints — undo file changes">⟲ Checkpoints</button>
		{#if term?.branch}
			<button
				class="act"
				onclick={() => (showMerge = true)}
				title="Merge this session's branch ({term.branch}) into {term.baseBranch}">⤵ Merge</button>
		{/if}
		<button class="act" disabled={!sessionId} onclick={fork} title="Fork this whole session">⑂ Fork</button>
	</div>
	{#if showCheckpoints && sessionId}
		<CheckpointList {projectId} {sessionId} disabled={busy} onStatus={setStatus} />
	{/if}
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
	<div class="body" role="presentation" bind:this={bodyEl} onscroll={onScroll} onclick={onBodyClick}>
		{#if visibleTurns.length === 0 && !pendingUserText && !showOverlay}
			<div class="hint">{sessionId ? 'No messages yet.' : 'Send a message to start the conversation.'}</div>
		{/if}
		{#each visibleTurns as t (t.uuid)}
			{@const toolOnly = t.blocks.length > 0 && t.blocks.every((b) => b.kind === 'toolResult')}
			{@const visible = t.blocks.filter(blockVisible)}
			{#if !toolOnly && visible.length > 0}
				<div class="turn {t.role}">
					<div class="who">
						<span>{t.role}</span>
						{#if t.role === 'assistant'}
							<button class="rewind" title="Rewind to here" onclick={(e) => openRewindMenu(t.uuid, e)}>↺ rewind</button>
							{#if parentTerm}
								<button class="toparent" title="Paste this response into the parent session ({parentTerm.title})" onclick={() => pasteToParent(t)}>→ parent</button>
							{/if}
						{/if}
						<button class="addctx" title="Add to project context" onclick={() => addToContext(t)}>＋ ctx</button>
					</div>
					{#each t.blocks as b}
						{#if b.kind === 'text' && filters.text}
							<div class="text md">{@html renderMarkdown(b.text ?? '')}</div>
						{:else if b.kind === 'thinking' && filters.thinking}
							<details class="thinking"><summary>thinking</summary><div class="pre">{b.text}</div></details>
						{:else if b.kind === 'toolUse' && filters.toolCalls}
							{@const res = b.id ? resultsById.get(b.id) : undefined}
							{@const questions = b.name === 'AskUserQuestion' ? parseQuestions(b.text) : null}
							{#if questions}
								{@render askCard(questions, res?.text)}
							{:else}
								<details class="toolcall">
									<summary>▸ {b.name} <span class="dim">{b.text}</span></summary>
									{#if res && filters.toolResults}
										<div class="tool result" class:err={res.isError}>{res.text}</div>
									{/if}
								</details>
							{/if}
						{:else if b.kind === 'toolResult' && filters.toolResults}
							<div class="tool result" class:err={b.isError}>⮑ <span class="dim">{b.text}</span></div>
						{/if}
					{/each}
				</div>
			{/if}
		{/each}

		<!-- Optimistic user bubble + live streaming assistant turn (overlay). -->
		{#if pendingUserText}
			<div class="turn user">
				<div class="who"><span>user</span></div>
				<div class="text md">{@html renderMarkdown(pendingUserText)}</div>
			</div>
		{/if}
		{#if showOverlay}
			<div class="turn assistant">
				<div class="who"><span>assistant</span><span class="streaming">streaming…</span></div>
				{#if streamingThinking && filters.thinking}
					<details class="thinking" open><summary>thinking</summary><div class="pre">{streamingThinking}</div></details>
				{/if}
				{#each liveTools as tu (tu.id)}
					<div class="tool">▸ {tu.name}</div>
				{/each}
				{#if streamingText}
					<div class="text md">{@html renderMarkdown(streamingText)}</div>
				{:else if !streamingThinking && liveTools.length === 0}
					<div class="dots">●●●</div>
				{/if}
			</div>
		{/if}
	</div>
</div>

{#if rewindMenu}
	<div class="rewind-menu" role="menu" tabindex="-1" style="left: {rewindMenu.x}px; top: {rewindMenu.y}px">
		<button onclick={() => rewindTo(rewindMenu!.uuid, false)}>Conversation only</button>
		<button disabled={!turnCheckpoints.has(rewindMenu.uuid)} onclick={() => rewindTo(rewindMenu!.uuid, true)}>
			Conversation + restore files
		</button>
	</div>
{/if}

{#if showMerge && term?.branch && terminalId}
	<MergePanel {projectId} terminalId={terminalId} onClose={() => (showMerge = false)} />
{/if}

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
	.streaming {
		font-weight: 400;
		text-transform: none;
		letter-spacing: 0;
		color: #8a7fb0;
		font-style: italic;
	}
	.dots {
		color: #6a6a6a;
		letter-spacing: 2px;
		animation: pulse 1.2s ease-in-out infinite;
	}
	.act.on {
		background: var(--bg);
		color: #d8b8f0;
		border-color: #5a4a7a;
	}
	.rewind-menu {
		position: fixed;
		z-index: 200;
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
		padding: 4px;
		min-width: 210px;
		display: flex;
		flex-direction: column;
	}
	.rewind-menu button {
		background: none;
		border: none;
		color: #d0d0d0;
		text-align: left;
		padding: 7px 10px;
		border-radius: 5px;
		cursor: pointer;
		font-size: 13px;
	}
	.rewind-menu button:hover:not(:disabled) {
		background: var(--accent-soft);
		color: #fff;
	}
	.rewind-menu button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	@keyframes pulse {
		0%,
		100% {
			opacity: 0.3;
		}
		50% {
			opacity: 1;
		}
	}
	.addctx,
	.rewind,
	.toparent {
		background: none;
		border: 1px solid #3a3a3a;
		color: #888;
		border-radius: 4px;
		font-size: 10px;
		padding: 1px 6px;
		cursor: pointer;
		text-transform: none;
		letter-spacing: 0;
		opacity: 0;
		transition: opacity 0.1s;
	}
	.turn:hover .addctx,
	.turn:hover .rewind,
	.turn:hover .toparent,
	.addctx:focus-visible,
	.rewind:focus-visible,
	.toparent:focus-visible {
		opacity: 1;
	}
	.toparent:hover {
		color: #fff;
		background: #1f3a2c;
		border-color: #2f6a4a;
	}
	.addctx:hover {
		color: #fff;
		background: #2a3344;
	}
	.rewind:hover {
		color: #fff;
		background: #3a2e4a;
		border-color: #5a4a7a;
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
		position: relative;
		background: #0d0d0d;
		padding: 10px;
		border-radius: 6px;
		overflow: auto;
		margin: 8px 0;
	}
	.text.md :global(pre .copy-btn) {
		position: absolute;
		top: 6px;
		right: 6px;
		background: #232323;
		border: 1px solid #3a3a3a;
		color: #b8b8b8;
		border-radius: 4px;
		padding: 2px 8px;
		font-size: 11px;
		font-family: ui-sans-serif, system-ui, sans-serif;
		cursor: pointer;
		opacity: 0;
		transition: opacity 0.12s;
	}
	.text.md :global(pre:hover .copy-btn) {
		opacity: 1;
	}
	.text.md :global(pre .copy-btn:hover) {
		color: #fff;
		background: #333;
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
	.askcard {
		margin: 4px 0;
		padding: 8px 10px;
		background: #1b2230;
		border: 1px solid #2f4366;
		border-radius: 8px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.ask-qhead {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 7px;
		margin-bottom: 5px;
	}
	.ask-tag {
		font-size: 9px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		padding: 1px 5px;
		border-radius: 4px;
		background: #2a3344;
		color: #9bbce0;
	}
	.ask-qtext {
		color: #e6e6e6;
		font-size: 13px;
		font-weight: 600;
	}
	.ask-opts {
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.ask-opt {
		display: flex;
		align-items: center;
		gap: 7px;
		font-size: 12px;
		color: #8a93a0;
		padding: 1px 0;
	}
	.ask-mark {
		font-size: 11px;
		color: #5a6472;
	}
	.ask-opt.chosen {
		color: #cfe0f5;
		font-weight: 600;
	}
	.ask-opt.chosen .ask-mark {
		color: var(--accent-text);
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
