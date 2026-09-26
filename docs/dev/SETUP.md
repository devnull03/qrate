# Setup & Release Runbook

How to stand this project up from scratch and how its CI/CD works. This is the
"do these things first" companion to `README.md` — especially before cutting a
release, since a release depends on several pieces being configured ahead of time.

---

## 1. What's in the repo

| Branch | Contents | Purpose |
|---|---|---|
| `main` | The Rust **GPUI** desktop app (`crates/*`) + release pipeline | Default branch and source of truth. Routine work lands here directly via short-lived **feature branches** (no PR required); tag-driven releases are cut from here. |
| `site` | An **Astro** site (no Rust) | `qrate.dvnl.work`, served by a Cloudflare Worker. Mostly prerendered; `/oauth/config` is a live endpoint the app depends on (`site-oauth-handoff.md`). |

> Branch features off `main` and land them back on `main` directly.

Workspace crates live under `crates/` and inherit `[workspace.package].version` (except
`qrate-export`); the binary is `crates/app` (`app`). The crate map in `CLAUDE.md` says what
each one holds.

---

## 2. Local prerequisites

**App (on `main`):**
- Rust **stable** toolchain with `rustfmt` + `clippy`. Run `rustup update stable`
  periodically to match CI.
- The app targets **Windows, macOS and Linux**; CI and the release pipeline cover all
  three. Linux needs gpui's system libraries first — the `apt-get` line in
  `.github/workflows/ci.yml` is the current list.

**Site (on `site`):**
- Bun. `bun install`, then `bun run dev` (`http://localhost:4321/` — the `/qrate`
  base went away with the custom domain). The dev server serves `/oauth/config`
  too, so the endpoint can be exercised without deploying.

**Google sign-in (optional locally):** the client id and secret are read at *compile* time by
`option_env!`, so they have to be in the environment before `cargo run` and a change to them needs
a rebuild of `data-exchange`. From the OAuth client JSON Google hands you (type **Desktop app**):

```powershell
$env:QRATE_GOOGLE_CLIENT_ID    = "…apps.googleusercontent.com"
$env:QRATE_GOOGLE_CLIENT_SECRET = "…"
cargo run
```

A build without them still compiles and runs — Google sign-in reports that this build has no
client id. Never commit the JSON; `.gitignore` covers `client_secret_*.json`, and the values
themselves belong in the environment or in the credential endpoint (`site-oauth-handoff.md`),
never in source.

**Plugins in local Windows builds:** use the development runner to read the public catalog key,
build qrate, register the debug executable for `qrate://`, run it, and remove that temporary
registration when qrate exits:

```powershell
.\scripts\run-dev.ps1
```

Debug builds use `http://localhost:4321` for every qrate site route, including plugin discovery,
catalog verification, feedback, Google configuration and Picker, and release pages. Release builds
use `https://qrate.dvnl.work`. A self-hosted build can set `QRATE_SITE_ORIGIN` at compile time to
use its own site for all of those routes. Updates do not go through the site (§2a).

This requires an authenticated GitHub CLI. To register an already-built debug executable without
running it, use:

```powershell
.\scripts\register-dev-protocol.ps1
```

Browser links then launch `target\debug\app.exe`; if that development instance is already running,
the new process hands the link to it and exits. Remove the development override before testing an
installed build:

```powershell
.\scripts\register-dev-protocol.ps1 -Unregister
```

**Preview binaries:** `./scripts/fetch-binaries.sh` (or `.ps1` on Windows) downloads PDFium and
ffmpeg beside the debug executable. Both are pinned to one release and checked against its SHA-256
(PDFium `chromium/7881`, the build `pdfium-render`'s `pdfium_latest` binds; ffmpeg BtbN LGPL
`n8.1.2-50-g1a748fe2cd` from `autobuild-2026-08-31-13-27` on Windows and Linux, Homebrew on macOS).
A hash that does not match is a failed download. To move a pin, change the release and every hash
in both scripts together.

