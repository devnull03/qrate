# design-sync notes — qrate-site

## What this repo syncs

- **Tokens and styles only.** qrate-site is an Astro site; its components are
  `.astro`, not React, so there is nothing the converter can bundle into
  `window.QrateSite`. The bundle is deliberately empty and the converter takes
  its documented tokens-only path (`[ZERO_MATCH] … treating as tokens-only DS`).
- **`Qrate UI Design System` (`82eb087b-…`) is a different project** — a
  hand-authored design system for the qrate *desktop app* (Button, Dialog,
  StatusBarItem, wizard, `guidelines/`, a MainWorkspace template). It did not
  come from this repo. Never sync into it; this repo pins
  `8a1a1545-7731-4a7c-ba35-5ab5fbbc2c9c` (`Qrate Site Design System`).

## Rebuild recipe (do these before the driver run)

The converter's config path fields must point at real files, so two artifacts
are generated into `.design-sync/.cache/` first. Both are gitignored — recreate
them on every fresh clone:

```sh
# 1. a full-surface Tailwind sheet compiled from the site's own theme.
#    A plain `bun run build` only emits the ~143 utilities the site itself uses,
#    which is far too thin for a design agent to lay out with. ds-tailwind.css
#    re-imports src/styles/global.css and safelists the utility families via
#    Tailwind v4 `@source inline(...)`.  ~110 KB out, ~3.5k variant classes.
bunx @tailwindcss/cli -i .design-sync/ds-tailwind.css \
  -o .design-sync/.cache/tw-full.css --minify

# 2. cssEntry = the Google Fonts @import + that sheet.
{ echo "@import url('https://fonts.googleapis.com/css2?family=Archivo:wght@400;500;600;700&family=IBM+Plex+Mono:wght@400;500;600&display=swap');"
  cat .design-sync/.cache/tw-full.css; } > .design-sync/.cache/site.css

# 3. the empty entry the converter bundles (there are no components).
echo 'export {};' > .design-sync/.cache/ds-entry.mjs

# 4. react/react-dom, which lib/emit.mjs requires for _vendor/ even with zero
#    components. Not a real dependency of this site — keep it out of package.json.
npm i --no-save --no-package-lock react react-dom

node .ds-sync/resync.mjs --config .design-sync/config.json \
  --node-modules ./node_modules --entry ./.design-sync/.cache/ds-entry.mjs \
  --out ./ds-bundle --no-render-check [--remote .design-sync/.cache/remote-sync.json]
```

`.design-sync/ds-tailwind.css` is **committed** (durable set), so only steps
1–4 above are per-clone chores. It carries `@source '../src'` to pin the scan
root — without it, Tailwind derives the root from the file's own location and
the output changes if the file moves. Scanning `src/` also sweeps in a handful
of palette classes (`bg-blue-500`, …) that appear in docs code samples; they are
harmless noise, and `conventions.md` tells the agent to ignore them.

## Gotchas

- **`--no-render-check` is correct here, not a shortcut.** There are zero
  preview cards to render, so playwright would have nothing to open. Validate's
  only failure without the flag is `[RENDER_SKIPPED]`.
- `[FONT_REMOTE]` for Archivo / IBM Plex Mono is expected — the site loads them
  from Google Fonts (`src/layouts/Base.astro:61`), and `styles.css` carries the
  same remote `@import`. Nothing ships in `fonts/`.
- `[DTS_REACT]` and `[ZERO_MATCH]` are both expected and harmless (no components).
- `cfg.tokensGlob` is **unusable** in this repo: `lib/css.mjs` `copyTokens()`
  resolves it under `node_modules/<cfg.tokensPkg>`, so it can't point at a plain
  path. The tokens therefore ship inside `_ds_bundle.css` (reachable from
  `styles.css`) rather than as a separate `tokens/` directory.
- `bunx` writes to `bun.lock` — `git checkout bun.lock` after step 1.
- Tailwind v4's `@theme inline` only emits `--color-*` aliases for utilities
  that are actually generated, so `--color-rust` is absent while `.text-rust`
  exists and resolves `var(--rust)`. Document the base token, not the alias.

## Known render warns

None — the render check does not run (no previews).

## Re-sync risks

- **`.design-sync/conventions.md` enumerates real class and token names.** They
  were verified against `ds-bundle/_ds_bundle.css` at sync time. If
  `src/styles/global.css` renames a token or drops a `@utility`, the header goes
  stale and the design agent will emit vocabulary that does not resolve —
  re-validate every backticked name against the fresh build.
- The safelist in `ds-tailwind.css` is hand-maintained. A new token family added
  to `global.css` will have tokens but **no utilities** until it is safelisted.
- `dist/client/_astro/Base.*.css` (the site's own build output) is no longer used
  as `cssEntry` — the standalone Tailwind compile replaced it. Don't reintroduce
  the hashed path; it changes on every build.
- Fonts load from Google Fonts at runtime. If the design environment ever blocks
  that host, every design falls back to system sans and nothing will flag it.
