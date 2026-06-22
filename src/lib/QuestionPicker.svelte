<script lang="ts">
	import type { PendingQuestion } from './types';

	let { pending, onAnswer }: { pending: PendingQuestion; onAnswer: (id: string, text: string) => void } =
		$props();

	// selections[i] = set of chosen labels for question i.
	let selections = $state<Set<string>[]>(pending.questions.map(() => new Set<string>()));

	const singleQuick = $derived(
		pending.questions.length === 1 && !pending.questions[0].multiSelect
	);

	function toggle(qi: number, label: string) {
		const q = pending.questions[qi];
		const next = pending.questions.map((_, i) => new Set(selections[i]));
		if (q.multiSelect) {
			next[qi].has(label) ? next[qi].delete(label) : next[qi].add(label);
		} else {
			next[qi] = new Set([label]);
		}
		selections = next;
		if (singleQuick) submit(); // one single-select question → answer on click
	}

	const canSubmit = $derived(selections.every((s) => s.size > 0));

	function submit() {
		const lines = pending.questions.map((q, i) => {
			const picked = [...selections[i]];
			return `${q.question} → ${picked.length ? picked.join(', ') : '(no selection)'}`;
		});
		onAnswer(pending.id, lines.join('\n'));
	}
</script>

<div class="picker">
	{#each pending.questions as q, qi (qi)}
		<div class="q">
			<div class="q-head">
				{#if q.header}<span class="q-tag">{q.header}</span>{/if}
				<span class="q-text">{q.question}</span>
				{#if q.multiSelect}<span class="q-multi">choose any</span>{/if}
			</div>
			<div class="opts">
				{#each q.options as o (o.label)}
					<button
						class="opt"
						class:sel={selections[qi].has(o.label)}
						title={o.description}
						onclick={() => toggle(qi, o.label)}>
						<span class="opt-label">{o.label}</span>
						{#if o.description}<span class="opt-desc">{o.description}</span>{/if}
					</button>
				{/each}
			</div>
		</div>
	{/each}
	{#if !singleQuick}
		<div class="actions">
			<button class="send" disabled={!canSubmit} onclick={submit}>Send answer</button>
		</div>
	{/if}
</div>

<style>
	.picker {
		margin: 6px 10px;
		padding: 10px 12px;
		background: #1b2230;
		border: 1px solid #34507a;
		border-radius: var(--radius-lg);
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.q-head {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 8px;
		margin-bottom: 7px;
	}
	.q-tag {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		padding: 1px 6px;
		border-radius: 4px;
		background: #2a3344;
		color: #9bbce0;
	}
	.q-text {
		flex: 1 1 auto;
		color: var(--text);
		font-size: 13px;
		font-weight: 600;
	}
	.q-multi {
		color: var(--text-muted);
		font-size: 11px;
	}
	.opts {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.opt {
		display: flex;
		flex-direction: column;
		gap: 2px;
		text-align: left;
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius);
		padding: 7px 10px;
		cursor: pointer;
		color: var(--text);
	}
	.opt:hover {
		border-color: var(--accent-line);
		background: #232b38;
	}
	.opt.sel {
		border-color: var(--accent-text);
		background: #233047;
	}
	.opt-label {
		font-size: 13px;
		font-weight: 600;
	}
	.opt-desc {
		font-size: 12px;
		color: #9fb0c8;
	}
	.actions {
		display: flex;
		justify-content: flex-end;
	}
	.send {
		background: var(--accent);
		border: 1px solid var(--accent-border);
		color: #fff;
		border-radius: var(--radius);
		padding: 5px 14px;
		font-size: 13px;
		cursor: pointer;
	}
	.send:disabled {
		opacity: 0.4;
		cursor: default;
	}
</style>
