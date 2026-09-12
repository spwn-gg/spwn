<script lang="ts">
	// "Clone repo" modal for New Project: takes a GitHub URL / owner/repo and a parent
	// folder, clones into <parent>/<repo-name>, and adds the clone as a project.
	import { cloneProject, fsList, pickDirectory } from './ipc';
	import type { ProjectRec } from './types';

	let { onclose, oncloned }: { onclose: () => void; oncloned: (p: ProjectRec) => void } =
		$props();

	let url = $state('');
	let parentDir = $state('');
	let busy = $state(false);
	let error = $state('');

	// Default the destination to the home dir (what the folder browser opens on).
	fsList(null, false)
		.then((l) => {
			if (!parentDir) parentDir = l.path;
		})
		.catch(() => {});

	const repoName = $derived(
		url
			.trim()
			.replace(/\/+$/, '')
			.split(/[/:]/)
			.pop()
			?.replace(/\.git$/, '') ?? ''
	);

	async function browse() {
		const dir = await pickDirectory();
		if (dir) parentDir = dir;
	}

	async function submit(e: Event) {
		e.preventDefault();
		if (busy || !url.trim() || !parentDir) return;
		busy = true;
		error = '';
		try {
			oncloned(await cloneProject(url, parentDir));
		} catch (err) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			busy = false;
		}
	}

	function onKey(e: KeyboardEvent) {
		if (e.key === 'Escape' && !busy) onclose();
	}
</script>

<svelte:window onkeydown={onKey} />

<div
	class="overlay"
	role="presentation"
	onclick={(e) => e.target === e.currentTarget && !busy && onclose()}>
	<div
		class="panel"
		role="dialog"
		aria-modal="true"
		aria-label="Clone a repository"
		tabindex="-1">
	<form onsubmit={submit}>
		<div class="head">
			<strong>Clone a repository</strong>
			<button type="button" class="x" onclick={onclose} disabled={busy} aria-label="Cancel">✕</button>
		</div>
		<div class="body">
			<label>
				<span>Repository</span>
				<!-- svelte-ignore a11y_autofocus -->
				<input
					bind:value={url}
					placeholder="owner/repo or https://github.com/owner/repo"
					autofocus
					spellcheck="false"
					disabled={busy} />
			</label>
			<label>
				<span>Clone into</span>
				<div class="dest">
					<input bind:value={parentDir} spellcheck="false" disabled={busy} />
					<button type="button" onclick={browse} disabled={busy}>Browse…</button>
				</div>
			</label>
			{#if repoName && parentDir}
				<div class="hint">→ {parentDir.replace(/\/+$/, '')}/{repoName}</div>
			{/if}
			{#if error}
				<div class="err">{error}</div>
			{/if}
		</div>
		<div class="foot">
			<button type="button" onclick={onclose} disabled={busy}>Cancel</button>
			<button type="submit" class="confirm" disabled={busy || !url.trim() || !parentDir}>
				{busy ? 'Cloning…' : 'Clone'}
			</button>
		</div>
	</form>
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.45);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 1000;
	}
	.panel {
		width: min(520px, 92vw);
		display: flex;
		flex-direction: column;
		background: var(--bg, #1e1e1e);
		color: var(--fg, #ddd);
		border: 1px solid var(--border, #3a3a3a);
		border-radius: 8px;
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
		overflow: hidden;
	}
	form {
		display: contents;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 12px;
		border-bottom: 1px solid var(--border, #3a3a3a);
	}
	.x {
		background: none;
		border: none;
		color: inherit;
		cursor: pointer;
		font-size: 14px;
	}
	.body {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 12px;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 12px;
	}
	label > span {
		opacity: 0.7;
	}
	input {
		flex: 1;
		min-width: 0;
		padding: 6px 8px;
		border-radius: 6px;
		border: 1px solid var(--border, #3a3a3a);
		background: var(--btn, #2a2a2a);
		color: inherit;
		font-family: ui-monospace, monospace;
		font-size: 12px;
	}
	.dest {
		display: flex;
		gap: 6px;
	}
	.hint {
		font-family: ui-monospace, monospace;
		font-size: 11px;
		opacity: 0.6;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.err {
		color: #e06c75;
		font-size: 12px;
		white-space: pre-wrap;
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding: 10px 12px;
		border-top: 1px solid var(--border, #3a3a3a);
	}
	button {
		padding: 6px 12px;
		border-radius: 6px;
		border: 1px solid var(--border, #3a3a3a);
		background: var(--btn, #2a2a2a);
		color: inherit;
		cursor: pointer;
		font-size: 13px;
	}
	.foot .confirm {
		background: var(--accent, #3b82f6);
		border-color: var(--accent, #3b82f6);
		color: #fff;
	}
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
