<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import ChatMirror from './ChatMirror.svelte';
	import InputBar from './InputBar.svelte';
	import PermissionPrompt from './PermissionPrompt.svelte';
	import QuestionPicker from './QuestionPicker.svelte';
	import { get } from 'svelte/store';
	import {
		openTerminal,
		setTerminalSession,
		claudePermission,
		claudeAnswer,
		checkpointProject,
		listCheckpoints,
		restoreCheckpoint,
		onClaudeEvent,
		onClaudeExit
	} from './ipc';
	import {
		setTabTerminalId,
		setTabSession,
		refreshProjects,
		markAttention,
		setSessionBusy,
		busySessions,
		activeCodeSession,
		activeTabKey
	} from './stores';
	import type { ClaudeEvent, PendingQuestion, PermissionReq, Turn } from './types';

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

	let id = $state<string | undefined>(terminalId);
	let liveSession = $state<string | undefined>(sessionId);
	let mode = $state<'default' | 'acceptEdits' | 'plan' | 'auto'>('default');
	// A brand-new tab (no terminalId yet) gets a pre-edit "baseline" checkpoint.
	const isFreshSession = terminalId === undefined;
	let baselineDone = false;

	// Live, in-flight turn (overlaid on top of the JSONL-rendered history).
	let busy = $state(false);
	let streamingText = $state('');
	let streamingThinking = $state('');
	let liveTools = $state<{ id: string; name: string }[]>([]);
	let pendingUserText = $state<string | null>(null);
	let pendingPermissions = $state<PermissionReq[]>([]);
	let pendingQuestions = $state<PendingQuestion[]>([]);
	let lastAssistantUuid: string | null = null;
	let clearTimer: ReturnType<typeof setTimeout> | undefined;
	let exited = $state(false);

	let unlisten: Array<() => void> = [];

	onMount(async () => {
		try {
			id = await openTerminal({
				projectId,
				terminalId,
				kind: 'claude',
				cols: 80,
				rows: 24,
				claudeResume,
				claudeFork,
				parentTerminalId
			});
		} catch (e) {
			console.error('open claude session failed', e);
			return;
		}
		setTabTerminalId(tabKey, id);
		refreshProjects();
		// The sidecar's `init` only fires after the first user turn, long after this
		// listener attaches — so there is no init race.
		unlisten.push(await onClaudeEvent(id, handleEvent));
		unlisten.push(
			await onClaudeExit(id, () => {
				busy = false;
				exited = true;
			})
		);
	});

	onDestroy(() => {
		clearTimeout(clearTimer);
		unlisten.forEach((u) => u());
		if (liveSession) setSessionBusy(liveSession, false);
	});

	// Publish this session's busy state so restores can gate on "no agent writing".
	$effect(() => {
		if (liveSession) setSessionBusy(liveSession, busy);
	});

	// Auto-restore the project to a session's code when you switch to it (user choice).
	// Guards: only when this pane is active, the session has a checkpoint, and NO
	// session is mid-turn (avoid racing a background write). A pre-switch snapshot of
	// the outgoing session preserves its on-disk state.
	let switchingCode = false;
	$effect(() => {
		const active = $activeTabKey === tabKey;
		const sid = liveSession;
		const anyBusy = $busySessions.size > 0;
		if (!active || !sid || busy || anyBusy || switchingCode) return;
		if ($activeCodeSession[projectId] === sid) return;
		autoRestoreOnSwitch(sid);
	});

	async function autoRestoreOnSwitch(sid: string) {
		switchingCode = true;
		try {
			const current = get(activeCodeSession)[projectId];
			if (current && current !== sid) {
				// Preserve the outgoing session's current files so switching back restores them.
				await checkpointProject(projectId, current, 'pre-switch', 'pre-switch').catch(() => {});
			}
			activeCodeSession.update((m) => ({ ...m, [projectId]: sid }));
			const cps = await listCheckpoints(sid);
			if (cps.length) await restoreCheckpoint(projectId, sid, cps[0].id, false);
		} catch (e) {
			console.error('auto-restore on switch', e);
		} finally {
			switchingCode = false;
		}
	}

	function resetLive() {
		streamingText = '';
		streamingThinking = '';
		liveTools = [];
	}

	function handleEvent(ev: ClaudeEvent) {
		switch (ev.t) {
			case 'init':
				liveSession = ev.sessionId;
				setTabSession(tabKey, ev.sessionId);
				if (id) setTerminalSession(projectId, id, ev.sessionId).then(refreshProjects);
				// Snapshot the project's pre-edit state once, for a fresh session.
				if (isFreshSession && !baselineDone) {
					baselineDone = true;
					checkpointProject(projectId, ev.sessionId, 'baseline', 'baseline').catch(() => {});
				}
				break;
			case 'delta':
				busy = true;
				streamingText += ev.text;
				break;
			case 'thinking':
				busy = true;
				streamingThinking += ev.text;
				break;
			case 'tool_use':
				busy = true;
				liveTools = [...liveTools, { id: ev.id, name: ev.name }];
				break;
			case 'assistant_uuid':
				lastAssistantUuid = ev.uuid;
				break;
			case 'permission':
				pendingPermissions = [
					...pendingPermissions,
					{ id: ev.id, tool: ev.tool, input: ev.input, title: ev.title }
				];
				// A background session is now blocked awaiting allow/deny.
				markAttention(tabKey);
				break;
			case 'question':
				pendingQuestions = [...pendingQuestions, { id: ev.id, questions: ev.questions }];
				markAttention(tabKey);
				break;
			case 'result':
				busy = false;
				// A background session finished its turn.
				markAttention(tabKey);
				// Snapshot the project's files at this turn (for undo / rewind-restore).
				if (liveSession && lastAssistantUuid) {
					checkpointProject(projectId, liveSession, lastAssistantUuid, 'turn').catch((e) =>
						console.error('checkpoint failed', e)
					);
				}
				// Keep the overlay until the JSONL reload brings the finished turn in
				// (onReload clears it); fall back to a timer so it can't get stuck.
				clearTimeout(clearTimer);
				clearTimer = setTimeout(() => {
					resetLive();
					pendingUserText = null;
				}, 1500);
				break;
			case 'error':
				busy = false;
				console.error('[claude]', ev.message);
				break;
		}
	}

	// Called by ChatMirror after each transcript reload — reconcile the optimistic
	// overlay against what's now persisted on disk.
	function onReload(turns: Turn[]) {
		if (pendingUserText) {
			const want = pendingUserText.trim();
			const has = turns.some(
				(t) =>
					t.role === 'user' &&
					t.blocks.some((b) => b.kind === 'text' && (b.text ?? '').trim() === want)
			);
			if (has) pendingUserText = null;
		}
		if (lastAssistantUuid && turns.some((t) => t.uuid === lastAssistantUuid)) {
			resetLive();
			lastAssistantUuid = null;
		}
	}

	function onSend(text: string) {
		pendingUserText = text;
		resetLive();
		busy = true;
	}

	function answerQuestion(qid: string, text: string) {
		if (id) claudeAnswer(id, qid, text);
		pendingQuestions = pendingQuestions.filter((q) => q.id !== qid);
		busy = true; // the held turn resumes once answered
	}

	// The session's sidecar was just restarted (rewind) — drop stale live state.
	function onRewound() {
		clearTimeout(clearTimer);
		busy = false;
		resetLive();
		pendingUserText = null;
		pendingPermissions = [];
		pendingQuestions = [];
		lastAssistantUuid = null;
	}

	function allow(pid: string) {
		if (id) claudePermission(id, pid, true);
		pendingPermissions = pendingPermissions.filter((p) => p.id !== pid);
	}
	function deny(pid: string) {
		if (id) claudePermission(id, pid, false);
		pendingPermissions = pendingPermissions.filter((p) => p.id !== pid);
	}
</script>

<div class="cpane">
	<div class="mirror-wrap">
		<ChatMirror
			{projectId}
			terminalId={id}
			sessionId={liveSession}
			{busy}
			{streamingText}
			{streamingThinking}
			{liveTools}
			{pendingUserText}
			{onReload}
			{onRewound} />
	</div>
	{#if exited}
		<div class="ended">Session ended — send a message to resume.</div>
	{/if}
	{#each pendingQuestions as pq (pq.id)}
		<QuestionPicker pending={pq} onAnswer={answerQuestion} />
	{/each}
	{#each pendingPermissions as p (p.id)}
		<PermissionPrompt req={p} onAllow={allow} onDeny={deny} />
	{/each}
	<InputBar terminalId={id} {busy} bind:mode {initialPrompt} {onSend} />
</div>

<style>
	.cpane {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-width: 0;
	}
	.mirror-wrap {
		flex: 1 1 auto;
		min-height: 0;
		overflow: hidden;
	}
	.ended {
		padding: 6px 12px;
		font-size: 12px;
		color: var(--text-dim);
		border-top: 1px solid var(--border);
	}
</style>
