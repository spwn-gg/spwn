<script lang="ts">
	// One-time explainer shown the first time a session spawns its own worktree, so a
	// user learns the isolation model up front (not at the scary delete dialog). Gated
	// by a localStorage flag — shows exactly once per machine.
	import { projects } from './stores';

	let {
		projectId,
		terminalId,
		onClose
	}: {
		projectId: string;
		terminalId: string | undefined;
		onClose: () => void;
	} = $props();

	const term = $derived(
		$projects.find((p) => p.id === projectId)?.terminals.find((t) => t.id === terminalId)
	);
	const base = $derived(term?.baseBranch ?? 'your current branch');
</script>

<div class="card" role="dialog" aria-label="How sessions are isolated">
	<div class="head">
		<span class="badge">✦ New session</span>
		<span class="ttl">This session got its own isolated copy of your code</span>
	</div>
	<div class="body">
		<p>
			spwn created a separate <strong>git worktree</strong> for this session
			{#if term?.cwd}<span class="mono">{term.cwd}</span>{/if}
			on a new branch
			{#if term?.branch}<span class="mono">{term.branch}</span>{/if}
			forked from <span class="mono">{base}</span>, and pre-cloned build dirs so it can run
			right away. <strong>Your main checkout is untouched.</strong>
		</p>
		<ul>
			<li><strong>Merge</strong> brings this session's committed work back into <span class="mono">{base}</span>.</li>
			<li><strong>Delete</strong> throws the whole copy away — spwn always names any unmerged work first.</li>
			<li>Switching between sessions is free: each has its own files, so nothing is copied in or out.</li>
		</ul>
	</div>
	<div class="foot">
		<button class="ok" onclick={onClose}>Got it</button>
	</div>
</div>

<style>
	.card {
		position: absolute;
		left: 50%;
		bottom: 16px;
		transform: translateX(-50%);
		z-index: 90;
		width: min(560px, 88%);
		background: var(--surface);
		border: 1px solid var(--accent-border);
		border-radius: var(--radius-lg);
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
		overflow: hidden;
	}
	.head {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px 14px;
		background: var(--surface-head);
		border-bottom: 1px solid var(--border);
	}
	.badge {
		flex: 0 0 auto;
		font-size: 11px;
		font-weight: 600;
		color: var(--accent-text);
	}
	.ttl {
		font-size: 13px;
		font-weight: 600;
		color: var(--text);
	}
	.body {
		padding: 12px 14px;
		font-size: 12.5px;
		line-height: 1.55;
		color: var(--text-dim);
	}
	.body p {
		margin: 0 0 8px;
	}
	.body ul {
		margin: 0;
		padding-left: 18px;
	}
	.body li {
		margin: 3px 0;
	}
	.body strong {
		color: var(--text);
	}
	.mono {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11.5px;
		color: #9bb0c8;
		word-break: break-all;
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		padding: 10px 14px;
		border-top: 1px solid var(--border);
	}
	.ok {
		background: var(--accent);
		border: 1px solid var(--accent-border);
		color: #fff;
		border-radius: var(--radius);
		padding: 6px 16px;
		font-size: 13px;
		cursor: pointer;
	}
	.ok:hover {
		filter: brightness(1.15);
	}
</style>
