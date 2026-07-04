<script lang="ts">
	import { projects, refreshProjects } from './stores';
	import {
		addScheduledTask,
		updateScheduledTask,
		removeScheduledTask,
		setScheduledTaskEnabled,
		runScheduledTaskNow
	} from './ipc';
	import type { ScheduledTask } from './types';

	let { projectId }: { projectId: string } = $props();

	const project = $derived($projects.find((p) => p.id === projectId) ?? null);
	const tasks = $derived(project?.scheduledTasks ?? []);

	const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

	// Add/edit form state.
	let editingId = $state<string | null>(null);
	let showForm = $state(false);
	let name = $state('');
	let prompt = $state('');
	let time = $state('09:00');
	let weekdays = $state<Set<number>>(new Set());
	let useContext = $state(true);
	let enabled = $state(true);

	function resetForm() {
		editingId = null;
		showForm = false;
		name = '';
		prompt = '';
		time = '09:00';
		weekdays = new Set();
		useContext = true;
		enabled = true;
	}

	function startAdd() {
		resetForm();
		showForm = true;
	}

	function startEdit(t: ScheduledTask) {
		editingId = t.id;
		showForm = true;
		name = t.name;
		prompt = t.prompt;
		time = t.time;
		weekdays = new Set(t.weekdays ?? []);
		useContext = t.useContext;
		enabled = t.enabled;
	}

	function toggleDay(d: number) {
		const next = new Set(weekdays);
		if (next.has(d)) next.delete(d);
		else next.add(d);
		weekdays = next;
	}

	const canSave = $derived(!!name.trim() && !!prompt.trim() && /^\d{1,2}:\d{2}$/.test(time));

	async function save() {
		if (!canSave) return;
		const days = [...weekdays].sort((a, b) => a - b);
		if (editingId) {
			await updateScheduledTask(
				projectId,
				editingId,
				name.trim(),
				prompt.trim(),
				time,
				days,
				useContext,
				enabled
			);
		} else {
			await addScheduledTask(projectId, name.trim(), prompt.trim(), time, days, useContext);
		}
		resetForm();
		await refreshProjects();
	}

	async function remove(t: ScheduledTask) {
		if (!confirm(`Delete scheduled task “${t.name}”?`)) return;
		if (editingId === t.id) resetForm();
		await removeScheduledTask(projectId, t.id);
		await refreshProjects();
	}

	async function toggleEnabled(t: ScheduledTask) {
		await setScheduledTaskEnabled(projectId, t.id, !t.enabled);
		await refreshProjects();
	}

	async function runNow(t: ScheduledTask) {
		await runScheduledTaskNow(projectId, t.id);
	}

	function cadence(t: ScheduledTask): string {
		if (!t.weekdays?.length) return `Daily at ${t.time}`;
		return `${t.weekdays.map((d) => DAYS[d]).join(', ')} at ${t.time}`;
	}

	function lastRunText(t: ScheduledTask): string {
		if (!t.lastRun) return 'never run';
		return `last run ${new Date(t.lastRun).toLocaleString()}`;
	}
</script>

