<script lang="ts">
	// P2.1 — the per-project exploration map. The fork forest as a canvas: each session
	// is a node, fork edges show where explorations diverged, and every node is annotated
	// with its live status, how far ahead of base it is, and a one-line "conclusion" (its
	// last assistant response). This makes spwn's signature loop — explore many, keep the
	// best — something you can SEE, not just an architecture you benefit from unknowingly.
	import { onMount, onDestroy } from 'svelte';
	import { claudeForest, placeForest } from './forest';
	import {
		projects,
		openTab,
		setClaudeStatus,
		claudeStatus,
		refreshProjects
	} from './stores';
	import {
		sessionMergeStatus,
		readTranscript,
		onProjectsChanged,
		clearTerminalAttention
	} from './ipc';
	import { GLYPHS, ACTIONS } from './labels';
	import BringWorkBack from './BringWorkBack.svelte';
	import type { MergeStatus, TerminalRec, Turn } from './types';

	let { projectId }: { projectId: string } = $props();

	const project = $derived($projects.find((p) => p.id === projectId) ?? null);
	const sessions = $derived((project?.terminals ?? []).filter((t) => t.kind === 'claude'));
	const roots = $derived(project ? claudeForest(project) : []);
	const placed = $derived(placeForest(roots));

	// Canvas geometry — lineage depth on X, one row per session on Y.
	const COL = 260;
	const ROW = 116;
	const CARD_W = 216;
	const CARD_H = 96;
	const PAD = 28;

	const posById = $derived.by(() => {
		const m = new Map<string, { x: number; y: number }>();
		for (const pl of placed) m.set(pl.node.t.id, { x: PAD + pl.depth * COL, y: PAD + pl.row * ROW });
		return m;
	});

	// Cubic-bezier fork edges from a parent's right edge to each child's left edge.
	const edges = $derived.by(() => {
		const es: { d: string }[] = [];
		for (const pl of placed) {
			const from = posById.get(pl.node.t.id);
			if (!from) continue;
			for (const c of pl.node.children) {
				const to = posById.get(c.t.id);
				if (!to) continue;
				const x1 = from.x + CARD_W;
				const y1 = from.y + CARD_H / 2;
				const x2 = to.x;
				const y2 = to.y + CARD_H / 2;
				const mx = (x1 + x2) / 2;
				es.push({ d: `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}` });
			}
		}
		return es;
	});

	const canvasW = $derived(
		placed.length ? Math.max(...placed.map((p) => p.depth)) * COL + CARD_W + PAD * 2 : 0
	);
	const canvasH = $derived(placed.length ? PAD * 2 + placed.length * ROW : 0);

	// Per-node annotations loaded lazily (git state + last-response conclusion).
	let mergeById = $state<Record<string, MergeStatus>>({});
	let concById = $state<Record<string, string>>({});
	let unlisten: (() => void) | undefined;

	function enrich() {
		for (const t of sessions) {
			sessionMergeStatus(projectId, t.id)
				.then((s) => (mergeById = { ...mergeById, [t.id]: s }))
				.catch(() => {});
			if (t.sessionId)
				readTranscript(t.sessionId)
					.then((turns) => (concById = { ...concById, [t.id]: lastAssistantText(turns) }))
					.catch(() => {});
		}
	}

	onMount(async () => {
		enrich();
		unlisten = await onProjectsChanged(() => enrich());
	});
	onDestroy(() => unlisten?.());

	function lastAssistantText(turns: Turn[]): string {
		for (let i = turns.length - 1; i >= 0; i--) {
			const t = turns[i];
			if (t.role !== 'assistant') continue;
			const text = t.blocks
				.filter((b) => b.kind === 'text')
				.map((b) => b.text ?? '')
				.join(' ')
				.trim();
			if (text) return text;
		}
		return '';
	}

	function firstLine(s: string): string {
		const line = s.split('\n').map((l) => l.trim()).find(Boolean) ?? '';
		return line.length > 160 ? line.slice(0, 160) + '…' : line;
	}

	type NodeStatus = 'thinking' | 'blocked' | 'done' | 'error' | null;
	function statusFor(t: TerminalRec): NodeStatus {
		const live = $claudeStatus.get(t.id);
		if (live === 'thinking') return 'thinking';
		if (live === 'blockedPermission' || live === 'blockedQuestion') return 'blocked';
		if (live === 'done') return 'done';
		if (live === 'error') return 'error';
		if (t.needsAttention)
			return t.attentionReason === 'blocked'
				? 'blocked'
				: t.attentionReason === 'error'
					? 'error'
					: 'done';
		return null;
	}

	function open(t: TerminalRec) {
		// Mirror the sidebar: viewing a session clears its "needs you" state.
		if ($claudeStatus.get(t.id) !== 'thinking') setClaudeStatus(t.id, 'idle');
		if (t.needsAttention) clearTerminalAttention(t.id).then(() => refreshProjects());
		openTab({
			projectId,
			kind: 'claude',
			terminalId: t.id,
			title: t.title,
			projectName: project?.name,
			sessionId: t.sessionId ?? undefined
		});
	}

	let bringBack = $state<{ terminalId: string; sessionId?: string; title: string } | null>(null);
