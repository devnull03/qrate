---
name: cut-release
description: Full release procedure for qrate — bump the version, tag it, wait for the cross-platform CI build, rewrite the auto-generated draft's release notes in the project's established style, and publish. Use when asked to "cut a release", "create a release", "ship a new version", "release qrate", "publish a release", or "tag a new version". Always confirm the version number and full-vs-pre-release with the user before running the destructive steps (tag push, publish) — this skill's own first step is that confirmation, don't skip it even if the request already sounds decided.
---

# Cut a qrate release

Ten steps, in order. Steps 3 onward push to the shared repo and publish something public —
nothing before step 1's confirmation should touch `origin` or GitHub.

## 1. Ask before doing anything

Even if the request names a version, confirm both with `AskUserQuestion` before touching git:

- **Version number.** Look at `grep -m1 version Cargo.toml` and `git tag -l` for the last one, then
  propose the obvious next step (bump minor and reset to `alpha.1` for a "minor"/feature release,
  bump the trailing `alpha.N` for a small follow-up to the same line) as the recommended option,
  plus the other as a second option.
- **Full release or pre-release.** GitHub's `prerelease` flag is *not* inferred from the version
  string — the workflow never sets it, so someone has to decide every time. Check what the last
  couple of tags actually shipped as (`gh release list`) and say so in the question; this repo's
  history has at least one alpha that shipped un-flagged as "Latest" by mistake, so don't assume
  "alpha in the name" implies pre-release without asking.

