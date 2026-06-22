// Node Agent SDK sidecar: drives ONE Claude chat session for a Claude terminal.
//
// Protocol (newline-delimited JSON):
//   Rust -> stdin:
//     {"t":"user","text":"..."}                      send a user turn
//     {"t":"permission","id":"<toolUseID>","allow":true|false,"message":"..."}
//     {"t":"interrupt"}
//     {"t":"set_mode","mode":"default|acceptEdits|plan|bypassPermissions"}
//     {"t":"answer","id":"<toolUseID>","text":"..."}   answer an AskUserQuestion
//   stdout adds:
//     {"t":"question","id":"<toolUseID>","questions":[{question,header,options,multiSelect}]}
//   stdout -> Rust (forwarded to the chat UI):
//     {"t":"init","sessionId":"..."}
//     {"t":"delta","text":"..."}           streamed assistant text
//     {"t":"thinking","text":"..."}        streamed thinking
//     {"t":"tool_use","id":"...","name":"...","input":{...}}
//     {"t":"tool_result","id":"...","text":"...","isError":bool}
//     {"t":"permission","id":"<toolUseID>","tool":"...","input":{...},"title":"..."}
//     {"t":"result","subtype":"success|error...","sessionId":"..."}
//     {"t":"error","message":"..."}
import { query } from '@anthropic-ai/claude-agent-sdk';
import readline from 'node:readline';

function arg(name, def) {
	const i = process.argv.indexOf(`--${name}`);
	return i >= 0 && i + 1 < process.argv.length ? process.argv[i + 1] : def;
}
const cwd = arg('cwd', process.cwd());
const resume = arg('resume', undefined);
const resumeAt = arg('resume-at', undefined); // branch/rewind: truncate at this msg uuid
const fork = process.argv.includes('--fork');
const claudePath = arg('claude-path', undefined);
const model = arg('model', undefined);

function emit(obj) {
	process.stdout.write(JSON.stringify(obj) + '\n');
}

// A promise-backed queue feeding the streaming-input async generator.
class AsyncQueue {
	constructor() {
		this.items = [];
		this.resolvers = [];
		this.done = false;
	}
	push(x) {
		const r = this.resolvers.shift();
		if (r) r(x);
		else this.items.push(x);
	}
	end() {
		this.done = true;
		const r = this.resolvers.shift();
		if (r) r(null);
	}
	async *[Symbol.asyncIterator]() {
		while (true) {
			if (this.items.length) yield this.items.shift();
			else if (this.done) return;
			else {
				const x = await new Promise((res) => this.resolvers.push(res));
				if (x === null) return;
				yield x;
			}
		}
	}
}

const userQ = new AsyncQueue();
const pendingPermissions = new Map(); // toolUseID -> resolve fn

// ExitPlanMode is a TUI-only plan-approval transition we can't render; decline it
// so Claude presents the plan as plain text instead.
const INTERACTIVE_TOOLS = new Set(['ExitPlanMode']);

// AskUserQuestion gets a real picker: we hold the permission call open, show the
// options in the UI, and resolve it once the user answers. The SDK can't return
// the answer via `updatedInput` (TUI-only), but a deny `message` becomes the
// tool_result Claude reads — so we deliver the selection that way.
const pendingQuestions = new Map(); // toolUseID -> resolve(PermissionResult)

const q = query({
	prompt: userQ,
	options: {
		cwd,
		resume,
		resumeSessionAt: resumeAt,
		forkSession: fork,
		model,
		includePartialMessages: true,
		permissionMode: 'default',
		pathToClaudeCodeExecutable: claudePath,
		canUseTool: (toolName, input, opts) => {
			const id = opts?.toolUseID ?? `perm-${Date.now()}`;
			// Interactive multiple-choice question → show a picker; resolve when answered.
			if (toolName === 'AskUserQuestion') {
				emit({ t: 'question', id, questions: input?.questions ?? [] });
				return new Promise((resolve) => pendingQuestions.set(id, resolve));
			}
			// Plan approval and other TUI-only prompts → decline with guidance so
			// Claude continues in plain text (the chat handles that fine).
			if (INTERACTIVE_TOOLS.has(toolName)) {
				return Promise.resolve({
					behavior: 'deny',
					message:
						"This client can't display interactive plan approval. Summarize the plan as plain text and ask the user to confirm in the chat."
				});
			}
			emit({ t: 'permission', id, tool: toolName, input, title: opts?.title ?? opts?.displayName });
			return new Promise((resolve) => pendingPermissions.set(id, resolve));
		}
	}
});

// Read commands from stdin.
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
	if (!line.trim()) return;
	let msg;
	try {
		msg = JSON.parse(line);
	} catch {
		return;
	}
	if (msg.t === 'user') {
		userQ.push({
			type: 'user',
			message: { role: 'user', content: msg.text },
			parent_tool_use_id: null,
			shouldQuery: true
		});
	} else if (msg.t === 'permission') {
		const resolve = pendingPermissions.get(msg.id);
		if (resolve) {
			pendingPermissions.delete(msg.id);
			resolve(
				msg.allow
					? { behavior: 'allow' }
					: { behavior: 'deny', message: msg.message || 'Denied by user' }
			);
		}
	} else if (msg.t === 'interrupt') {
		q.interrupt?.();
	} else if (msg.t === 'answer') {
		// The user answered an AskUserQuestion picker; deliver it as the tool_result.
		const resolve = pendingQuestions.get(msg.id);
		if (resolve) {
			pendingQuestions.delete(msg.id);
			resolve({ behavior: 'deny', message: msg.text || 'No selection.' });
		}
	} else if (msg.t === 'set_mode') {
		// Live permission-mode change (the Shift-Tab affordance): default →
		// acceptEdits → plan. Guarded so older SDKs degrade gracefully.
		q.setPermissionMode?.(msg.mode).catch((e) =>
			emit({ t: 'error', message: String(e?.message ?? e) })
		);
	}
});
rl.on('close', () => userQ.end());

let sessionId = resume;

try {
	for await (const m of q) {
		switch (m.type) {
			case 'system':
				if (m.subtype === 'init') {
					sessionId = m.session_id;
					emit({ t: 'init', sessionId });
				}
				break;
			case 'stream_event': {
				const ev = m.event;
				if (ev?.type === 'content_block_delta') {
					if (ev.delta?.type === 'text_delta') emit({ t: 'delta', text: ev.delta.text });
					else if (ev.delta?.type === 'thinking_delta')
						emit({ t: 'thinking', text: ev.delta.thinking });
				}
				break;
			}
			case 'assistant':
				// The message uuid is the anchor for rewind/branch (resumeSessionAt).
				if (m.uuid) emit({ t: 'assistant_uuid', uuid: m.uuid });
				for (const b of m.message?.content ?? []) {
					if (b.type === 'tool_use') emit({ t: 'tool_use', id: b.id, name: b.name, input: b.input });
				}
				break;
			case 'user':
				// Tool results come back as user messages with tool_result blocks.
				for (const b of m.message?.content ?? []) {
					if (b?.type === 'tool_result') {
						const text = Array.isArray(b.content)
							? b.content.map((c) => c.text ?? '').join('')
							: typeof b.content === 'string'
								? b.content
								: '';
						emit({ t: 'tool_result', id: b.tool_use_id, text, isError: !!b.is_error });
					}
				}
				break;
			case 'result':
				emit({ t: 'result', subtype: m.subtype, sessionId: m.session_id ?? sessionId });
				break;
		}
	}
} catch (e) {
	emit({ t: 'error', message: String(e?.message ?? e) });
}
