<script lang="ts">
	import { pendingUpdate, installProgress, installUpdate, dismissUpdate } from './updater';

	let installing = $state(false);

	async function install() {
		const update = $pendingUpdate;
		if (!update || installing) return;
		installing = true;
		try {
			await installUpdate(update); // relaunches on success
		} catch {
			installing = false; // error surfaced via updateStatus; let the user retry/dismiss
		}
	}

	const pct = $derived(
		$installProgress === null || $installProgress === undefined
			? null
			: Math.round($installProgress * 100)
	);
</script>

{#if $pendingUpdate}
	<div class="update-banner" role="dialog" aria-label="Update available">
		<div class="info">
			<span class="badge">Update</span>
			<span class="ver">Version {$pendingUpdate.version} is available</span>
			{#if installing}
				<span class="prog">{pct === null ? 'Installing…' : `Downloading ${pct}%`}</span>
			{/if}
		</div>
		<div class="actions">
			{#if installing}
				<div class="bar"><div class="fill" style="width: {pct ?? 30}%"></div></div>
			{:else}
				<button class="primary" onclick={install}>Install &amp; Restart</button>
				<button class="ghost" onclick={dismissUpdate}>Later</button>
			{/if}
		</div>
	</div>
{/if}

<style>
	.update-banner {
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		padding: 7px 12px 7px 14px;
		background: var(--accent);
		border-bottom: 1px solid var(--accent-border);
		color: #fff;
		font-size: 13px;
	}
	.info {
		display: flex;
		align-items: center;
		gap: 10px;
		min-width: 0;
	}
	.badge {
		flex: 0 0 auto;
		background: rgba(255, 255, 255, 0.18);
		border-radius: 4px;
		padding: 1px 7px;
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}
	.ver {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.prog {
		color: #d6e2f5;
		font-size: 12px;
	}
	.actions {
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.primary {
		background: #fff;
		color: var(--accent);
		border: none;
		border-radius: 5px;
		padding: 4px 12px;
		font-weight: 600;
		font-size: 12px;
		cursor: pointer;
	}
	.primary:hover {
		background: #eef3fb;
	}
	.ghost {
		background: transparent;
		color: #dce6f6;
		border: 1px solid rgba(255, 255, 255, 0.35);
		border-radius: 5px;
		padding: 4px 10px;
		font-size: 12px;
		cursor: pointer;
	}
	.ghost:hover {
		background: rgba(255, 255, 255, 0.12);
		color: #fff;
	}
	.bar {
		width: 160px;
		height: 6px;
		background: rgba(255, 255, 255, 0.25);
		border-radius: 3px;
		overflow: hidden;
	}
	.fill {
		height: 100%;
		background: #fff;
		transition: width 0.2s ease;
	}
</style>
