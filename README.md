# qrate — releases site

The public releases page for [`devnull03/qrate`](https://github.com/devnull03/qrate),
built with [Astro](https://astro.build) and deployed to GitHub Pages.

This is the **`site` branch** — it contains only the Astro project, no Rust app
code. The release list is fetched from the GitHub API **at build time** (in CI,
with the job's `GITHUB_TOKEN`), so the shipped HTML is fully static and carries no
token.

## Develop

```sh
npm install
npm run dev        # http://localhost:4321/qrate/
npm run build      # static output in dist/
npm run preview
```

## Deploy

`.github/workflows/deploy-site.yml` builds and publishes to Pages on every push to
`site` (and via `workflow_dispatch`). Publishing a release on `main` triggers
`redeploy-site-on-release.yml`, which dispatches this workflow so the page refreshes
automatically. See `docs/SETUP.md` on `main` for the full pipeline.
