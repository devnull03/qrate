import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	optimizeDeps: {
		exclude: ['@ai-sdk/svelte', '@ai-sdk/openai'],
		include: ['clsx', 'tailwind-merge']
	},
	server: {
		fs: {
			strict: false
		}
	}
});
