<script lang="ts">
	import type { PermissionReq } from './types';

	let { req, onAllow, onDeny }: {
		req: PermissionReq;
		onAllow: (id: string) => void;
		onDeny: (id: string) => void;
	} = $props();

	// A compact one-line summary of the tool input (command / path / first field).
	function summary(input: unknown): string {
		if (input == null) return '';
		if (typeof input === 'string') return input;
		const o = input as Record<string, unknown>;
		const pick = o.command ?? o.file_path ?? o.path ?? o.pattern ?? o.url ?? o.description;
		if (typeof pick === 'string') return pick;
		try {
			return JSON.stringify(input);
		} catch {
			return '';
		}
	}
</script>

<div class="perm">
	<div class="info">
		<span class="badge">permission</span>
		<span class="tool">{req.title ?? req.tool}</span>
		<span class="sum">{summary(req.input)}</span>
	</div>
	<div class="actions">
		<button class="deny" onclick={() => onDeny(req.id)}>Deny</button>
		<button class="allow" onclick={() => onAllow(req.id)}>Allow</button>
	</div>
</div>

<style>
	.perm {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 8px 12px;
		margin: 6px 10px;
		background: #2a2616;
		border: 1px solid #5a4a1a;
		border-radius: var(--radius-lg);
	}
	.info {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
	}
	.badge {
		flex: 0 0 auto;
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		padding: 1px 6px;
		border-radius: 4px;
		background: #5a4a1a;
		color: #e8d48a;
	}
	.tool {
		flex: 0 0 auto;
		color: #e8d48a;
		font-weight: 600;
	}
	.sum {
		flex: 1 1 auto;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: #b8b0a0;
		font-family: ui-monospace, Menlo, monospace;
	}
	.actions {
		flex: 0 0 auto;
		display: flex;
		gap: 6px;
	}
	.actions button {
		border-radius: 5px;
		padding: 4px 12px;
		font-size: 12px;
		cursor: pointer;
		border: 1px solid var(--border-strong);
	}
	.allow {
		background: #2a4a2a;
		border-color: #3a6a3a;
		color: #cfe8cf;
	}
	.allow:hover {
		filter: brightness(1.2);
	}
	.deny {
		background: var(--bg-elevated);
		color: var(--danger);
	}
	.deny:hover {
		background: var(--danger-bg);
		color: #fff;
	}
</style>
