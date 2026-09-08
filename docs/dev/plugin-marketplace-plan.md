# Plugin marketplace plan

## Decision

Build a separate public `qrate-plugin-registry` repository for curated plugin metadata and
release verification. Keep the public website and documentation on qrate's existing `site`
branch. Keep each plugin's source and releases in its author's repository.

The application installs versioned HTTPS release artifacts pinned by size and SHA-256. It does not
run `git clone`. The direct-install field accepts a GitHub repository or release URL, so users can
still paste the link authors share. qrate resolves it to a versioned release artifact.

This leaves the Lua runtime model simple while giving the marketplace a clear review and trust
boundary.

## Goals

- Let users discover curated qrate plugins on the website and in qrate.
- Let an author submit a public plugin through GitHub issues and pull requests.
- Install and update reviewed release packages without requiring Git on a user's machine.
- Permit direct installation of an unlisted public GitHub plugin, with an explicit trust warning.
- Use browser install buttons that open qrate at a confirmation screen.
- Preserve manual folder installation for local, institutional, offline, and development plugins.
- Make package identity, compatibility, integrity, permission changes, and revocation explicit.

## Non-goals for the first release

- A package manager for Luau dependencies.
- Generic Git remotes, SSH URLs, private repositories, branches, commits, submodules, or Git LFS.
- Automatically enabling newly installed plugins.
- Automatically installing plugin updates.
- A hosted database, user account system, payment system, or package-upload service.
- Claiming that an official listing makes third-party code safe.

## Existing constraints

Plugins are currently ordinary files in qrate's application-data plugins directory. A plugin is
either a single `<name>.lua` file or `<name>/init.lua`, and a folder plugin can require sibling Lua
modules. This is the runtime package format and should remain so.

The current Lua table returned by `init.lua` is the runtime descriptor. It is not appropriate
marketplace metadata because discovering a descriptor would require executing untrusted Lua.

The plugin host currently recognizes API version 1 and `net` as its one permission. The installer
must validate API compatibility before a plugin is installed; the runtime remains authoritative for
the permissions a loaded plugin actually declares.

The updater already verifies Ed25519-signed metadata, semantic versions, and SHA-256 artifact
hashes. Plugin distribution should reuse those trust concepts and implementation patterns, with a
separate plugin-registry signing key.

The `site` branch is already an isolated Astro project with its own deployment and plugin
documentation. There is no current benefit to moving it before marketplace work starts.

## Repository boundaries

| Repository or branch | Owns | Does not own |
| --- | --- | --- |
| `qrate` `main` | Desktop installer, plugin host, install UI, deep links, embedded registry public key, user docs | Curated listings and third-party source |
| `qrate-plugin-template` | Canonical Lua API definitions, example plugin, package manifest, author release tooling | Registry data |
| Plugin author repositories | Plugin source, tests, release artifacts, release notes, support | Official discoverability by itself |
| `qrate-plugin-registry` | Curated records, schemas, validation CI, signed catalog, submission and incident policy | Website implementation or plugin source |
| `qrate` `site` branch | Marketplace pages, author guides, install links, and build-time catalog rendering | Catalog signing or plugin review |

### Why the registry is a separate repository

The registry needs an independent contributor and review boundary. A marketplace listing is a
reviewed statement about a particular release artifact, its hash, compatibility, publisher, and
permissions. It should not be entangled with website styling, Rust application work, or the source
history of independent plugins.

One static signed catalog can serve both qrate and the website. This avoids an application-owned
database and gives maintainers an auditable pull-request history for every listed version.

### Alternatives rejected

| Alternative | Why not |
| --- | --- |
| Store registry entries in the `site` branch | Couples catalog review and signing to website changes and deployment; makes contributor permissions less clear |
| Move the site to `qrate-site` immediately | Adds migration work but no present technical advantage; revisit when ownership or deployment needs diverge |
| A central repository holding all plugin source | Creates false ownership of third-party plugins and couples unrelated release schedules |
| GitHub topic/search as discovery | Search cannot pin a reviewed artifact, verify integrity, express compatibility, or revoke a release |
| Clone arbitrary Git URLs from qrate | Depends on Git or a large Git library and introduces mutable refs, credentials, SSH, LFS, submodules, and interactive failure modes |

