# qrate-export: crate and publish workflow spec

> Implementation decisions: `data-exchange` remains the import and desktop Google crate. Browser ZIP export and its `files_folder`, `images_needed`, and `to_zip` WASM methods are omitted; native ZIP export stays in `qrate-export`. JSON-LD preserves the app's row IDs and hierarchy links. The interface and ZIP notes below describe the original spike.

This spec is for the crate behind qrate.dvnl.work/convert. The page (`src/pages/convert.astro`) already
runs against a throwaway spike that implements the interface below. The spike is at
`Projects/worktrees/qrate-export-spike` and is outside both repos. This file says what the real crate and
its workflow must contain, and what the spike found out.

## Goal

The website and the app must read a `.qrate` file the same way and write every export format the same
way. They do this by using one crate:

- The app uses it by path, as a member of the workspace.
- The site uses the same crate, built to WASM and published to npm as `@devnull03/qrate-export`.

## Crate

**Location:** `crates/qrate-export` in the qrate repo.

**Package settings:**

- `name = "qrate-export"`, starting at version `0.1.0`.
- It has its own version. Do not use `version.workspace`, because it is published separately from the app.
- `crate-type = ["cdylib", "rlib"]`.
- It is a workspace member, so the current CI clippy and nextest runs already cover it.
- It has no dependencies on other workspace crates.

**Features:**

- Default: native only. The app uses this.
- `wasm`: adds `wasm-bindgen`, `js-sys`, `serde`, and `serde-wasm-bindgen`, and adds `src/wasm.rs`. The
  app never turns this feature on, so no wasm-bindgen code gets into the desktop build.

**Dependencies.** Use the app's versions, so that output is the same byte for byte:

| Crate | Notes |
|---|---|
| `rusqlite = "0.40"` | **The app must move from 0.37 to 0.40 first.** See the rusqlite finding below. Native builds keep `bundled`. Add `serialize` for the WASM read. |
| `csv = "1.3"` | |
| `serde_json` with `preserve_order` | JSON-LD keys stay in column order. Without this feature, the keys are sorted. |
| `zip = "2"`, `default-features = false`, `features = ["deflate"]` | The deflate backend is pure Rust and builds on wasm32. |
| `rust_xlsxwriter` with `features = ["wasm"]` | This is new. The app gets an "Excel…" export too. |

### What moves into the crate

Move each item below. Do not copy it. The old location calls the crate.

| From | Into the crate as |
|---|---|
| `settings/src/project.rs`: `QRATE_APPLICATION_ID`, `QRATE_SCHEMA_VERSION` | Public constants. `settings` uses them from the crate. |
| `settings/src/project.rs`: `load_dataset` | `read_dataset(&Connection) -> (headers, row_ids, rows)`. Keep the same rules: columns in `pragma_table_info` order, without `_row_id` and `_row_order`; rows in `ORDER BY _row_order`; `NULL` becomes `""`. |
| `settings/src/filenames.rs` | The whole file (`keys`, `lookup_keys`). This is the "one rule" its own doc comment describes. |
| `table/src/photos.rs`: `PhotoIndex` | `PhotoIndex::from_paths(impl IntoIterator<Item = PathBuf>)` keeps the matching logic. `table` keeps the disk walk and passes its result into `from_paths`. Keep the tie-break as the component-wise `Path` order. A plain string compare gives a different winner for `a-b/x` against `a/b`. |
| `data-exchange/src/export.rs` | `csv_bytes`, `jsonld_value`, `csl_items`, `CSL_FIELDS`, `CslMapping`. `write_zip` becomes `zip_to(writer, headers, rows, images)`, and `data-exchange` keeps only the part that opens the file. |
| `data-exchange/src/export.rs`: `derive_csl_mapping` | Takes `(header, declared data_type string)` pairs. Move `ColumnType::from_declared` into the crate as well, so `settings` and the crate share one parser. |
| New | `xlsx_bytes(headers, rows)`: a bold, frozen header row, and every cell written as a string. Writing cells as strings keeps dates and identifiers exactly as typed. |

`google.rs` stays in `data-exchange`. What Sheets needs is only headers and rows, and the site makes its
two Sheets calls from JS.

### WASM interface (`src/wasm.rs`)

The page codes against exactly this interface. The spike implements it today.

