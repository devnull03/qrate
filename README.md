# qrate — site

The public site for [`devnull03/qrate`](https://github.com/devnull03/qrate) —
home, get started, changelog, licence, privacy and terms — built with
[Astro](https://astro.build) and Tailwind, deployed to
[qrate.dvnl.work](https://qrate.dvnl.work) on GitHub Pages.

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

## Deploy

`.github/workflows/deploy-site.yml` builds and publishes to Pages on every push
to `site` (and via `workflow_dispatch`). Publishing a release on `main` triggers
`redeploy-site-on-release.yml`, which dispatches this workflow so the
release-driven pages refresh.

The custom domain lives in the repository's Pages settings, not in a `CNAME`
file, and the site is served from the **root** of `qrate.dvnl.work` — hence no
`base` in `astro.config.mjs`.

### Moving to Cloudflare Workers

GitHub Pages can serve the six pages but not `/oauth/config`, so the move stops
being optional once Google sync ships. Everything in this repo is ready; what
is left is dashboard work:

1. Workers & Pages → create a Worker from this repo, branch `site`, build
   command `bun run build`, deploy command `bunx wrangler deploy`.
2. Add `GH_TOKEN` as a **build** secret. Without it the build fetches the
   release list unauthenticated at 60 requests/hour from a shared build IP, and
   `getReleases()` throws on a non-OK response in production rather than
   shipping an empty changelog — so a rate-limited build fails the deploy.
3. Add the runtime secrets the Worker reads:
   `wrangler secret put GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`,
   `QRATE_GOOGLE_CONFIG_TOKEN`. Never in `wrangler.jsonc`. The last one must be
   the same string qrate is built with (`option_env!("QRATE_GOOGLE_CONFIG_TOKEN")`
   in `crates/data-exchange/src/google.rs`) — same name on both sides so it is
   obvious they are one value.
4. Point `qrate.dvnl.work` at the Worker and remove the Pages custom domain.
5. Keep `deploy-site.yml` — it is what a published release dispatches — but swap
   its deploy job for a POST to a Cloudflare **Deploy Hook**.

`bun run deploy` builds and pushes straight from your machine after
`wrangler login`. Both `bun run dev` and `bun run preview` read the secrets from
an untracked `.dev.vars`, so the endpoint works locally.

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

The Worker only runs for `/oauth/config`; static assets are matched first, so
every page is still served from the edge with no invocation.

## Theming

Six qrate palettes live as `[data-theme]` blocks in `src/styles/global.css` and
are exposed to Tailwind through `@theme inline`. The picker in the nav cycles
them and stores the choice in `localStorage`; the accent, hero wash, screenshot
tint, in-page app window and logo tile all follow.
