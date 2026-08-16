import { defineConfig } from 'astro/config';
import cloudflare from '@astrojs/cloudflare';
import tailwindcss from '@tailwindcss/vite';
import starlight from '@astrojs/starlight';
import starlightLlmsTxt from 'starlight-llms-txt';
import starlightMdTxt from 'starlight-md-txt';
import starlightMcp from 'starlight-mcp';
import syncedSidebar from './src/sidebar.generated.js';
import stripHtmlLinks from './src/integrations/strip-html-links.mjs';

// Cloudflare Web Analytics, cookieless and page-level. The token is public (it
// ships in the HTML) but stays out of the repo so a fork does not report into
// this account. Unset means no beacon at all, which is what local builds want.
// src/layouts/Base.astro carries the same tag for the pages Starlight does not
// render; both have to exist for the funnel to span / -> /docs/install -> /thanks.
const beacon = process.env.CF_BEACON_TOKEN;

// Served from the root of its own domain, so no `base` and no BASE_URL juggling:
// every internal link is a plain absolute path.
//
// `format: 'file'` emits get-started.html rather than get-started/index.html, so
// Cloudflare's auto-trailing-slash handling serves /get-started directly instead
// of 307-redirecting it to /get-started/. Starlight honours this — see
// createPathFormatter in the package — so the docs routes follow the same rule.
// Every page is still prerendered; only /oauth/config and /mcp opt out with
// `export const prerender = false`, so the Worker runs for those two paths and
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

  // The download page moved into the docs. Keeps the URL people have linked to,
  // and the #source fragment survives the hop.
  redirects: {
    '/get-started': '/docs/install',
  },

  integrations: [
    starlight({
      title: 'qrate',
      description:
        'Documentation for qrate: free, open-source desktop software for collection metadata.',
      // src/pages/404.astro already serves this site's 404; without this,
      // Starlight injects its own at the same route and the build fails.
      disable404Route: true,
      customCss: ['./src/styles/qrate-docs.css'],
      // Matches src/layouts/Base.astro, so docs and site load the same faces
      // from the same cache entries.
      head: [
        { tag: 'link', attrs: { rel: 'preconnect', href: 'https://fonts.googleapis.com' } },
        {
          tag: 'link',
          attrs: { rel: 'preconnect', href: 'https://fonts.gstatic.com', crossorigin: true },
        },
        {
          tag: 'link',
          attrs: {
            rel: 'stylesheet',
            href: 'https://fonts.googleapis.com/css2?family=Archivo:wght@400;500;600;700&family=IBM+Plex+Mono:wght@400;500;600&display=swap',
          },
        },
        ...(beacon
          ? [
              {
                tag: 'script',
                attrs: {
                  defer: true,
                  src: 'https://static.cloudflareinsights.com/beacon.min.js',
                  'data-cf-beacon': JSON.stringify({ token: beacon }),
                },
              },
            ]
          : []),
      ],
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/devnull03/qrate' }],
      components: {
        SocialIcons: './src/components/DocsNavLinks.astro',
        SiteTitle: './src/components/DocsSiteTitle.astro',
        PageTitle: './src/components/DocsPageTitle.astro',
        Pagination: './src/components/DocsPagination.astro',
      },
      // Rewrites the tab title to the site's own `qrate docs · Page` order.
      routeMiddleware: './src/docsRouteData.ts',
      // Pages synced from main:docs/ get their edit link redirected to the real
      // source in src/docsRouteData.ts; this baseUrl covers the rest.
      editLink: { baseUrl: 'https://github.com/devnull03/qrate/edit/site/' },
      lastUpdated: true,
      // "Start here" and "Plugins" come from docs/index.md on main via
      // scripts/sync-docs.mjs, so the sidebar cannot drift from the source docs.
      // The two groups around them are site-authored.
      sidebar: [
        // Overview is the docs index, which is otherwise unreachable once you
        // follow any link into the tree. The site title goes there too.
        { label: 'Get started', items: [{ label: 'Overview', link: 'docs' }, 'docs/install'] },
        ...syncedSidebar,
        { label: 'Reference', items: ['docs/ai-tools'] },
      ],
      expressiveCode: {
        // The code colours from design frame 1c, as one theme used for both
        // modes: the block is ink-on-paper in light and dark alike.
        themes: [
          {
            name: 'qrate',
            type: 'dark',
            settings: [
              { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: '#8f8578' } },
              { scope: ['keyword', 'storage', 'storage.type', 'keyword.control'], settings: { foreground: '#c07ad4' } },
              { scope: ['string', 'string.quoted', 'constant.character'], settings: { foreground: '#93a37e' } },
              { scope: ['entity.name.function', 'support.function', 'constant.numeric'], settings: { foreground: '#e0a13f' } },
              { scope: ['variable', 'meta.definition.variable'], settings: { foreground: '#f2ede4' } },
            ],
            colors: {
              'editor.background': '#17140f',
              'editor.foreground': '#f2ede4',
              'editorLineNumber.foreground': '#8f8578',
              'diffEditor.insertedTextBackground': '#93a37e21',
              'diffEditor.removedTextBackground': '#a3301f29',
            },
          },
        ],
        styleOverrides: {
          borderRadius: '0',
          borderColor: 'var(--sl-color-hairline)',
          frames: { shadowColor: 'transparent' },
        },
      },
      plugins: [starlightLlmsTxt(), starlightMdTxt()],
    }),

    // Not a Starlight plugin: it injects its own routes. The live /mcp endpoint
    // is mounted by src/pages/mcp.ts, because this project is static output with
    // an adapter rather than `output: 'server'`, which is what the integration
    // checks before injecting one itself.
    starlightMcp({
      serverName: 'qrate-docs',
      siteLabel: 'qrate',
      docsRedirect: '/docs/ai-tools',
      toolDescriptions: {
        search_docs:
          'Search the qrate documentation: projects, the grid, columns and validators, linked files, diagnostics, export and Google Sheets sync, and the Lua plugin API.',
      },
    }),

    // Must come after Starlight: it corrects the `.html` links Starlight derives
    // from `build.format: 'file'`, which Cloudflare would otherwise 307 away.
    stripHtmlLinks(),
  ],

  vite: {
    plugins: [tailwindcss()],
  },
});
