# Setup & Release Runbook

How to stand this project up from scratch and how its CI/CD works. This is the
"do these things first" companion to `README.md` — especially before cutting a
release, since a release depends on several pieces being configured ahead of time.

---

## 1. What's in the repo

| Branch | Contents | Purpose |
|---|---|---|
| `main` | The Rust **GPUI** desktop app (`crates/*`) + release pipeline | Default branch and source of truth. Routine work lands here directly via short-lived **feature branches** (no PR required); tag-driven releases are cut from here. |
| `site` | An **Astro** static site (no Rust) | The public releases page deployed to GitHub Pages. Independent of the app. |

> Branch features off `main` and land them back on `main` directly.

Workspace crates (versions are inherited from `[workspace.package].version`):
`crates/app` (binary `app`), `crates/ai`, `crates/settings`,
`crates/window-wrapper`.

---

## 2. Local prerequisites

**App (on `main`):**
- Rust **stable** toolchain with `rustfmt` + `clippy`. Run `rustup update stable`
  periodically to match CI.
- The app only targets **Windows + macOS** (gpui needs heavy system libs on Linux),
  so build/test there.

**Site (on `site`):**
- Node 20+ and npm. `npm install`, then `npm run dev`
  (`http://localhost:4321/qrate/`).

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
themselves belong in the environment or in the credential endpoint (`docs/site-oauth-handoff.md`),
never in source.

---

## 3. One-time GitHub setup (do this before the first release)

These are the "things to set up beforehand" — without them the pipelines fail or
produce nothing visible.

1. **GitHub Pages → GitHub Actions source.**
   Settings → Pages → Build and deployment → Source = **GitHub Actions**
   (API `build_type: "workflow"`). Required for `deploy-site.yml` to publish.
   Verify: `gh api repos/devnull03/qrate/pages --jq .build_type` → `workflow`.

2. **Actions permissions.** Settings → Actions → General → Workflow permissions:
   allow workflows to write (the release job needs `contents: write`; the site
   dispatcher needs `actions: write`). These are also declared per-workflow.

3. **Branch protection (optional).** This repo pushes directly to `main`, so there
   is no PR gate by default. If you want one later, protect `main` and require the
   `CI` checks — but note `ci.yml` only runs on PRs / `dev` pushes today (§4), and
   tag pushes bypass protection by design.

4. **Code signing (optional, currently OFF).** Releases are **unsigned**:
   - Windows: SmartScreen "unknown publisher" warning.
   - macOS: Gatekeeper quarantine (`xattr -dr com.apple.quarantine ...`).
   To sign later you'd add secrets (Apple Developer ID cert + notarization creds,
   a Windows code-signing cert) and signing steps in `release.yml`. None exist yet,
   so no secrets are required to build today.

5. **Custom domain (optional).** Settings → Pages writes a `CNAME`; then set
   `site:` in `astro.config.mjs` to the domain and drop/empty `base`.

6. **Google credentials (only if release builds should sign in).** Add `GOOGLE_CLIENT_ID`,
   `GOOGLE_CLIENT_SECRET` and `GOOGLE_CONFIG_TOKEN` as Actions secrets. `release.yml` already maps
   all three into the `QRATE_*` build vars, so nothing else needs editing. They are the fallback
   the binary carries; the live pair comes from the credential endpoint at runtime, so rotating the
   Cloud project does **not** need a new release.

---

## 4. CI/CD pipelines

Four workflows, two on `main`, one on `site`, and CI on `dev`/PRs.

### `ci.yml` — quality gate (on `main`)
- **Triggers:** push to `dev`; PRs targeting `dev` or `main`.
- **Does:** on Windows + macOS, runs `cargo fmt --check`, `cargo clippy … -D warnings`,
  `cargo test`, `cargo build`. Cancels superseded runs to save minutes.
- **Heads-up:** it does **not** run on direct pushes to `main`. With the
  feature-branch-straight-to-`main` flow, run `cargo fmt`/`clippy`/`test` locally
  first, or open a PR (which does trigger it). To gate direct pushes, add
  `push: [main]` to the triggers.

