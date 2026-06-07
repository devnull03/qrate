# Replication Guide — Releases Site + CI/CD

A complete, self-contained record of everything built in this project's
releases-site + release-automation setup, written so you can **reproduce it on
another repository**. It includes every GitHub Actions workflow, the website, the
decisions made (and the questions behind them), and the bugs hit along the way.

> Companion docs: `docs/SETUP.md` is the ongoing runbook; this file is the
> "how it was built / how to rebuild it elsewhere" guide.

---

## 0. What this builds

A public **GitHub Pages** website that lists a repo's **GitHub Releases** with
download links, kept in sync with releases automatically:

```
        ┌─ app repo (default branch `main`) ─────────────────────────┐
        │  Cargo/code + release pipeline                              │
        │  push tag vX.Y.Z ─▶ release.yml ─▶ build ─▶ DRAFT release   │
        │                                          │ (you publish)    │
        │                          release:published                 │
        │                                          │                  │
        │                redeploy-site-on-release.yml ──gh workflow run│
        └──────────────────────────────────────────│─────────────────┘
                                                    ▼ (--ref site)
        ┌─ `site` branch (Astro, no app code) ───────────────────────┐
        │  deploy-site.yml ─▶ Astro build (fetches releases at build  │
        │  time via GITHUB_TOKEN) ─▶ deploy-pages ─▶ Pages CDN        │
        └─────────────────────────────────────────────────────────────┘
```

Key properties:
- **Static, pre-rendered** (Astro ships ~0 JS); release data fetched at **build
  time**, so no token in client code and no per-visitor API rate limit.
- The site lives on its **own branch** (`site`), independent of the app.
- Publishing a release **auto-rebuilds** the site.

---

## 1. Decision log (questions asked → choices made)

These are the actual decisions that shaped the build. Re-answer them for your repo.

| # | Question | Options | Chosen | Why |
|---|---|---|---|---|
| 1 | Framework for the site | Astro / plain HTML+JS / SvelteKit | **Astro** | Content-first, ~0 JS, build-time data fetch, official Pages action. |
| 2 | When to fetch release data | Build time / browser runtime | **Build time** + redeploy on release | No client token, no rate limit, pre-rendered; refreshed by a release trigger. |
| 3 | Branch layout for the site | (A) clean branch, project at root / (B) subfolder | **A** | Keeps `package.json` away from app files; cleanest. Removed leftover app files from `site`. |
| 4 | First pre-release tag | `v0.1.0-alpha.1` (bump Cargo) / `v0.1.0` / `v0.1.0-rc.1` | **`v0.1.0-alpha.1`** | Conventional pre-release semver; version guard requires the manifest to match. |
| 5 | Push the tag now vs hold | Push now / create locally | **Push now** | Run the real build immediately; draft stays private until published. |
| 6 | Where to put the runbook | `main: docs/SETUP.md` / root / `site` | **`main: docs/SETUP.md`** | Project-level docs next to the code they describe. |
| 7 | How to land the NSIS bug fix | reuse tag `alpha.1` / bump `alpha.2` / fix only | **Reuse `alpha.1`** | The failed tag produced no release, so moving it is harmless. |

Process decisions (stated, not from a menu):
- **Push directly to `main`, no PRs** for routine changes; use **feature branches
  instead of a `dev` branch**.
- **Parallelize the macOS build** (perf).

---

## 2. Prerequisites

- A GitHub repo that publishes **Releases** with downloadable assets.
- **GitHub CLI** (`gh`) authenticated: `gh auth login`.
- **Node 20+** and npm (for the Astro site).
- Repo setting: **Settings → Pages → Build and deployment → Source =
  GitHub Actions** (API field `build_type: "workflow"`). Verify with:
  ```sh
  gh api repos/<owner>/<repo>/pages --jq .build_type   # -> "workflow"
  ```
- Repo setting: **Settings → Actions → General → Workflow permissions** — ensure
  workflows can use the scopes the YAML requests (`pages: write`, `id-token: write`,
  `actions: write`). Per-workflow `permissions:` blocks declare these explicitly.

---

## 3. Part A — the Astro releases site (on a `site` branch)

