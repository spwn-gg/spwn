<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import '@xterm/xterm/css/xterm.css';
	import {
		openTerminal,
		writeToPty,
		resizePty,
		closeTerminal,
		setTerminalSession,
		onPtyOutput,
		onPtyExit,
		onPtySessionId
	} from './ipc';
	import { setTabTerminalId, setTabSession, refreshProjects } from './stores';
	import type { TerminalKind } from './types';

	let {
		tabKey,
		projectId,
		kind = 'shell',
		terminalId = undefined,
		claudeResume = undefined,
		claudeFork = undefined,
		parentTerminalId = undefined,
		initialPrompt = undefined
	}: {
		tabKey: string;
		projectId: string;
		kind?: TerminalKind;
		terminalId?: string;
		claudeResume?: string;
		claudeFork?: string;
		parentTerminalId?: string;
		initialPrompt?: string;
	} = $props();

	let container: HTMLDivElement;
	let term: Terminal | undefined;
	let fit: FitAddon | undefined;
	let id: string | null = null;
	let unlisten: Array<() => void> = [];
	let resizeObserver: ResizeObserver | undefined;

	onMount(async () => {
		term = new Terminal({
			fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
			fontSize: 13,
			cursorBlink: true,
			theme: { background: '#1e1e1e', foreground: '#e6e6e6' }
		});
		fit = new FitAddon();
		term.loadAddon(fit);
		term.open(container);
		fit.fit();

		try {
			id = await openTerminal({
				projectId,
				terminalId,
				kind,
				cols: term.cols,
				rows: term.rows,
				claudeResume,
				claudeFork,
				parentTerminalId,
				initialPrompt
			});
		} catch (e) {
			term.writeln(`\r\n\x1b[31m[open error] ${e}\x1b[0m`);
			return;
		}
		setTabTerminalId(tabKey, id);
		refreshProjects();

		unlisten.push(await onPtyOutput(id, (bytes) => term?.write(bytes)));
		unlisten.push(await onPtyExit(id, () => term?.writeln('\r\n\x1b[90m[session ended]\x1b[0m')));

		// New/forked Claude sessions reveal their id once written to disk; bind it
		// so the chat mirror can read the transcript.
		if (kind === 'claude') {
			unlisten.push(
				await onPtySessionId(id, (sid) => {
					setTabSession(tabKey, sid);
					if (id) setTerminalSession(projectId, id, sid).then(refreshProjects);
				})
			);
		}

		term.onData((d) => {
			if (id) writeToPty(id, d);
		});

		resizeObserver = new ResizeObserver(() => {
			fit?.fit();
			if (id && term) resizePty(id, term.cols, term.rows);
		});
		resizeObserver.observe(container);
	});

	onDestroy(() => {
		resizeObserver?.disconnect();
		unlisten.forEach((u) => u());
		if (id) closeTerminal(id); // detach (rmux session stays alive)
		term?.dispose();
	});
</script>

<div class="term" bind:this={container}></div>

<style>
	.term {
		width: 100%;
		height: 100%;
		padding: 4px 6px;
		box-sizing: border-box;
		background: #1e1e1e;
	}
</style>
