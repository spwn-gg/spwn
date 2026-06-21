// Standalone check: does the Agent SDK authenticate (local Claude login) and
// stream a response on this machine? Run: node sidecar/test.mjs
import { query } from '@anthropic-ai/claude-agent-sdk';

async function* prompt() {
	yield {
		type: 'user',
		message: { role: 'user', content: 'Reply with exactly: SDK_OK' },
		parent_tool_use_id: null,
		shouldQuery: true
	};
}

const q = query({
	prompt: prompt(),
	options: {
		permissionMode: 'bypassPermissions',
		includePartialMessages: false,
		pathToClaudeCodeExecutable: `${process.env.HOME}/.local/bin/claude`
	}
});

for await (const m of q) {
	if (m.type === 'system' && m.subtype === 'init') console.error('[init] session', m.session_id);
	else if (m.type === 'assistant') {
		for (const b of m.message.content || []) {
			if (b.type === 'text') console.error('[text]', b.text);
		}
	} else if (m.type === 'result') {
		console.error('[result]', m.subtype);
		break;
	}
}