### 3.1 Create the branch layout
The site lives on its own branch with **only** the site at the root (decision #3).
If the branch already has unrelated files, remove them (`git rm …`) so the Astro
project sits at the root.

### 3.2 Project files

`package.json`
```json
{
  "name": "<repo>-releases",
  "type": "module",
  "private": true,
  "scripts": { "dev": "astro dev", "build": "astro build", "preview": "astro preview" },
  "dependencies": { "astro": "^5.6.0", "marked": "^15.0.0" }
}
```

`astro.config.mjs` — **the base path is the #1 gotcha for project pages**:
```js
import { defineConfig } from 'astro/config';
export default defineConfig({
  site: 'https://<owner>.github.io',
  base: '/<repo>',          // project-pages subpath; wrong value => 404'd CSS
  trailingSlash: 'ignore',
});
```

`src/pages/index.astro` — **build-time** fetch (runs in CI, not the browser):
```astro
---
import Base from '../layouts/Base.astro';
import Release from '../components/Release.astro';
const REPO = '<owner>/<repo>';
const token = import.meta.env.GH_TOKEN;          // present in CI only
const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=20`, {
  headers: {
    Accept: 'application/vnd.github+json',
    'X-GitHub-Api-Version': '2022-11-28',
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
  },
});
if (!res.ok && import.meta.env.PROD) throw new Error(`GitHub API ${res.status}`);
const releases = res.ok ? await res.json() : [];
const firstStableIdx = releases.findIndex((r) => !r.prerelease);
---
<Base>
  {releases.length === 0
    ? <p>No releases yet — see the <a href={`https://github.com/${REPO}/releases`}>Releases page</a>.</p>
    : releases.map((r, i) => <Release release={r} latest={i === firstStableIdx} />)}
