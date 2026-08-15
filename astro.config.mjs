import { defineConfig } from 'astro/config';
import cloudflare from '@astrojs/cloudflare';
import tailwindcss from '@tailwindcss/vite';

// Served from the root of its own domain, so no `base` and no BASE_URL juggling:
// every internal link is a plain absolute path.
//
// `format: 'file'` emits get-started.html rather than get-started/index.html, so
// Cloudflare's auto-trailing-slash handling serves /get-started directly instead
// of 307-redirecting it to /get-started/.
// Every page is still prerendered; only /oauth/config opts out with
// `export const prerender = false`, so the Worker runs for that one path and
// the rest is served straight from Cloudflare's edge.
//
// `prerenderEnvironment: 'node'` because the build fetches the release list
// from the GitHub API — prerendering defaults to workerd, and the build-time
// data layer is written for Node.
export default defineConfig({
  site: 'https://qrate.dvnl.work',
  adapter: cloudflare({ prerenderEnvironment: 'node' }),
  trailingSlash: 'ignore',
  build: { format: 'file' },
  vite: {
    plugins: [tailwindcss()],
  },
});
