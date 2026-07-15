// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	// GitHub Pages project site: https://spwn-gg.github.io/spwn/
	site: 'https://spwn-gg.github.io',
	base: '/spwn',
	integrations: [
		starlight({
			title: 'spwn',
			description:
				'A desktop app that gives Claude Code a per-project context space, persistent sessions, and scheduled runs.',
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/spwn-gg/spwn' },
			],
			editLink: {
				baseUrl: 'https://github.com/spwn-gg/spwn/edit/main/docs/',
			},
			sidebar: [
				{
					label: 'Getting Started',
					items: [
						{ label: 'Introduction', slug: 'getting-started/introduction' },
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Quick Start', slug: 'getting-started/quick-start' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Projects', slug: 'guides/projects' },
						{ label: 'Sessions', slug: 'guides/terminals' },
						{ label: 'Claude Sessions', slug: 'guides/claude-sessions' },
						{ label: 'Fork & Rewind', slug: 'guides/fork-and-rewind' },
						{ label: 'Parallel Sessions', slug: 'guides/parallel-sessions' },
						{ label: 'Per-Session Services', slug: 'guides/services' },
						{ label: 'Composing Context', slug: 'guides/context-composer' },
						{ label: 'Scheduled Tasks', slug: 'guides/scheduled-tasks' },
						{ label: 'Settings', slug: 'guides/settings' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'How it works & your data', slug: 'reference/architecture' },
						{ label: 'Building from Source', slug: 'reference/building' },
					],
				},
			],
		}),
	],
});
