<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import ProjectTree from '$lib/ProjectTree.svelte';
	import PaneManager from '$lib/PaneManager.svelte';
	import Settings from '$lib/Settings.svelte';
	import UpdateBanner from '$lib/UpdateBanner.svelte';
	import { showSettings, openTabs, activeTabKey, closeTab } from '$lib/stores';
	import { onStoreError } from '$lib/ipc';
	import { checkForUpdate } from '$lib/updater';
	import { get } from 'svelte/store';

	const MIN_W = 200;
	const MAX_W = 520;

	let sidebarWidth = $state(280);
	let collapsed = $state(false);
	let resizing = $state(false);
	let errorMsg = $state('');
	let unlistenError: (() => void) | undefined;

	// Restore persisted sidebar layout.
	onMount(async () => {
		const w = Number(localStorage.getItem('cm.sidebarWidth'));
		if (w >= MIN_W && w <= MAX_W) sidebarWidth = w;
		collapsed = localStorage.getItem('cm.sidebarCollapsed') === '1';
		window.addEventListener('keydown', onKey);
		unlistenError = await onStoreError((m) => (errorMsg = m));
		// Check GitHub for a newer release; silent if offline / endpoint unset.
		checkForUpdate({ silent: true });
	});
	onDestroy(() => {
		window.removeEventListener('keydown', onKey);
		unlistenError?.();
		stopResize();
	});

	function toggleSidebar() {
		collapsed = !collapsed;
		localStorage.setItem('cm.sidebarCollapsed', collapsed ? '1' : '0');
	}

	function onResizeMove(e: MouseEvent) {
		sidebarWidth = Math.min(MAX_W, Math.max(MIN_W, e.clientX));
	}
	function stopResize() {
		if (!resizing) return;
		resizing = false;
		localStorage.setItem('cm.sidebarWidth', String(Math.round(sidebarWidth)));
		window.removeEventListener('mousemove', onResizeMove);
		window.removeEventListener('mouseup', stopResize);
	}
	function startResize(e: MouseEvent) {
		e.preventDefault();
		resizing = true;
		window.addEventListener('mousemove', onResizeMove);
		window.addEventListener('mouseup', stopResize);
	}

	// Global keyboard shortcuts.
	function onKey(e: KeyboardEvent) {
		const mod = e.metaKey || e.ctrlKey;
		if (e.key === 'Escape' && get(showSettings)) {
			showSettings.set(false);
			return;
		}
		if (!mod) return;
		if (e.key === 'b') {
			e.preventDefault();
			toggleSidebar();
		} else if (e.key === 'w') {
			const active = get(activeTabKey);
			if (active) {
				e.preventDefault();
				closeTab(active);
			}
		} else if (/^[1-9]$/.test(e.key)) {
			const tabs = get(openTabs);
			const idx = Number(e.key) - 1;
			if (tabs[idx]) {
				e.preventDefault();
				activeTabKey.set(tabs[idx].key);
			}
		}
	}
</script>