```rust
#[wasm_bindgen] pub struct Project { /* headers, rows, column types, settings, resolved images */ }

#[wasm_bindgen] impl Project {
    pub fn open(bytes: Vec<u8>) -> Result<Project, JsError>;
    pub fn name(&self) -> Option<String>;              // __settings.name
    pub fn row_count(&self) -> usize;
    pub fn headers(&self) -> Vec<String>;
    pub fn preview(&self, n: usize) -> JsValue;        // string[][]
    pub fn files_folder(&self) -> Option<String>;      // __settings.files_folder
    pub fn csl_fields() -> Vec<String>;                // static
    pub fn csl_default(&self) -> JsValue;              // saved csl_mapping, else derived. A PLAIN OBJECT (see findings)
    pub fn to_csv(&self) -> Result<Vec<u8>, JsError>;
    pub fn to_xlsx(&self) -> Result<Vec<u8>, JsError>;
    pub fn to_jsonld(&self) -> Result<Vec<u8>, JsError>;
    pub fn to_csl(&self, mapping: JsValue) -> Result<Vec<u8>, JsError>;
    pub fn images_needed(&mut self, available: Vec<String>) -> Vec<String>; // webkitRelativePath list in, distinct matched paths out
    pub fn to_zip(&self, paths: Vec<String>, blobs: Vec<js_sys::Uint8Array>) -> Result<Vec<u8>, JsError>;
    pub fn sheet_values(&self) -> JsValue;             // [headers, ...rows] for values.update
}
```

**Error contract.** Every `JsError` message has the form `code: detail`. The page shows its own sentence
for each code (`src/lib/convert.ts` `openErrorMessage`):

| Code | When |
|---|---|
| `not-qrate` | The file is not SQLite, or its `application_id` is not `1097887558`. |
| `newer` | `user_version` is higher than `QRATE_SCHEMA_VERSION`. |
| `empty` | There is no `dataset_main` table, or it has no rows. |
| `corrupt` | SQLite could not deserialize the file, or a query failed. |
| `write` | A format writer failed. |
| `mapping` | `to_csl` got a mapping it could not parse. |

### Tests (plain `cargo test`, run in CI)

- **Parity with the app's read.** Create a project with `settings::project` (use the existing
  `create_and_fill_dataset` test helpers), then check that `read_dataset` on that file gives the same result
  as `settings` loading it.
- **Golden files.** One per format, from a small fixed project: CSV, JSON-LD, CSL-JSON (with a mapping and
  with an empty one), the file list inside the ZIP, and the sheet names and cell values in the XLSX.
- **`PhotoIndex`.** `from_paths` over a folder listing resolves every row the same way as `build(folder)`.
  Include the nested `2020_04/2020_04_001/2020_04_001_001.jpg` layout and the `a-b` against `a/b`
  tie-break.
- **`derive_csl_mapping`.** Title, Identifier, Date, Url, and Text columns map to the expected CSL fields.

## Publish workflow: `.github/workflows/publish-export.yml`

**Rules:**

- Publish only when the crate's version is not on npm yet. Edits without a version bump are never
  published, and a re-run is a no-op.
- Never run because the app is released. App releases come from `v*` tags and pushes to `main`, and this
  workflow listens to neither.
- Create no git tag and no GitHub release. The site's `releases.js` lists GitHub releases, and
  `redeploy-site-on-release.yml` runs on release publish, so a release here would appear in the changelog
  and trigger a site deploy.

```yaml
name: Publish qrate-export

on:
  push:
    branches: [dev]
    paths: ['crates/qrate-export/**']
  workflow_dispatch:

permissions:
  contents: read
  id-token: write # npm trusted publishing (OIDC); no NPM_TOKEN secret

concurrency: publish-export

jobs:
  publish:
    runs-on: ubuntu-latest # has clang, which sqlite-wasm-rs needs
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: wasm32-unknown-unknown }
      - uses: actions/setup-node@v5
        with: { node-version: 24, registry-url: 'https://registry.npmjs.org' }
      - name: Version
        id: v
        run: |
          v=$(cargo metadata --no-deps --format-version 1 \
            | jq -r '.packages[] | select(.name=="qrate-export") | .version')
          echo "version=$v" >> "$GITHUB_OUTPUT"
          # Empty output (the version is not there) or E404 (the package is not there) both mean publish.
          if npm view "@devnull03/qrate-export@$v" version 2>/dev/null | grep -q .; then
            echo "::notice::@devnull03/qrate-export@$v is already published; bump the crate version to publish."
            echo "skip=true" >> "$GITHUB_OUTPUT"
          fi
      - if: steps.v.outputs.skip != 'true'
        run: cargo install wasm-pack --locked
      - if: steps.v.outputs.skip != 'true'
        run: wasm-pack build crates/qrate-export --release --target web --scope devnull03 -- --features wasm
      - if: steps.v.outputs.skip != 'true'
        run: npm publish crates/qrate-export/pkg --access public --provenance
```

**One-time setup:**

1. Publish `0.1.0` by hand once: `wasm-pack build …` and then `npm publish --access public` from `pkg/`,
   logged in as your npm account. The package has to exist before a trusted publisher can be attached to
   it.
2. On npmjs.com, go to the package's Settings, then Trusted publishing, and add GitHub Actions:
   `devnull03/qrate`, workflow `publish-export.yml`.
3. Optional: a CI step on pull requests that warns when files under `crates/qrate-export/**` changed but
   the version did not.

