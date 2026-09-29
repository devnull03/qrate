# GitHub wiki sync

The [qrate wiki](https://github.com/devnull03/qrate/wiki) is generated from every Markdown
page under `docs/` on `main`. Edit the source documentation here; edits to generated wiki
pages are replaced on the next sync.

`.github/workflows/sync-wiki.yml` publishes changes when documentation, the renderer, or
the workflow changes on `main`. To publish manually:

```sh
gh workflow run sync-wiki.yml --ref main
```

The workflow uses its GitHub token with `contents: write`. The wiki must be enabled and
have an initial page before its Git repository can be cloned.

## Page names and links

`scripts/sync-wiki.py` makes `docs/index.md` the wiki's `Home` page. Other page names use
their path under `docs/`, joined with hyphens, with `.md` and a final `index` removed.
For example, `getting-started/index.md` becomes `getting-started`, and
`plugins/api-reference.md` becomes `plugins-api-reference`. Conflicting names stop the sync.

Links to documentation pages become wiki links. Links to other repository files and
directories point to `main` on GitHub; images outside the wiki use raw repository URLs.
Fenced code blocks are preserved. A generated sidebar lists all the documentation pages.

The wiki's `.qrate-docs-pages.json` records the files owned by the sync. When a source page
is removed or renamed, its old generated page is removed. Other wiki pages are preserved.
The sync adds a commit only when the generated content changes and keeps wiki history.

## Preview locally

Render into a temporary folder or a local wiki clone:

```sh
python scripts/sync-wiki.py /path/to/wiki-preview --repository devnull03/qrate
```

This command writes the generated files locally. The workflow clones the wiki before
rendering, then commits and pushes those files.
