<script lang="ts">
	import { onMount } from 'svelte';
	import { getSettings, setSettings, detectClaude, pickFile, openGlobalHooksDir } from './ipc';
	import { showSettings } from './stores';
	import type { WorktreeLocation } from './types';

	let claudePath = $state('');
	let worktreeLocation = $state<WorktreeLocation>('sibling');
	let globalHooksEnabled = $state(true);
	let detected = $state<string | null>(null);
	let saved = $state(false);
	let version = $state('');

	onMount(async () => {
		const s = await getSettings();
		claudePath = s.claudePath ?? '';
		worktreeLocation = s.worktreeLocation ?? 'sibling';
		globalHooksEnabled = s.globalHooksEnabled ?? true;
		detected = await detectClaude();
		try {
			const res = await fetch('/api/version');
			if (res.ok) version = (await res.json()).version ?? '';
		} catch {
			/* version is cosmetic */
		}
	});

	async function browse() {
		const p = await pickFile();
		if (p) claudePath = p;
	}

	async function save() {
		await setSettings({
			claudePath: claudePath.trim() || null,
			worktreeLocation,
			globalHooksEnabled
		});
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
				<div class="lbl">Claude CLI path</div>
				<div class="row">
					<input bind:value={claudePath} placeholder={detected ?? '/path/to/claude'} spellcheck="false" />
					<button class="browse" onclick={browse}>Browse…</button>
				</div>
				<div class="hint">
					{#if detected}
						Auto-detected: <code>{detected}</code>
					{:else}
						No <code>claude</code> auto-detected — set its path here.
					{/if}
				</div>
				<div class="hint">Leave blank to use the auto-detected path.</div>
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