### 2a. Where updates come from, and the download source

The update check reads the signed `update-manifest.json` straight from a GitHub release:

| Build | Feed |
|---|---|
| Stable (`0.6.0`) | `https://github.com/devnull03/qrate/releases/latest/download/update-manifest.json`. GitHub's latest release is never a pre-release. |
| Pre-release (`0.6.0-beta.1`) | The newest release by SemVer that carries `update-manifest.json`, stable or not, found through `api.github.com/repos/devnull03/qrate/releases`, then that tag's `update-manifest.json`. |

The signature, channel, version and artifact rules (`updater::select_update`) are the same as
before; only the address changed. A release reaches installed apps when its draft is published.
Development builds never check for updates, so a real test of this path needs a packaged build with
an install marker.

**Download source.** One app-wide setting, **Settings ▸ Application ▸ Updates and downloads ▸
Download source** (`updater::DOWNLOAD_SOURCE_KEY`), points updates and the plugin catalog at a
mirror or a local folder. The environment variable `QRATE_DOWNLOAD_SOURCE` overrides it. Empty means
the defaults, and a value qrate cannot use is ignored with a warning in the log. It accepts
`https://` anywhere, `http://` only on `localhost` or a loopback address, and `file:///` folders.
A mirror has this layout:

```
<source>/updates/stable.json      the envelope a stable build should see (copy of update-manifest.json)
<source>/updates/beta.json        the envelope a pre-release build should see
<source>/plugins/catalog.json     the signed plugin catalog, and catalog.json.sig beside it
<source>/github/<path>            any https://github.com/<path> release asset, e.g.
                                  github/devnull03/qrate/releases/download/v0.6.0/qrate-0.6.0-setup.exe
```

The mirror is untrusted. The update envelope and the catalog are signed, and every artifact is
checked against the size and SHA-256 in the signed manifest before anything is rewritten, so a
mirror can withhold a download but cannot change one. When the mirror answers 404 (or the file is
missing from a folder), qrate logs a warning and takes that one file from GitHub or the site
instead, so a partial mirror still works. Any other failure is reported as it is.

```bash
mkdir -p mirror/updates mirror/github/devnull03/qrate/releases/download/v0.6.0-beta.1
cp dist/update-manifest.json mirror/updates/beta.json
cp dist/qrate-0.6.0-beta.1-setup.exe mirror/github/devnull03/qrate/releases/download/v0.6.0-beta.1/
(cd mirror && python -m http.server 8000)
QRATE_DOWNLOAD_SOURCE=http://127.0.0.1:8000 ./qrate      # or file:///C:/path/to/mirror
```

### 2b. Optional components

PDFium, ffmpeg, the Pi agent runtime and the CLIP weights can also be installed on demand, into
`<data dir>/components/<id>/<version>` (`crates/components`, `docs/dev/components-plan.md`). Each
release carries them as `component-<id>-<version>-<os>-<arch>.tar.gz` (the CLIP weights as an
uncompressed `component-clip-<revision>-any-any.tar`) and lists them in a signed `components.json`.
qrate reads that file from **its own version's tag**, through the download source, so a
development build whose version has no release finds none. The CLIP weights are the exception:
their default source is Hugging Face at the pinned revision, which needs no manifest, and the
release copy is the fallback (the `clip_source` setting can reverse the two).

To test components locally, sign a manifest with a development key. Debug builds trust it under the
key id `qrate-dev` when `QRATE_DEV_SIGNING_KEY` holds its public half; release builds never do.

