<script lang="ts">
	import ProjectTree from '$lib/ProjectTree.svelte';
	import PaneManager from '$lib/PaneManager.svelte';
	import Settings from '$lib/Settings.svelte';
	import { showSettings } from '$lib/stores';
</script>

<div class="app">
	<aside class="sidebar">
		<div class="sidebar-header">
			<span>Projects</span>
			<button class="gear" title="Settings" onclick={() => showSettings.set(true)}>⚙</button>
		</div>
		<ProjectTree />
	</aside>
	<main class="main">
		<PaneManager />
	</main>
</div>

{#if $showSettings}
	<Settings />
{/if}

<style>
	:global(html, body) {
		margin: 0;
		height: 100%;
		background: #1e1e1e;
		color: #e6e6e6;
		font-family: ui-sans-serif, system-ui, sans-serif;
	}
	:global(body > div) {
		height: 100%;
	}

	.app {
		display: flex;
		height: 100vh;
		width: 100vw;
		overflow: hidden;
	}

	.sidebar {
		flex: 0 0 280px;
		background: #181818;
		border-right: 1px solid #2c2c2c;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}
	.sidebar-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px 10px 8px 14px;
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: #9a9a9a;
		border-bottom: 1px solid #2c2c2c;
	}
	.gear {
		background: none;
		border: none;
		color: #9a9a9a;
		font-size: 18px;
		cursor: pointer;
		padding: 0 4px;
	}
	.gear:hover {
		color: #fff;
	}

	.main {
		flex: 1 1 auto;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
</style>
