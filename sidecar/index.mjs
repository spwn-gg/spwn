// Node Agent SDK sidecar: drives ONE Claude chat session for a Claude terminal.
//
// Protocol (newline-delimited JSON):
//   Rust -> stdin:
//     {"t":"user","text":"..."}                      send a user turn
//     {"t":"permission","id":"<toolUseID>","allow":true|false,"message":"..."}
//     {"t":"interrupt"}
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