</script>

<div class="map">
	<div class="bar">
		<span class="title">{GLYPHS.map} Exploration Map — {project?.name ?? ''}</span>
		<span class="legend">
			Each node is a session · edges show forks · <span class="k">↑</span> = commits ahead of base
		</span>
	</div>

	{#if placed.length === 0}
		<div class="empty">No sessions yet. Start a session, fork it to explore alternatives, then come back here to compare and combine them.</div>
	{:else}
		<div class="scroll">
			<div class="canvas" style="width:{canvasW}px; height:{canvasH}px;">
				<svg class="edges" width={canvasW} height={canvasH} aria-hidden="true">
					{#each edges as e (e.d)}
						<path d={e.d} />
					{/each}
				</svg>
				{#each placed as pl (pl.node.t.id)}
					{@const t = pl.node.t}
					{@const pos = posById.get(t.id)}
					{@const st = statusFor(t)}
					{@const ms = mergeById[t.id]}
					{@const concl = concById[t.id]}
					<div
						class="node"
						class:root={pl.depth === 0}
						style="left:{pos?.x ?? 0}px; top:{pos?.y ?? 0}px; width:{CARD_W}px; height:{CARD_H}px;">
						<div class="node-head">
							{#if st === 'thinking'}<span class="spin" title="Working…"></span>
							{:else if st === 'blocked'}<span class="dot blocked" title="Waiting for you"></span>
							{:else if st === 'done'}<span class="dot done" title="Turn finished — awaiting you"></span>
							{:else if st === 'error'}<span class="dot error" title="Session error"></span>
							{:else}<span class="dot idle"></span>{/if}
							<span class="node-icon">{pl.depth > 0 ? GLYPHS.lineage : GLYPHS.session}</span>
							<span class="node-title" title={t.title}>{t.title}</span>
						</div>
						<div class="node-meta">
							{#if t.branch}<span class="chip" title="git branch: {t.branch}">{GLYPHS.branch} {t.branch.replace(/^cm\//, '')}</span>{/if}
							{#if ms && ms.ahead > 0}<span class="ahead" title="{ms.ahead} commit(s) ahead of base">{ms.ahead}↑</span>{/if}
							{#if ms?.uncommitted}<span class="uncommitted" title="uncommitted changes">●</span>{/if}
						</div>
						<div class="node-concl" title={concl ?? ''}>{concl ? firstLine(concl) : 'No response yet.'}</div>
						<div class="node-actions">
							<button onclick={() => open(t)}>Open</button>
							<button
								class="bring"
								title={ACTIONS.bringWorkBack}
								onclick={() => (bringBack = { terminalId: t.id, sessionId: t.sessionId ?? undefined, title: t.title })}
								>↩ Bring back</button>
						</div>
					</div>
				{/each}
			</div>
		</div>
	{/if}
</div>

{#if bringBack}
	<BringWorkBack
		{projectId}
		terminalId={bringBack.terminalId}
		sessionId={bringBack.sessionId}
		title={bringBack.title}
		onClose={() => (bringBack = null)} />
{/if}

<style>
	.map {
		display: flex;
		flex-direction: column;
		height: 100%;
		background: var(--bg);
	}
	.bar {
		display: flex;
		align-items: baseline;
		gap: 14px;
		padding: 10px 16px;
		border-bottom: 1px solid var(--border);
		flex: 0 0 auto;
	}
	.title {
		font-weight: 600;
		color: var(--text);
	}
	.legend {
		font-size: 11px;
		color: var(--text-muted);
	}
	.legend .k {
		color: var(--accent-text);
	}
	.empty {
		padding: 28px;
		color: var(--text-muted);
		max-width: 460px;
		line-height: 1.5;
	}
	.scroll {
		flex: 1 1 auto;
		overflow: auto;
		min-height: 0;
	}
	.canvas {
		position: relative;
	}
	.edges {
		position: absolute;
		inset: 0;
		pointer-events: none;
		overflow: visible;
	}
	.edges path {
		fill: none;
		stroke: var(--border-strong);
		stroke-width: 1.5;
	}
	.node {
		position: absolute;
		box-sizing: border-box;
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 8px 10px;
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		box-shadow: 0 2px 8px rgba(0, 0, 0, 0.25);
	}
	.node.root {
		border-color: var(--accent-border);
	}
	.node-head {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}
	.node-icon {
		color: var(--accent-text);
		flex: 0 0 auto;
		font-size: 13px;
	}
	.node-title {
		flex: 1 1 auto;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12px;
		font-weight: 600;
		color: var(--text);
	}
	.node-meta {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 10px;
	}
	.chip {
		font-family: ui-monospace, Menlo, monospace;
		color: var(--text-dim);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 120px;
	}
	.ahead {
		color: var(--ok);
		font-weight: 600;
	}
	.uncommitted {
		color: #e0a83a;
		font-size: 8px;
	}
	.node-concl {
		flex: 1 1 auto;
		overflow: hidden;
		font-size: 11px;
		line-height: 1.35;
		color: var(--text-dim);
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
	}
	.node-actions {
		display: flex;
		gap: 6px;
	}
	.node-actions button {
		flex: 0 0 auto;
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
		border-radius: 4px;
		padding: 2px 8px;
		font-size: 11px;
		cursor: pointer;
	}
	.node-actions button:hover {
		color: #fff;
		filter: brightness(1.2);
	}
	.node-actions .bring {
		color: var(--accent-text);
	}
	.dot {
		flex: 0 0 auto;
		width: 7px;
		height: 7px;
		border-radius: 50%;
	}
	.dot.idle {
		background: var(--text-muted);
		opacity: 0.5;
	}
	.dot.done {
		background: #e0a83a;
		box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.25);
	}
	.dot.blocked {
		background: #e0a83a;
		box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.25);
		animation: attn-pulse 1.4s ease-in-out infinite;
	}
	.dot.error {
		background: #e06c6c;
		box-shadow: 0 0 0 2px rgba(224, 108, 108, 0.25);
	}
	.spin {
		flex: 0 0 auto;
		width: 9px;
		height: 9px;
		border-radius: 50%;
		border: 2px solid rgba(240, 198, 116, 0.25);
		border-top-color: #e0a83a;
		animation: spin 0.7s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}
	@keyframes attn-pulse {
		0%,
		100% {
			opacity: 1;
		}
		50% {
			opacity: 0.5;
		}
	}
</style>
