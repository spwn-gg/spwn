<script lang="ts">
	import Terminal from './Terminal.svelte';
	import ClaudePane from './ClaudePane.svelte';
	import AgentPane from './AgentPane.svelte';
	import ContextComposer from './ContextComposer.svelte';
	import ScheduledTasks from './ScheduledTasks.svelte';
	import { openTabs, activeTabKey, closeTab, hookRunning, claudeStatus } from './stores';
	import type { OpenTab } from './stores';
	import { GLYPHS } from './labels';

	/** The icon glyph for a tab, kept 1:1 with the sidebar via labels.ts. */
	function tabIcon(kind: OpenTab['kind']): string {
		return kind === 'claude' || kind === 'agent'
			? GLYPHS.session
			: kind === 'context'
				? GLYPHS.mergeTray
				: kind === 'schedule'
					? GLYPHS.schedule
					: GLYPHS.shell;
	}

	function close(key: string, e: Event) {
		e.stopPropagation();
		closeTab(key);
	}

	/** Attention state for a tab, derived from the live status feed. Suppressed for the
	 * active tab (you're looking at it); a "thinking" spinner still shows via isBusy. */
	function tabAttn(tab: OpenTab, activeKey: string | null): 'blocked' | 'done' | 'error' | null {
		if (!tab.terminalId || tab.key === activeKey) return null;
		const s = $claudeStatus.get(tab.terminalId);
		if (s === 'blockedPermission' || s === 'blockedQuestion') return 'blocked';
		if (s === 'done') return 'done';
		if (s === 'error') return 'error';
		return null;
	}
	const isBusy = (tab: OpenTab) => tab.terminalId && $claudeStatus.get(tab.terminalId) === 'thinking';
</script>

<div class="panes">
	<div class="tabbar">
		{#each $openTabs as tab (tab.key)}
			{@const attn = tabAttn(tab, $activeTabKey)}
			<div class="tab" class:active={tab.key === $activeTabKey} class:attn={attn === 'blocked' || attn === 'done'} class:err={attn === 'error'}>
				<button
					class="tab-main"
					onclick={() => activeTabKey.set(tab.key)}
					title={tab.projectName ? `${tab.title} — ${tab.projectName}` : tab.title}>
					{#if attn === 'error'}<span class="attn-dot error" title="Session error"></span>
					{:else if attn === 'blocked'}<span class="attn-dot blocked" title="Waiting for you"></span>
					{:else if attn === 'done'}<span class="attn-dot" title="Turn finished — awaiting you"></span>
					{:else if isBusy(tab)}<span class="think-spin" title="Working…"></span>{/if}
					{#if tab.terminalId && $hookRunning.has(tab.terminalId)}<span class="hook-spin" title="Running {$hookRunning.get(tab.terminalId)} hook…"></span>{/if}
					<span class="tab-icon">{tabIcon(tab.kind)}</span>
					<span class="tab-title">{tab.title}</span>
					{#if tab.projectName && tab.kind !== 'context' && tab.kind !== 'schedule'}
						<span class="tab-proj">· {tab.projectName}</span>
					{/if}
				</button>
				<button class="tab-close" title="Close tab (⌘W)" onclick={(e) => close(tab.key, e)}>×</button>
			</div>
		{/each}
	</div>

	<div class="stack">
		{#if $openTabs.length === 0}
			<div class="empty">Pick a project in the sidebar, then start a Session or open a Shell.</div>
		{/if}
		{#each $openTabs as tab (tab.key)}
			<div class="pane" class:active={tab.key === $activeTabKey}>
				{#if tab.kind === 'context'}
					<ContextComposer projectId={tab.projectId} />
				{:else if tab.kind === 'schedule'}
					<ScheduledTasks projectId={tab.projectId} />
				{:else if tab.kind === 'agent'}
					<AgentPane
						tabKey={tab.key}
						projectId={tab.projectId}
						agent={tab.agent}
						terminalId={tab.terminalId}
						sessionId={tab.sessionId}
						claudeResume={tab.claudeResume}
						claudeFork={tab.claudeFork}
						parentTerminalId={tab.parentTerminalId}
						initialPrompt={tab.initialPrompt} />
				{:else if tab.kind === 'claude'}
					<ClaudePane
						tabKey={tab.key}
						projectId={tab.projectId}
						terminalId={tab.terminalId}
						sessionId={tab.sessionId}
						claudeResume={tab.claudeResume}
						claudeFork={tab.claudeFork}
						parentTerminalId={tab.parentTerminalId}
						initialPrompt={tab.initialPrompt} />
				{:else}
					<Terminal tabKey={tab.key} projectId={tab.projectId} kind="shell" terminalId={tab.terminalId} />
				{/if}
			</div>
		{/each}
	</div>
</div>

<style>
	.panes {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-width: 0;
	}
	.tabbar {
		display: flex;
		gap: 2px;
		background: var(--bg-elevated);
		border-bottom: 1px solid var(--border);
		overflow-x: auto;
		min-height: 34px;
	}
	.tab {
		display: flex;
		align-items: center;
		max-width: 240px;
		background: var(--bg-elevated);
		border-right: 1px solid #1c1c1c;
		color: #b8b8b8;
	}
	.tab.active {
		background: var(--bg);
		color: #fff;
	}
	.tab.attn:not(.active) {
		color: #f0c674;
	}
	.tab.err:not(.active) {
		color: #e06c6c;
	}
	.attn-dot {
		flex: 0 0 auto;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: #e0a83a;
		box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.25);
	}
	.attn-dot.blocked {
		animation: attn-pulse 1.4s ease-in-out infinite;
	}
	.attn-dot.error {
		background: #e06c6c;
		box-shadow: 0 0 0 2px rgba(224, 108, 108, 0.25);
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
	.think-spin {
		flex: 0 0 auto;
		width: 10px;
		height: 10px;
		border-radius: 50%;
		border: 2px solid rgba(240, 198, 116, 0.25);
		border-top-color: #e0a83a;
		animation: hook-spin 0.7s linear infinite;
	}
	.hook-spin {
		flex: 0 0 auto;
		width: 10px;
		height: 10px;
		border-radius: 50%;
		border: 2px solid rgba(127, 163, 223, 0.3);
		border-top-color: var(--accent-text);
		animation: hook-spin 0.7s linear infinite;
	}
	@keyframes hook-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.tab-main {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
		flex: 1 1 auto;
		background: none;
		border: none;
		color: inherit;
		cursor: pointer;
		font-size: 12px;
		padding: 7px 4px 7px 10px;
	}
	.tab-icon {
		color: var(--accent-text);
		font-size: 15px;
		flex: 0 0 auto;
	}
	.tab-title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.tab-proj {
		color: var(--text-muted);
		font-size: 11px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		flex: 0 1 auto;
	}
	.tab-close {
		background: none;
		border: none;
		font-size: 15px;
		line-height: 1;
		color: #888;
		border-radius: 3px;
		padding: 2px 6px;
		margin-right: 4px;
		cursor: pointer;
	}
	.tab-close:hover {
		color: #fff;
		background: #444;
	}
	.stack {
		position: relative;
		flex: 1 1 auto;
		min-height: 0;
	}
	.empty {
		padding: 20px;
		color: var(--text-muted);
	}
	.pane {
		position: absolute;
		inset: 0;
		visibility: hidden;
	}
	.pane.active {
		visibility: visible;
	}
</style>