<div class="sched">
	<div class="bar">
		<span class="title">Scheduled Tasks — {project?.name ?? ''}</span>
		<button class="primary" onclick={startAdd}>＋ New Task</button>
	</div>

	<div class="note">
		Runs are <strong>read-only</strong> (no file edits) and fire only while spwn is running (it stays
		alive in the menu bar). Each run opens a new session under this project.
	</div>

	{#if showForm}
		<div class="form">
			<div class="row">
				<label for="st-name">Name</label>
				<input id="st-name" bind:value={name} placeholder="e.g. Morning status report" />
			</div>
			<div class="row">
				<label for="st-prompt">Prompt</label>
				<textarea
					id="st-prompt"
					bind:value={prompt}
					placeholder="What should Claude do? (it can read the project + context, but not edit files)"
				></textarea>
			</div>
			<div class="row inline">
				<div>
					<label for="st-time">Time</label>
					<input id="st-time" type="time" bind:value={time} />
				</div>
				<label class="chk">
					<input type="checkbox" bind:checked={useContext} />
					Use project context
				</label>
				{#if editingId}
					<label class="chk">
						<input type="checkbox" bind:checked={enabled} />
						Enabled
					</label>
				{/if}
			</div>
			<div class="row">
				<span class="days-label">Days <em>(none = every day)</em></span>
				<div class="days">
					{#each DAYS as d, i}
						<button
							type="button"
							class="day"
							class:on={weekdays.has(i)}
							onclick={() => toggleDay(i)}>{d}</button>
					{/each}
				</div>
			</div>
			<div class="form-btns">
				<button class="primary" disabled={!canSave} onclick={save}>
					{editingId ? 'Save' : 'Add task'}
				</button>
				<button onclick={resetForm}>Cancel</button>
			</div>
		</div>
	{/if}

	<div class="list">
		{#if tasks.length === 0}
			<div class="hint">No scheduled tasks yet. Click “＋ New Task” to add one.</div>
		{/if}
		{#each tasks as t (t.id)}
			<div class="task" class:disabled={!t.enabled}>
				<div class="thead">
					<span class="tname">{t.name}</span>
					<span class="cadence">{cadence(t)}</span>
					{#if t.useContext}<span class="tag">context</span>{/if}
					<span class="spacer"></span>
					<span class="last">{lastRunText(t)}</span>
				</div>
				<div class="tprompt">{t.prompt.slice(0, 300)}{t.prompt.length > 300 ? '…' : ''}</div>
				<div class="tbtns">
					<button onclick={() => runNow(t)}>Run now</button>
					<button onclick={() => toggleEnabled(t)}>{t.enabled ? 'Disable' : 'Enable'}</button>
					<button onclick={() => startEdit(t)}>Edit</button>
					<button class="danger" onclick={() => remove(t)}>Delete</button>
				</div>
			</div>
		{/each}
	</div>
</div>

<style>
	.sched {
		display: flex;
		flex-direction: column;
		height: 100%;
		background: var(--bg-sidebar);
		overflow-y: auto;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px 14px;
		border-bottom: 1px solid var(--border);
	}
	.title {
		flex: 1 1 auto;
		font-weight: 600;
		color: var(--text);
	}
	.note {
		padding: 8px 14px;
		font-size: 12px;
		color: var(--text-dim);
		border-bottom: 1px solid var(--border);
	}
	.primary {
		background: var(--accent);
		border: 1px solid var(--accent-border);
		color: #fff;
		border-radius: var(--radius);
		padding: 6px 14px;
		cursor: pointer;
	}
	.primary:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.form {
		padding: 12px 14px;
		border-bottom: 1px solid var(--border);
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.form .row {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.form .row.inline {
		flex-direction: row;
		align-items: flex-end;
		gap: 18px;
	}
	.form label {
		font-size: 12px;
		color: var(--text-dim);
	}
	.form input:not([type]),
	.form input[type='time'],
	.form textarea {
		box-sizing: border-box;
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		color: var(--text);
		padding: 8px 10px;
		font-family: inherit;
		font-size: 13px;
	}
	.form textarea {
		width: 100%;
		height: 80px;
		resize: vertical;
	}
	.chk {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		color: var(--text);
	}
	.days-label em {
		color: var(--text-muted);
		font-style: normal;
	}
	.days {
		display: flex;
		gap: 4px;
		margin-top: 4px;
	}
	.day {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #cfcfcf;
		border-radius: 5px;
		padding: 4px 8px;
		cursor: pointer;
		font-size: 12px;
	}
	.day.on {
		background: var(--accent);
		border-color: var(--accent-border);
		color: #fff;
	}
	.form-btns {
		display: flex;
		gap: 8px;
	}
	.form-btns button:not(.primary),
	.tbtns button {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #cfcfcf;
		border-radius: 5px;
		padding: 4px 10px;
		cursor: pointer;
		font-size: 12px;
	}
	.list {
		padding: 12px 14px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.hint {
		color: var(--text-muted);
		font-size: 13px;
	}
	.task {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 10px 12px;
		background: var(--bg-elevated);
	}
	.task.disabled {
		opacity: 0.55;
	}
	.thead {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
	}
	.tname {
		font-weight: 600;
		color: var(--text);
	}
	.cadence {
		color: var(--accent-text);
		font-size: 12px;
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
	.last {
		color: var(--text-muted);
		font-size: 11px;
	}
	.tprompt {
		color: var(--text-dim);
		font-size: 12px;
		margin: 6px 0 8px;
		white-space: pre-wrap;
	}
	.tbtns {
		display: flex;
		gap: 6px;
	}
	.danger {
		color: var(--danger) !important;
	}
</style>