## Site-side swap (after the first publish)

1. Replace `"qrate-export": "file:../worktrees/qrate-export-spike/qrate-export-0.0.0-spike.2.tgz"` with an
   exact version pin: `"@devnull03/qrate-export": "0.1.0"`.
2. Change the two `import('qrate-export')` / `typeof import('qrate-export')` specifiers in `convert.astro`.
3. Bump the pin when you want the site to pick up a new crate version. The pin keeps a deploy from
   changing the formats behind your back.

## Also needed outside this repo

- **Google Cloud, project 805791669854:**
  - Create a *Web application* OAuth client with the origins `https://qrate.dvnl.work` and
    `http://localhost:4321`.
  - Set its client ID as the `PUBLIC_GOOGLE_WEB_CLIENT_ID` build variable, locally in `.env` and in the
    Cloudflare build settings.
  - Allow the `/convert` referrer on the Picker key.
  - The app's current client is a Desktop client and cannot have JavaScript origins, so it can't be used.
  - Until these steps are done, both Sheets rows show "Not set up on this build."
- **Docs link:** add "No qrate installed? Convert a project in your browser: /convert" to
  `docs/export-and-sync.md` on `main`. `scripts/sync-docs.mjs` overwrites the copy on this branch, so it
  can't be added here.
- **Privacy page:** one paragraph saying the converter reads the file only in the browser, and that the
  Sheets export uses Google sign-in on the page.

## What the spike found

| Question | Answer |
|---|---|
| Does rusqlite build for `wasm32-unknown-unknown`? | **0.37 cannot.** Its `libsqlite3-sys` 0.35 handles only WASI. **0.40 can:** its default `ffi-sqlite-wasm-rs` feature compiles SQLite and a small musl subset through `sqlite-wasm-rs`. Two rusqlite versions can't be in one build graph, because `libsqlite3-sys` has `links = "sqlite3"`, so the **app workspace must move to rusqlite 0.40** before it can depend on the crate. |
| Toolchain | The WASM build needs **clang**. Windows needs LLVM (`winget install LLVM.LLVM`). `ubuntu-latest` already has clang. |
| Opening the bytes | `Connection::deserialize_read_exact(MAIN_DB, …, read_only = true)` works. 0.40 renamed `DatabaseName::Main` to `MAIN_DB`. |
| Size | 2.3 MB raw, **790 KB gzip, 637 KB brotli**, loaded only once someone picks a file. The page's own script is 11.7 KB and the JS glue 10.7 KB. |
| `wasm-opt` | The binaryen that wasm-pack 0.15 downloads **crashes** on this module (`Precompute.cpp:838 UNREACHABLE`). The spike sets `[package.metadata.wasm-pack.profile.release] wasm-opt = false`. Try a newer binaryen before shipping, because it would shrink the module further. |
| Astro/Vite | `astro build` emits `_astro/qrate_export_bg.<hash>.wasm`, and the built glue references that hashed name. No Vite config was needed. The browser end-to-end run has **not been done yet**, because the Chrome extension was not connected. `astro dev` may need `optimizeDeps.exclude: ['qrate-export']`. |
| Serving | Cloudflare assets under `wrangler dev` serve the file as `Content-Type: application/wasm`, `Content-Encoding: br`, so `instantiateStreaming` works. |
| Speed (bun, same WASM) | 123 rows × 21 columns: open 38 ms, all five exports 102 ms. **1,894 rows × 32 columns (3 MB): open 110 ms, all five exports 743 ms.** Matching images against 622 files takes 220 ms. This is fine on the main thread, and there is no need for a Web Worker yet. Revisit at around 20k rows. |
| Maps across the boundary | `serde-wasm-bindgen` turns a Rust map into a JS `Map` by default, and a lookup like `mapping[field]` on it finds nothing. Serialize maps with `Serializer::json_compatible()`. The spike had this bug in `csl_default`. |
| **ZIP memory** | 25 of the SACDA test project's images came to 122 MB. The full 305 would be about 1.5 GB, held more than once: the file blobs, the WASM copy, the output `Vec`, and the `Blob`. wasm32 memory tops out at 4 GB. **The real crate needs a streaming ZIP.** Either write entry by entry to a JS `WritableStream` (`showSaveFilePicker` where it's available, otherwise a chunked `Blob`), or let the crate write only the two data files plus a stored-entry header per image, and let JS append the image bytes. |
| Schema drift, live | `Documents/qrate/wallpapers.qrate` already has `user_version = 4`, while `main` is on 3, so a branch build has bumped the schema. The spike correctly refuses the file with `newer`. This is exactly the drift the shared crate prevents: its constant moves with the app. |
| bun + `file:` packages on Windows | A `file:` folder dependency fails with EPERM. A `file:` tarball works, but bun caches it by version, so every rebuild needs a new version number. None of this applies once the package comes from npm. |
