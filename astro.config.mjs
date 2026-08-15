import { defineConfig } from 'astro/config';
import tailwindcss from '@tailwindcss/vite';

// Served from the root of its own domain, so no `base` and no BASE_URL juggling:
// every internal link is a plain absolute path.
//
// `format: 'file'` emits get-started.html rather than get-started/index.html, so
// Cloudflare's auto-trailing-slash handling serves /get-started directly instead
// of 307-redirecting it to /get-started/.
export default defineConfig({
  site: 'https://qrate.dvnl.work',
  trailingSlash: 'ignore',
  build: { format: 'file' },
  vite: {
    plugins: [tailwindcss()],
  },
});
