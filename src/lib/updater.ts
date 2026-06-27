// Self-update from GitHub releases via the Tauri updater plugin.
//
// `check()` fetches the `latest.json` endpoint baked into tauri.conf.json,
// compares its version against the running app, and verifies the bundle's
// minisign signature against the embedded public key. On accept we download +
// install in place and relaunch. Auto-checks on launch are silent on failure
// (offline / endpoint not yet configured); manual checks surface the outcome.

import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { writable } from 'svelte/store';

/** An available update awaiting the user's go-ahead (drives the banner). */
export const pendingUpdate = writable<Update | null>(null);
/** Human-readable result of the last manual check (shown in Settings). */
export const updateStatus = writable<string>('');
/** 0..1 while installing, or null for indeterminate; undefined when idle. */
export const installProgress = writable<number | null | undefined>(undefined);

let inFlight = false;

/**
 * Check the release endpoint. `silent` (startup) swallows errors and only
 * reacts when an update exists; a manual check reports every outcome.
 */
export async function checkForUpdate(opts: { silent: boolean }): Promise<void> {
	if (inFlight) return;
	inFlight = true;
	if (!opts.silent) updateStatus.set('Checking…');
	try {
		const update = await check();
		if (update) {
			pendingUpdate.set(update);
			if (!opts.silent) updateStatus.set(`Update available: ${update.version}`);
		} else if (!opts.silent) {
			updateStatus.set("You're on the latest version.");
		}
	} catch (e) {
		console.error('update check failed', e);
		if (!opts.silent) updateStatus.set(`Update check failed: ${e}`);
	} finally {
		inFlight = false;
	}
}

/** Download + install the pending update (with progress), then relaunch. */
export async function installUpdate(update: Update): Promise<void> {
	let total = 0;
	let got = 0;
	installProgress.set(null);
	try {
		await update.downloadAndInstall((ev) => {
			switch (ev.event) {
				case 'Started':
					total = ev.data.contentLength ?? 0;
					installProgress.set(total ? 0 : null);
					break;
				case 'Progress':
					got += ev.data.chunkLength;
					installProgress.set(total ? got / total : null);
					break;
				case 'Finished':
					installProgress.set(1);
					break;
			}
		});
		// Replaces the .app in place and restarts into the new version.
		await relaunch();
	} catch (e) {
		console.error('update install failed', e);
		updateStatus.set(`Install failed: ${e}`);
		installProgress.set(undefined);
		throw e;
	}
}

/** Dismiss the banner for this launch (re-offered on next startup check). */
export function dismissUpdate(): void {
	pendingUpdate.set(null);
}