Move the `site` branch into a separate repository only when site ownership, site issue triage,
release cadence, branch protections, or a substantial server component make the branch an actual
operational constraint.

## Package format

Each distributable release includes a non-executable manifest at its package root:

```json
{
  "schema": 1,
  "id": "org.islandora.vocabularies",
  "name": "Islandora vocabularies",
  "version": "1.2.0",
  "api_version": 1,
  "entry": "init.lua",
  "description": "Checks and suggests Islandora taxonomy terms.",
  "homepage": "https://github.com/devnull03/qrate-islandora-plugin",
  "license": "MIT",
  "permissions": ["net"]
}
```

`qrate-plugin.json` is distribution metadata, not a replacement for the Lua descriptor:

- `id` is a stable reverse-domain-style distribution identity, distinct from a local folder name.
- `version` is SemVer and matches the release record.
- `api_version` tells the installer what host version the package needs.
- `entry` is a safe relative path under the package root.
- `permissions` lets qrate show an install preview but must agree with the loaded descriptor.
- The Lua descriptor remains the runtime source of hook, setting, and command declarations.

Authors publish an explicit versioned archive such as
`qrate-islandora-vocabularies-1.2.0.zip`. The archive contains its manifest, entry file, sibling Lua
modules, README, license, and optional editor types at the archive root.

Do not use GitHub-generated "Source code (zip)" downloads as the package contract. A GitHub Release
asset has an intentional layout and can be checked against the hash in the registry.

GitHub lets a publisher replace a release asset. The URL is not the content identity. The registry
pins the reviewed bytes with the SHA-256 and size. The app rejects replacement bytes at that URL.

## Catalog and trust

The registry publishes these static files:

```text
catalog.json
catalog.json.sig
```

The app embeds the registry's Ed25519 public key. It downloads the catalog, verifies its signature,
validates its schema, and caches only the last valid result. A record contains:

- distribution ID, name, description, categories, icon/screenshots, license, and support links;
- publisher GitHub account or organization and source repository;
- exact release version and publication time;
- exact release asset URL, byte size, and SHA-256;
- supported qrate plugin API version;
- declared permissions;
- release notes URL;
- optional revocation status and replacement recommendation.

The registry's private signing key belongs in a protected GitHub Actions environment requiring
maintainer approval, never in a repository. Catalogs include a key ID so that key rotation can be
explicitly supported.

`catalog.json.sig` is a small JSON document:

```json
{
  "schema": 1,
  "key_id": "qrate-plugin-catalog-1",
  "algorithm": "Ed25519",
  "sha256": "<catalog SHA-256>",
  "signature_base64": "<signature of the exact catalog.json bytes>"
}
```

The app and site reject an unknown key ID or algorithm. They compare the hash and verify the
signature before they parse `catalog.json`.

"Official catalog" means the release metadata and artifact hash were reviewed and published in the
signed catalog. It does not mean qrate guarantees that a plugin is harmless.

## Acquisition paths

| Path | Input | Trust state | Update source |
| --- | --- | --- | --- |
| Official catalog | Search or browse result | Signed catalog and artifact hash | Registry |
| Direct GitHub release | Repository or release URL | Unlisted, user-confirmed source | That repository's releases |
| Manual folder | Files copied or cloned by the user | Unmanaged local code | No qrate update action |

For direct installation, qrate accepts an ordinary public GitHub repository URL or GitHub Release
URL. It resolves a release asset and shows the source, version, and an unlisted-source warning.
After the user selects **Review package**, qrate downloads the archive without extracting or running
it. The final review shows the computed SHA-256 and static manifest before installation.

V1 supports public GitHub releases only. This keeps the workflow familiar without requiring a Git
executable. Generic `git+https`, SSH, branch, commit, private-repository, submodule, and LFS
support must be designed as a later source-provider feature if actual user needs justify it.

## Safe install and update lifecycle

Managed source files live beneath qrate application data, while a qrate-owned receipt lives outside
the package source:

```text
<app-data>/plugins/
  org.islandora.vocabularies/
    qrate-plugin.json
    init.lua
    ...
<app-data>/plugin-installs/
  org.islandora.vocabularies.json
```

