<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getSettings,
		setSettings,
		pickFile,
		openGlobalHooksDir,
		listAgents,
		reloadAgents,
		openAgentsDir
	} from './ipc';
	import { showSettings } from './stores';
	import type { WorktreeLocation, AgentSummary } from './types';

	let worktreeLocation = $state<WorktreeLocation>('sibling');
	let globalHooksEnabled = $state(true);
	let saved = $state(false);
	let version = $state('');

	// --- Agents ---
	let agents = $state<AgentSummary[]>([]);
	let agentPaths = $state<Record<string, string>>({});
	let defaultAgent = $state<string>('');
	let agentMsg = $state('');
	let agentErrors = $state<string[]>([]);

	const installed = $derived(agents.filter((a) => a.binary));

	async function loadAgents() {
		agents = await listAgents();
	}

	onMount(async () => {
		const s = await getSettings();
		worktreeLocation = s.worktreeLocation ?? 'sibling';
		globalHooksEnabled = s.globalHooksEnabled ?? true;
		agentPaths = { ...(s.agentPaths ?? {}) };
		defaultAgent = s.defaultAgent ?? '';
		await loadAgents();
		try {
			const res = await fetch('/api/version');
			if (res.ok) version = (await res.json()).version ?? '';
		} catch {
			/* version is cosmetic */
		}
	});

	async function browseAgent(id: string) {
		const p = await pickFile();
		if (p) agentPaths = { ...agentPaths, [id]: p };
	}

	async function reload() {
		agentMsg = '';
		try {
			agentErrors = await reloadAgents();
			await loadAgents();
			agentMsg = agentErrors.length
				? `${agentErrors.length} definition(s) failed to parse`
				: 'Definitions reloaded.';
		} catch (e) {
			agentMsg = String(e);
		}
		setTimeout(() => (agentMsg = ''), 4000);
	}

	async function revealAgents() {
		try {
			await openAgentsDir();
		} catch (e) {
			agentMsg = String(e);
		}
	}

	async function save() {
		// Drop blank overrides so they mean "auto-detect" rather than "this empty path".
		const paths = Object.fromEntries(
			Object.entries(agentPaths).filter(([, v]) => v.trim())
		);
		await setSettings({
			agentPaths: paths,
			defaultAgent: defaultAgent || null,
			worktreeLocation,
			globalHooksEnabled
		});
		agentPaths = paths;
		await loadAgents();
		saved = true;
		setTimeout(() => (saved = false), 1500);
	}

	let hooksMsg = $state('');
	async function openHooksFolder() {
		try {
			await openGlobalHooksDir();
		} catch (e) {
			hooksMsg = String(e);
		}
	}

	function close() {
		showSettings.set(false);
	}
</script>