```bash
openssl genpkey -algorithm ed25519 -out dev-key.pem          # once
pub="$(openssl pkey -in dev-key.pem -pubout -outform DER | tail -c 32 | base64 | tr '+/' '-_' | tr -d '=\n')"
dir=mirror/github/devnull03/qrate/releases/download/v0.6.0-beta.1   # the version in Cargo.toml
./scripts/package-component.sh pdfium pdfium-src windows x86_64 "$dir"   # pdfium-src holds pdfium.dll
QRATE_UPDATE_SIGNING_KEY="$(cat dev-key.pem)" QRATE_SIGNING_KEY_ID=qrate-dev \
  ./scripts/build-components-manifest.sh "$dir" v0.6.0-beta.1
QRATE_DOWNLOAD_SOURCE=file:///C:/path/to/mirror QRATE_DEV_SIGNING_KEY="$pub" cargo run
```

`package-component.sh` archives the *contents* of the folder it is given and takes the version
from the pin in the fetch script, so an archive cannot carry a version it is not. It packs the
ffmpeg component only with the build's `LICENSE.txt` beside the binary (LGPL).

---

## 3. One-time GitHub setup (do this before the first release)

These are the "things to set up beforehand" — without them the pipelines fail or
produce nothing visible.

1. **Cloudflare deploy hook.** Cloudflare → the qrate Worker → Settings → Builds →
   Deploy hooks. Store the URL as the repo secret `CLOUDFLARE_DEPLOY_HOOK`; it is
   what `redeploy-site-on-release.yml` posts to (§4). Verify by hand once:
   `curl -fsS -X POST "<hook>"` → `{"result":{...},"success":true}`.

2. **Actions permissions.** Settings → Actions → General → Workflow permissions:
   allow workflows to write (the release job needs `contents: write`). These are
   also declared per-workflow.

3. **Branch protection (optional).** This repo pushes directly to `main`, so there
   is no PR gate by default. If you want one later, protect `main` and require the
   `CI` checks — but note `ci.yml` only runs on PRs / `dev` pushes today (§4), and
   tag pushes bypass protection by design.

4. **Code signing (optional, currently OFF).** Releases are **unsigned**:
   - Windows: SmartScreen "unknown publisher" warning.
   - macOS: Gatekeeper quarantine (`xattr -dr com.apple.quarantine ...`).
   To sign later you'd add secrets (Apple Developer ID cert + notarization creds,
   a Windows code-signing cert) and signing steps in `release.yml`. None exist yet,
   so no code-signing secrets are required today. This is separate from the update
   signing key in step 6.

5. **Google credentials.** Actions secrets, mapped into the `QRATE_*` build vars by
   `release.yml` — nothing else needs editing. Only one of the three is required:

   | Secret | Needed? |
   |---|---|
   | `QRATE_GOOGLE_CONFIG_TOKEN` | **Yes.** Without it the binary sends an empty bearer, the credential endpoint answers 401, and a fresh install can never sign in at all. |
   | `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` | Optional. The live pair comes from the endpoint at runtime; these are only the last rung, for a first-ever sign-in while **our** endpoint is down but Google is up. |

   Rotating the Cloud project does not need a release — that is the whole point of
   the endpoint (`site-oauth-handoff.md`).

6. **Update signing key.** The `release` job signs `update-manifest.json` with
   `QRATE_UPDATE_SIGNING_KEY`, a secret of the `release-signing` environment.
   `scripts/provision-update-key.sh` generates the Ed25519 pair once, patches the public
   half into `crates/updater` and the site's feed route, and stores the private half in
   that environment. Without it, the release job cannot sign the manifest the in-app updater
   trusts.

---

## 4. CI/CD pipelines

Six workflows cover CI, build caches, releases, the export package, and site deployment.

### `ci.yml` — quality gate
- **Triggers:** push to `dev`; PRs targeting `dev` or `main`; manual `workflow_dispatch`.
- **Does:** on Windows, macOS, and Linux, runs `cargo fmt --check`,
  `cargo clippy … -D warnings`, and `cargo nextest`. Cancels superseded runs to save minutes.
- **Cache:** restores dependency artifacts that `warm-ci-cache.yml` saved on `main`. It does not save
  PR-local caches because GitHub confines those caches to one PR and they consume the shared quota.