The receipt records the distribution ID, installed version, source class, source identity,
artifact hash, and install date.

Install and update steps:

1. Download to a temporary file with a configured size limit.
2. Verify byte size and SHA-256 before extraction.
3. Extract into a unique temporary directory.
4. Reject absolute paths, `..` traversal, symlinks, duplicate roots, oversized expansion, malformed
   manifests, and invalid entry paths.
5. Validate manifest schema and API compatibility.
6. Atomically replace the managed installation directory, retaining the prior version until a
   successful reload.
7. Reload through the existing plugin reload lifecycle.
8. Write the receipt only after the new package is valid.

The installer validates that static manifest identity and permission metadata agree with the loaded
Lua descriptor. A mismatch is an install failure, not a warning.

First-time installs are disabled by default. Installation and runtime permission grants remain
separate choices. A direct source must receive a stronger warning in the UI.

Updates never replace manual/unmanaged folders. They are rejected when they require a newer host API
than the running qrate. Existing enablement may be retained for the same verified publisher/source
identity. Existing permission grants are retained only if the new version requests the same or
fewer permissions. New permissions remain ungranted until the user explicitly approves them.

If download, extraction, validation, or reload fails, qrate leaves the previous package usable.

## Revocation

Catalog records support a `revoked` state, reason, and replacement recommendation. A signed catalog
update can then stop new installs and warn users of a known bad installed version without waiting
for a qrate application release.

The initial policy should warn and prevent new installations. Automatic disabling is reserved for a
documented emergency policy, because qrate is used in offline and institutional environments.
qrate must never delete a plugin or its data automatically.

## Application UX

Extend **Extensions** with:

```text
Discover plugins…
Install plugin from link…
Manage plugins…
────────────────────
Plugins Folder
Reload Plugins
```

The discover view provides search, categories, official/unlisted/installed state, package details,
compatibility, requested permissions, source, license, release notes, and install/update actions.
Only signed catalog records receive an Official catalog badge.

The install-from-link flow is:

1. The user pastes a GitHub repository or release URL.
2. qrate resolves a selectable release artifact.
3. The source screen shows the publisher, version, asset URL, and an unlisted-source warning.
4. The user selects **Review package**.
5. qrate downloads the archive but does not extract or run it.
6. The review screen shows the computed hash, description, compatibility, and permissions.
7. The user selects **Install**.
8. qrate installs the disabled plugin and opens its settings.

Settings remains the single source of truth for enabling a plugin and granting runtime
permissions. Extend the existing Plugins settings page with source, installed version, update
availability, integrity state, and managed/unmanaged status rather than creating a competing
control surface.

Removal only deletes packages proven to be qrate-managed. It must never remove an arbitrary manual
folder discovered in the plugins directory.

## Browser-to-app links

The website can use a `qrate` URI scheme:

```text
qrate://plugin/install?source=registry&id=org.islandora.vocabularies
qrate://plugin/install?source=github&repo=devnull03/qrate-islandora-plugin
qrate://plugin/install?source=url&release=https%3A%2F%2Fgithub.com%2F...
```

The app strictly validates source types, identifiers, and URLs. Links contain no credentials, file
paths, shell arguments, or JavaScript. Opening a link always goes to the installation review
screen; it never silently downloads, installs, enables, or grants permission to a plugin.

Each website install button also has a browser fallback: source/repository link and manual
installation instructions for people without qrate or with protocol handlers disabled.

Platform integration:

| Platform | Work |
| --- | --- |
| Windows | Add `qrate` protocol registration to NSIS with safely quoted executable and argument handling |
| macOS | Add `CFBundleURLTypes` for `qrate` in the app bundle |
| Linux | Install a desktop entry for `x-scheme-handler/qrate` and update the desktop database where packaging permits |

The protocol handler alone is insufficient: clicking a browser link launches a new process even
when qrate is open. The feature therefore includes a per-user single-instance handoff:

1. The primary process owns a local endpoint.
2. A later invocation validates and forwards the deep link, focuses the primary window, and exits.
3. The primary process opens the install review.
4. A stale endpoint is safely recovered.

