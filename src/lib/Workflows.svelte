<script lang="ts">
	import { onMount, onDestroy, tick } from 'svelte';
	import { projects, openTab, confirmDialog } from './stores';
	import {
		listWorkflows,
		setWorkflowsEnabled,
		setWorkflowAutostart,
		runWorkflow,
		newWorkflow,
		stopWorkflow,
		workflowLog,
		onWorkflowRun,
		onWorkflowLog,
		type UnlistenFn
	} from './ipc';
	import type {
		WorkflowInfo,
		WorkflowInput,
		WorkflowListing,
		WorkflowLogLine,
		WorkflowRun,
		WorkflowRunStatus
	} from './types';

	let { projectId }: { projectId: string } = $props();

	const project = $derived($projects.find((p) => p.id === projectId) ?? null);

	let listing = $state<WorkflowListing | null>(null);
	let loadError = $state<string | null>(null);
	let actionError = $state<string | null>(null);

	/** The run whose log is shown. */
	let shownRun = $state<WorkflowRun | null>(null);
	let lines = $state<WorkflowLogLine[]>([]);
	let logEl = $state<HTMLDivElement | null>(null);
	let stopLogFeed: UnlistenFn | null = null;

	/** The workflow whose inputs form is open. */
	let formFor = $state<string | null>(null);
	let formValues = $state<Record<string, string | boolean>>({});

	/** The "New workflow" form. */
	let creating = $state(false);
	let newName = $state('');
	let newTs = $state(false);
	let createdMsg = $state<string | null>(null);

	let unlisten: UnlistenFn[] = [];

	async function create() {
		const name = newName.trim();
		if (!name) return;
		await act(async () => {
			const file = await newWorkflow(projectId, name, newTs);
			createdMsg = `Created ${file} — open it in your editor. spwn.d.ts beside it gives you completion.`;
			creating = false;
			newName = '';
		});
	}

	const LIVE: WorkflowRunStatus[] = ['running', 'stopping', 'restarting'];
	const isLive = (run?: WorkflowRun | null) => !!run && LIVE.includes(run.status);

	async function load() {
		try {
			listing = await listWorkflows(projectId);
			loadError = null;
		} catch (e) {
			loadError = String(e instanceof Error ? e.message : e);
		}
	}

	async function act(fn: () => Promise<unknown>) {
		actionError = null;
		try {
			await fn();
		} catch (e) {
			actionError = String(e instanceof Error ? e.message : e);
		}
		await load();
	}

	async function enable() {
		const res = await confirmDialog({
			title: `Allow workflows in “${project?.name ?? 'this project'}”?`,
			body:
				'Workflows are scripts from the project’s .spwn/workflows folder. They run with your ' +
				'permissions: they can start sessions, run commands, and use your GitHub token. Only ' +
				'enable this for code you trust.',
			confirmLabel: 'Allow workflows'
		});
		if (res !== 'confirm') return;
		await act(() => setWorkflowsEnabled(projectId, true));
	}

	async function disable() {
		const res = await confirmDialog({
			title: 'Turn off workflows?',
			body: 'Running workflows in this project are stopped, and none start until you allow them again.',
			confirmLabel: 'Turn off'
		});
		if (res !== 'confirm') return;
		await act(() => setWorkflowsEnabled(projectId, false));
	}

	const inputsOf = (w: WorkflowInfo): [string, WorkflowInput][] => Object.entries(w.meta?.inputs ?? {});

	function startRun(w: WorkflowInfo) {
		if (inputsOf(w).length === 0) {
			void launch(w, {});
			return;
		}
		formFor = w.name;
		formValues = Object.fromEntries(
			inputsOf(w).map(([key, spec]) => [
				key,
				spec.type === 'boolean' ? Boolean(spec.default) : spec.default == null ? '' : String(spec.default)
			])
		);
	}

	function coerce(spec: WorkflowInput, v: string | boolean): unknown {
		if (spec.type === 'boolean') return Boolean(v);
		if (spec.type === 'number') return v === '' ? null : Number(v);
		return v;
	}

	async function launch(w: WorkflowInfo, inputs: Record<string, unknown>) {
		actionError = null;
		try {
			const run = await runWorkflow(projectId, w.name, inputs);
			formFor = null;
			await showRun(run);
		} catch (e) {
			actionError = String(e instanceof Error ? e.message : e);
		}
		await load();
	}

	function submitForm(w: WorkflowInfo) {
		const inputs = Object.fromEntries(inputsOf(w).map(([key, spec]) => [key, coerce(spec, formValues[key])]));
		void launch(w, inputs);
	}

	async function showRun(run: WorkflowRun) {
		stopLogFeed?.();
		stopLogFeed = null;
		shownRun = run;
		lines = [];
		// Subscribe before fetching so nothing written in between is lost; drop the overlap.
		const buffered: WorkflowLogLine[] = [];
		let fetched = false;
		stopLogFeed = await onWorkflowLog(run.id, (line) => {
			if (fetched) {
				lines = [...lines, line];
				void scrollToEnd();
			} else buffered.push(line);
		});
		try {
			const past = await workflowLog(run.id);
			const lastAt = past.length ? past[past.length - 1].at : -Infinity;
			lines = [...past, ...buffered.filter((l) => l.at > lastAt)];
		} catch {
			lines = [...buffered];
		}
		fetched = true;
		await scrollToEnd();
	}

	async function scrollToEnd() {
		await tick();
		if (logEl) logEl.scrollTop = logEl.scrollHeight;
	}

	function openSession(terminalId: string) {
		const t = project?.terminals.find((x) => x.id === terminalId);
		if (!t || !project) return;
		openTab({
			projectId: project.id,
			kind: t.kind,
			agent: t.agent ?? undefined,
			terminalId: t.id,
			title: t.title,
			projectName: project.name,
			sessionId: t.sessionId ?? undefined
		});
	}

	const sessionTitle = (terminalId: string) =>
		project?.terminals.find((t) => t.id === terminalId)?.title ?? 'deleted session';

	function when(ms: number) {
		return new Date(ms).toLocaleString();
	}

	function clock(ms: number) {
		return new Date(ms).toLocaleTimeString();
	}

	onMount(async () => {
		await load();
		unlisten.push(
			await onWorkflowRun((run) => {
				if (run.projectId !== projectId) return;
				if (shownRun?.id === run.id) shownRun = run;
				if (listing) {
					listing = {
						...listing,
						workflows: listing.workflows.map((w) => (w.name === run.workflow ? { ...w, run } : w))
					};
				}
			})
		);
	});

	onDestroy(() => {
		stopLogFeed?.();
		unlisten.forEach((u) => u());
	});
