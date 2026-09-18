<script lang="ts">
	// Setup screen for a spwn that has nothing yet: no Claude login, no projects.
	//
	// It exists because a server deployment starts on an empty home volume, and every
	// way to do this by hand needs a shell -- which needed a project, which needed a
	// clone, which needed the sign-in you were trying to do. The three steps below are
	// the whole bootstrap, in order, in the browser.
	import { onDestroy } from 'svelte';
	import Terminal from './Terminal.svelte';
	import GitHubTokenField from './GitHubTokenField.svelte';
	import CloneRepoDialog from './CloneRepoDialog.svelte';
	import { claudeAuthStatus } from './ipc';
	import { refreshProjects } from './stores';
	import type { ProjectRec } from './types';

	let { ondone }: { ondone: () => void } = $props();

	type Step = 'claude' | 'github' | 'repo';
	let step = $state<Step>('claude');

	let binary = $state<string | null>(null);
	let loggedIn = $state<boolean | null>(null);
	let account = $state<string | null>(null);
	let checking = $state(true);

	let shellOpen = $state(false);
	let cloneOpen = $state(false);
	let cloned = $state<ProjectRec | null>(null);

	// Nothing pushes an auth change: signing in happens inside the terminal, in a
	// process spwn only owns the pty for. Poll while the screen is up, and stop as
	// soon as it succeeds.
	let timer: ReturnType<typeof setInterval> | undefined;

	async function check() {
		try {
			const s = await claudeAuthStatus();
			binary = s.binary;
			account = s.account;
			loggedIn = s.loggedIn;
			// `null` means we couldn't tell -- never treat that as signed out, or a
			// config format we don't recognise would trap the user on this step.
			if (s.loggedIn !== false && step === 'claude') step = 'github';
		} catch {
			/* keep the last known state; the user can always skip */
		} finally {
			checking = false;
		}
	}

	check();
	timer = setInterval(() => {
		if (step === 'claude') check();
	}, 2000);

	onDestroy(() => clearInterval(timer));

	function finish() {
		refreshProjects();
		ondone();
	}
</script>

