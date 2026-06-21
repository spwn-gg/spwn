<script lang="ts">
	import Terminal from './Terminal.svelte';
	import ClaudePane from './ClaudePane.svelte';
	import ContextComposer from './ContextComposer.svelte';
	import { openTabs, activeTabKey, closeTab } from './stores';

	function close(key: string, e: Event) {
		e.stopPropagation();
		closeTab(key);
	}
</script>

<div class="panes">
	<div class="tabbar">
		{#each $openTabs as tab (tab.key)}
			<button
				class="tab"
				class:active={tab.key === $activeTabKey}
				onclick={() => activeTabKey.set(tab.key)}
				title={tab.title}>
				<span class="tab-icon">{tab.kind === 'claude' ? '✦' : tab.kind === 'context' ? '▦' : '$'}</span>
				<span class="tab-title">{tab.title}</span>
				<span
					class="tab-close"
					role="button"
					tabindex="0"
					onclick={(e) => close(tab.key, e)}
					onkeydown={(e) => e.key === 'Enter' && close(tab.key, e)}>×</span>
			</button>
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
		background: #232323;
		border-bottom: 1px solid #2c2c2c;
		overflow-x: auto;
		min-height: 34px;
	}
	.tab {
		display: flex;
		align-items: center;
		gap: 6px;
		max-width: 220px;
		padding: 7px 10px;
		background: #2a2a2a;
		border: none;
		border-right: 1px solid #1c1c1c;
		color: #b8b8b8;
		cursor: pointer;
		font-size: 12px;
	}
	.tab.active {
		background: #1e1e1e;
		color: #fff;
	}
	.tab-icon {
		color: #7fa3df;
		font-size: 15px;
	}
	.tab-close {
		font-size: 15px;
		line-height: 1;
	}
	.tab-title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.tab-close {
		color: #888;
		border-radius: 3px;
		padding: 0 4px;
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
		color: #6a6a6a;
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
