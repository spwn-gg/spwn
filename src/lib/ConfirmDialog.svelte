<script lang="ts">
	// Global confirm modal. Mounted once (in +page.svelte); driven by the
	// `confirmState` store via the `confirmDialog()` promise helper. Replaces the
	// native confirm() so destructive flows can name exactly what's at stake.
	import { confirmState, type ConfirmResult } from './stores';

	function answer(r: ConfirmResult) {
		const cur = $confirmState;
		confirmState.set(null);
		cur?.resolve(r);
	}

	function onKey(e: KeyboardEvent) {
		if (!$confirmState) return;
		if (e.key === 'Escape') {
			e.preventDefault();
			answer('cancel');
		} else if (e.key === 'Enter') {
			e.preventDefault();
			answer('confirm');
		}
	}
</script>

<svelte:window on:keydown={onKey} />

{#if $confirmState}
	{@const o = $confirmState.opts}
	<div class="overlay" role="presentation" onclick={() => answer('cancel')}>
		<div
			class="panel"
			role="alertdialog"
			aria-modal="true"
			aria-label={o.title}
			tabindex="-1"
			onclick={(e) => e.stopPropagation()}>
			<div class="title">{o.title}</div>
			<div class="body">{o.body}</div>
			{#if o.rows?.length}
				<div class="rows">
					{#each o.rows as r (r.label)}
						<div class="stake" class:danger={r.danger}>
							<span class="k">{r.label}</span>
							<span class="v">{r.value}</span>
						</div>
					{/each}
				</div>
			{/if}
			<div class="foot">
				<button class="ghost" onclick={() => answer('cancel')}>Cancel</button>
				{#if o.secondaryLabel}
					<button class="secondary" onclick={() => answer('secondary')}>{o.secondaryLabel}</button>
				{/if}
				<button class="primary" class:danger={o.danger ?? true} onclick={() => answer('confirm')}>
					{o.confirmLabel ?? 'Delete'}
				</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 200;
	}
	.panel {
		width: 460px;
		max-width: 90vw;
		background: var(--bg);
		border: 1px solid var(--border-strong);
		border-radius: 10px;
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.55);
		padding: 18px 18px 14px;
		outline: none;
	}
	.title {
		font-size: 15px;
		font-weight: 600;
		color: var(--text);
		margin-bottom: 8px;
	}
	.body {
		font-size: 13px;
		line-height: 1.5;
		color: var(--text-dim);
		white-space: pre-wrap;
	}
	.rows {
		margin-top: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}
	.stake {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		padding: 7px 11px;
		font-size: 12px;
		background: var(--surface);
	}
	.stake + .stake {
		border-top: 1px solid var(--border);
	}
	.stake .k {
		color: var(--text-muted);
	}
	.stake .v {
		color: var(--text);
		text-align: right;
		font-family: ui-monospace, Menlo, monospace;
	}
	.stake.danger .v {
		color: var(--danger);
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 16px;
	}
	.foot button {
		border-radius: var(--radius);
		padding: 6px 14px;
		font-size: 13px;
		cursor: pointer;
		border: 1px solid var(--border-strong);
	}
	.ghost {
		background: var(--bg-elevated);
		color: var(--text-dim);
	}
	.ghost:hover {
		color: #fff;
	}
	.secondary {
		background: var(--accent);
		border-color: var(--accent-border);
		color: #fff;
	}
	.primary {
		background: var(--bg-elevated);
		color: var(--text);
	}
	.primary.danger {
		background: var(--danger-bg);
		border-color: #7a3a3a;
		color: #fff;
	}
	.foot button:hover {
		filter: brightness(1.15);
	}
</style>
