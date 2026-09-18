<script lang="ts">
	// The app-wide settings page. It lives in the tab bar like any other pane (see
	// PaneManager), not in a modal — the agent list grows with every .toml in
	// ~/.spwn/agents, so it needs a scroller of its own rather than a centred box.
	import { onMount, untrack } from 'svelte';
	import { base } from '$app/paths';
	import {
		getSettings,
		setSettings,
		pickFile,
		openGlobalHooksDir,
		listAgents,
		reloadAgents,
		openAgentsDir
	} from './ipc';
	import GitHubTokenField from './GitHubTokenField.svelte';
	import { activeTabKey } from './stores';
	import type { WorktreeLocation, AgentSummary } from './types';

	let { tabKey }: { tabKey: string } = $props();

	const SECTIONS = [
		{ id: 'agents', label: 'Agents' },
		{ id: 'sessions', label: 'Sessions' },
		{ id: 'hooks', label: 'Hooks' },
		{ id: 'github', label: 'GitHub' },
		{ id: 'about', label: 'About' }
	] as const;
	type SectionId = (typeof SECTIONS)[number]['id'];
	let section = $state<SectionId>('agents');

	let worktreeLocation = $state<WorktreeLocation>('sibling');
	let globalHooksEnabled = $state(true);
	let saved = $state(false);
	let version = $state('');

	/** Set by every editable control, cleared on save. Guards the refetch below from
	 * overwriting edits you haven't committed yet. */
	let dirty = $state(false);

	// The field saves and removes on its own, not with the Save button; this only
	// tracks the result so the header can show the "token saved" chip.
	let tokenSaved = $state(false);

	// --- Agents ---
	let agents = $state<AgentSummary[]>([]);
	let agentPaths = $state<Record<string, string>>({});
	let defaultAgent = $state<string>('');
	let agentMsg = $state('');
	let agentErrors = $state<string[]>([]);

	const installed = $derived(agents.filter((a) => a.binary));
	const missing = $derived(agents.filter((a) => !a.binary).length);

	async function loadAgents() {
		agents = await listAgents();
	}

	async function loadSettings() {
		const s = await getSettings();
		worktreeLocation = s.worktreeLocation ?? 'sibling';
		globalHooksEnabled = s.globalHooksEnabled ?? true;
		agentPaths = { ...(s.agentPaths ?? {}) };
		defaultAgent = s.defaultAgent ?? '';
	}

	onMount(async () => {
		try {
			const res = await fetch(`${base}/api/version`);
			if (res.ok) version = (await res.json()).version ?? '';
		} catch {
			/* version is cosmetic */
		}
	});

	// Panes are kept alive, so onMount would only ever run once and the page would go
	// stale behind you (an agent installed in a shell, hooks changed on disk). Reload
	// whenever this tab is brought to the front instead — which also covers first open.
	$effect(() => {
		if ($activeTabKey !== tabKey) return;
		untrack(() => {
			loadAgents();
			if (!dirty) loadSettings();
		});
	});

	async function browseAgent(id: string) {
		const p = await pickFile();
		if (p) {
			agentPaths = { ...agentPaths, [id]: p };
			dirty = true;
		}
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
		dirty = false;
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
</script>

<div class="settings">
	<div class="bar">
		<span class="title">Settings</span>
		{#if saved}<span class="ok">Saved ✓</span>{:else if dirty}<span class="pending">Unsaved changes</span>{/if}
		<button class="primary" onclick={save}>Save</button>
	</div>

	<div class="body">
		<div class="rail" role="tablist" aria-label="Settings sections" aria-orientation="vertical">
			{#each SECTIONS as s (s.id)}
				<button
					role="tab"
					id="set-tab-{s.id}"
					aria-selected={section === s.id}
					aria-controls="set-panel"
					class:on={section === s.id}
					onclick={() => (section = s.id)}>
					<span class="r-label">{s.label}</span>
					{#if s.id === 'agents' && agents.length}
						<span class="r-count" class:warn={missing > 0}>{installed.length}/{agents.length}</span>
					{/if}
					{#if s.id === 'github' && tokenSaved}<span class="r-dot" title="Token saved"></span>{/if}
				</button>
			{/each}
		</div>

		<div
			class="content"
			id="set-panel"
			role="tabpanel"
			tabindex="0"
			aria-labelledby="set-tab-{section}">
			<div class="inner">
				{#if section === 'agents'}
					<h2 class="sec-title">Agents</h2>
					<p class="sec-desc">
						Each agent is a <code>.toml</code> file describing how to drive one CLI. Edit or add
						your own in <code>~/.spwn/agents</code> — a change there takes effect on Reload, with
						no rebuild.
					</p>

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
										oninput={(e) => {
											agentPaths = { ...agentPaths, [a.id]: e.currentTarget.value };
											dirty = true;
										}}
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
						{#if agentMsg}<span class="hint sm inline">{agentMsg}</span>{/if}
					</div>

					<div class="field">
						<div class="lbl">Default agent for new sessions</div>
						<select bind:value={defaultAgent} onchange={() => (dirty = true)}>
							<option value="">First installed ({installed[0]?.name ?? 'none'})</option>
							{#each installed as a (a.id)}
								<option value={a.id}>{a.name}</option>
							{/each}
						</select>
						<div class="hint">Used when you start a session without picking an agent.</div>
					</div>
				{:else if section === 'sessions'}
					<h2 class="sec-title">Sessions</h2>
					<p class="sec-desc">How spwn lays out the working copy each new session runs in.</p>

					<div class="field">
						<div class="lbl">Session worktree location</div>
						<select bind:value={worktreeLocation} onchange={() => (dirty = true)}>
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
				{:else if section === 'hooks'}
					<h2 class="sec-title">Hooks</h2>
					<p class="sec-desc">
						Shared scripts in <code>~/.spwn/hooks</code> run for every session in every project,
						layered before each repo's own <code>.spwn/hooks</code>.
					</p>

					<div class="field">
						<div class="lbl">Global hooks</div>
						<label class="toggle">
							<input
								type="checkbox"
								bind:checked={globalHooksEnabled}
								onchange={() => (dirty = true)} />
							<span>Run shared global hooks</span>
						</label>
						<div class="hint">
							spwn ships its built-in worktree create/remove and per-turn commit + checkpoint
							here as editable defaults.
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
				{:else if section === 'github'}
					<h2 class="sec-title">
						GitHub
						{#if tokenSaved}<span class="chip ok">token saved</span>{/if}
					</h2>
					<p class="sec-desc">
						Saved as soon as you press Save token — this one doesn't wait for the Save button
						above.
					</p>

					<div class="field">
						<GitHubTokenField onchange={(s) => (tokenSaved = s)} />
					</div>
				{:else}
					<h2 class="sec-title">About</h2>
					<div class="field">
						<div class="lbl">Version</div>
						<div class="version">spwn {version ? `v${version}` : ''}</div>
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>

<style>
	.settings {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
		background: var(--bg-sidebar);
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px 14px;
		border-bottom: 1px solid var(--border);
		flex: 0 0 auto;
	}
	.title {
		flex: 1 1 auto;
		font-weight: 600;
		color: var(--text);
	}
	.ok {
		color: var(--ok);
		font-size: 12px;
	}
	.pending {
		color: var(--text-muted);
		font-size: 12px;
	}
	.bar .primary {
		background: var(--accent);
		border: 1px solid var(--accent-border);
		color: #fff;
		border-radius: var(--radius);
		padding: 5px 14px;
		cursor: pointer;
		font-size: 12px;
	}
	.bar .primary:hover {
		filter: brightness(1.2);
	}

	/* The two-column body: a fixed rail, and the one region that scrolls. */
	.body {
		flex: 1 1 auto;
		display: flex;
		min-height: 0;
	}
	.rail {
		flex: 0 0 176px;
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 10px 8px;
		overflow-y: auto;
		border-right: 1px solid var(--border);
	}
	.rail button {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		text-align: left;
		background: none;
		border: 1px solid transparent;
		color: var(--text-dim);
		border-radius: var(--radius);
		padding: 6px 10px;
		font-size: 13px;
		cursor: pointer;
	}
	.rail button:hover {
		color: var(--text);
		background: var(--bg-hover);
	}
	.rail button.on {
		background: var(--bg-elevated);
		border-color: var(--border-strong);
		color: var(--text);
	}
	.r-label {
		flex: 1 1 auto;
	}
	.r-count {
		font-size: 10px;
		color: var(--text-muted);
		border: 1px solid var(--border);
		border-radius: 999px;
		padding: 0 5px;
	}
	.r-count.warn {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.r-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--ok);
	}

	.content {
		flex: 1 1 auto;
		min-width: 0;
		overflow-y: auto;
		background: var(--bg);
		padding: 18px 22px 40px;
	}
	.content:focus-visible {
		outline: none;
	}
	.inner {
		max-width: 640px;
	}
	.sec-title {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 0;
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--text-dim);
	}
	.sec-desc {
		margin: 8px 0 0;
		font-size: 12px;
		line-height: 1.55;
		color: var(--text-muted);
	}
	.sec-desc code,
	.hint code {
		color: var(--ok);
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11px;
	}
	.field {
		margin-top: 18px;
		padding-top: 16px;
		border-top: 1px solid var(--border);
	}
	.lbl {
		font-size: 13px;
		color: var(--text);
		margin-bottom: 6px;
	}
	.version {
		font-size: 13px;
		color: var(--text-dim);
	}
	.row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.hint + .row {
		margin-top: 10px;
	}
	.toggle {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		color: var(--text);
		cursor: pointer;
	}
	.toggle input {
		width: 15px;
		height: 15px;
		cursor: pointer;
		accent-color: var(--accent);
	}
	.row input {
		flex: 1 1 auto;
		min-width: 0;
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		color: var(--text);
		padding: 8px 10px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 13px;
	}
	select {
		width: 100%;
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		color: var(--text);
		padding: 8px 10px;
		font-size: 13px;
		cursor: pointer;
	}
	.browse {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
		border-radius: var(--radius);
		padding: 7px 12px;
		font-size: 12px;
		cursor: pointer;
		white-space: nowrap;
	}
	.browse:hover {
		background: var(--bg-hover);
		color: var(--text);
	}
	.agents {
		display: flex;
		flex-direction: column;
		gap: 10px;
		margin: 14px 0 12px;
	}
	.agent {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--bg-elevated);
		padding: 10px 12px;
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
		color: var(--text);
	}
	.spacer {
		flex: 1;
	}
	.chip {
		font-size: 10px;
		padding: 1px 6px;
		border-radius: 999px;
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
		text-transform: none;
		letter-spacing: 0;
		font-weight: 400;
	}
	.chip.ok {
		color: #7fb069;
		border-color: #3c5a30;
	}
	.chip.miss,
	.chip.warn {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.caps {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin-bottom: 8px;
	}
	.cap {
		font-size: 11px;
		color: var(--text-muted);
	}
	.cap.on {
		color: var(--text-dim);
	}
	.agent-errs {
		border: 1px solid #5c2a2a;
		border-radius: var(--radius);
		padding: 6px 8px;
		margin: 6px 0 12px;
	}
	.err-line {
		font-size: 11px;
		color: #e06c75;
		white-space: pre-wrap;
	}
	.hint {
		font-size: 11px;
		line-height: 1.55;
		color: var(--text-muted);
		margin-top: 6px;
	}
	.hint.sm {
		font-size: 11px;
	}
	.hint.inline {
		margin-top: 0;
	}
</style>