- **Heads-up:** it does **not** run on direct pushes to `main`. With the
  feature-branch-straight-to-`main` flow, run `cargo fmt`/`clippy`/`test` locally
  first, or open a PR (which does trigger it). To gate direct pushes, add
  `push: [main]` to the triggers.

### `warm-ci-cache.yml` — reusable CI dependency cache
- **Trigger:** a dependency manifest, lockfile, Rust toolchain, or CI cache configuration changes on
  `main`; it can also be run manually.
- **Does:** builds the same clippy dependency variants as CI on all three operating systems and saves
  them on the default branch, where every subsequent PR can restore them.

### `release.yml` — build & publish artifacts (on `main`)
- **Trigger:** pushing a tag matching `v*`. Merging to `main` alone does nothing.
- **Guard:** the `version` job asserts the tag (minus `v`) **exactly equals**
  `Cargo.toml`'s `[workspace.package].version`. Mismatch ⇒ the build fails fast.
- **Builds:**
  - macOS: the two arches (x86_64 + aarch64) build **in parallel** (a matrix on
    separate runners with per-target caches); a `bundle-macos` job then `lipo`s them
    into a universal `.app` → `.dmg` (`scripts/bundle-mac.sh`).
  - Windows: `.exe` → portable `*-x86_64.zip` + NSIS `*-setup.exe`
    (`scripts/installer.nsi`) + per-machine WiX `*-x86_64.msi` (`scripts/installer.wxs`).
    The job derives a numeric `VIProductVersion` (`X.X.X.X`) from the tag, so semver
    pre-releases (e.g. `0.1.0-alpha.1`) package cleanly instead of tripping NSIS's strict
    version format.
  - Linux: binary + `.desktop` + icon in a `*-x86_64-linux.tar.gz`.
  - Every platform also builds the `qrate-update-helper` binary and bundles PDFium and the
    pinned Pi agent (`docs/dev/agent-runtime.md`); Windows bundles ffmpeg too.
  - Beside those full bundles, which do not change, each job packs the optional components
    (§2b) with `scripts/package-component.sh`: PDFium and Pi for Windows, Linux and each macOS
    architecture (from the `build-macos` matrix, not the universal dmg), and ffmpeg for Windows
    and Linux with its LGPL `LICENSE.txt`. macOS has no ffmpeg component; it stays Homebrew's.
  - The `release` job downloads the pinned CLIP weights from Hugging Face
    (`scripts/fetch-clip-weights.sh`, checked against their SHA-256) and packs them. If Hugging Face
    does not serve them, it takes the copy an earlier release carried, checked against the same
    pins, so the weights stay available release after release.
- **Publishes:** a **DRAFT** release with the artifacts, the `component-*` archives,
  `SHA256SUMS.txt` (covering the artifacts and the component archives), the signed
  `update-manifest.json`, and the signed `components.json` (`scripts/build-components-manifest.sh`,
  same key, payload kind `qrate-components`). The manifest step fails if a component is missing for
  any platform the jobs build, and it verifies its own signature before writing. A version with a
  `-` suffix (e.g. `0.5.0-beta.1`) is marked as a pre-release automatically.
- **Size:** each release carries the 607 MB CLIP archive and roughly 100 MB of components per
  platform on top of the full bundles.

### `warm-release-cache.yml` — release dependency cache
- **Trigger:** `Cargo.lock` or the toolchain changes on `main`, every third day on a schedule,
  or manually.
- **Does:** saves the dependency caches `release.yml` restores on `main`, because a tag run can
  only read caches from its own ref or the default branch.

### `publish-export.yml` — the `qrate-export` WASM package
- **Trigger:** a push to `main` that touches `crates/qrate-export`, or manually.
- **Does:** builds `crates/qrate-export` with `wasm-pack` (`--features wasm`) and publishes it
  to GitHub Packages as `@devnull03/qrate-export`, linked to this repo through the crate's
  `repository` field. It skips the publish when that crate version is already there, so bump
  `crates/qrate-export`'s version to release a new package.