</script>

<div class="wf">
	<div class="bar">
		<span class="title">Workflows — {project?.name ?? ''}</span>
		<button onclick={() => ((creating = !creating), (createdMsg = null))}>＋ New workflow</button>
		<button onclick={load} title="Re-read .spwn/workflows">Refresh</button>
		{#if listing?.enabled}
			<button onclick={disable}>Turn off</button>
		{/if}
	</div>

	{#if creating}
		<form
			class="create"
			onsubmit={(e) => {
				e.preventDefault();
				void create();
			}}>
			<input bind:value={newName} placeholder="name, e.g. triage" spellcheck="false" />
			<label class="chk"><input type="checkbox" bind:checked={newTs} /> TypeScript</label>
			<button class="primary" type="submit" disabled={!newName.trim()}>Create</button>
			<button type="button" onclick={() => (creating = false)}>Cancel</button>
		</form>
	{/if}
	{#if createdMsg}
		<div class="created">{createdMsg}</div>
	{/if}
	{#if loadError}
		<div class="error">{loadError}</div>
	{/if}
	{#if actionError}
		<div class="error">{actionError}</div>
	{/if}

	{#if listing && !listing.enabled}
		<div class="trust">
			<div>
				Workflows are scripts in <code>{listing.dir}</code> that start and drive sessions. They’re code from
				this project, so none run until you allow them.
			</div>
			<button class="primary" onclick={enable}>Allow workflows…</button>
		</div>
	{/if}

	<div class="list">
		{#if listing && listing.workflows.length === 0}
			<div class="hint">
				No workflows yet. Click “＋ New workflow”, or add a script to <code>{listing.dir}/</code> in this
				project, for example <code>hello.js</code>:
				<pre>{`export const meta = { description: "Say hello" };

export default async function main(spwn, inputs) {
  const s = await spwn.sessions.create({ title: "hello", prompt: "Say hello" });
  const { text } = await s.waitForTurn();
  spwn.log(text);
}`}</pre>
			</div>
		{/if}

		{#each listing?.workflows ?? [] as w (w.name)}
			{@const run = w.run}
			{@const live = isLive(run)}
			<div class="card" class:broken={!!w.error}>
				<div class="head">
					<span class="dot {run?.status ?? 'idle'}" title={run?.status ?? 'not run yet'}></span>
					<span class="name">{w.meta?.name || w.name}</span>
					<span class="file">{w.file}</span>
					{#if w.meta?.keepAlive}<span class="tag" title="Restarted whenever it exits">keep-alive</span>{/if}
					<span class="spacer"></span>
					{#if run}
						<button class="link" onclick={() => showRun(run)}>
							{run.status}{run.restarts ? ` · ${run.restarts} restart${run.restarts === 1 ? '' : 's'}` : ''}
						</button>
					{/if}
				</div>
				{#if w.meta?.description}<div class="desc">{w.meta.description}</div>{/if}
				{#if w.error}<pre class="werr">{w.error}</pre>{/if}

				{#if formFor === w.name}
					<form
						class="inputs"
						onsubmit={(e) => {
							e.preventDefault();
							submitForm(w);
						}}>
						{#each inputsOf(w) as [key, spec] (key)}
							<label class:chk={spec.type === 'boolean'}>
								{#if spec.type === 'boolean'}
									<input type="checkbox" bind:checked={formValues[key] as boolean} />
									<span>{key}</span>
								{:else}
									<span>{key}{spec.required ? ' *' : ''}</span>
									<input
										type={spec.type === 'number' ? 'number' : 'text'}
										bind:value={formValues[key]}
										placeholder={spec.description ?? ''} />
								{/if}
								{#if spec.description && spec.type === 'boolean'}<em>{spec.description}</em>{/if}
							</label>
						{/each}
						<div class="btns">
							<button class="primary" type="submit">Run</button>
							<button type="button" onclick={() => (formFor = null)}>Cancel</button>
						</div>
					</form>
				{/if}

				<div class="btns">
					{#if live && run}
						<button onclick={() => act(() => stopWorkflow(run.id))} disabled={run.status === 'stopping'}>Stop</button>
					{:else}
						<button class="primary" disabled={!listing?.enabled || !!w.error} onclick={() => startRun(w)}>Run</button>
					{/if}
					<label class="chk" title="Start when spwn starts, and keep it running">
						<input
							type="checkbox"
							checked={w.autostart}
							disabled={!listing?.enabled}
							onchange={(e) => act(() => setWorkflowAutostart(projectId, w.name, e.currentTarget.checked))} />
						Start with spwn
					</label>
				</div>
			</div>
		{/each}
	</div>

	{#if shownRun}
		<div class="log-panel">
			<div class="log-head">
				<span class="dot {shownRun.status}"></span>
				<strong>{shownRun.workflow}</strong>
				<span class="muted">{shownRun.status} · started {when(shownRun.startedAt)} · {shownRun.trigger}</span>
				<span class="spacer"></span>
				<button class="link" onclick={() => ((shownRun = null), stopLogFeed?.(), (stopLogFeed = null))}>close</button>
			</div>
			{#if shownRun.sessions.length}
				<div class="sessions">
					Sessions:
					{#each shownRun.sessions as sid (sid)}
						<button class="link" onclick={() => openSession(sid)}>✦ {sessionTitle(sid)}</button>
					{/each}
				</div>
			{/if}
			{#if shownRun.error}<pre class="werr">{shownRun.error}</pre>{/if}
			<div class="log" bind:this={logEl}>
				{#each lines as l, i (i)}
					<div class="line {l.level}"><span class="at">{clock(l.at)}</span><span class="msg">{l.msg}</span></div>
				{/each}
				{#if lines.length === 0}<div class="muted">No output yet.</div>{/if}
			</div>
		</div>
	{/if}
</div>

<style>
	.wf {
		display: flex;
		flex-direction: column;
		height: 100%;
		background: var(--bg-sidebar);
		min-height: 0;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 10px 14px;
		border-bottom: 1px solid var(--border);
	}
	.title {
		flex: 1 1 auto;
		font-weight: 600;
		color: var(--text);
	}
	button {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #cfcfcf;
		border-radius: 5px;
		padding: 4px 10px;
		cursor: pointer;
		font-size: 12px;
	}
	button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.primary {
		background: var(--accent);
		border-color: var(--accent-border);
		color: #fff;
	}
	.link {
		background: none;
		border: none;
		padding: 0 2px;
		color: var(--accent-text);
	}
	code {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
		color: var(--text);
	}
	.error {
		padding: 8px 14px;
		color: var(--danger);
		background: var(--danger-bg);
		font-size: 12px;
		white-space: pre-wrap;
	}
	.create {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 14px;
		border-bottom: 1px solid var(--border);
	}
	.create input:not([type='checkbox']) {
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		color: var(--text);
		padding: 5px 8px;
		font-size: 13px;
	}
	.created {
		padding: 8px 14px;
		font-size: 12px;
		color: var(--ok);
		border-bottom: 1px solid var(--border);
	}
	.trust {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 10px 14px;
		font-size: 12px;
		color: var(--text-dim);
		border-bottom: 1px solid var(--border);
		background: var(--accent-soft);
	}
	.trust button {
		flex: 0 0 auto;
	}
	.list {
		padding: 12px 14px;
		display: flex;
		flex-direction: column;
		gap: 10px;
		overflow-y: auto;
		flex: 1 1 auto;
		min-height: 0;
	}
	.hint {
		color: var(--text-muted);
		font-size: 13px;
	}
	.hint pre {
		margin: 8px 0 0;
		padding: 10px;
		background: var(--bg-input);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		color: var(--text-dim);
		font-size: 12px;
		overflow-x: auto;
	}
	.card {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 10px 12px;
		background: var(--bg-elevated);
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.card.broken {
		border-color: var(--danger-bg);
	}
	.head {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
	}
	.name {
		font-weight: 600;
		color: var(--text);
	}
	.file {
		color: var(--text-muted);
		font-size: 11px;
		font-family: ui-monospace, Menlo, monospace;
	}
	.tag {
		font-size: 11px;
		color: var(--text-dim);
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		padding: 0 5px;
	}
	.spacer {
		flex: 1 1 auto;
	}
	.desc {
		color: var(--text-dim);
		font-size: 12px;
	}
	.werr {
		margin: 0;
		padding: 6px 8px;
		color: var(--danger);
		background: var(--bg-input);
		border-radius: var(--radius);
		font-size: 11px;
		white-space: pre-wrap;
		max-height: 160px;
		overflow: auto;
	}
	.btns {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.chk {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 12px;
		color: var(--text-dim);
	}
	.inputs {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 8px 0;
	}
	.inputs label:not(.chk) {
		display: flex;
		flex-direction: column;
		gap: 3px;
		font-size: 12px;
		color: var(--text-dim);
	}
	.inputs input:not([type='checkbox']) {
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		color: var(--text);
		padding: 6px 8px;
		font-size: 13px;
	}
	.inputs em {
		color: var(--text-muted);
		font-style: normal;
	}
	.dot {
		flex: 0 0 auto;
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--border-strong);
	}
	.dot.running {
		background: var(--ok);
	}
	.dot.restarting,
	.dot.stopping {
		background: #e0a83a;
	}
	.dot.failed {
		background: #e06c6c;
	}
	.log-panel {
		flex: 0 0 45%;
		display: flex;
		flex-direction: column;
		border-top: 1px solid var(--border-strong);
		min-height: 0;
	}
	.log-head,
	.sessions {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px;
		padding: 6px 14px;
		font-size: 12px;
		color: var(--text-dim);
	}
	.log-panel .werr {
		margin: 0 14px 6px;
	}
	.muted {
		color: var(--text-muted);
	}
	.log {
		flex: 1 1 auto;
		overflow: auto;
		padding: 6px 14px 10px;
		background: var(--bg-input);
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
	}
	.line {
		display: flex;
		gap: 10px;
		color: var(--text);
		white-space: pre-wrap;
	}
	.line .at {
		flex: 0 0 auto;
		color: var(--text-muted);
	}
	.line.debug,
	.line.hook {
		color: var(--text-dim);
	}
	.line.warn {
		color: #e0c074;
	}
	.line.error {
		color: #e06c6c;
	}
</style>
