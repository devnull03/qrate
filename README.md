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
bun run dev            # http://localhost:4321
bun run build          # static output in dist/
bun run preview        # astro's own preview server
bun run preview:worker # build, then serve via wrangler exactly as production does
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

`wrangler.jsonc` is already written and verified against `wrangler dev`, so the
switch is mostly dashboard work when you want it:

1. Workers & Pages → create a Worker from this repo, branch `site`, build
   command `bun run build`, deploy command `bunx wrangler deploy`.
2. Add `GH_TOKEN` as a **build** secret. Without it the build fetches the
   release list unauthenticated at 60 requests/hour from a shared build IP, and
   `getReleases()` throws on a non-OK response in production rather than
   shipping an empty changelog — so a rate-limited build fails the deploy.
3. Point `qrate.dvnl.work` at the Worker and remove the Pages custom domain.
4. Either delete this workflow and let Cloudflare watch the branch, or keep it
   and swap the deploy job for a POST to a Cloudflare **Deploy Hook**.

`bun run deploy` already builds and pushes to Cloudflare straight from your
machine after `wrangler login`, if you want to try it before switching.

`wrangler.jsonc` is assets-only: no Worker script, so Cloudflare serves `dist/`
from its edge and nothing is billed per request. To add server-side routes later,
set `main` and give the assets a `binding` — the comment in that file spells out
the change.

## Theming

Six qrate palettes live as `[data-theme]` blocks in `src/styles/global.css` and
are exposed to Tailwind through `@theme inline`. The picker in the nav cycles
them and stores the choice in `localStorage`; the accent, hero wash, screenshot
tint, in-page app window and logo tile all follow.