- **Consumers:** GitHub's npm registry needs a token even for a public package. The site's
  `.npmrc` reads it from `GH_TOKEN`, the same build variable the releases page uses, so that one
  Cloudflare build variable must be a classic token with `read:packages`.

### Building the site — Cloudflare Workers Builds (on `site`)
`qrate.dvnl.work` is served by a Cloudflare Worker, which Cloudflare rebuilds on
each commit to `site`. Nothing in this repo drives that; it is configured on the
Cloudflare side. The build fetches the **release list at build time**, so the
releases page only ever shows what existed when it last ran.

GitHub Pages is off for this repo, and `deploy-site.yml` — the workflow that used
to publish a second, unvisited copy there — has been removed from `site`. The
Cloudflare Worker above is the only thing that serves `qrate.dvnl.work`.

### `redeploy-site-on-release.yml` — refresh the site on release (on `main`)
- **Trigger:** `release: published` (and `workflow_dispatch`).
- **Why it lives on `main`:** `release` events only fire for workflows on the
  **default branch**.
- **Does:** `POST`s the Cloudflare deploy hook (`CLOUDFLARE_DEPLOY_HOOK`). A
  release publish makes no commit anywhere, so without this Cloudflare has no
  reason to rebuild and the site silently keeps showing the previous release.

```
tag vX.Y.Z ─▶ release.yml (build dmg/zip/exe) ─▶ DRAFT release
                                                     │ (you publish, optionally pre-release)
                                                     ▼
                                          release: published
                                                     │
                              redeploy-site-on-release.yml (on main)
                                                     │ POST the deploy hook
                                                     ▼
                                  Cloudflare Workers Builds ─▶ qrate.dvnl.work updates
```

---

## 5. Cutting a release (runbook)

1. **Land the code** on `main` (push your feature branch to `main`). Run
   `cargo fmt`/`clippy`/`test` locally first — direct pushes skip CI (§4).
2. **Bump the version** in `Cargo.toml` `[workspace.package].version` (inherited by
   every crate except `qrate-export`, which versions its npm package on its own), and sync
   the lockfile (any cargo build updates the member versions in `Cargo.lock`). Commit to `main`.
3. **Tag and push** — the tag must match the version exactly:
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. **Wait for `release.yml`** to finish; it leaves a **draft** release with the
   `.dmg`, `.zip`, `-setup.exe`, `.msi`, `.tar.gz`, `SHA256SUMS.txt`,
   `update-manifest.json`, the `component-*` archives, and `components.json`.
5. **Publish the draft** (Releases → edit the draft → *Publish release*). This is
   when the release becomes visible to the API, to the site, and to installed apps
   checking for updates (§2a).
6. Publishing fires `redeploy-site-on-release.yml` → the site rebuilds with the new
   release.

### Pre-releases (alpha / rc / beta)
- Use a **semver pre-release version**, e.g. `0.1.0-alpha.1`, in `Cargo.toml`, and
  tag `v0.1.0-alpha.1` (the version guard requires the match — `0.1.0-alpha.1`
  is a valid Cargo version). The Windows installer's numeric version is derived
  automatically (§4), so the `-alpha.N` suffix needs no manual handling.
- `release.yml` already marks such a draft as a pre-release (API `prerelease: true`); leave
  "Set as a pre-release" checked when you publish it. The site renders it with a
  **"Pre-release"** badge and never awards it the **"Latest"** badge — "Latest" only goes
  to the newest *stable* (non-pre-release) release.
- Drafts are hidden from the public API, so an in-progress release never leaks to
  the site until you publish it.

### Rolling back a bad tag
```sh
git push origin :refs/tags/v0.1.0   # delete remote tag
git tag -d v0.1.0                    # delete local tag
```
Then delete the draft/release in the GitHub UI if one was created.
