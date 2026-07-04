<script lang="ts">
	import Terminal from './Terminal.svelte';
	import ClaudePane from './ClaudePane.svelte';
	import ContextComposer from './ContextComposer.svelte';
	import ScheduledTasks from './ScheduledTasks.svelte';
	import { openTabs, activeTabKey, closeTab } from './stores';

	function close(key: string, e: Event) {
		e.stopPropagation();
		closeTab(key);
	}
</script>

<div class="panes">
	<div class="tabbar" data-tauri-drag-region>
		{#each $openTabs as tab (tab.key)}
			<div class="tab" class:active={tab.key === $activeTabKey} class:attn={tab.needsAttention}>
				<button
					class="tab-main"
					onclick={() => activeTabKey.set(tab.key)}
					title={tab.projectName ? `${tab.title} — ${tab.projectName}` : tab.title}>
					{#if tab.needsAttention}<span class="attn-dot" title="Needs attention"></span>{/if}
					<span class="tab-icon">{tab.kind === 'claude' ? '✦' : tab.kind === 'context' ? '▦' : tab.kind === 'schedule' ? '◷' : '$'}</span>
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
			<div class="empty">Pick a project in the sidebar, then open a Shell or Claude terminal.</div>
		{/if}
		{#each $openTabs as tab (tab.key)}
			<div class="pane" class:active={tab.key === $activeTabKey}>
				{#if tab.kind === 'context'}
					<ContextComposer projectId={tab.projectId} />
				{:else if tab.kind === 'schedule'}
					<ScheduledTasks projectId={tab.projectId} />
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
	.attn-dot {
		flex: 0 0 auto;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: #e0a83a;
		box-shadow: 0 0 0 2px rgba(224, 168, 58, 0.25);
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
