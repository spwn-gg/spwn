<script lang="ts">
	import { projects, openTab, refreshProjects } from './stores';
	import {
		addContextBlock,
		addContextFile,
		removeContextBlock,
		updateContextBlock,
		reorderContext,
		clearContext,
		pickFile
	} from './ipc';
	import type { ContextBlock } from './types';

	let { projectId }: { projectId: string } = $props();

	let note = $state('');
	let editingId = $state<string | null>(null);
	let editText = $state('');
	let dragIndex = $state<number | null>(null);
	let overIndex = $state<number | null>(null);

	const project = $derived($projects.find((p) => p.id === projectId) ?? null);
	const blocks = $derived(project?.context ?? []);

	function assemble(bs: ContextBlock[]): string {
		return bs
			.map((b) =>
				b.kind === 'file'
					? `## File: ${b.label}\n\n${b.text}`
					: b.kind === 'session'
						? `## From a session (${b.label})\n\n${b.text}`
						: b.text
			)
			.join('\n\n---\n\n');
	}
	const assembledLen = $derived(assemble(blocks).length);
	// Rough heuristic — ~4 chars/token for English prose; good enough to gauge budget.
	const estTokens = $derived(Math.round(assembledLen / 4));

	async function addNote() {
		const t = note.trim();
		if (!t) return;
		await addContextBlock(projectId, 'note', 'note', t);
		note = '';
		await refreshProjects();
	}
	async function addFile() {
		const p = await pickFile();
		if (!p) return;
		await addContextFile(projectId, p);
		await refreshProjects();
	}
	async function remove(id: string) {
		if (editingId === id) editingId = null;
		await removeContextBlock(projectId, id);
		await refreshProjects();
	}
	async function clearAll() {
		if (!confirm('Clear all context blocks for this project?')) return;
		await clearContext(projectId);
		await refreshProjects();
	}

	function startEdit(b: ContextBlock) {
		editingId = b.id;
		editText = b.text;
	}
	function cancelEdit() {
		editingId = null;
		editText = '';
	}
	async function saveEdit(id: string) {
		await updateContextBlock(projectId, id, editText);
		editingId = null;
		await refreshProjects();
	}

	async function persistOrder(ids: string[]) {
		await reorderContext(projectId, ids);
		await refreshProjects();
	}
	function move(i: number, dir: -1 | 1) {
		const ids = blocks.map((b) => b.id);
		const j = i + dir;
		if (j < 0 || j >= ids.length) return;
		[ids[i], ids[j]] = [ids[j], ids[i]];
		persistOrder(ids);
	}

	// Native drag-to-reorder.
	function onDragStart(i: number) {
		dragIndex = i;
	}
	function onDragOver(i: number, e: DragEvent) {
		e.preventDefault();
		overIndex = i;
	}
	function onDrop(i: number) {
		if (dragIndex === null || dragIndex === i) {
			dragIndex = overIndex = null;
			return;
		}
		const ids = blocks.map((b) => b.id);
		const [moved] = ids.splice(dragIndex, 1);
		ids.splice(i, 0, moved);
		dragIndex = overIndex = null;
		persistOrder(ids);
	}
	function onDragEnd() {
		dragIndex = overIndex = null;
	}

	function inject() {
		if (!blocks.length) return;
		openTab({
			projectId,
			kind: 'claude',
			title: 'context session',
			projectName: project?.name,
			initialPrompt: assemble(blocks)
		});
	}
</script>