Use Windows named pipes and Unix-domain sockets, or a vetted cross-platform single-instance
implementation. Do not expose a network TCP listener.

## Submission workflow

1. An author starts from `qrate-plugin-template`.
2. The author develops and tests in their own public repository.
3. The author produces a versioned plugin archive and GitHub Release.
4. The author opens a pull request to `qrate-plugin-registry` adding a plugin or version record.
5. Registry CI downloads the release asset and validates hash, size, archive structure, manifest,
   API compatibility, and declared permissions.
6. Maintainers review the source and machine-generated report.
7. Merge regenerates, signs, and publishes the catalog and triggers a site refresh.

Use a GitHub issue form for early listing requests and author support, and a pull request for the
authoritative published record. A PR is required because it precisely pins and reviews the metadata
the application consumes.

The registry needs `CONTRIBUTING.md`, `SECURITY.md`, `PUBLISHING.md`, `SCHEMA.md`, issue and PR
templates, a maintainer checklist, and explicit criteria for removing or revoking listings.

## Repository implementation plans

The repositories share contracts, but each repository has one clear job. Each subsection below can
become an issue or a small issue group in that repository.

### `qrate-plugin-registry`: catalog source and publication

Create `devnull03/qrate-plugin-registry` as a public repository. Use this initial layout:

```text
.github/
  ISSUE_TEMPLATE/
    listing-request.yml
    security-report.yml
  workflows/
    validate.yml
    publish.yml
  pull_request_template.md
plugins/
  org.islandora.vocabularies.json
schemas/
  catalog.schema.json
  listing.schema.json
  package.schema.json
scripts/
  build-catalog.mjs
  validate-artifact.mjs
  validate-records.mjs
test/
  fixtures/
    packages/
    records/
CONTRIBUTING.md
LICENSE
PUBLISHING.md
README.md
SCHEMA.md
SECURITY.md
package.json
```

Keep one source record in `plugins/<distribution-id>.json`. The file contains listing metadata and a
version history. It does not copy the plugin README or Lua descriptor.

Each version record pins these fields:

- version and publication date;
- plugin API version;
- release page and release asset URLs;
- SHA-256 and byte size;
- package manifest fields needed for search and install review;
- review state, revocation state, and replacement version if one exists.

Use `schemas/package.schema.json` as the source of truth for `qrate-plugin.json`. Copy or generate
the same schema for qrate tests and template checks. Store the schema version in each document.

The pull-request workflow performs these checks:

1. Validate all source records against `listing.schema.json`.
2. Reject duplicate distribution IDs, versions, release assets, and normalized repository URLs.
3. Download each new or changed release asset with time and size limits.
4. Verify the submitted SHA-256 and byte size.
5. Inspect the ZIP without running Lua.
6. Reject path traversal, symlinks, duplicate names, invalid roots, and expansion limit violations.
7. Validate `qrate-plugin.json` against `package.schema.json`.
8. Compare the package manifest with the registry version record.
9. Confirm that the release belongs to the declared public GitHub repository.
10. Build an unsigned catalog and validate it against `catalog.schema.json`.
11. Attach a short validation report to the pull request.

The first version can use Node and a small set of pinned packages. Use the same validator modules in
pull-request CI and publication CI. Do not maintain a second validation implementation in workflow
YAML.

The publication workflow runs after a protected `main` merge. It performs these actions:

1. Repeat all record and artifact checks.
2. Build `dist/catalog.json` with a stable record order and stable JSON serialization.
3. Sign the exact catalog bytes with the registry Ed25519 key.
4. Write `dist/catalog.json.sig` and a small `dist/status.json`.
5. Publish `dist/` to GitHub Pages at a stable HTTPS URL.
6. Create a deployment record that contains the source commit.
7. Trigger a Cloudflare site rebuild.

Use a protected `catalog-production` environment for the private signing key and site deploy hook.
Require maintainer approval for the publication job. Give the workflow read-only repository access
plus only the Pages and deployment permissions it needs.

`status.json` contains the catalog schema, generation time, source commit, key ID, and catalog hash.
The site can show this information in an error page. qrate does not trust `status.json`.

The repository policy must define:

- minimum source and release requirements;
- a required package-root license file and the accepted SPDX identifiers;
- stable publisher and distribution identity rules;
- review rules for new plugins and new versions;
- permission-change review;
- abandoned-plugin and transfer rules;
- removal, revocation, appeal, and compromised-release steps;
- key rotation and emergency catalog publication.

Do not accept a listing through an issue alone. The issue form helps an author prepare a pull
request. The pull request remains the reviewed change that publication consumes.

Follow Zed's extension-registry model for licenses. A plugin author keeps their copyright and
chooses an accepted license. V1 accepts Apache-2.0, BSD-2-Clause, BSD-3-Clause, CC-BY-4.0,
GPL-3.0-only, GPL-3.0-or-later, LGPL-3.0-only, LGPL-3.0-or-later, MIT, Unlicense, and Zlib. Registry
CI rejects packages with no root license file, an identifier outside this list, or a mismatch
between the package manifest and license text. The template example uses MIT.

### `qrate-plugin-template`: author contract and release tooling

The template currently has five tracked files. It has no tests, package manifest, release workflow,
or archive tool. The current `init.lua` comment also calls the runtime descriptor a manifest. Change
that term before the static package manifest is introduced.

Add these files:

```text
.github/
  workflows/
    check.yml
    release.yml
qrate-plugin.json
scripts/
  package-plugin.mjs
test/
  manifest.test.mjs
LICENSE
```

Keep the template small. Do not turn it into a JavaScript application. The Node script can use the
standard library and the local Git executable. It needs no runtime dependency.

`qrate-plugin.json` contains obvious placeholder values that an author must change. The distribution
ID must not use the local folder name. The README must explain how authors choose and retain a
stable ID.

`scripts/package-plugin.mjs` performs these actions:

1. Validate the manifest fields and SemVer version.
2. Confirm that `entry` names a tracked file under the repository root.
3. Confirm that required README and license files exist.
4. Collect tracked package files from an explicit allowlist.
5. Create `dist/<id>-<version>.zip` with files at the ZIP root.
6. Print and write the SHA-256 and byte size.
7. Fail if the worktree has package changes that are not committed.

The allowlist includes the manifest, entry file, Lua modules, types, README, license, and plugin
assets. It excludes `.git`, workflows, tests, local logs, and `dist`.

The check workflow runs on pushes and pull requests. It validates the manifest, runs the package
script in check mode, and runs Luau type checks. Pin the Luau tool version.

The release workflow runs on a `v*` tag. It requires an exact match between the tag and manifest
version. It creates the ZIP and checksum, then publishes both as GitHub Release assets. It must fail
instead of replacing an asset on an existing release.

Update the README with this author path:

1. Create a repository from the GitHub template button.
2. Set the distribution ID, name, version, repository, license, and permissions.
3. Develop against `types/qrate.lua`.
4. Test the plugin in qrate as an unmanaged folder.
5. Run the local package check.
6. Tag the release.
7. Inspect the generated release assets.
8. Submit the release to the registry.

Keep `types/qrate.lua` as the canonical plugin API copy. Marketplace metadata does not change the
runtime API. A package schema change does not require a plugin API version increase.

### Plugin author repositories: release ownership

Each plugin repository owns its source and support process. The registry never becomes a source
mirror.

Authors must:

- keep one stable distribution ID;
- publish a static package manifest;
- publish a versioned ZIP and checksum;
- keep old reviewed assets available;
- document requested permissions and data use;
- link release notes and a support or issue page;
- report a compromised release through the registry security process.

The first-party Islandora plugin is the end-to-end pilot. Apply the template files to that repository
without replacing its existing tests or documentation. Publish a new release through the same
workflow that community authors will use. Do not seed the catalog with a hand-built special case.

The pilot must test a folder plugin with sibling modules, network permission, user settings, project
settings, and a larger real package. Add a second small plugin before launch to test a no-permission
package and a different publisher.

### `qrate` `main`: package manager, UI, and operating-system integration

Create a focused `plugin-package` crate. Keep package download and installation policy out of
`plugin-host`. The host continues to load installed Lua and enforce runtime permissions.

The crate owns:

- signed catalog parsing and verification;
- catalog cache and refresh state;
- GitHub release URL parsing and direct-release metadata;
- bounded downloads and SHA-256 checks;
- safe ZIP inspection and extraction;
- package manifest validation;
- install receipts and managed installation paths;
- atomic install, update, rollback, and removal operations;
- compatibility, source identity, and permission delta checks.

Expose typed operations to the app. Do not expose raw archive paths or partly installed directories.
Use one error type that keeps a safe user message and a detailed log cause.

Reuse the updater's Ed25519 envelope rules, SemVer checks, SHA-256 helper, and atomic JSON write
patterns where their contracts match. Use a separate registry key ID and domain types. Do not make a
plugin package look like an application update.

Update `plugin-host` to return enough identity for the package manager to match a loaded plugin with
its receipt. Keep unmanaged folder discovery. Restrict the working-directory `./plugins` path to
development builds so a packaged app has one production plugin directory.

Add these application surfaces:

- `DiscoverPlugins` and `InstallPluginFromLink` actions in both Extensions menu implementations;
- a discovery window or workspace panel that uses the signed catalog cache;
- a source screen and package review screen for direct GitHub installs;
- managed source, version, integrity, update, and remove fields on the existing Plugins settings page;
- update and revocation notices that link to release or incident details;
- recovery actions for failed reloads and retained rollback copies.

Keep enablement and permission grants in the existing Plugins settings page. An install button must
not enable a plugin or grant `net`.

Update `docs/plugins/index.md` with official, direct, and manual install paths. Update
`docs/plugins/api-reference.md` only when the runtime API changes. Add package and submission links
to `CONTRIBUTING.md` and the root README. Update the three-repository rule in `CLAUDE.md` so it names
the registry schema and site marketplace.

The app startup path must parse a `qrate://` argument before it opens the normal window. Add one
strict URI parser with unit tests. Both initial launch and existing-process handoff must use the same
parsed command type.

Package integration includes:

- NSIS URL protocol keys and uninstall cleanup;
- MSI protocol registration for managed Windows deployments;
- `CFBundleURLTypes` in the macOS bundle;
- `x-scheme-handler/qrate` in the Linux desktop file;
- a local per-user handoff for a second qrate process;
- focus and install-review routing in the primary process.

Portable builds cannot reliably register a protocol handler. Document manual registration as
unsupported in V1 and keep the website fallback visible.

Add test layers:

1. Unit tests cover URI, schema, version, source identity, and permission delta rules.
2. Archive tests cover every hostile fixture.
3. Local HTTP tests cover catalog, download, cancellation, timeout, and changed bytes.
4. Integration tests cover install, update, rollback, conflict, and removal.
5. Packaging smoke tests inspect each platform artifact for protocol registration.
6. Manual release tests cover a closed app, an open app, an offline app, and a missing app.

Do not add anonymous marketplace telemetry in V1. Existing Cloudflare page analytics can measure site
traffic. Support reports and registry CI failures are enough for the first rollout.

### `qrate` `site` branch: marketplace and author documentation

The site is an Astro 7 and Starlight project deployed by Cloudflare Workers Builds. It already
fetches qrate releases at build time. The marketplace must follow the same static-build model.

Add this site structure:

```text
src/
  components/
    PluginCard.astro
    PluginInstallButton.astro
    PluginPermissionList.astro
  lib/
    plugins.ts
  pages/
    plugins/
      index.astro
      [id].astro
      submit.astro
public/
  plugin-assets/
```

`src/lib/plugins.ts` fetches `catalog.json` and `catalog.json.sig` at build time. It verifies the
signature with Node crypto before it returns records. It validates the catalog schema version and
fails a production build on a bad signature, bad schema, duplicate ID, or missing required field.
Local development can use a committed fixture through `QRATE_PLUGIN_CATALOG_URL`.

Do not copy registry records into the site branch. Do not fetch the catalog in the visitor's browser.
Astro must prerender the browse and detail pages so search engines, no-script users, and the MCP/docs
surfaces receive complete content.

`/plugins` contains:

- a title and a short explanation of catalog trust;
- client-side search over the prerendered records;
- category and permission filters;
- compatibility, version, publisher, license, and update date on each card;
- a clear revoked or incompatible state;
- links to author source, support, and the submission guide.