<div class="overlay" onclick={close} role="presentation">
	<div class="panel" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
		<div class="head">
			<span>Settings</span>
			<button class="x" onclick={close} title="Close">×</button>
		</div>

		<div class="body">
			<div class="field">
				<div class="lbl">Agents</div>
				<div class="hint">
					Each agent is a <code>.toml</code> file describing how to drive one CLI. Edit or add
					your own in <code>~/.spwn/agents</code> — a change there takes effect on Reload, with
					no rebuild.
				</div>

				<div class="agents">
					{#each agents as a (a.id)}
						<div class="agent" class:missing={!a.binary}>
							<div class="a-head">
								<span class="a-icon">{a.icon ?? '✦'}</span>
								<span class="a-name">{a.name}</span>
								{#if a.untested}
									<span class="chip warn" title="Ships with spwn but has never been driven against the real CLI">experimental</span>
								{/if}
								<span class="chip scope">{a.scope === 'builtIn' ? 'built-in' : a.scope}</span>
								<span class="spacer"></span>
								{#if a.binary}
									<span class="chip ok">installed</span>
								{:else}
									<span class="chip miss">not found</span>
								{/if}
							</div>

							<div class="caps">
								{#each [['transcript', a.capabilities.transcript], ['status', a.capabilities.status], ['rewind', a.capabilities.rewind], ['scheduled', a.capabilities.headless]] as [label, on] (label)}
									<span class="cap" class:on>{on ? '✓' : '—'} {label}</span>
								{/each}
							</div>

							<div class="row">
								<input
									value={agentPaths[a.id] ?? ''}
									oninput={(e) => (agentPaths = { ...agentPaths, [a.id]: e.currentTarget.value })}
									placeholder={a.binary ?? `path to ${a.id}`}
									spellcheck="false" />
								<button class="browse" onclick={() => browseAgent(a.id)}>Browse…</button>
							</div>
							<div class="hint sm">
								{#if a.binary}
									Using <code>{a.binary}</code>. Leave blank to keep auto-detecting.
								{:else}
									Not on your <code>PATH</code> — set it here, or install the CLI.
								{/if}
							</div>
						</div>
					{/each}
				</div>

				{#if agentErrors.length}
					<div class="agent-errs">
						{#each agentErrors as e (e)}<div class="err-line">{e}</div>{/each}
					</div>
				{/if}

				<div class="row">
					<button class="browse" onclick={revealAgents}>Open ~/.spwn/agents</button>
					<button class="browse" onclick={reload}>Reload definitions</button>
					{#if agentMsg}<span class="hint sm">{agentMsg}</span>{/if}
				</div>
			</div>

			<div class="field">
				<div class="lbl">Default agent for new sessions</div>
				<select bind:value={defaultAgent}>
					<option value="">First installed ({installed[0]?.name ?? 'none'})</option>
					{#each installed as a (a.id)}
						<option value={a.id}>{a.name}</option>
					{/each}
				</select>
				<div class="hint">Used when you start a session without picking an agent.</div>
			</div>

			<div class="field">
				<div class="lbl">Session worktree location</div>
				<select bind:value={worktreeLocation}>
					<option value="sibling">Sibling folder (recommended)</option>
					<option value="internal">Inside repo (.spwn/worktrees)</option>
					<option value="appData">App data folder</option>
				</select>
				<div class="hint">
					{#if worktreeLocation === 'sibling'}
						Worktrees go in a dot-prefixed folder beside each repo
						(<code>../.&lt;repo&gt;-worktrees</code>) — outside the working tree, so builds,
						file watchers, and IDE indexers never see them.
					{:else if worktreeLocation === 'internal'}
						Worktrees go in <code>.spwn/worktrees</code> inside the repo, registered in
						<code>.git/info/exclude</code>. The dot-prefix keeps most tooling from scanning
						them, but tools with explicit include globs may still pick them up.
					{:else}
						Worktrees go under the app's data folder, away from your repos entirely.
					{/if}
				</div>
				<div class="hint">Applies to new sessions; existing worktrees stay where they are.</div>
			</div>

			<div class="field">
				<div class="lbl">Global hooks</div>
				<label class="toggle">
					<input type="checkbox" bind:checked={globalHooksEnabled} />
					<span>Run shared global hooks</span>
				</label>
				<div class="hint">
					Shared scripts in <code>~/.spwn/hooks</code> run for every session in every project
					(layered before each repo's own <code>.spwn/hooks</code>). spwn ships its built-in
					worktree create/remove and per-turn commit + checkpoint here as editable defaults.
					{#if !globalHooksEnabled}
						Disabled — only per-repo <code>.spwn/hooks</code> run. spwn no longer manages
						worktrees: new sessions run in the project folder with no isolated worktree or
						branch, existing session worktrees aren't auto-removed on delete, and the shared
						per-turn commit + checkpoint won't run.
					{/if}
				</div>
				<div class="row">
					<button class="browse" onclick={openHooksFolder}>Open hooks folder…</button>
				</div>
				{#if hooksMsg}<div class="hint">{hooksMsg}</div>{/if}
			</div>

			<div class="field">
				<div class="lbl">Version</div>
				<div class="version">spwn {version ? `v${version}` : ''}</div>
			</div>
		</div>

		<div class="foot">
			{#if saved}<span class="ok">Saved ✓</span>{/if}
			<button class="primary" onclick={save}>Save</button>
			<button onclick={close}>Close</button>
		</div>
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
	}
	.panel {
		width: 560px;
		max-width: 90vw;
		background: var(--bg);
		border: 1px solid var(--border-strong);
		border-radius: 10px;
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
		display: flex;
		flex-direction: column;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid #2c2c2c;
		font-weight: 600;
		color: #e6e6e6;
	}
	.x {
		background: none;
		border: none;
		color: #999;
		font-size: 18px;
		cursor: pointer;
	}
	.x:hover {
		color: #fff;
	}
	.body {
		padding: 16px;
	}
	.field + .field {
		margin-top: 20px;
		padding-top: 18px;
		border-top: 1px solid #2c2c2c;
	}
	.lbl {
		font-size: 13px;
		color: #cfcfcf;
		margin-bottom: 6px;
	}
	.version {
		flex: 1 1 auto;
		align-self: center;
		font-size: 13px;
		color: #9a9a9a;
	}
	.row {
		display: flex;
		gap: 8px;
	}
	.toggle {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		color: #e6e6e6;
		cursor: pointer;
		margin-bottom: 2px;
	}
	.toggle input {
		width: 15px;
		height: 15px;
		cursor: pointer;
		accent-color: var(--accent);
	}
	.row input {
		flex: 1 1 auto;
		background: #161616;
		border: 1px solid #3a3a3a;
		border-radius: 6px;
		color: #e6e6e6;
		padding: 8px 10px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 13px;
	}
	.body select {
		width: 100%;
		background: #161616;
		border: 1px solid #3a3a3a;
		border-radius: 6px;
		color: #e6e6e6;
		padding: 8px 10px;
		font-size: 13px;
		cursor: pointer;
	}
	.browse {
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 6px;
		padding: 0 12px;
		cursor: pointer;
	}
	.browse:hover {
		background: #333;
		color: #fff;
	}
	.agents {
		display: flex;
		flex-direction: column;
		gap: 10px;
		margin: 8px 0;
	}
	.agent {
		border: 1px solid var(--border, #2a2a2a);
		border-radius: 6px;
		padding: 8px 10px;
	}
	.agent.missing {
		opacity: 0.72;
	}
	.a-head {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-bottom: 6px;
	}
	.a-name {
		font-weight: 600;
	}
	.spacer {
		flex: 1;
	}
	.chip {
		font-size: 10px;
		padding: 1px 6px;
		border-radius: 999px;
		border: 1px solid var(--border, #2a2a2a);
		color: var(--fg-dim, #9a9a9a);
	}
	.chip.ok {
		color: #7fb069;
		border-color: #3c5a30;
	}
	.chip.miss {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.chip.warn {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.caps {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin-bottom: 6px;
	}
	.cap {
		font-size: 11px;
		color: var(--fg-dim, #7a7a7a);
	}
	.cap.on {
		color: var(--fg, #cfcfcf);
	}
	.agent-errs {
		border: 1px solid #5c2a2a;
		border-radius: 6px;
		padding: 6px 8px;
		margin: 6px 0;
	}
	.err-line {
		font-size: 11px;
		color: #e06c75;
		white-space: pre-wrap;
	}
	.hint.sm {
		font-size: 11px;
	}
	.hint {
		font-size: 11px;
		color: #777;
		margin-top: 6px;
	}
	.hint code {
		color: #9bbf8a;
	}
	.foot {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 10px;
		padding: 12px 16px;
		border-top: 1px solid #2c2c2c;
	}
	.ok {
		color: #9bbf8a;
		font-size: 12px;
		margin-right: auto;
	}
	.foot button {
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 6px;
		padding: 6px 14px;
		cursor: pointer;
	}
	.foot .primary {
		background: var(--accent);
		border-color: var(--accent-border);
		color: #fff;
	}
	.foot button:hover {
		filter: brightness(1.2);
	}
</style>