<div class="overlay" role="presentation">
	<div class="panel" role="dialog" aria-modal="true" aria-label="Set up spwn" tabindex="-1">
		<div class="head">
			<strong>Set up spwn</strong>
			<button class="x" onclick={ondone} aria-label="Skip setup">✕</button>
		</div>

		<div class="steps">
			<span class="step" class:active={step === 'claude'} class:done={loggedIn === true}>
				1 · Sign in
			</span>
			<span class="step" class:active={step === 'github'}>2 · GitHub</span>
			<span class="step" class:active={step === 'repo'} class:done={!!cloned}>3 · First repo</span>
		</div>

		<div class="body">
			{#if step === 'claude'}
				<p class="lede">
					spwn runs Claude Code for you; it doesn't handle the login itself. Sign in once
					here and it's stored in this machine's home directory, where it survives restarts.
				</p>

				{#if checking}
					<div class="status">Checking…</div>
				{:else if loggedIn === true}
					<div class="status"><span class="chip ok">signed in</span>{#if account}&nbsp;as {account}{/if}</div>
				{:else if !binary}
					<div class="status">
						<span class="chip miss">claude not found</span>
						<div class="hint">
							spwn couldn't find the <code>claude</code> CLI. Install it, or set its path in
							Settings → Agents, then reopen this screen.
						</div>
					</div>
				{:else if loggedIn === null}
					<div class="status">
						<span class="chip warn">can't tell</span>
						<div class="hint">
							spwn couldn't read Claude's config. Sign in below if you haven't, or just skip
							this step — it won't stop anything working.
						</div>
					</div>
				{:else}
					<div class="status"><span class="chip miss">not signed in</span></div>
				{/if}

				{#if shellOpen}
					<div class="term">
						<Terminal tabKey="firstrun" kind="shell" runOnOpen="claude" />
					</div>
					<div class="hint">
						Follow the prompts — Claude prints a URL to open and a code to paste back. This
						screen moves on by itself once you're signed in.
					</div>
				{:else if binary && loggedIn !== true}
					<button class="primary" onclick={() => (shellOpen = true)}>Sign in to Claude</button>
					<div class="hint">
						Opens a shell here and runs <code>claude</code>. Nothing else is set up yet, so
						this pane belongs to no project — it's just a terminal.
					</div>
				{/if}
			{:else if step === 'github'}
				<p class="lede">
					Optional. A token lets spwn — and git in your shells and agents — clone, pull and
					push <em>private</em> GitHub repos. Public repos need nothing.
				</p>
				<GitHubTokenField />
			{:else}
				<p class="lede">
					Clone something to work on. spwn gives each session its own git branch in its own
					worktree, so this should be a repo you're happy to branch freely.
				</p>
				{#if cloned}
					<div class="status">
						<span class="chip ok">cloned</span>&nbsp;{cloned.name}
						<div class="hint">{cloned.directory}</div>
					</div>
				{:else}
					<button class="primary" onclick={() => (cloneOpen = true)}>Clone a repository</button>
					<div class="hint">
						Already have code on this machine? Skip this and use <strong>＋ New Project</strong>
						in the sidebar to point spwn at a folder.
					</div>
				{/if}
			{/if}
		</div>

		<div class="foot">
			<button onclick={ondone}>Skip setup</button>
			{#if step === 'claude'}
				<button class="primary" onclick={() => (step = 'github')}>
					{loggedIn === true ? 'Next' : 'Later'}
				</button>
			{:else if step === 'github'}
				<button onclick={() => (step = 'claude')}>Back</button>
				<button class="primary" onclick={() => (step = 'repo')}>Next</button>
			{:else}
				<button onclick={() => (step = 'github')}>Back</button>
				<button class="primary" onclick={finish}>{cloned ? 'Start working' : 'Done'}</button>
			{/if}
		</div>
	</div>
</div>

{#if cloneOpen}
	<CloneRepoDialog
		onclose={() => (cloneOpen = false)}
		oncloned={(p) => {
			cloned = p;
			cloneOpen = false;
			refreshProjects();
		}} />
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
		display: flex;
		flex-direction: column;
		width: min(720px, 92vw);
		max-height: 88vh;
		background: var(--bg, #1e1e1e);
		border: 1px solid var(--border-strong, #3a3a3a);
		border-radius: 10px;
		box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 14px;
		border-bottom: 1px solid var(--border, #2c2c2c);
		font-size: 14px;
	}
	.x {
		background: none;
		border: none;
		color: #8a8a8a;
		cursor: pointer;
		font-size: 14px;
	}
	.x:hover {
		color: #e6e6e6;
	}
	.steps {
		display: flex;
		gap: 6px;
		padding: 10px 14px;
		border-bottom: 1px solid var(--border, #2c2c2c);
	}
	.step {
		font-size: 11px;
		padding: 3px 9px;
		border-radius: 999px;
		border: 1px solid var(--border, #2c2c2c);
		color: #8a8a8a;
	}
	.step.active {
		color: #e6e6e6;
		border-color: var(--accent-border, #3a5a88);
		background: var(--accent-soft, #2f3a4a);
	}
	.step.done {
		color: #7fb069;
		border-color: #3c5a30;
	}
	.body {
		padding: 14px;
		overflow: auto;
	}
	.lede {
		margin: 0 0 14px;
		font-size: 13px;
		line-height: 1.6;
		color: #cfcfcf;
	}
	.status {
		margin-bottom: 12px;
		font-size: 13px;
		color: #cfcfcf;
	}
	.chip {
		font-size: 10px;
		padding: 1px 6px;
		border-radius: 999px;
		border: 1px solid var(--border, #2a2a2a);
		color: #9a9a9a;
	}
	.chip.ok {
		color: #7fb069;
		border-color: #3c5a30;
	}
	.chip.miss {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.chip.warn {
		color: #d8a657;
		border-color: #5c4a2a;
	}
	.term {
		height: 320px;
		border: 1px solid var(--border, #2c2c2c);
		border-radius: 6px;
		overflow: hidden;
		background: #1e1e1e;
	}
	.hint {
		margin-top: 8px;
		font-size: 11px;
		color: #8a8a8a;
		line-height: 1.5;
	}
	code {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 11px;
	}
	.foot {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding: 12px 14px;
		border-top: 1px solid var(--border, #2c2c2c);
	}
	button {
		background: #2a2a2a;
		border: 1px solid #3a3a3a;
		color: #cfcfcf;
		border-radius: 6px;
		padding: 6px 12px;
		font-size: 12px;
		cursor: pointer;
	}
	button:hover {
		background: #333;
		color: #fff;
	}
	button.primary {
		background: var(--accent, #2a4a78);
		border-color: var(--accent-border, #3a5a88);
		color: #fff;
	}
	button.primary:hover {
		background: var(--accent-line, #4a78c8);
	}
</style>