`/plugins/[id]` contains:

- full listing metadata and screenshots;
- current version and qrate API compatibility;
- requested permissions with plain descriptions;
- release notes, license, publisher, source, and support links;
- an **Open in qrate** button for an active compatible release;
- a copyable direct link and manual instructions as fallbacks;
- the exact Official catalog wording and trust explanation;
- revocation details instead of an install button for a revoked release.

`PluginInstallButton.astro` creates only this stable link:

```text
qrate://plugin/install?source=registry&id=<percent-encoded-id>
```

Do not put an artifact URL, version, hash, or title in the URI. qrate gets current signed data from
the registry and opens a review screen. Add a visible source link beside the button because browsers
do not report custom-protocol failure consistently.

`/plugins/submit` explains the author flow and links to the template, registry issue form, registry
contribution guide, and package schema. Keep the API reference in the synced qrate user docs. Put
marketplace submission instructions on the site because they span repositories.

Replace the hard-coded `plugins` array on the home page with catalog-backed featured plugin cards.
Keep the template and **Write your own** cards as separate author calls to action. Change the current
folder-install wording to discovery wording, and retain a manual-install link.

Add **Plugins** to the main navigation and footer. Add catalog pages to the sitemap and structured
data. Use `SoftwareApplication` or `SoftwareSourceCode` metadata only where the fields are accurate.
Do not add rating markup without real ratings.

The site already syncs user docs from `main:docs/`. Update those docs in qrate first, then run
`bun run sync-docs` on the site branch. Never hand-edit a generated copy under
`src/content/docs/docs/`.

The registry publication job triggers the existing Cloudflare deploy hook after it publishes the
signed catalog. This requires no site commit. The Cloudflare build fetches the new catalog and
prerenders the pages. Keep the release-triggered rebuild because release pages still use build-time
GitHub data.

Add build tests for:

- a valid signed fixture;
- a bad signature;
- a revoked plugin;
- no plugins;
- duplicate IDs;
- a missing screenshot;
- link generation with reserved characters;
- mobile and keyboard access for search and filters.

The production build must fail rather than publish stale marketplace data after a signature or
schema error. Cloudflare keeps the last successful deployment online.

## Cross-repository contracts and order

Three files are public contracts:

| Contract | Source of truth | Consumers |
| --- | --- | --- |
| `qrate-plugin.json` schema | Registry `schemas/package.schema.json` | Template, author repositories, qrate |
| Signed `catalog.json` schema | Registry `schemas/catalog.schema.json` | qrate, site |
| `qrate://plugin/install` URI | qrate parser tests and ADR | Site |

Use versioned fixture bundles to keep consumers in step. Tag the registry contract as `v1` before
qrate or the site consumes a production URL. Pin CI downloads to that tag or a commit, not `main`.

Implement the work in this dependency order:

1. Create schemas, fixtures, and unsigned catalog generation in the registry.
2. Add the package manifest and package check to the template.
3. Publish the Islandora pilot artifact.
4. Validate and merge the first registry record.
5. Add catalog verification and local install tests to qrate.
6. Add qrate UI after package operations pass without UI.
7. Add the site against the signed test catalog.
8. Add operating-system deep links after the app review route exists.
9. Publish site install buttons only after packaged deep-link tests pass.
10. Rehearse revocation and key rotation before launch.

For a contract change, open linked pull requests. Merge the source-of-truth change first. Keep each
consumer compatible with the old and new schema during the transition when possible.

## Pull-request and repository checklist

Use small pull requests that leave a testable state. Do not use one cross-repository launch pull
request.

