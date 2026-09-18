<script lang="ts">
	// The GitHub personal access token field, shared by Settings and the setup screen
	// so there is one place that explains what the token is for and one that saves it.
	import { onMount } from 'svelte';
	import { githubAuthStatus, setGithubToken } from './ipc';

	let { onchange }: { onchange?: (saved: boolean) => void } = $props();

	let tokenSaved = $state(false);
	let tokenInput = $state('');
	let tokenMsg = $state('');
	let tokenBusy = $state(false);

	onMount(async () => {
		try {
			tokenSaved = (await githubAuthStatus()).tokenSaved;
			onchange?.(tokenSaved);
		} catch {
			/* leave it as "not saved"; saving still works */
		}
	});

	async function saveToken(token: string) {
		tokenBusy = true;
		tokenMsg = '';
		try {
			tokenSaved = (await setGithubToken(token)).tokenSaved;
			tokenInput = '';
			tokenMsg = tokenSaved ? 'Token saved.' : 'Token removed.';
			onchange?.(tokenSaved);
		} catch (e) {
			tokenMsg = e instanceof Error ? e.message : String(e);
		} finally {
			tokenBusy = false;
		}
	}
</script>

<div class="row">
	<input
		type="password"
		bind:value={tokenInput}
		placeholder={tokenSaved ? 'Paste a new token to replace the saved one' : 'ghp_… or github_pat_…'}
		spellcheck="false"
		autocomplete="off"
		disabled={tokenBusy} />
	<button class="browse" onclick={() => saveToken(tokenInput)} disabled={tokenBusy || !tokenInput.trim()}>
		Save token
	</button>
	{#if tokenSaved}
		<button class="browse" onclick={() => saveToken('')} disabled={tokenBusy}>Remove</button>
	{/if}
</div>
<div class="hint">
	A personal access token lets spwn clone, fetch, pull and push private GitHub repos over
	HTTPS, and lets git in your shells and agents do the same. Use a classic token with the
	<code>repo</code> scope, or a fine-grained one with read and write access to Contents.
	It's saved on its own in spwn's data folder, readable only by you.
</div>
{#if tokenMsg}<div class="hint">{tokenMsg}</div>{/if}

<style>
	/* Matches the field styling in Settings.svelte, which this was lifted out of. */
	.row {
		display: flex;
		gap: 8px;
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
	.browse {
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 6px;
		padding: 0 12px;
		cursor: pointer;
		white-space: nowrap;
	}
	.browse:hover:not(:disabled) {
		background: #333;
		color: #fff;
	}
	.browse:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.hint {
		font-size: 11px;
		color: #8a8a8a;
		line-height: 1.5;
		margin-top: 6px;
	}
	code {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11px;
	}
</style>
