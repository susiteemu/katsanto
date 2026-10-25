import { defineConfig } from 'vitest/config';
import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';

// In development the Rust server runs on :3000 with KATSANTO_PUBLIC_URL=http://localhost:5173,
// so cookies, the same-origin check and passkeys all see the Vite origin.
const backend = process.env.KATSANTO_BACKEND ?? 'http://localhost:3000';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Single-page app: the Rust server serves index.html for all app routes.
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	server: {
		proxy: {
			'/api': backend,
			'/proxy': backend
		}
	},
	test: {
		expect: { requireAssertions: true },
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'unit',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