Do not proceed past this step on an assumption. If the user answers with something that doesn't
match either offered option (e.g. a version you didn't propose), use exactly what they said.

## 2. Preflight

```bash
git status --porcelain      # must be clean — stop and ask if not
git branch --show-current   # must be main; this repo releases directly off main, no release branch
```

Run the same three checks CI gates on, workspace-wide, **before** bumping anything — a release that
fails its own CI is a wasted 15-minute build:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings -A dead_code
cargo test --workspace
```

If `scripts/installer.wxs` or `scripts/installer.nsi` changed since the last release, rebuild them
locally too — the workspace checks above don't touch either, so a broken installer script only
surfaces on the Windows build leg, 15+ minutes into the run. See "Gotchas" below for the exact
`wix build`/`makensis` invocations and why "I tested it earlier in the session" isn't good enough.

## 3. Bump the version

`Cargo.toml`'s `[workspace.package].version` is the single source of truth — every workspace crate
inherits it, and `release.yml`'s `version` job **fails the whole run** if the pushed tag (minus its
leading `v`) doesn't match it exactly. Edit that one line, then regenerate `Cargo.lock` (it pins a
`version = "..."` entry per local crate too, so it goes stale otherwise):

```bash
cargo build -p app
```

Re-run the three checks from step 2 — the version bump touched every crate's `Cargo.toml`-derived
metadata, so it's cheap insurance, not redundant.

## 4. Commit the bump

Conventional Commits, matching the last two releases' style — short prose body, not a changelog
(the real changelog goes in the GitHub release notes, step 8):

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(release): X.Y.Z-suffix

One or two sentences on the headline changes since the last tag."
```

Look at `git log v<PREV_TAG>..HEAD --oneline` to write that body — it's the same commit range the
release notes in step 8 come from, so read it once and keep both in mind.

## 5. Push the commit, then the tag

```bash
git push origin main
git tag vX.Y.Z-suffix
git push origin vX.Y.Z-suffix
```

Order matters: the tag should point at a commit that's actually reachable on `origin/main`, not an
orphaned local one. Pushing straight to `main` bypasses this repo's "changes must go through a PR"
branch rule for whoever has bypass rights — that's expected for a release, not an error to work
around.

## 6. Wait for the build

The tag push triggers `.github/workflows/release.yml`, which fans out to Windows (NSIS + MSI +
portable zip), macOS (universal dmg), and Linux (tarball), then collects everything into a **draft**
release. Past runs took 12–18 minutes. Find the run and watch it rather than polling:

```bash
gh run list --workflow=release.yml --limit 1
gh run watch <run-id> --exit-status
```

If it fails, the `version` job failing fast almost always means step 3's edit didn't match the tag —
fix and re-tag (delete the bad tag both locally and on `origin` first). Any other job failing means
an actual build break; do not publish a broken draft, fix the underlying issue and push a new patch
tag instead of retrying the same one (tags are meant to be immutable once pushed).

## 7. Find the draft

```bash
gh release view vX.Y.Z-suffix --json body,draft,prerelease,assets
```

Confirm all five assets are there (`*-setup.exe`, `*.msi`, `*-x86_64.zip`, `*-universal.dmg`,
`*-x86_64-linux.tar.gz`, `SHA256SUMS.txt` — six, not five) before touching the notes.

## 8. Rewrite the release notes

The workflow's auto-generated body is purely mechanical — Downloads / Enterprise / preview-binary
notes / the unsigned-build warning. Keep that part (it's accurate and current), but **prepend** a
hand-written summary in the style of the last two releases (see `gh release view v0.2.0-alpha.1` for
the fullest example): an opening one-liner, then `### What is new` with `**Bold subhead**` groups of
short bullets, organized by area of the app rather than by commit. Write it from the
`git log v<PREV_TAG>..HEAD --oneline` range you already read in step 4 — for a small release just
write it directly; for a large one (many unrelated features) it's fine to draft it in a scratch file
first and read it back before publishing.

Do not just paste commit messages — they're written for a developer reading `git log`, release notes
are written for someone deciding whether to download this. A `fix(installer): pin WiX to 5.0.2, fix
the .wxs schema errors` commit becomes something like "The enterprise MSI installer now builds
correctly" in the notes — the internal detail (WiX version pinning) isn't the reader's problem.

Write the combined body to a temp file, then:

```bash
gh release edit vX.Y.Z-suffix --notes-file /path/to/notes.md
```

## 9. Publish

```bash
gh release edit vX.Y.Z-suffix --draft=false --prerelease=<true|false>   # from step 1's answer
```

## 10. Verify

```bash
gh release view vX.Y.Z-suffix
```

Confirm: `draft: false`, `prerelease` matches what was decided in step 1, all six assets present, and
the body reads correctly on the actual GitHub page (formatting, especially the `> [!NOTE]` callout,
doesn't always round-trip through `--notes-file` the way it looks in a local editor).

## Gotchas

- **The tag-vs-Cargo.toml check has no slack.** `v1.2.3` against `version = "1.2.3-alpha.1"` fails;
  they must be byte-identical after stripping the leading `v`.
- **`prerelease` is never inferred.** It is a separate, manual decision every single release — see
  step 1. Don't skip asking just because the version string has `alpha` in it.
- **Tags are lightweight, not annotated**, matching this repo's existing tags (`git tag vX.Y.Z`, no
  `-a`/`-m`). Don't switch styles.
- **A failed build leaves a stale draft or nothing at all** — `softprops/action-gh-release` only
  creates the release on success. Don't try to "finish" a failed run's draft; fix and tag again.
- **Six assets, not five** — `SHA256SUMS.txt` is easy to overlook when eyeballing the asset list.
- **`scripts/installer.wxs` breaks silently between edits.** Its `<!-- ... -->` header is real XML —
  a literal `--` anywhere inside it (e.g. writing out a `--global`/`--version` CLI flag in prose) is
  a hard `WIX0104` parse error, and nothing catches it except an actual `wix build` or the Windows CI
  leg. This has broken a release before: the comment was fixed once, then a *later* edit to the same
  comment block reintroduced a `--flag` without re-running the build, and it shipped straight to a
  pushed tag. Don't trust "I verified this earlier in the session" — verify again after the last edit,
  right before tagging:
  ```bash
  dotnet tool install --global wix --version 5.0.2   # once; skip if already installed
  wix extension add -g WixToolset.UI.wixext/5.0.2    # once; skip if already installed
  wix build scripts/installer.wxs -ext WixToolset.UI.wixext \
    -d Version=0.0.0.0 -d SrcExe=target/release/app.exe -d SrcDir=target/release \
    -d IconFile=assets/icons/app-icon.ico -d HasPdfium=0 -d HasFfmpeg=0 \
    -o /tmp/verify.msi
  ```
  Prefer prose over literal flag syntax inside that comment block generally — it's what caused this.
