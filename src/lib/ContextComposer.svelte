<script lang="ts">
	import { projects, openTab, refreshProjects } from './stores';
	import {
		addContextBlock,
		addContextFile,
		removeContextBlock,
		clearContext,
		pickFile
	} from './ipc';
	import type { ContextBlock } from './types';

	let { projectId }: { projectId: string } = $props();

	let note = $state('');
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
		await removeContextBlock(projectId, id);
		await refreshProjects();
	}
	async function clearAll() {
		await clearContext(projectId);
		await refreshProjects();
	}
	function inject() {
		if (!blocks.length) return;
		openTab({
			projectId,
			kind: 'claude',
			title: 'context session',
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
			<span class="len">{assembledLen.toLocaleString()} chars</span>
			{#if blocks.length}<button class="danger" onclick={clearAll}>Clear</button>{/if}
		</div>
	</div>

	<div class="blocks">
		{#if blocks.length === 0}
			<div class="hint">
				No context yet. Add notes or files above, or ＋ Context on a message in a chat.
			</div>
		{/if}
		{#each blocks as b (b.id)}
			<div class="block">
				<div class="bhead">
					<span class="kind {b.kind}">{b.kind}</span>
					<span class="label">{b.label}</span>
					<button class="x" title="Remove" onclick={() => remove(b.id)}>×</button>
				</div>
				<div class="preview">{b.text.slice(0, 600)}{b.text.length > 600 ? '…' : ''}</div>
			</div>
		{/each}
	</div>
</div>

<style>
	.composer {
		display: flex;
		flex-direction: column;
		height: 100%;
		background: #181818;
	}
	.bar {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px 14px;
		border-bottom: 1px solid #2c2c2c;
	}
	.title {
		flex: 1 1 auto;
		font-weight: 600;
		color: #e6e6e6;
	}
	.primary {
		background: #2a4a78;
		border: 1px solid #3a5a88;
		color: #fff;
		border-radius: 6px;
		padding: 6px 14px;
		cursor: pointer;
	}
	.primary:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.add {
		padding: 12px 14px;
		border-bottom: 1px solid #2c2c2c;
	}
	.add textarea {
		width: 100%;
		box-sizing: border-box;
		height: 70px;
		resize: vertical;
		background: #161616;
		border: 1px solid #3a3a3a;
		border-radius: 8px;
		color: #e6e6e6;
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
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
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
		color: #cf9a9a;
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
		color: #6a6a6a;
		font-size: 13px;
		padding: 8px;
	}
	.block {
		border: 1px solid #2c2c2c;
		border-radius: 8px;
		background: #1c1c1c;
		overflow: hidden;
	}
	.bhead {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 10px;
		background: #202020;
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
		color: #9bbf8a;
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
	.x {
		background: none;
		border: none;
		color: #888;
		font-size: 15px;
		cursor: pointer;
	}
	.x:hover {
		color: #fff;
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
</style>
