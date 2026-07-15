<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import ChatMirror from './ChatMirror.svelte';
	import InputBar from './InputBar.svelte';
	import PermissionPrompt from './PermissionPrompt.svelte';
	import QuestionPicker from './QuestionPicker.svelte';
	import {
		openTerminal,
		setTerminalSession,
		claudeSend,
		claudePermission,
		claudeAnswer,
		checkpointProject,
		commitSessionTurn,
		onClaudeEvent,
		onClaudeExit
	} from './ipc';
	import {
		setTabTerminalId,
		setTabSession,
		refreshProjects,
		markAttention,
		setSessionBusy,
		pasteToInput
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
	// Auto-seed: text that should be sent into this conversation (from "start with
	// context" / initialPrompt, or a "→ parent" paste) rather than parked in the
	// composer. Flushed via onSend once the terminal is live and any in-flight turn
	// has finished — see the flush effect below.
	let ready = $state(false);
	let pendingSeed = $state<string | null>(null);
	function seed(text: string | undefined) {
		const t = text?.trim();
		if (!t) return;
		// Append/coalesce so multiple seeds arriving while busy aren't lost.
		pendingSeed = pendingSeed ? `${pendingSeed}\n\n${t}` : t;
	}
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
	let lastError = $state<string | null>(null);

	// Watchdog: if a turn goes fully silent (no events at all) for this long, the
	// sidecar's child has likely stalled with its pipes open — which produces no
	// exit/error, so nothing else would ever clear the indicator. Free the UI and
	// tell the user. A genuinely streaming turn re-arms this on every event.
	const STALL_MS = 120_000;
	let stallTimer: ReturnType<typeof setTimeout> | undefined;
	function armStall() {
		clearTimeout(stallTimer);
		stallTimer = setTimeout(() => {
			if (!busy) return;
			busy = false;
			lastError =
				'No response for 2 minutes — the assistant may have stalled. Send another message to retry, or rewind the turn.';
		}, STALL_MS);
	}
	function disarmStall() {
		clearTimeout(stallTimer);
	}

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
				disarmStall();
				exited = true;
			})
		);
		// Listeners are attached and `id` is assigned — safe to flush seeds now
		// (their init/delta/result events will be captured).
		ready = true;
		if (initialPrompt) seed(initialPrompt);
	});

	onDestroy(() => {
		clearTimeout(clearTimer);
		disarmStall();
		unlisten.forEach((u) => u());
		if (liveSession) setSessionBusy(liveSession, false);
	});

	// Publish this session's busy state (used by manual restore/rewind gating).
	$effect(() => {
		if (liveSession) setSessionBusy(liveSession, busy);
	});

	// Consume a "→ parent" paste targeted at this session and queue it as a seed.
	// Only the pane whose id matches claims the slot; others see null and no-op.
	$effect(() => {
		const inj = $pasteToInput;
		if (inj && id && inj.terminalId === id) {
			pasteToInput.set(null); // consume immediately (re-triggers with null → no-op)
			seed(inj.text);
		}
	});

	// Flush a queued seed via onSend once the terminal is live and idle. If a turn
	// is in flight (busy), the seed waits here until `result` flips busy → false,
	// then this effect re-runs and sends — no interleaving, no drop.
	$effect(() => {
		if (ready && id && !busy && pendingSeed) {
			const t = pendingSeed;
			pendingSeed = null; // clear BEFORE onSend so this can't re-fire and double-send
			onSend(t);
		}
	});

	// No file swapping on session switch: each session runs in its own git worktree,
	// so switching tabs is a pure UI focus change. (Swapping the shared project dir
	// in place would corrupt any autonomous session running concurrently.) Per-turn
	// undo checkpoints still snapshot each session's own worktree.

	function resetLive() {
		streamingText = '';
		streamingThinking = '';
		liveTools = [];
	}

	function handleEvent(ev: ClaudeEvent) {
		// Any event means the sidecar is alive and progressing — reset the watchdog.
		// Terminal events (result/error) disarm it explicitly below.
		armStall();
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
				disarmStall();
				// A background session finished its turn.
				markAttention(tabKey);
				// Snapshot the project's files at this turn (for undo / rewind-restore).
				if (liveSession && lastAssistantUuid) {
					checkpointProject(projectId, liveSession, lastAssistantUuid, 'turn').catch((e) =>
						console.error('checkpoint failed', e)
					);
					// Commit onto the session's worktree branch so it carries mergeable
					// history (no-op if this session has no branch). Keyed by terminal id.
					if (id) {
						commitSessionTurn(id, `spwn turn ${lastAssistantUuid.slice(0, 8)}`).catch((e) =>
							console.error('commit failed', e)
						);
					}
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
				disarmStall();
				lastError = ev.message;
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
		if (!id) return;
		pendingUserText = text;
		resetLive();
		lastError = null;
		busy = true;
		armStall();
		// Own the send here so a rejected invoke (e.g. the sidecar already exited)
		// surfaces and clears the indicator instead of leaving it spinning.
		claudeSend(id, text).catch((e) => {
			busy = false;
			disarmStall();
			lastError = `Couldn't send message: ${e?.message ?? e}`;
			console.error('[claude] send failed', e);
		});
	}

	function answerQuestion(qid: string, text: string) {
		if (id) claudeAnswer(id, qid, text);
		pendingQuestions = pendingQuestions.filter((q) => q.id !== qid);
		busy = true; // the held turn resumes once answered
	}

	// The session's sidecar was just restarted (rewind) — drop stale live state.
	function onRewound() {
		clearTimeout(clearTimer);
		disarmStall();
		busy = false;
		resetLive();
		lastError = null;
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
	{#if lastError}
		<div class="cerror" role="alert">
			<span class="msg">{lastError}</span>
			<button class="dismiss" onclick={() => (lastError = null)} aria-label="Dismiss">×</button>
		</div>
	{/if}
	{#if exited}
		<div class="ended">Session ended — send a message to resume.</div>
	{/if}
	{#each pendingQuestions as pq (pq.id)}
		<QuestionPicker pending={pq} onAnswer={answerQuestion} />
	{/each}
	{#each pendingPermissions as p (p.id)}
		<PermissionPrompt req={p} onAllow={allow} onDeny={deny} />
	{/each}
	<InputBar terminalId={id} {busy} bind:mode {onSend} />
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
	.cerror {
		display: flex;
		align-items: flex-start;
		gap: 8px;
		padding: 8px 12px;
		font-size: 12px;
		color: #fca5a5;
		background: rgba(220, 38, 38, 0.1);
		border-top: 1px solid rgba(220, 38, 38, 0.4);
		white-space: pre-wrap;
	}
	.cerror .msg {
		flex: 1 1 auto;
		min-width: 0;
	}
	.cerror .dismiss {
		flex: 0 0 auto;
		background: none;
		border: none;
		color: inherit;
		cursor: pointer;
		font-size: 14px;
		line-height: 1;
		opacity: 0.7;
	}
	.cerror .dismiss:hover {
		opacity: 1;
	}
</style>