<div class="app">
	<UpdateBanner />
	<div class="titlebar" data-tauri-drag-region>
		<button
			class="collapse"
			title={collapsed ? 'Show sidebar (⌘B)' : 'Hide sidebar (⌘B)'}
			onclick={toggleSidebar}>{collapsed ? '⇥' : '⇤'}</button>
	</div>
	<div class="workspace">
		{#if !collapsed}
			<aside class="sidebar" style="width: {sidebarWidth}px">
				<div class="sidebar-header">
					<span>Projects</span>
					<button class="gear" title="Settings" onclick={() => showSettings.set(true)}>⚙</button>
				</div>
				<ProjectTree />
			</aside>
			<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
			<div
				class="resizer"
				class:active={resizing}
				role="separator"
				aria-orientation="vertical"
				aria-label="Resize sidebar"
				tabindex="-1"
				onmousedown={startResize}></div>
		{/if}
		<main class="main">
			<PaneManager />
		</main>
	</div>
	{#if resizing}<div class="resize-overlay"></div>{/if}
	{#if errorMsg}
		<div class="error-banner" role="alert">
			<span>{errorMsg}</span>
			<button onclick={() => (errorMsg = '')} title="Dismiss">×</button>
		</div>
	{/if}
</div>

{#if $showSettings}
	<Settings />
{/if}

<style>
	:global(:root) {
		--bg: #1e1e1e;
		--bg-sidebar: #181818;
		--bg-elevated: #232323;
		--bg-input: #161616;
		--bg-hover: #222;
		--surface: #1c1c1c;
		--surface-head: #202020;
		--border: #2c2c2c;
		--border-strong: #3a3a3a;
		--text: #e6e6e6;
		--text-dim: #9a9a9a;
		--text-muted: #6a6a6a;
		--accent: #2a4a78;
		--accent-border: #3a5a88;
		--accent-line: #4a78c8;
		--accent-text: #7fa3df;
		--accent-soft: #2f3a4a;
		--danger: #cf9a9a;
		--danger-bg: #5a2a2a;
		--ok: #9bbf8a;
		--radius: 6px;
		--radius-lg: 8px;
		--titlebar-h: 30px;
		--traffic-pad: 78px;
	}
	:global(html, body) {
		margin: 0;
		height: 100%;
		background: var(--bg);
		color: var(--text);
		font-family: ui-sans-serif, system-ui, sans-serif;
	}
	:global(body > div) {
		height: 100%;
	}

	.app {
		display: flex;
		flex-direction: column;
		height: 100vh;
		width: 100vw;
		overflow: hidden;
		position: relative;
	}

	/* Draggable strip that hosts the macOS traffic lights (titleBarStyle: Overlay). */
	.titlebar {
		flex: 0 0 auto;
		height: var(--titlebar-h);
		display: flex;
		align-items: center;
		justify-content: flex-end;
		padding-left: var(--traffic-pad);
		padding-right: 8px;
		background: var(--bg-sidebar);
		border-bottom: 1px solid var(--border);
	}
	.collapse {
		background: none;
		border: none;
		color: var(--text-dim);
		font-size: 15px;
		line-height: 1;
		cursor: pointer;
		padding: 2px 6px;
		border-radius: 4px;
	}
	.collapse:hover {
		color: #fff;
		background: var(--bg-elevated);
	}

	.workspace {
		flex: 1 1 auto;
		display: flex;
		min-height: 0;
	}

	.sidebar {
		flex: 0 0 auto;
		background: var(--bg-sidebar);
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
		color: var(--text-dim);
		border-bottom: 1px solid var(--border);
	}
	.gear {
		background: none;
		border: none;
		color: var(--text-dim);
		font-size: 18px;
		cursor: pointer;
		padding: 0 4px;
	}
	.gear:hover {
		color: #fff;
	}

	.resizer {
		flex: 0 0 5px;
		margin-left: -2px;
		cursor: col-resize;
		background: var(--border);
		transition: background 0.12s;
	}
	.resizer:hover,
	.resizer.active {
		background: var(--accent-line);
	}
	.resize-overlay {
		position: absolute;
		inset: 0;
		z-index: 60;
		cursor: col-resize;
	}

	.main {
		flex: 1 1 auto;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}

	.error-banner {
		position: absolute;
		bottom: 12px;
		left: 50%;
		transform: translateX(-50%);
		z-index: 120;
		display: flex;
		align-items: center;
		gap: 12px;
		max-width: 80vw;
		background: #5a2a2a;
		border: 1px solid #7a3a3a;
		color: #fff;
		padding: 8px 10px 8px 14px;
		border-radius: var(--radius-lg);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
		font-size: 13px;
	}
	.error-banner button {
		background: none;
		border: none;
		color: #fff;
		font-size: 16px;
		line-height: 1;
		cursor: pointer;
		padding: 0 4px;
	}
</style>