### `release.yml` — build & publish artifacts (on `main`)
- **Trigger:** pushing a tag matching `v*`. Merging to `main` alone does nothing.
- **Guard:** the `version` job asserts the tag (minus `v`) **exactly equals**
  `Cargo.toml`'s `[workspace.package].version`. Mismatch ⇒ the build fails fast.
- **Builds:**
  - macOS: the two arches (x86_64 + aarch64) build **in parallel** (a matrix on
    separate runners with per-target caches); a `bundle-macos` job then `lipo`s them
    into a universal `.app` → `.dmg` (`scripts/bundle-mac.sh`).
  - Windows: `.exe` → portable `*-x86_64.zip` + NSIS `*-setup.exe`
    (`scripts/installer.nsi`). The job derives a numeric `VIProductVersion`
    (`X.X.X.X`) from the tag, so semver pre-releases (e.g. `0.1.0-alpha.1`) package
    cleanly instead of tripping NSIS's strict version format.
- **Publishes:** a **DRAFT** release with the artifacts + `SHA256SUMS.txt`. It does
  **not** set the pre-release flag — you choose that when you publish (§5).

### `deploy-site.yml` — build & deploy the site (on `site`)
- **Triggers:** push to `site`; `workflow_dispatch`.
- **Does:** `withastro/action` builds the Astro site, which **fetches the release
  list at build time** using the job's `GITHUB_TOKEN` (1000 req/hr, no token in the
  shipped HTML), then `actions/deploy-pages` publishes it.

### `redeploy-site-on-release.yml` — refresh the site on release (on `main`)
- **Trigger:** `release: published` (and `workflow_dispatch`).
- **Why it lives on `main`:** `release` events only fire for workflows on the
  **default branch**. The site's build is on `site`, so this small workflow bridges
  the gap: it runs `gh workflow run deploy-site.yml --ref site`.
- **Net effect:** publishing a release ⇒ the site rebuilds and shows it.

```
tag vX.Y.Z ─▶ release.yml (build dmg/zip/exe) ─▶ DRAFT release
                                                     │ (you publish, optionally pre-release)
                                                     ▼
                                          release: published
                                                     │
                              redeploy-site-on-release.yml (on main)
                                                     │ gh workflow run --ref site
                                                     ▼
                                    deploy-site.yml (on site) ─▶ Pages updates
```

---

## 5. Cutting a release (runbook)

1. **Land the code** on `main` (push your feature branch to `main`). Run
   `cargo fmt`/`clippy`/`test` locally first — direct pushes skip CI (§4).
2. **Bump the version** in `Cargo.toml` `[workspace.package].version` (inherited by
   all crates), and sync the lockfile (any cargo build updates the member versions
   in `Cargo.lock`). Commit to `main`.
3. **Tag and push** — the tag must match the version exactly:
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. **Wait for `release.yml`** to finish; it leaves a **draft** release with the
   `.dmg`, `.zip`, `-setup.exe`, and `SHA256SUMS.txt`.
5. **Publish the draft** (Releases → edit the draft → *Publish release*). This is
   when the release becomes visible to the API and to the site.
6. Publishing fires `redeploy-site-on-release.yml` → the site rebuilds with the new
   release.

### Pre-releases (alpha / rc / beta)
- Use a **semver pre-release version**, e.g. `0.1.0-alpha.1`, in `Cargo.toml`, and
  tag `v0.1.0-alpha.1` (the version guard requires the match — `0.1.0-alpha.1`
  is a valid Cargo version). The Windows installer's numeric version is derived
  automatically (§4), so the `-alpha.N` suffix needs no manual handling.
- When publishing the draft, **check "Set as a pre-release"** (API `prerelease: true`).
  The site renders it with a **"Pre-release"** badge and never awards it the
  **"Latest"** badge — "Latest" only goes to the newest *stable* (non-pre-release)
  release.
- Drafts are hidden from the public API, so an in-progress release never leaks to
  the site until you publish it.

### Rolling back a bad tag
```sh
git push origin :refs/tags/v0.1.0   # delete remote tag
git tag -d v0.1.0                    # delete local tag
```
Then delete the draft/release in the GitHub UI if one was created.
