# qrate — site

The public site for [`devnull03/qrate`](https://github.com/devnull03/qrate) —
home, get started, changelog, licence, privacy and terms — built with
[Astro](https://astro.build) and Tailwind, deployed to
[qrate.dvnl.work](https://qrate.dvnl.work) on Cloudflare Workers.

This is the **`site` branch** — it contains only the Astro project, no Rust app
code. The release list is fetched from the GitHub API **at build time** (in CI,
with the job's `GITHUB_TOKEN`), so the shipped HTML is fully static and carries no
token. Get started and changelog rebuild themselves from whatever the latest
release is.

## Develop

```sh
bun install
bun run dev      # http://localhost:4321 — serves /oauth/config too
bun run build    # dist/client (static tree) + dist/server (the Worker)
bun run preview  # build, then serve via wrangler exactly as production does
```

Astro's MDX development renderer cannot load dependencies from a Windows Delta worktree whose path
ends in `~`. Commit or stash changes, then run `bun run dev:windows`. It serves the same branch from
a temporary safe-path worktree, seeds its local KV from the signed production catalog, and removes
that worktree when the server exits. It uses a checked-in signed fixture and does not require
Cloudflare credentials. Use `bun run dev:windows:refresh` to test the latest production catalog with
an authenticated Wrangler CLI.

## Deploy

Cloudflare builds the `site` branch and deploys the `qrate` Worker. GitHub release and catalog
workflows request rebuilds through the configured Cloudflare deploy hook. `bun run deploy` builds
and pushes from an authenticated local Wrangler CLI.

Set `SITE_URL` when hosting under another origin. Add runtime credentials with Wrangler or the
Cloudflare dashboard; never put them in `wrangler.jsonc`. Local Worker secrets belong in the
untracked `.dev.vars` file.

## Serving qrate itself

Two routes exist for the desktop app rather than for readers. The contract is
public on purpose: an institution that will not route its staff through this
deployment can run the same two routes against their own Google Cloud project
and point qrate at them under **Settings ▸ Google ▸ Credential endpoint**.

- **`GET /oauth/config`** (`src/pages/oauth/config.ts`) — returns the app's Google
  `client_id` and `client_secret` behind a bearer token, with an ETag so qrate
  can revalidate cheaply. The bearer ships inside every qrate binary: it stops
  casual scraping and nothing more, and does not need to do more, because
  Google treats an installed app's secret as non-confidential and loopback +
  PKCE is what protects the exchange. This must never grow into a token-exchange
  proxy — user Drive tokens stay on the user's machine.
- **`/picker`** (`src/pages/picker.astro`) — a static page hosting Google's file
  chooser, so `drive.file` can reach a spreadsheet the user already owns.
  Needs a browser API key in `PICKER_API_KEY`, referrer-restricted to this site.

The Worker runs for `/oauth/config` and the plugin catalog endpoints. Static
assets are matched first, so every page is still served from the edge.

## Plugin catalog

The `/plugins` pages are static pages built from qrate's signed plugin catalog.
The Worker serves the catalog from Cloudflare KV at these paths:

- `/plugins/catalog.json`
- `/plugins/catalog.json.sig`
- `/plugins/catalog-status.json`
- `/plugins/schemas/<name>.json`

The site source pins the trusted catalog public key. Set
`QRATE_PLUGIN_CATALOG_PUBLIC_KEY` in the build environment only to test a
planned key rotation.

Builds use the checked-in signed fixture by default. `QRATE_PLUGIN_CATALOG_FILE` can select another
exact signed catalog file; its signature must be beside it with a `.sig` suffix.
`QRATE_PLUGIN_CATALOG_URL` instead selects a catalog service. A self-hosted deployment can therefore
build without contacting qrate's deployment or point at its own registry. Set `SITE_URL` to the
deployment's public origin for canonical URLs, sitemap entries, and structured metadata.

The build fetches `catalog.json.sig` beside the catalog. It verifies the SHA-256,
key ID, and signature before it parses any records. A bad catalog fails the
build, so Cloudflare keeps the last successful deployment online.

The site build fails if the configured catalog cannot pass signature and schema
checks. Cloudflare then keeps the last successful deployment online.

## Theming

Six qrate palettes live as `[data-theme]` blocks in `src/styles/global.css` and
are exposed to Tailwind through `@theme inline`. The picker in the nav cycles
them and stores the choice in `localStorage`; the accent, hero wash, screenshot
tint, in-page app window and logo tile all follow.