<div class="composer">
	<div class="bar">
		<span class="title">Context — {project?.name ?? ''}</span>
		<button class="primary" disabled={blocks.length === 0} onclick={inject}>
			Inject → new Claude session
		</button>
	</div>

	<div class="add">
		<textarea
			bind:value={note}
			placeholder="Add a note to the context… (instructions, requirements, snippets)"></textarea>
		<div class="add-btns">
			<button onclick={addNote} disabled={!note.trim()}>＋ Note</button>
			<button onclick={addFile}>＋ File</button>
			<span class="spacer"></span>
			<span class="len">{assembledLen.toLocaleString()} chars · ~{estTokens.toLocaleString()} tokens</span>
			{#if blocks.length}<button class="danger" onclick={clearAll}>Clear</button>{/if}
		</div>
	</div>

	<div class="blocks">
		{#if blocks.length === 0}
			<div class="hint">
				No context yet. Add notes or files above, or ＋ ctx on a message in a chat.
			</div>
		{/if}
		{#each blocks as b, i (b.id)}
			<div
				class="block"
				class:dragging={dragIndex === i}
				class:over={overIndex === i && dragIndex !== i}
				draggable={editingId !== b.id}
				ondragstart={() => onDragStart(i)}
				ondragover={(e) => onDragOver(i, e)}
				ondrop={() => onDrop(i)}
				ondragend={onDragEnd}
				role="listitem">
				<div class="bhead">
					<span class="grip" title="Drag to reorder">⠿</span>
					<span class="kind {b.kind}">{b.kind}</span>
					<span class="label">{b.label}</span>
					<div class="ord">
						<button class="x" title="Move up" disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
						<button class="x" title="Move down" disabled={i === blocks.length - 1} onclick={() => move(i, 1)}>↓</button>
					</div>
					{#if editingId === b.id}
						<button class="x save" title="Save" onclick={() => saveEdit(b.id)}>✓</button>
						<button class="x" title="Cancel" onclick={cancelEdit}>×</button>
					{:else}
						<button class="x" title="Edit" onclick={() => startEdit(b)}>✎</button>
						<button class="x" title="Remove" onclick={() => remove(b.id)}>🗑</button>
					{/if}
				</div>
				{#if editingId === b.id}
					<textarea class="edit" bind:value={editText}></textarea>
				{:else}
					<div class="preview">{b.text.slice(0, 600)}{b.text.length > 600 ? '…' : ''}</div>
				{/if}
			</div>
		{/each}
	</div>
</div>

<style>
	.composer {
		display: flex;
		flex-direction: column;
		height: 100%;
		background: var(--bg-sidebar);
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
	.add {
		padding: 12px 14px;
		border-bottom: 1px solid var(--border);
	}
	.add textarea {
		width: 100%;
		box-sizing: border-box;
		height: 70px;
		resize: vertical;
		background: var(--bg-input);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		color: var(--text);
		padding: 10px 12px;
		font-family: inherit;
		font-size: 13px;
	}
	.add-btns {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 8px;
	}
	.add-btns button {
		background: var(--bg-elevated);
		border: 1px solid var(--border-strong);
		color: #cfcfcf;
		border-radius: 5px;
		padding: 4px 10px;
		cursor: pointer;
		font-size: 12px;
	}
	.add-btns button:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.add-btns .danger {
		color: var(--danger);
		border-color: #5a3a3a;
	}
	.spacer {
		flex: 1 1 auto;
	}
	.len {
		color: #777;
		font-size: 11px;
	}
	.blocks {
		flex: 1 1 auto;
		overflow-y: auto;
		padding: 12px 14px;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.hint {
		color: var(--text-muted);
		font-size: 13px;
		padding: 8px;
	}
	.block {
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--surface);
		overflow: hidden;
	}
	.block.dragging {
		opacity: 0.5;
	}
	.block.over {
		border-color: var(--accent-line);
		box-shadow: inset 0 2px 0 var(--accent-line);
	}
	.bhead {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 10px;
		background: var(--surface-head);
	}
	.grip {
		color: #666;
		cursor: grab;
		font-size: 12px;
		user-select: none;
	}
	.kind {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		padding: 1px 6px;
		border-radius: 4px;
		background: #2a3344;
		color: #9bbce0;
	}
	.kind.file {
		background: #2a3a2a;
		color: var(--ok);
	}
	.kind.note {
		background: #3a3320;
		color: #d8c48a;
	}
	.label {
		flex: 1 1 auto;
		color: #cfcfcf;
		font-size: 12px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.ord {
		display: flex;
		gap: 2px;
	}
	.x {
		background: none;
		border: none;
		color: #888;
		font-size: 13px;
		cursor: pointer;
		padding: 1px 4px;
		border-radius: 4px;
	}
	.x:hover:not(:disabled) {
		color: #fff;
		background: #333;
	}
	.x:disabled {
		opacity: 0.3;
		cursor: default;
	}
	.x.save {
		color: var(--ok);
	}
	.preview {
		padding: 8px 10px;
		font-size: 12px;
		color: #aaa;
		white-space: pre-wrap;
		word-break: break-word;
		max-height: 140px;
		overflow: auto;
		font-family: ui-monospace, Menlo, monospace;
	}
	.edit {
		width: 100%;
		box-sizing: border-box;
		min-height: 120px;
		resize: vertical;
		background: var(--bg-input);
		border: none;
		border-top: 1px solid var(--border);
		color: var(--text);
		padding: 8px 10px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 12px;
	}
</style>