</Base>
```

`src/components/Release.astro` — one card; badges + asset labels + sizes:
```astro
---
import { marked } from 'marked';
const { release, latest = false } = Astro.props;
function labelFor(n) {
  if (/-setup\.exe$/i.test(n)) return 'Windows installer';
  if (/-x86_64\.zip$/i.test(n)) return 'Windows (portable)';
  if (/\.dmg$/i.test(n)) return 'macOS (Intel + Apple Silicon)';
  if (/SHA256SUMS\.txt$/i.test(n)) return 'Checksums';
  return n;
}
function humanSize(b) {
  if (b == null) return ''; const u = ['B','KB','MB','GB']; let n = b, i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${i ? n.toFixed(1) : n} ${u[i]}`;
}
const notesHtml = release.body ? marked.parse(release.body) : '';  // owner-authored = trusted
---
<article class="release">
  <div class="release-head">
    <h2>{release.name || release.tag_name}</h2>
    {latest && <span class="badge">Latest</span>}
    {release.prerelease && <span class="badge pre">Pre-release</span>}
  </div>
  <p class="date">{new Date(release.published_at).toLocaleDateString('en-US',{year:'numeric',month:'long',day:'numeric'})}</p>
  {notesHtml && <div class="notes" set:html={notesHtml} />}
  <ul class="downloads">
    {(release.assets ?? []).map((a) => (
      <li><a href={a.browser_download_url}>{labelFor(a.name)}</a><span class="size">{humanSize(a.size)}</span></li>
    ))}
  </ul>
</article>
```

`src/layouts/Base.astro` — header/footer + **base-aware favicon** (public/ files
are NOT URL-rewritten by Astro, so prepend `base` yourself):
```astro
---
import '../styles/global.css';   // imported => Astro fingerprints + base-rewrites it
const favicon = `${import.meta.env.BASE_URL.replace(/\/$/, '')}/favicon.svg`;
---
<html lang="en"><head>
  <meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" />
  <link rel="icon" type="image/svg+xml" href={favicon} />
  <title>Releases</title>
</head><body><main><slot /></main></body></html>
```

Plus `src/styles/global.css` (system font, centered column, light/dark via
`prefers-color-scheme`) and `public/favicon.svg`.

### 3.3 Local check
```sh
npm install
npm run build && npm run preview   # confirm no 404'd assets under /<repo>/
```
Build-mode (`PROD`) throws on a failed fetch, so a green build means the API call
worked. Locally the anonymous API (60 req/hr) is used; that's fine.

### 3.4 Site deploy workflow — `.github/workflows/deploy-site.yml` (on `site`)
```yaml
name: Deploy site to Pages
on:
  push:
    branches: [site]
  workflow_dispatch:        # lets the release dispatcher (and you) trigger it
permissions:
  contents: read
  pages: write
  id-token: write
concurrency:
  group: pages
  cancel-in-progress: false
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: withastro/action@v6     # installs deps, builds, uploads Pages artifact
        env:
          GH_TOKEN: ${{ github.token }}   # consumed by the build-time fetch
  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v5
```
Notes:
- No `.nojekyll` needed — the Actions artifact path serves files verbatim (Jekyll
  only runs for branch-source Pages).
- `withastro/action@v6` auto-detects the package manager and uploads the artifact,
  so no separate `configure-pages`/`upload-pages-artifact` steps.

Push to `site` → first deploy. Site URL: `https://<owner>.github.io/<repo>/`.

---

## 4. Part B — auto-refresh the site when a release is published

### 4.1 The cross-branch problem
`release` (and `schedule`, `repository_dispatch`) events **only fire for workflows
on the default branch**. The site's build lives on `site`. So the trigger can't be
in `deploy-site.yml`; it needs a tiny companion on `main` that dispatches it.

### 4.2 `.github/workflows/redeploy-site-on-release.yml` (on `main`)
```yaml
name: Redeploy site on release publish
on:
  release:
    types: [published]      # fires for pre-releases too (only `released` is stable-only)
  workflow_dispatch:
permissions:
  actions: write            # needed to dispatch another workflow
jobs:
  trigger:
    runs-on: ubuntu-latest
    steps:
      - name: Trigger site deploy
        # --repo is REQUIRED: this job has no checkout, so gh can't infer the repo
        # from a local git remote and would fail with "not a git repository".
        run: gh workflow run deploy-site.yml --ref site --repo "${{ github.repository }}"
        env:
          GH_TOKEN: ${{ github.token }}
```
> **Gotcha we hit:** without `--repo`, the dispatcher fails instantly with
> `fatal: not a git repository`. Either pass `--repo` (shown) or add an
> `actions/checkout` step. `--repo` is lighter.

### 4.3 Expected latency
After you publish, the chain takes ~1–2 minutes:
`release:published` → dispatcher (~10s) → Astro build (~40s) → deploy-pages →
Pages CDN propagation. Not instant, but automatic.

---

## 5. Part C — the tag-driven release pipeline (`release.yml` on `main`)

This is the app's build/publish pipeline. The reusable **patterns** below matter
even if your build steps differ.

### 5.1 Shape
- **Trigger:** `on: push: tags: ['v*']` — only tags, never branch pushes.
- **Version guard:** a `version` job asserts the tag (minus `v`) equals the
  manifest version (here `Cargo.toml [workspace.package].version`). Mismatch fails
  fast — so bump the manifest before tagging.
- **Build jobs** produce platform artifacts; a final **`release`** job collects
  them, checksums, and publishes a **DRAFT** via `softprops/action-gh-release@v2`
  (`draft: true`). It does **not** set the pre-release flag — you choose that at
  publish time.

### 5.2 Reusable pattern: parallelize a multi-arch macOS build
Instead of building both arches sequentially on one runner, use a matrix (parallel
runners, per-target caches) + a bundle job that `lipo`s them:
```yaml
  build-macos:
    needs: version
    runs-on: macos-latest
    strategy:
      fail-fast: false
      matrix:
        target: [x86_64-apple-darwin, aarch64-apple-darwin]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: '${{ matrix.target }}' }
      - uses: Swatinem/rust-cache@v2
        with: { key: '${{ matrix.target }}' }
      - run: cargo build --release -p app --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: macos-bin-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/app
          if-no-files-found: error
  bundle-macos:
    needs: [version, build-macos]
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with: { pattern: macos-bin-*, path: macos-bins }
      - run: |
          lipo -create macos-bins/macos-bin-*/app -output universal-app
          # ... bundle .app + .dmg ...
      - uses: actions/upload-artifact@v4
        with: { name: macos-dmg, path: dist/*.dmg, if-no-files-found: error }
```
The final `release` job then `needs: [version, bundle-macos, build-windows]` and
downloads **only the final artifacts by name** (not the intermediate per-arch
binaries, which would collide on `merge-multiple`):
```yaml
      - uses: actions/download-artifact@v4
        with: { name: macos-dmg, path: dist }
      - uses: actions/download-artifact@v4
        with: { name: windows-build, path: dist }
```

### 5.3 Reusable pattern: NSIS version for pre-releases
Windows' NSIS `VIProductVersion` must be a strict numeric `X.X.X.X`. A naive
`"${VERSION}.0"` works for `0.1.0` but breaks for `0.1.0-alpha.1`
(`invalid VIProductVersion format`). Derive a clean numeric version in the
workflow and pass it as a separate define:
```bash
# in the `version` job (bash on the runner):
core="${version%%-*}"; core="${core%%+*}"     # strip -alpha.1 / +meta
IFS=. read -r a b c d <<< "$core"
vi_version="${a:-0}.${b:-0}.${c:-0}.${d:-0}"   # -> 0.1.0.0
echo "vi_version=$vi_version" >> "$GITHUB_OUTPUT"
```
```pwsh
# in the Windows packaging step, pass to makensis:
& $nsis "/DVERSION=$ver" "/DVIVERSION=${{ needs.version.outputs.vi_version }}" ... scripts\installer.nsi
```
```nsis
; installer.nsi
!ifndef VIVERSION
  !define VIVERSION "0.0.0.0"
!endif
VIProductVersion "${VIVERSION}"
```
The human-readable `VERSION` stays for filenames and string version fields.

---

## 6. Cutting a (pre-)release — the runbook

1. **Bump the manifest version** to match the tag (incl. lockfile), commit to `main`.
   For a pre-release use a semver suffix, e.g. `0.1.0-alpha.1`.
2. **Tag and push:**
   ```sh
   git tag v0.1.0-alpha.1
   git push origin v0.1.0-alpha.1
   ```
3. `release.yml` builds and leaves a **draft** release with the artifacts +
   `SHA256SUMS.txt`. Watch it: `gh run watch <id> --exit-status`.
4. **Publish the draft.** For a pre-release, check **"Set as a pre-release"**
   (or pre-flag the draft: `gh release edit <tag> --prerelease`).
5. Publishing fires `redeploy-site-on-release.yml` → the site rebuilds and shows
   the release. Pre-releases get a **"Pre-release"** badge and never the "Latest"
   badge (which only goes to the newest stable release).

**Move/rollback a tag** (safe only if it produced no real release):
```sh
git push origin :refs/tags/v0.1.0-alpha.1     # delete remote
git tag -d v0.1.0-alpha.1                      # delete local
git tag v0.1.0-alpha.1 <commit> && git push origin v0.1.0-alpha.1
```

---

## 7. Gotchas & lessons (things that bit us)

1. **Project-pages base path.** Set `base: '/<repo>'` in `astro.config.mjs`. Import
   CSS (don't hand-write `<link href="/...">`) so Astro rewrites it; for `public/`
   assets prepend `import.meta.env.BASE_URL` manually.
2. **`release` events only fire on the default branch** — hence the `main`-side
   dispatcher for a `site`-branch build.
3. **`gh workflow run` needs a repo context** — pass `--repo` when the job has no
   checkout (`not a git repository` otherwise).
4. **NSIS `VIProductVersion`** must be numeric `X.X.X.X` — derive it, strip the
   pre-release suffix (§5.3).
5. **`download-artifact` name collisions** — when fanning out a matrix, download
   only the final artifacts by name in the collector job, or `merge-multiple` will
   clobber same-named files.
6. **Caching is already there** (`Swatinem/rust-cache@v2`) — the *first* build is
   cold; warm builds restore `target/` deps. Cache key = lockfile + rustc, so a
   version bump or floating `stable` toolchain busts it.
7. **Action Node-20 deprecation** — `actions/checkout@v4` /
   `upload-artifact@v4` will be forced to Node 24 after 2026-06-16; bump to `@v5`
   when convenient.
8. **No CI on direct `main` pushes** — `ci.yml` triggers on `dev` pushes + PRs, so
   the feature-branch-straight-to-`main` flow skips CI. Run `fmt`/`clippy`/`test`
   locally, or add `push: [main]` to the triggers.

---

## 8. Replication checklist

- [ ] Repo publishes Releases with named assets.
- [ ] `gh auth login`; Pages source set to **GitHub Actions**.
- [ ] Create `site` branch; add the Astro project at root (§3.2) with the correct
      `base`.
- [ ] Add `deploy-site.yml` on `site`; push → first Pages deploy works.
- [ ] Add `redeploy-site-on-release.yml` on `main` (with `--repo`).
- [ ] (If building installers) add the NSIS numeric-version derivation (§5.3).
- [ ] (Optional) parallelize multi-arch builds (§5.2).
- [ ] Tag a (pre-)release; publish it; confirm the site auto-refreshes (~1–2 min).