| Order | Repository or branch | Pull request | Required result |
| --- | --- | --- | --- |
| 1 | `qrate-plugin-registry` | Bootstrap repository and schemas | Source records and fixtures validate without signing |
| 2 | `qrate-plugin-template` | Add manifest and package checks | A tagged fixture produces the expected ZIP and checksum |
| 3 | Islandora plugin | Adopt package contract | A real GitHub Release contains validated assets |
| 4 | `qrate-plugin-registry` | Add Islandora and publication | GitHub Pages serves a signed V1 catalog |
| 5 | `qrate` `main` | Add `plugin-package` foundation | Local tests install, update, reject, and roll back packages |
| 6 | `qrate` `main` | Add discovery and management UI | Official and direct install flows work in a packaged app |
| 7 | `qrate` `main` | Add deep-link and single-instance support | All packaged platforms open the same review route |
| 8 | `qrate` `site` | Add marketplace pages | Static pages build from the signed catalog |
| 9 | `qrate` `site` | Publish install buttons | Buttons appear only after packaged deep-link tests pass |
| 10 | All affected repositories | Documentation and launch pass | Links, policies, fixtures, and runbooks agree |

No marketplace change is required in `qrate-pi-extension`. The extension does not load, install, or
describe Lua plugins. Add work there only if a later agent feature needs marketplace access.

## Implementation phases

### Phase 0: contracts and security fixtures

- Write an architecture decision record in qrate.
- Specify the static package manifest and signed catalog schemas.
- Create valid and hostile archive fixtures: traversal, symlink, duplicate root, oversized archive,
  malformed manifest, unsupported API, manifest/descriptor mismatch, and bad hash.
- Decide registry ownership, public naming, signing custody, and revocation policy.

Exit criterion: schemas and fixtures have been reviewed before download/install UI code starts.

### Phase 1: registry and author release tooling

- Create `qrate-plugin-registry` with schema validation, artifact validation CI, catalog signing,
  submission policy, and templates.
- Add package manifest and release-asset tooling to `qrate-plugin-template`.
- Package Islandora through the same process as the first real registry entry.
- Update template and Islandora documentation under the existing multi-repository plugin API rule.

Exit criterion: a maintainer can merge one registry PR and produce a signed catalog without editing
the website data by hand.

### Phase 2: installer foundation

- Add a focused qrate package-distribution module or crate; do not combine downloader and archive
  policy with the Lua runtime.
- Implement catalog verification/cache, download, extraction, manifest checks, receipts, atomic
  replacement, rollback, and managed/unmanaged detection.
- Reuse updater trust primitives where that avoids duplicated security logic, with a separate key
  and artifact domain.
- Remove the development working-directory plugin fallback from packaged builds.
- Test all Phase 0 fixtures through integration tests.

Exit criterion: a local test server can exercise successful install, rejection, update, rollback,
and recovery without UI automation.

### Phase 3: qrate UI

- Add discovery, install-from-link, update, error, and recovery UI.
- Extend the existing Plugins settings page with installed-package metadata.
- Add compatible-update and permission-delta confirmation.
- Test cancellation, network failure, restart during installation, broken updates, manual-folder
  conflicts, and downgrade/reinstall behavior.

Exit criterion: a packaged qrate build installs from both the official catalog and a direct GitHub
release without Git installed.

### Phase 4: deep links and website

- Add secure URI parsing, initial-launch processing, and existing-process handoff.
- Register the URI scheme in Windows, macOS, and supported Linux packaging.
- Add `/plugins` browse, search, and detail pages to the site branch, rendered from registry data.
- Add install buttons and non-protocol browser fallback.
- Rebuild the site when a signed registry catalog is published.

Exit criterion: a link works identically when qrate is closed and when it is already open.

### Phase 5: rollout and operations

- Launch with Islandora and one small additional test/community plugin.
- Describe direct installations as unreviewed and catalog listings as officially listed.
- Rehearse catalog key rotation and a release revocation.
- Monitor installation failures, release-author friction, and support volume.
- Decide whether catalog refresh is automatic and whether update checks follow qrate's application
  update preference. Installation remains user-confirmed in the first release.

Exit criterion: contributors and maintainers can follow tested publication and incident-response
runbooks.

## Decisions still to confirm

1. Create the public repository as `devnull03/qrate-plugin-registry`.
2. Use **Official catalog** or **Listed by qrate**, never "verified safe", in product language.
3. Make V1 updates manual-confirmation only.
4. Limit direct V1 sources to public GitHub releases.
5. Fully support Windows and macOS deep links; decide whether Linux is a release requirement or
   documented packaging-dependent support.
6. Keep the signing key in a protected GitHub Actions environment with required maintainer approval.
