<script lang="ts">
	import { onDestroy } from 'svelte';
	import Terminal from './Terminal.svelte';
	import ChatMirror from './ChatMirror.svelte';

	let {
		tabKey,
		projectId,
		terminalId = undefined,
		sessionId = undefined,
		claudeResume = undefined,
		claudeFork = undefined,
		parentTerminalId = undefined,
		initialPrompt = undefined
	}: {
		tabKey: string;
		projectId: string;
		terminalId?: string;
		sessionId?: string;
		claudeResume?: string;
		claudeFork?: string;
		parentTerminalId?: string;
		initialPrompt?: string;
	} = $props();

	let container: HTMLDivElement;
	let topFrac = $state(0.5); // chat-mirror share of the height (terminal is below)
	let dragging = $state(false);

	function onMove(e: MouseEvent) {
		if (!container) return;
		const rect = container.getBoundingClientRect();
		let f = (e.clientY - rect.top) / rect.height;
		topFrac = Math.min(0.9, Math.max(0.1, f));
	}
	function onUp() {
		dragging = false;
		window.removeEventListener('mousemove', onMove);
		window.removeEventListener('mouseup', onUp);
	}
	function onDown(e: MouseEvent) {
		e.preventDefault();
		dragging = true;
		window.addEventListener('mousemove', onMove);
		window.addEventListener('mouseup', onUp);
	}
	onDestroy(onUp);
</script>

<div class="cpane" bind:this={container}>
	<div class="top" style="height: {topFrac * 100}%">
		<ChatMirror {projectId} {terminalId} {sessionId} />
	</div>
	<div class="divider" class:dragging role="separator" tabindex="-1" onmousedown={onDown}></div>
	<div class="bottom">
		<Terminal
			{tabKey}
			{projectId}
			kind="claude"
			{terminalId}
			{claudeResume}
			{claudeFork}
			{parentTerminalId}
			{initialPrompt} />
	</div>
	{#if dragging}
		<!-- Capture the mouse during a drag so the terminal canvas doesn't eat it. -->
		<div class="drag-overlay"></div>
	{/if}
</div>

<style>
	.cpane {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-width: 0;
		position: relative;
	}
	.top {
		min-height: 60px;
		overflow: hidden;
	}
	.divider {
		flex: 0 0 auto;
		height: 6px;
		background: #2c2c2c;
		cursor: row-resize;
	}
	.divider:hover,
	.divider.dragging {
		background: #3a78c8;
	}
	.bottom {
		flex: 1 1 auto;
		min-height: 60px;
		overflow: hidden;
	}
	.drag-overlay {
		position: absolute;
		inset: 0;
		z-index: 50;
		cursor: row-resize;
	}
</style>
