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

Workspace crates (versions are inherited from `[workspace.package].version`):
`crates/app` (binary `app`), `crates/ai`, `crates/settings`,
`crates/window-wrapper`.

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
   so no secrets are required to build today.

5. **Google credentials.** Actions secrets, mapped into the `QRATE_*` build vars by
   `release.yml` — nothing else needs editing. Only one of the three is required:

   | Secret | Needed? |
   |---|---|
   | `GOOGLE_CONFIG_TOKEN` | **Yes.** Without it the binary sends an empty bearer, the credential endpoint answers 401, and a fresh install can never sign in at all. |
   | `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` | Optional. The live pair comes from the endpoint at runtime; these are only the last rung, for a first-ever sign-in while **our** endpoint is down but Google is up. |

   Rotating the Cloud project does not need a release — that is the whole point of
   the endpoint (`site-oauth-handoff.md`).

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

### Building the site — Cloudflare Workers Builds (on `site`)
`qrate.dvnl.work` is served by a Cloudflare Worker, which Cloudflare rebuilds on
each commit to `site`. Nothing in this repo drives that; it is configured on the
Cloudflare side. The build fetches the **release list at build time**, so the
releases page only ever shows what existed when it last ran.

`deploy-site.yml` on `site` still publishes the static half to GitHub Pages. That
is now a second, unvisited copy — decide whether to keep it, but don't mistake a
green run there for the live site updating.

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
