import type { APIRoute } from 'astro';
import { createMcpHandler, type DocEntry } from 'starlight-mcp/handler';
import { env } from 'cloudflare:workers';
import config from 'virtual:starlight-mcp/config';

// One of the two routes on the site that isn't prerendered.
//
// starlight-mcp only mounts this itself when `output: 'server'`. This project is
// static output with an adapter, so the corpus and /mcp-schema.json are emitted
// as static assets by the integration and the live endpoint is mounted here —
// the same mechanism /oauth/config uses. Nothing is matched at /mcp in the
// assets tree, so the request falls through to the Worker without needing a
// `run_worker_first` rule in wrangler.jsonc.
export const prerender = false;

// The corpus is a static asset. Reading it over the ASSETS binding keeps it a
// same-isolate lookup rather than a public HTTP round trip. loadIndex runs on
// every tools/call, so hold the promise for the life of the isolate: it
// refreshes on deploy, which is exactly when the corpus changes.
let corpus: Promise<DocEntry[]> | null = null;

const handler = createMcpHandler({
  serverInfo: { name: config.serverName, version: config.serverVersion },
  siteLabel: config.siteLabel,
  instructions: config.instructions,
  docsRedirect: config.docsRedirect,
  toolDescriptions: config.toolDescriptions,
  loadIndex: () =>
    (corpus ??= env.ASSETS.fetch(
      new URL(`${config.path}/docs-index.json`, 'https://qrate.dvnl.work')
    ).then((res) => {
      if (!res.ok) {
        corpus = null; // do not cache a failure for the isolate's whole life
        throw new Error(`docs-index.json: ${res.status}`);
      }
      return res.json() as Promise<DocEntry[]>;
    })),
});

export const ALL: APIRoute = ({ request }) => handler(request);
