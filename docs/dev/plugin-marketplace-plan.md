# Plugin marketplace plan

## Decision

Build a separate public `qrate-plugin-registry` repository for curated plugin metadata and
release verification. Keep the public website and documentation on qrate's existing `site`
branch. Keep each plugin's source and releases in its author's repository.

The application installs HTTPS release artifacts with immutable versions and hashes. It does not
run `git clone`. The direct-install field accepts a GitHub repository or release URL, so users can
still paste the link authors share; qrate resolves it to a versioned release artifact.

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
| `qrate` `main` | Desktop installer, plugin host, install UI, deep links, embedded registry public key | Curated listings and third-party source |
| `qrate-plugin-template` | Canonical Lua API definitions, example plugin, author package tooling | Registry data |
| Plugin author repositories | Plugin source, tests, release artifacts, support | Official discoverability by itself |
| `qrate-plugin-registry` | Curated records, schemas, validation CI, catalog signatures, submission policy | Website implementation or plugin source |
| `qrate` `site` branch | Marketplace pages, documentation, install links, rendering registry data | Trust decisions in the app binary |

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
`qrate-islandora-vocabularies-1.2.0.zip`. The archive has one plugin root and contains its
manifest, entry file, sibling Lua modules, README, license, and optional editor types.

Do not use GitHub-generated "Source code (zip)" downloads as the package contract. A GitHub Release
asset has an intentional layout and can be checked against the hash in the registry.

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
- immutable artifact URL, byte size, and SHA-256;
- supported qrate plugin API version;
- declared permissions;
- release notes URL;
- optional revocation status and replacement recommendation.

The registry's private signing key belongs in a protected GitHub Actions environment requiring
maintainer approval, never in a repository. Catalogs include a key ID so that key rotation can be
explicitly supported.

"Official catalog" means the release metadata and artifact hash were reviewed and published in the
signed catalog. It does not mean qrate guarantees that a plugin is harmless.

## Acquisition paths

| Path | Input | Trust state | Update source |
| --- | --- | --- | --- |
| Official catalog | Search or browse result | Signed catalog and artifact hash | Registry |
| Direct GitHub release | Repository or release URL | Unlisted, user-confirmed source | That repository's releases |
| Manual folder | Files copied or cloned by the user | Unmanaged local code | No qrate update action |

For direct installation, qrate accepts an ordinary public GitHub repository URL or GitHub Release
URL. It resolves a release asset, then shows the author, repository, exact version, artifact URL,
SHA-256, permissions, and an unlisted-source warning before it downloads anything.

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

1. User pastes a GitHub repository or release URL.
2. qrate resolves a selectable release artifact.
3. The confirmation screen shows source, publisher, version, immutable URL, hash, description,
   permissions, and an unlisted-source warning.
4. The user selects **Install**.
5. qrate installs the disabled plugin and takes the user to plugin settings.

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
