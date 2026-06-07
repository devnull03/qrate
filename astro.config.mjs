import { defineConfig } from 'astro/config';

// Project pages are served from a subpath, so `base` must match the repo name.
// Every internal link/asset must go through this base (use import.meta.env.BASE_URL
// or let Astro rewrite imported assets) or it will 404 on GitHub Pages.
export default defineConfig({
  site: 'https://devnull03.github.io',
  base: '/qrate',
  trailingSlash: 'ignore',
});
