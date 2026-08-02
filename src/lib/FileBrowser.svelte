<script lang="ts">
	// Global modal that fulfils pickFile()/pickDirectory() by browsing the host
	// filesystem via /api/fs/list. Mounted once in +page.svelte.
	import { fileBrowserRequest, fsList, type FsEntry } from './ipc';

	let dir = $state<string>('');
	let parent = $state<string | null>(null);
	let entries = $state<FsEntry[]>([]);
	let error = $state<string>('');
	let loading = $state(false);

	// The active request (null when the modal is closed).
	let req = $state<import('./ipc').FileBrowserRequest | null>(null);
	fileBrowserRequest.subscribe((r) => {
		req = r;
		if (r) void load(null);
	});

	const wantFiles = $derived(req ? !req.directory : false);

	async function load(path: string | null) {
		loading = true;
		error = '';
		try {
			const listing = await fsList(path, wantFiles);
			dir = listing.path;
			parent = listing.parent;
			entries = listing.entries;
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function choose(path: string | null) {
		req?.resolve(path);
		fileBrowserRequest.set(null);
	}

	function pick(entry: FsEntry) {
		if (entry.isDir) void load(entry.path);
		else choose(entry.path); // file mode: selecting a file confirms it
	}
</script>

{#if req}
	<div class="overlay" onclick={() => choose(null)} role="presentation">
		<div class="panel" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
			<div class="head">
				<strong>{req.directory ? 'Choose a folder' : 'Choose a file'}</strong>
				<button class="x" onclick={() => choose(null)} aria-label="Cancel">✕</button>
			</div>
			<div class="path">{dir || '…'}</div>
			<div class="list">
				{#if parent !== null}
					<button class="row up" onclick={() => load(parent)}>⬆ ..</button>
				{/if}
				{#if loading}
					<div class="muted">Loading…</div>
				{:else if error}
					<div class="err">{error}</div>
				{:else if entries.length === 0}
					<div class="muted">Empty folder</div>
				{:else}
					{#each entries as entry (entry.path)}
						<button class="row" onclick={() => pick(entry)}>
							<span class="icon">{entry.isDir ? '📁' : '📄'}</span>
							<span class="name">{entry.name}</span>
						</button>
					{/each}
				{/if}
			</div>
			<div class="foot">
				<button class="cancel" onclick={() => choose(null)}>Cancel</button>
				{#if req.directory}
					<button class="confirm" onclick={() => choose(dir)} disabled={!dir}>
						Choose this folder
					</button>
				{/if}
			</div>
		</div>
	</div>
{/if}

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
		width: min(560px, 92vw);
		max-height: 80vh;
		display: flex;
		flex-direction: column;
		background: var(--bg, #1e1e1e);
		color: var(--fg, #ddd);
		border: 1px solid var(--border, #3a3a3a);
		border-radius: 8px;
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
		overflow: hidden;
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
	.path {
		padding: 6px 12px;
		font-family: ui-monospace, monospace;
		font-size: 12px;
		opacity: 0.7;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.list {
		flex: 1;
		overflow-y: auto;
		padding: 4px 0;
	}
	.row {
		display: flex;
		gap: 8px;
		align-items: center;
		width: 100%;
		padding: 6px 12px;
		background: none;
		border: none;
		color: inherit;
		text-align: left;
		cursor: pointer;
		font-size: 13px;
	}
	.row:hover {
		background: var(--hover, rgba(255, 255, 255, 0.06));
	}
	.up {
		opacity: 0.8;
	}
	.name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.muted {
		padding: 12px;
		opacity: 0.6;
		font-size: 13px;
	}
	.err {
		padding: 12px;
		color: #e06c75;
		font-size: 13px;
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding: 10px 12px;
		border-top: 1px solid var(--border, #3a3a3a);
	}
	.foot button {
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
	.foot .confirm:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
