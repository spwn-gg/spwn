<script lang="ts">
	import { claudeSetMode, claudeInterrupt } from './ipc';
	import { pasteToInput } from './stores';

	let {
		terminalId,
		busy = false,
		mode = $bindable('default'),
		initialPrompt = '',
		onSend = () => {}
	}: {
		terminalId: string | undefined;
		busy?: boolean;
		mode?: PermMode;
		initialPrompt?: string;
		onSend?: (text: string) => void;
	} = $props();

	type PermMode = 'default' | 'acceptEdits' | 'plan' | 'auto';
	// Matches the Claude Code TUI Shift-Tab cycle: default → accept edits → plan,
	// with the optional `auto` (classifier-gated) mode last. We omit
	// `bypassPermissions`, which the TUI only exposes via a dangerous launch flag.
	const MODES: PermMode[] = ['default', 'acceptEdits', 'plan', 'auto'];
	const MODE_LABEL: Record<PermMode, string> = {
		default: 'default',
		acceptEdits: 'accept edits',
		plan: 'plan',
		auto: 'auto'
	};

	let text = $state(initialPrompt ?? '');
	let ta: HTMLTextAreaElement | undefined;

	// Consume a response pasted in from a child session (e.g. "→ parent").
	$effect(() => {
		const inj = $pasteToInput;
		if (inj && inj.terminalId === terminalId) {
			text = text.trim() ? `${text.trimEnd()}\n\n${inj.text}` : inj.text;
			pasteToInput.set(null);
			queueMicrotask(() => {
				autogrow();
				ta?.focus();
			});
		}
	});

	function autogrow() {
		if (!ta) return;
		ta.style.height = 'auto';
		ta.style.height = Math.min(ta.scrollHeight, 200) + 'px';
	}

	function send() {
		const t = text.trim();
		if (!t || !terminalId) return;
		// onSend (in ClaudePane) owns the actual claudeSend so a failed send can
		// clear the busy indicator and surface the error in one place.
		onSend(t);
		text = '';
		queueMicrotask(autogrow);
	}

	function cycleMode() {
		const next = MODES[(MODES.indexOf(mode) + 1) % MODES.length];
		mode = next;
		if (terminalId) claudeSetMode(terminalId, next);
	}

	function onKey(e: KeyboardEvent) {
		if (e.key === 'Tab' && e.shiftKey) {
			e.preventDefault(); // Shift-Tab normally moves focus
			cycleMode();
		} else if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault(); // Shift+Enter inserts a newline
			send();
		} else if (e.key === 'Escape' && busy && terminalId) {
			e.preventDefault();
			claudeInterrupt(terminalId);
		}
	}
</script>

<div class="bar">
	<button
		class="mode {mode}"
		title="Permission mode — Shift-Tab to cycle (default → accept edits → plan)"
		onclick={cycleMode}>{MODE_LABEL[mode]}</button>
	<textarea
		bind:this={ta}
		bind:value={text}
		oninput={autogrow}
		onkeydown={onKey}
		rows="1"
		placeholder={terminalId ? 'Message Claude…  (Enter to send, Shift+Enter for newline)' : 'Starting session…'}
	></textarea>
	{#if busy && terminalId}
		<button class="stop" title="Interrupt (Esc)" onclick={() => claudeInterrupt(terminalId)}>Stop</button>
	{:else}
		<button class="send" disabled={!text.trim() || !terminalId} onclick={send}>Send</button>
	{/if}
</div>

<style>
	.bar {
		display: flex;
		align-items: flex-end;
		gap: 8px;
		padding: 10px 12px;
		border-top: 1px solid var(--border);
		background: var(--bg-sidebar);
	}
	textarea {
		flex: 1 1 auto;
		resize: none;
		min-height: 38px;
		max-height: 200px;
		box-sizing: border-box;
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		color: var(--text);
		padding: 9px 12px;
		font-family: inherit;
		font-size: 13px;
		line-height: 1.4;
	}
	textarea:focus {
		outline: none;
		border-color: var(--accent-line);
	}
	.mode {
		flex: 0 0 auto;
		align-self: stretch;
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		padding: 0 10px;
		font-size: 11px;
		font-weight: 600;
		cursor: pointer;
		white-space: nowrap;
		background: var(--bg-elevated);
		color: var(--text-dim);
	}
	.mode.acceptEdits {
		background: #3a3320;
		border-color: #5a4a1a;
		color: #e8d48a;
	}
	.mode.plan {
		background: #1f2c44;
		border-color: #34507a;
		color: #9bbce0;
	}
	.mode.auto {
		background: #1f3a2c;
		border-color: #2f6a4a;
		color: #8fd6a8;
	}
	.send,
	.stop {
		flex: 0 0 auto;
		align-self: stretch;
		border-radius: var(--radius);
		padding: 0 16px;
		font-size: 13px;
		cursor: pointer;
		border: 1px solid var(--accent-border);
	}
	.send {
		background: var(--accent);
		color: #fff;
	}
	.send:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.stop {
		background: var(--danger-bg);
		border-color: #7a3a3a;
		color: #fff;
	}
</style>
