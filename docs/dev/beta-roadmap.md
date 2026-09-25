# First beta: remaining work, by branch

Work that has to land before the first major beta, split into branches you create yourself. Small
items go straight onto `main`. Out of scope for now: agent work (on `qrate-cli`), the Microsoft
Store (ASNT-106/107, after the beta), and row merge/explode (ASNT-79, dropped).

## On `main`, no branch

- **Tracker cleanup.** Close ASNT-50 (the multi-select is shipped: `ComboboxState` with a
  `CheckList` in `app_settings`). Move ASNT-77 (duplicate policy) to Review: the resolver, the
  wizard, and the open table all shipped (see `import-duplicates-plan.md` § Status). Only its
  step 4 is left, which is tracker and docs work. Archive ASNT-79.
- **Update source override.** One app-wide URL that replaces `https://github.com/...` for the
  update manifest, the plugin catalog, and (later) components. It is a small setting plus a
  resolver; `feat/components` builds on it. A mirror or a local `python -m http.server` also
  serves development.

## `feat/file-integrity`: every file stays linked, or the archivist is told

Today a row links a file through its Filename cell and `source_path`, resolved against one
`files_folder`. `file_links.rs` reports missing files as problems. File ▸ Relink Missing Files…
only re-points the whole folder. Gaps:

1. **Detect a moved base.** On open, if `files_folder` is gone, or holds almost none of the
   expected names, say so once with a banner ("Files folder not found: Relink…"). Don't show
   hundreds of per-row problems.
2. **Relink with a preview.** Choosing a new folder shows "N of M files found here" before it
   commits. Offer to search a parent folder when the answer is 0.
3. **Per-row relink.** In the Details panel, "Locate file…" on a missing row (ASNT-41). If the
   archivist picks a file that sits next to other missing ones, offer to relink those too.
4. **Files outside the files folder.** You can already drop a file from anywhere onto a Filename
   cell or into the grid. Decide on one of these and apply it everywhere:
   - **(a)** copy it into the files folder on import (the folder stays self-contained;
     recommended),
   - **(b)** store an absolute path in `source_path` and report it as "outside the project",
   - **(c)** support several named roots.

   (a) keeps export ZIPs and relinking simple. (b) is what happens today by accident.
5. **One file index.** `photos::refresh`, `file_links::Walked`, and the ZIP export each walk the
   folder. Replace them with one cached `PhotoIndex` owned by the table and invalidated by
   events. The watcher branch below feeds it.

## `feat/watch-folder`: based on `feat/file-integrity` (ASNT-78)

Watch the files folder with `notify`, debounced. A new file that matches a row links it. Unmatched
new files go into a "New files (N)" import prompt that uses the ASNT-77 duplicate policy. A delete
or rename updates the index and the missing-file findings live. The same events invalidate the
preview caches (page counts, thumbnails), which are then not re-checked each frame.

## `feat/text-wrap` (ASNT-44)

Wrap cell text the way Google Sheets does: per-column Overflow / Wrap / Clip from the header menu.
Rows grow to their tallest wrapped cell. The hard part is variable row heights in the virtualized
grid; check what `gpui-component`'s table supports before designing around it. Row density
(compact/comfortable) from the settings audit fits here.

## `feat/components`: optional features on demand (ASNT-105)

The base installer ships without the heavy optional parts. An archivist installs one when they
first use it (opening a PDF, turning on visual search, opening the agent panel), and the full
bundle stays on GitHub Releases for people who want everything.

- **What can be optional:** only files the app loads at runtime:
  - PDFium (`dlopen`)
  - ffmpeg (a subprocess)
  - the CLIP weights (already downloaded on demand)
  - the Pi agent runtime

  Code compiled into qrate (candle, the viewer) is not optional unless it moves out of process.
  Measure what each part costs in the installer before deciding.
- **Delivery with no server of ours:** each release publishes one asset per component and
  platform plus a `components.json` manifest, signed with the updater's key. The new
  `components` crate fetches the manifest, verifies it with `updater`'s signature code (share it,
  don't copy it), unpacks into `<data dir>/components/<name>/<version>`, and records a receipt the
  way `plugin-package` does.
- **Lookup order:** `preview` and `agent-runtime` look beside the exe, then in the components
  dir, then on the system. That way the full bundle and the on-demand install behave the same.
- **Settings ▸ Components:** installed/available, size, Install/Remove, and the update source
  override from `main` above.
- **CI:** `release.yml` builds the component assets next to the full bundle. ASNT-103
  (fail when a preview binary is missing) applies to the full bundle only.

## Order

```
main (cleanup, update source) ─┬─ feat/file-integrity ── feat/watch-folder
                               ├─ feat/text-wrap
                               └─ feat/components
```

`file-integrity` and `components` both change how `preview` finds things, so merge whichever is
second after rebasing on the first.
