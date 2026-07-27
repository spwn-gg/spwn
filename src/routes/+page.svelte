<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import ProjectTree from '$lib/ProjectTree.svelte';
	import PaneManager from '$lib/PaneManager.svelte';
	import Settings from '$lib/Settings.svelte';
	import UpdateBanner from '$lib/UpdateBanner.svelte';
	import QuestionPicker from '$lib/QuestionPicker.svelte';
	import ConfirmDialog from '$lib/ConfirmDialog.svelte';
	import {
		showSettings,
		openTabs,
		activeTabKey,
		activeTab,
		closeTab,
		setHookRunning,
		setClaudeStatus
	} from '$lib/stores';
	import {
		onStoreError,
		onHookRunning,
		onClaudeStatus,
		clearTerminalAttention,
		onHookPrompt,
		onHookPromptClose,
		hooksPromptAnswer
	} from '$lib/ipc';
	import type { SessionStatus, HookPromptEvent, PendingQuestion } from '$lib/types';
	import { checkForUpdate } from '$lib/updater';
	import { get } from 'svelte/store';

	const MIN_W = 200;
	const MAX_W = 520;

	let sidebarWidth = $state(280);
	let collapsed = $state(false);
	let resizing = $state(false);
	let errorMsg = $state('');
	let unlistenError: (() => void) | undefined;
	let unlistenHook: (() => void) | undefined;
	let unlistenStatus: (() => void) | undefined;
	let unlistenHookPrompt: (() => void) | undefined;
	let unlistenHookPromptClose: (() => void) | undefined;

	// Blocking multiple-choice prompts raised by running hooks, awaiting the user's pick.
	// Rendered globally (a hook can fire when no session pane is mounted).
	let hookPrompts = $state<HookPromptEvent[]>([]);

	// Wrap a hook prompt as the picker's single-question shape.
	function asPending(p: HookPromptEvent): PendingQuestion {
		return {
			id: p.id,
			questions: [
				{ question: p.question, header: p.header, multiSelect: p.multiSelect, options: p.options }
			]
		};
	}

	function answerHookPrompt(id: string, text: string) {
		hooksPromptAnswer(id, text).catch(() => {});
		hookPrompts = hookPrompts.filter((p) => p.id !== id);
	}

	// States that "need you" — suppressed for the session you're already looking at.
	const NEEDS_YOU: SessionStatus[] = ['done', 'blockedPermission', 'blockedQuestion', 'error'];

	// Restore persisted sidebar layout.
	onMount(async () => {
		const w = Number(localStorage.getItem('cm.sidebarWidth'));
		if (w >= MIN_W && w <= MAX_W) sidebarWidth = w;
		collapsed = localStorage.getItem('cm.sidebarCollapsed') === '1';
		window.addEventListener('keydown', onKey);
		unlistenError = await onStoreError((m) => (errorMsg = m));
		// Track hooks running across all sessions → drives tab / tree spinners.
		unlistenHook = await onHookRunning((e) => setHookRunning(e.terminalId, e.event));
		// A running hook can raise a blocking multiple-choice prompt; show it globally.
		unlistenHookPrompt = await onHookPrompt((e) => {
			hookPrompts = [...hookPrompts.filter((p) => p.id !== e.id), e];
		});
		unlistenHookPromptClose = await onHookPromptClose((e) => {
			hookPrompts = hookPrompts.filter((p) => p.id !== e.id);
		});
		// Track live Claude session status → drives sidebar/tab-bar spinners + attention.
		unlistenStatus = await onClaudeStatus((e) => {
			const active = get(activeTab);
			// If you're already looking at this session, don't nag — clear it (and the
			// persisted flag) instead of lighting a dot. A live "thinking" still shows.
			if (active?.terminalId === e.terminalId && NEEDS_YOU.includes(e.status)) {
				setClaudeStatus(e.terminalId, 'idle');
				clearTerminalAttention(e.terminalId).catch(() => {});
				return;
			}
			setClaudeStatus(e.terminalId, e.status);
		});
		// Check GitHub for a newer release; silent if offline / endpoint unset.
		checkForUpdate({ silent: true });
	});
	onDestroy(() => {
		window.removeEventListener('keydown', onKey);
		unlistenError?.();
		unlistenHook?.();
		unlistenStatus?.();
		unlistenHookPrompt?.();
		unlistenHookPromptClose?.();
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
	{#if hookPrompts.length}
		<div class="hook-prompts" role="dialog" aria-label="Hook prompt">
			{#each hookPrompts as p (p.id)}
				<div class="hook-prompt">
					<div class="hook-prompt-head">Hook · {p.event}</div>
					<QuestionPicker pending={asPending(p)} onAnswer={answerHookPrompt} raw />
				</div>
			{/each}
		</div>
	{/if}
</div>

{#if $showSettings}
	<Settings />
{/if}

<ConfirmDialog />

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

	.hook-prompts {
		position: absolute;
		bottom: 12px;
		left: 50%;
		transform: translateX(-50%);
		z-index: 130;
		display: flex;
		flex-direction: column;
		gap: 8px;
		width: min(520px, 80vw);
	}
	.hook-prompt {
		background: var(--surface);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
		overflow: hidden;
	}
	.hook-prompt-head {
		padding: 7px 12px;
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-dim);
		background: var(--surface-head);
		border-bottom: 1px solid var(--border);
	}
</style>
