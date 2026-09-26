# Optional components: implementation plan (ASNT-105)

Scope: the base installer ships without the heavy parts that qrate loads at runtime. An archivist
installs one the first time they need it. The full bundle stays on GitHub Releases. Components come
from GitHub Release assets with no server of ours, and one download source setting can point
updates, the plugin catalog and components at a mirror or a local folder.

Starting points: `beta-roadmap.md` § `feat/components` and § main "Update source override".

## 1. What exists today

### How each part is found

| Part | Lookup | Where | Notes |
|---|---|---|---|
| PDFium | beside the exe, then `Pdfium::bind_to_system_library()` | `crates/preview/src/pdf.rs:31-55` | `OnceLock` caches a **miss for the whole session** (`:29-30`). Binding twice aborts the process (`:59-62`), so `pdfium()` binds once (`:66-81`). |
| ffmpeg | beside the exe, then `ffmpeg -version` on `PATH` | `crates/preview/src/media.rs:41-62` | Not cached: with no bundled copy, every decode spawns a `-version` probe. |
| CLIP weights | `<data dir>/models/clip-vit-base-patch32` | `crates/table/src/visual.rs:69-75` | Pinned revision, sizes and SHA-256 in `crates/visual-search/src/lib.rs:25-51`; `download` fetches from Hugging Face (`:63-108`). Already on demand. |
| Pi agent | `<exe dir>/agent`, then `Contents/Resources/agent` | `crates/agent-runtime/src/runtime.rs:95-110` | `init` at startup (`crates/app/src/main.rs:653`) sets the `AgentRuntime` global only if found; the terminal bails without it (`crates/agent-runtime/src/terminal.rs:478`). Pi 0.84.2 pinned at `runtime.rs:6`. |

Other facts that shape the design:

- The PDF tier falls through to the OS thumbnailer when PDFium is missing
  (`crates/preview/src/lib.rs:450-495`), and `can_preview` is extension only (`:94-106`). So a
  missing PDFium is silent. The gpui asset cache keeps the `None` result per
  `(path, max_edge, page)` key (`lib.rs:744-748`), so installing PDFium mid-session does not
  redraw cards that already failed.
- `pdfium-render` uses the `pdfium_latest` API feature (`crates/preview/Cargo.toml:23`), but
  `scripts/fetch-binaries.sh:49` downloads `releases/latest` from bblanchon, and ffmpeg comes from
  gyan.dev's floating "release-essentials" (`:79`). Neither is pinned. A component needs a pinned
  version and hash, so this changes.
- Packaging: NSIS takes the sidecars `/nonfatal` and the `agent\` tree (`scripts/installer.nsi:117-126`),
  solid LZMA (`:64`). The MSI guards them with `HasPdfium`/`HasFfmpeg` (`scripts/installer.wxs:26-27,109-120`).
  `bundle-mac.sh:39-56` copies `libpdfium.dylib`, `ffmpeg` and `agent/` from beside the binary.
- CI: `release.yml` fetches strict sidecars on Windows (`:201-212`), PDFium only on macOS (`:131-144`)
  and Linux (`:311-316`). No platform bundles ffmpeg except Windows. Pi is fetched as
  `darwin-universal`, `linux-x64` or `windows-x64` only (`scripts/fetch-agent-runtime.sh`, `.ps1`).
- The update feed is **not** fetched from GitHub. The app reads `qrate.dvnl.work/updates/{channel}.json`
  (`crates/app/src/update_check.rs:180-183`), which the site prerenders from each release's
  `update-manifest.json` asset (`qrate-site/src/pages/updates/[channel].json.ts`). The plugin
  catalog also comes from the site (`crates/app/src/plugin_marketplace.rs:150-154`). The site origin is
  `localhost:4321` in debug builds (`crates/app/src/site.rs:1-12`).

### Reusable pieces

- `updater` (`crates/updater/src/lib.rs`): `SignedEnvelope` (`:31-37`), embedded Ed25519 key
  `qrate-update-1` (`:25-29`), `verify_envelope`/`verify_with` (`:144-174`), the hash-while-streaming
  download loop inside `fetch_and_stage` (`:244-281`), `sha256_file`/`verify_artifact` (`:358-382`),
  `write_json_atomic` (`:432-442`), the GitHub host check in `select_update` (`:341-346`).
  `verify_with` is private and hard-wired to `UpdateManifest`, so the components crate cannot call it as is.
- `plugin-package` (`crates/plugin-package/src/lib.rs`): the receipt shape (`:117-129`), the signed
  catalog cache that still works offline (`:268-289`), staged install with rollback and receipt written
  last (`:541-659`), receipt-checked removal (`:662-687`), bounded extraction that refuses symlinks
  (`:847-890`). The install and extraction code is plugin specific (zip, `qrate-plugin.json`), so the
  components crate reuses the **pattern**, not the functions.

## 2. Measurements

Measured from the published `v0.5.0-beta.1` assets. Nothing is built locally (`target/` has no
PDFium, ffmpeg or Pi), and no cargo was run for this plan.

| Asset | Size |
|---|---|
| `qrate-0.5.0-beta.1-setup.exe` (NSIS, full) | 75.5 MB |
| `qrate-0.5.0-beta.1-x86_64.zip` (portable) | 110.1 MB |
| `qrate-0.5.0-beta.1-x86_64.msi` | 90.6 MB |
| `qrate-0.5.0-beta.1-universal.dmg` | 123.6 MB |
| `qrate-0.5.0-beta.1-x86_64-linux.tar.gz` | 78.6 MB |

Windows portable zip, per part (raw and deflate sizes from `unzip -lv`; the NSIS column scales each
part's deflate size by 75.5 / 110.1, so it is an estimate, not an LZMA measurement):

| Part | Raw | In zip | Est. in NSIS | Component download |
|---|---|---|---|---|
| `qrate.exe` + update helper | 57.3 MB | 20.8 MB | ~14.3 MB | (base) |
| `pdfium.dll` | 7.4 MB | 3.8 MB | ~2.6 MB | ~3.8 MB |
| `ffmpeg.exe` | 102.9 MB | 38.7 MB | ~26.6 MB | ~38.7 MB |
| `agent/` (226 files, `pi.exe` 108.7 MB) | 118.6 MB | 46.8 MB | ~32.1 MB | ~46.8 MB |
| CLIP weights | not shipped | | | 607.4 MB (`visual-search/src/lib.rs:38-51`) |

Result for Windows: a base NSIS without ffmpeg and Pi is about **16.9 MB** (about 14.3 MB without
PDFium too), from 75.5 MB. ffmpeg and Pi are 78% of the installer. PDFium is 3%.

Linux tarball, raw: `qrate` 103.2 MB, `agent/` 114.5 MB (`pi` 104.5 MB), `libpdfium.so` 7.8 MB,
no ffmpeg. Pi is about half the payload, so a base tarball is roughly 40 MB (estimate). The dmg
carries a lipo'd universal Pi (two architectures), so a per-arch Pi component halves the macOS
download too.

Pi ships material qrate never uses: `agent/docs` 2.7 MB, `agent/examples` 1.0 MB (a Doom WASM),
`agent/assets` 0.5 MB, `CHANGELOG.md` 0.5 MB. Trimming them is a packaging choice (open question 6).

Code that stays compiled in regardless: `candle-core`/`candle-nn`/`candle-transformers`
(visual-search), the `pdfium-render` bindings, `alacritty_terminal` (agent terminal),
`symphonia`/`rodio` (audio). Their share of `qrate.exe` is not measurable from what is on disk:
no release build exists locally and debug rlibs (for example `libcandle_core` at 28.5 MB) say nothing
about linked size. Step 0 measures it. Only moving CLIP out of process would make candle optional,
which is out of scope.

### Step 0 results (2026-09-25)

**Linked size of the always-compiled code: an estimate, not a measurement.** `cargo bloat --release
-p app --crates` was started and stopped partway, because a full release build on top of the other
builds on this machine was too heavy. Its partial `target/bloat/release/deps` still holds 646
release rlibs (3178 MB). By rlib bytes, `candle-*` with `gemm-*`, `safetensors` and `visual-search`
are 287 MB (9.0%), `pdfium-render` 43 MB (1.3%), `symphonia-*` with `rodio` 43 MB (1.4%), and
`alacritty_terminal` 8 MB (0.3%). An rlib holds metadata and generic code that the linker drops, so
these shares are only an upper-bound guide: candle is probably a few MB of the 57.3 MB `qrate.exe`,
not the 9% its rlibs suggest. Run the real `cargo bloat` in CI or on an idle machine before quoting
a number.

**Pinned component inputs** (`scripts/fetch-binaries.sh` and `.ps1`, each download checked against
its SHA-256; digests from the GitHub release API, and the Windows x64 ones checked again locally):

PDFium, `bblanchon/pdfium-binaries` release `chromium/7881`, the build `pdfium-render` 0.9.3's
`pdfium_latest` feature binds (`pdfium_7881`). `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7881/pdfium-<platform>.tgz`:

| Platform | Size | SHA-256 |
|---|---|---|
| `win-x64` | 3733154 | `73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08` |
| `win-arm64` | 3522432 | `d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84` |
| `linux-x64` | 3644759 | `1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d` |
| `linux-arm64` | 3588127 | `ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674` |
| `mac-x64` | 3588813 | `6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b` |
| `mac-arm64` | 3533019 | `52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40` |
| `mac-univ` | 7006774 | `df451a413c3609585e84a4a91110a9bc889cff05fe3b2db0ed817c9e90c3f7d3` |

ffmpeg, BtbN LGPL static build `n8.1.2-50-g1a748fe2cd` (the 8.1 release branch) from
`BtbN/FFmpeg-Builds` release `autobuild-2026-08-31-13-27`. It is a month-end autobuild because BtbN
keeps those for about two years and the daily ones for two weeks.
`https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-n8.1.2-50-g1a748fe2cd-<target>-lgpl-8.1.<ext>`:

| Target | Size | SHA-256 |
|---|---|---|
| `win64` (`.zip`) | 146078616 | `f6274bbd9c247f9e90c1bbed066b03ed4a3907cece2fb91be6dd352393936365` |
| `winarm64` (`.zip`) | 95498313 | `0ad4d6e7342d6d77bbae4ac230964ebd51bdd6192414392e5f521d295b39b111` |
| `linux64` (`.tar.xz`) | 112545684 | `7d6d93e9c39e0e461feb13c118e91e4eec2515e4da3a01d4ad6790996731bbee` |
| `linuxarm64` (`.tar.xz`) | 97141720 | `56b37b6f2832ba37bd4979ae5c4521ae718efa41846a0d3ecfbbe492137c66f6` |

macOS keeps `brew install ffmpeg` (§ 7).

Component sizes these give on Windows x64, measured by packing the fetched file into a tar.gz:
PDFium `pdfium.dll` 7.2 MB raw, **3.6 MB** download. BtbN `ffmpeg.exe` 114.0 MB raw, **47.5 MB**
download. The LGPL build is larger than the gyan.dev GPL "essentials" build it replaces (102.9 MB
raw, 38.7 MB deflated), so the ffmpeg banner's size grows by about 9 MB. The size shown to the user
comes from the manifest, so nothing is hardcoded.

## 3. Design

### Crate: `components`

New crate `crates/components`. It depends on `updater` (feature `client`), `settings`, `gpui`,
`semver`, `serde`, `serde_json`, `flate2`, `tar`, `tempfile`, `log`. Two files:

- `src/lib.rs`: the gpui-free core. Manifest types, verification, disk layout, `locate`, install,
  remove, sweep. Every function takes explicit paths or a `Source` so tests run on temp dirs.
- `src/jobs.rs`: the `Components` gpui global. Single-flight installs, progress, cancel, and the
  state that Settings, first-use prompts, the agent panel, the search bar and onboarding all read.

`preview`, `agent-runtime`, `table`, `workspace`, `app` and the onboarding crate depend on it.
`preview` gains its first network-capable dependency. That adds compile edges, not binary size,
because `app` already links `reqwest`.

### Public API

```rust
// lib.rs
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId { Pdfium, Ffmpeg, Agent, Clip }

impl ComponentId {
    pub const ALL: [ComponentId; 4];
    pub fn label(self) -> &'static str;          // "PDF preview", "Video preview", ...
}

pub struct Manifest { pub app_version: Version, pub components: Vec<Component> }
pub struct Component { pub id: ComponentId, pub version: String, pub app: VersionReq,
                       pub license: String, pub asset: Asset }   // asset for this os/arch only
pub struct Asset { pub url: String, pub size: u64, pub sha256: String,
                   pub installed_size: u64, pub archive: Archive }
pub struct Receipt { pub schema: u32, pub id: ComponentId, pub version: String,
                     pub app: VersionReq, pub url: String, pub sha256: String, pub bytes: u64,
                     pub installed_at_unix_seconds: u64 }

pub enum Found { Bundled(PathBuf), Installed(PathBuf), System(PathBuf) }
pub enum Removal { Removed, AfterRestart }

pub fn root() -> Option<PathBuf>;                                      // <data dir>/components
pub fn locate(id: ComponentId) -> Option<PathBuf>;                     // installed + compatible; no network
pub fn generation() -> u64;                                            // bumps on install/remove
pub fn receipt(id: ComponentId) -> Option<Receipt>;
pub fn fetch_manifest(source: &updater::Source) -> Result<Manifest>;   // falls back to the cached copy
pub fn install(id: ComponentId, manifest: &Manifest, source: &updater::Source,
               progress: &dyn Fn(u64, u64), cancel: &AtomicBool) -> Result<Receipt>;
pub fn remove(id: ComponentId) -> Result<Removal>;
pub fn sweep();                                                        // startup: pending removals, stale versions, partials

// jobs.rs
pub enum State {
    Bundled, System(PathBuf),
    Missing { download: Option<u64> },            // None until a manifest has been read
    Installing { received: u64, total: u64 },
    Installed { version: String },
    UpdateRequired { download: Option<u64> },     // receipt no longer matches this app version
    Unavailable(SharedString),                    // no asset for this platform/version, or offline with no cache
    Failed(SharedString),
}
pub fn init(cx: &mut App);
pub fn state(id: ComponentId, cx: &App) -> State;
pub fn install(id: ComponentId, cx: &mut App) -> Task<Result<()>>;  // single-flight: a second call joins the first
pub fn cancel(id: ComponentId, cx: &mut App);
pub fn remove(id: ComponentId, cx: &mut App) -> Result<Removal>;
pub fn refresh(cx: &mut App);                                       // re-read the manifest
```

Observers use `cx.observe_global::<Components>`. That is the whole onboarding surface: `ComponentId::ALL`,
`state`, `install` (awaitable), `cancel`, and the observer. No UI types cross it.

`locate` never touches the network, so every tier can call it on the render path. It caches its
answer per `generation()` in a `RwLock`, and it rejects a receipt whose `app` range does not match
`CARGO_PKG_VERSION`. The crate uses `version.workspace = true`, so that is the app's version.

### Lookup order

Beside the exe, then the components dir, then the system. The full bundle and an on-demand install
then behave the same, and a bundled copy always matches its app version.

| Part | Change |
|---|---|
| PDFium | `library()` tries beside exe, then `components::locate(Pdfium)`, then system. Cache a **hit** only; on a miss, probe again when `components::generation()` has moved. `pdfium()` still binds at most once. |
| ffmpeg | `binary()` tries beside exe, then `locate(Ffmpeg)`, then `PATH`. Cache the result per generation, which also removes today's per-decode `-version` spawn. |
| Pi | `bundled_root()` adds `locate(Agent)` after the two bundle paths. After an install, `jobs.rs` calls `agent_runtime::init(cx)` again, which already replaces the global. |
| CLIP | `model_dir()` becomes `locate(Clip)`. `visual_search::download` is deleted. |

Stale preview misses: `preview::source` puts `components::generation()` into the asset key for
extensions the PDF or media tiers handle (0 for everything else). An install makes new keys for
PDFs and videos only, and raster images do not decode again.

Removal while loaded: PDFium stays mapped once bound, and a running `pi.exe` is locked on Windows.
`remove` renames the version dir to `.trash-<n>` when it can. If it cannot, it records
`pending-removal.json` and returns `Removal::AfterRestart`. `sweep()` finishes removals at the next
startup, before any tier loads.

### Disk layout

```
<data dir>/components/
  receipts/<id>.json            written last; a dir without a receipt does not exist
  <id>/<version>/...            the installed tree
  <id>/.staging-XXXX/           tempdir during an install, renamed into place
  downloads/<asset>.partial     resumable download
  manifest-<app version>.json   last verified signed envelope (offline fallback)
  pending-removal.json
```

Install order follows `plugin-package` (`lib.rs:541-659`). Download to `.partial` while hashing,
check size and SHA-256 against the signed manifest, extract into a staging tempdir under
`<id>/`, rename it to `<id>/<version>`, write the receipt atomically with `updater::write_json_atomic`,
bump `generation()`, and only then delete the previous version. If the rename or receipt fails, the
previous version and receipt stay in place.

Extraction: tar (optionally gzip), entry by entry. It rejects absolute paths, `..`, symlinks and
hard links. It stops when the running total passes the signed `installed_size`. It keeps mode bits
so `pi` and `ffmpeg` stay executable. This is new code, about 40 lines. `plugin-package`'s
extractor is zip only and private.

CLIP migration: `init` adopts an existing `<data dir>/models/clip-vit-base-patch32` when
`visual_search::installed` passes. That is the same size check that marks it installed today. It
moves the dir to `clip/<version>/` and writes a receipt, with no 600 MB download again.

### Sharing `updater`'s trust code (make pub, do not copy)

In `crates/updater/src/lib.rs`:

1. Split `verify_with` (`:152-174`) in two:
   `pub fn verify_payload_with(key: &VerifyingKey, key_id: &str, envelope: &SignedEnvelope) -> Result<Vec<u8>>`
   (schema, key id, base64, signature; returns the verified bytes) and
   `pub fn verify_payload(envelope: &SignedEnvelope) -> Result<Vec<u8>>` (embedded key, `UPDATE_KEY_ID`).
   `verify_with` becomes `verify_payload_with(key, UPDATE_KEY_ID, e)` plus the `UpdateManifest`
   parse. Existing tests keep passing unchanged.
2. `pub const UPDATE_KEY_ID`.
3. Pull the download loop out of `fetch_and_stage` (`:244-281`) into
   `#[cfg(feature = "client")] pub fn download_verified(url: &str, size: u64, sha256: &str, dest: &Path, progress: impl FnMut(u64, u64), cancel: &AtomicBool) -> Result<()>`.
   It adds a cancel check per chunk, `Range` resume from an existing `.partial` (GitHub serves 206),
   and `file://` sources. `fetch_and_stage` calls it. `visual_search::download` is deleted, so no
   third copy of the loop remains.
4. `sha256_file` and `write_json_atomic` are already pub. The components crate uses them.

Domain separation: the same key signs both manifests. The components payload has
`"kind": "qrate-components"` and a `components` array, and `verify` rejects a payload without that
kind. So an update envelope cannot pass as a components envelope. The reverse also fails:
`UpdateManifest` needs `channel` and `artifacts`, which a components payload does not have.

Dev trust: only in debug builds (`cfg(debug_assertions)`), `components` also accepts key id
`qrate-dev` with the public key from `QRATE_DEV_SIGNING_KEY` (base64url). The plugin catalog does the
same thing at runtime (`plugin_marketplace.rs:132-134`). Release builds trust only the embedded key.

### Delivery and data formats

Each release uploads one asset per component and platform, plus `components.json`:

```
https://github.com/devnull03/qrate/releases/download/v<app version>/components.json
https://github.com/devnull03/qrate/releases/download/v<app version>/component-<id>-<version>-<os>-<arch>.tar.gz
```

The app fetches `components.json` from **its own version's tag**. The tag decides compatibility,
not a "latest" lookup, and the site is not involved. A development build whose version has no
release gets a 404, reported as `Unavailable("No components are published for this development
build")`. The `fetch-binaries` scripts and the download source still work there.

`components.json` uses the envelope that `update-manifest.json` uses:

```json
{
  "schema": 1,
  "key_id": "qrate-update-1",
  "payload_base64": "<base64 of the payload below>",
  "signature_base64": "<Ed25519 over those exact bytes>"
}
```

Payload (sizes and versions are illustrative):

```json
{
  "kind": "qrate-components",
  "schema": 1,
  "app_version": "0.6.0-beta.1",
  "published_at": "2026-10-01T00:00:00Z",
  "components": [
    {
      "id": "pdfium",
      "version": "chromium-7350",
      "app": ">=0.6.0-beta.1, <0.7.0",
      "license": "Apache-2.0 OR BSD-3-Clause",
      "assets": [
        {
          "os": "windows", "arch": "x86_64",
          "url": "https://github.com/devnull03/qrate/releases/download/v0.6.0-beta.1/component-pdfium-chromium-7350-windows-x86_64.tar.gz",
          "size": 3758011,
          "sha256": "<64 hex>",
          "installed_size": 7375360,
          "archive": "tar.gz"
        }
      ]
    },
    {
      "id": "clip",
      "version": "b33cedf",
      "app": ">=0.6.0-beta.1",
      "license": "MIT",
      "assets": [
        {
          "os": "any", "arch": "any",
          "url": "https://github.com/devnull03/qrate/releases/download/components-clip-b33cedf/component-clip-b33cedf.tar",
          "size": 607385600,
          "sha256": "<64 hex>",
          "installed_size": 607381925,
          "archive": "tar"
        }
      ]
    }
  ]
}
```

Rules checked after verification:

- `app_version` equals the running version. A mirror cannot serve another release's manifest.
- Unknown `id`s are ignored, for forward compatibility.
- `url` must start with `https://github.com/devnull03/qrate/releases/download/`. This is the same
  rule as `select_update:341-346`, and it applies to the signed URL before any mirror rewrite.
- `size`, `installed_size` and `sha256` are well formed. `arch: "any"` matches every platform.
- `app` is a `semver::VersionReq`. The receipt copies it, so after an app update `locate` stops
  returning a component the new version cannot use. Why it matters: `pdfium-render`'s API feature
  fixes the PDFium ABI it expects, and the Pi extension speaks one bridge protocol (`AGENTS.md`: protocol 2).

CLIP's asset lives in one fixed release (`components-clip-b33cedf`), uploaded once, because putting
607 MB into every release adds nothing. Each release's `components.json` points at it. The tar is
uncompressed because float weights barely compress.

Receipt (`receipts/pdfium.json`), shaped like `plugin_package::InstallReceipt`:

```json
{
  "schema": 1,
  "id": "pdfium",
  "version": "chromium-7350",
  "app": ">=0.6.0-beta.1, <0.7.0",
  "url": "https://github.com/devnull03/qrate/releases/download/v0.6.0-beta.1/component-pdfium-chromium-7350-windows-x86_64.tar.gz",
  "sha256": "<64 hex>",
  "bytes": 3758011,
  "installed_at_unix_seconds": 1790000000
}
```

### Download source (the roadmap's "update source override")

One app-wide setting, `download_source` (`updater::DOWNLOAD_SOURCE_KEY`), stored in `AppSettings`
like `automatic_updates`. The environment variable `QRATE_DOWNLOAD_SOURCE` takes precedence, for
development. Empty means the defaults.

`updater::Source` (behind the `client` feature, so the helper binary does not grow):

```rust
pub struct Source { base: Option<reqwest::Url> }
impl Source {
    pub fn parse(value: Option<&str>) -> Result<Source>;   // https://any, http://loopback only, file:///dir
    pub fn site(&self, default_origin: &str, path: &str) -> String;
    pub fn github(&self, url: &str) -> String;
}
```

Rewrites, applied only after the signed content has been verified or when the target is a hashed
download:

| What | Default | With a download source |
|---|---|---|
| Update feed | GitHub: `releases/latest/download/update-manifest.json` for a stable build, the newest signed tag from the releases API for a pre-release build (step 1, § 7) | `<base>/updates/{channel}.json`, then GitHub on 404 |
| Plugin catalog | `<site>/plugins/catalog.json` (+ `.sig`) | `<base>/plugins/catalog.json` (+ `.sig`) |
| Any GitHub release asset (update artifact, components, plugin artifact) | `https://github.com/<path>` | `<base>/github/<path>` |

The mirror is untrusted by design. Every payload is signed (update envelope, catalog `.sig`,
components envelope), and every artifact is checked against a signed size and SHA-256. A mirror
can withhold content but cannot change it. When a rewritten asset returns 404, the download tries
the original GitHub URL once and logs a `warn`. A partial mirror, such as a dev folder with only
components, still works.

Dev recipe (goes in `docs/dev/SETUP.md`):

```bash
openssl genpkey -algorithm ed25519 -out dev-key.pem          # once
mkdir -p mirror/github/devnull03/qrate/releases/download/v0.6.0-beta.1
QRATE_UPDATE_SIGNING_KEY="$(cat dev-key.pem)" QRATE_SIGNING_KEY_ID=qrate-dev \
  ./scripts/build-components-manifest.sh mirror/github/devnull03/qrate/releases/download/v0.6.0-beta.1 v0.6.0-beta.1
(cd mirror && python -m http.server 8000)
QRATE_DOWNLOAD_SOURCE=http://127.0.0.1:8000 QRATE_DEV_SIGNING_KEY=<base64url pub> cargo run
# or QRATE_DOWNLOAD_SOURCE=file:///C:/path/to/mirror
```

Reading the setting: `settings` already owns the persisted value. `components::source(cx) -> updater::Source`
is the one reader: environment variable, then `AppSettings`, and an invalid value falls back to the
defaults with one `log::warn!`. `app` calls it for the update feed and the plugin catalog.

### UX

First-use prompts are **inline and non-modal**, where the need shows. They never appear per
thumbnail, so a gallery of PDFs does not nag.

| Trigger | Where | Text |
|---|---|---|
| A PDF opens in the viewer or details panel and PDFium is `Missing` | `crates/workspace/src/viewer/mod.rs`, `panels/details.rs` | "PDF pages need PDF preview, a 4 MB download. [Install] [Not now]" |
| A video opens and ffmpeg is `Missing` (none beside the exe or on `PATH`) | same | "Video frames need Video preview, a 39 MB download. [Install] [Not now]" |
| Agent panel opens with no runtime | `crates/workspace/src/panels/agent.rs` | "The assistant needs its runtime, a 47 MB download. [Install]" |
| Visual search with no model | existing button, `crates/table/src/panel.rs:1807-1808` | "Download model (607 MB)", now driven by `components::install(Clip)` |

Sizes come from the manifest, never hardcoded. The brief's "30 MB" for PDF is about 4 MB measured.
While `Installing`, the banner shows "Downloading 12 / 39 MB [Cancel]". On success, PDFs and videos
redraw through the generation key and the agent panel starts its terminal. "Not now" hides the
banner for the session. Settings stays the way back in.

Settings ▸ Components (new page in `crates/app/src/app_settings/mod.rs`, one row per `ComponentId`):

- Status line: "Included with this install" (bundled) / "Using the system copy at /usr/bin/ffmpeg" /
  "Installed, chromium-7350, 7.4 MB on disk" / "Not installed, 3.8 MB download" /
  "Needs an update for this version of qrate" / the `Unavailable` reason.
- Buttons: Install, Update, Remove (confirm; "Removed after restart" when `AfterRestart`), Cancel while installing.
- The existing "Visual search model / Remove model…" item (`app_settings/mod.rs:803-830`) moves here and is deleted from its old place.
- The download source text field lives in Application ▸ "Updates and downloads" (`:133`), because it
  covers updates and plugins too. The Components page links to it.

Offline: `locate` and installed components never need the network. `fetch_manifest` falls back to
the last verified `manifest-<version>.json`, as `fetch_catalog_cached` does, so sizes still show. An
install that cannot connect ends in `Failed("qrate could not reach GitHub. Check the connection or
the download source in Settings.")` and logs a `warn` naming the URL.

After an app update, a component with a receipt whose `app` range no longer matches becomes
`UpdateRequired`. When automatic updates are on, `jobs.rs` reinstalls it in the background, since the
user consented once already. When they are off, the first-use banner says "needs an update".

### ASNT-103 on the full bundle

- CI: the full-bundle jobs keep `QRATE_STRICT_BINARIES=1`, and the release job checks that every
  full artifact holds each part its platform bundles, and that `components.json` has an asset for
  every component × platform the matrix builds.
- Runtime: `InstallMarker` gains `#[serde(default)] flavor: Flavor` (`base` | `full`, default `full`,
  so old markers read as full). On startup, a `full` install whose bundled part is missing logs
  `log::error!("this qrate install is missing <part>; reinstall or use Settings ▸ Components")`. That
  line reaches bug reports through Help ▸ Copy Debug Info.

### Base and full bundles with the updater

A full install must update to a full package, and a base install to a base one. Otherwise an update
leaves stale sidecars beside the exe, which win the lookup, or drops the bundled parts. So:

- `UpdateArtifact` gains `#[serde(default)] flavor: Flavor`, and `artifact_for` (`updater/src/lib.rs:176-191`) matches it against the marker.
- `build-update-manifest.sh` lists **full artifacts first**. 0.5 clients ignore `flavor` and take the first
  match for their kind, and every 0.5 install is full.
- Its descriptors (`:26`) gain a flavor column, and the base and full file names get distinct
  suffixes. Today `*"-setup.exe"` would match both installers, and the loop at `:44` keeps the last one.

## 4. File-by-file changes

| File | Change |
|---|---|
| `Cargo.toml` | Add `crates/components` to members. |
| `crates/components/{Cargo.toml,src/lib.rs,src/jobs.rs}` | New crate (above). |
| `crates/updater/src/lib.rs` | `verify_payload`, `verify_payload_with`, `pub UPDATE_KEY_ID`, `download_verified`, `Source`, `DOWNLOAD_SOURCE_KEY`, `Flavor` on `InstallMarker`/`UpdateArtifact`, flavor in `artifact_for`, `fetch_and_stage` takes `&Source`. |
| `crates/preview/Cargo.toml`, `src/pdf.rs`, `src/media.rs`, `src/lib.rs` | Lookup order; cache hits only, per generation; generation in the asset key for pdf/media. |
| `crates/agent-runtime/Cargo.toml`, `src/runtime.rs` | `locate(Agent)` in `bundled_root`; the missing-runtime message points to Settings ▸ Components. |
| `crates/visual-search/Cargo.toml`, `src/lib.rs` | Delete `download`; drop `reqwest` and `sha2`; `installed` stays for migration. |
| `crates/table/src/visual.rs`, `src/panel.rs`, `src/lib.rs` | `model_dir`/`install`/`remove_model` go through `components`; `Status::Downloading` reads `components::state`. |
| `crates/workspace/src/viewer/mod.rs`, `panels/details.rs`, `panels/agent.rs` | First-use banners; agent panel install state. |
| `crates/app/src/main.rs` | `components::sweep()` before `preview`/`agent_runtime` init; `components::init(cx)`. |
| `crates/app/src/update_check.rs`, `plugin_marketplace.rs` | Use `components::source(cx)`. |
| `crates/app/src/app_settings/mod.rs` | Components page; download source field; delete the old visual model item. |
| `scripts/fetch-binaries.sh`, `.ps1` | Pin the PDFium and ffmpeg versions and hashes (they are the component inputs now). |
| `scripts/fetch-agent-runtime.sh` | Add `darwin-x64`, `darwin-arm64` single-arch targets. |
| `scripts/package-component.sh` (new) | `<id> <src dir> <os> <arch> <dist>`: makes the tar.gz, prints a descriptor line. |
| `scripts/build-components-manifest.sh` (new) | Like `build-update-manifest.sh`: collects `component-*`, signs `components.json`; honours `QRATE_SIGNING_KEY_ID`. |
| `scripts/build-update-manifest.sh` | Flavor column; full first; distinct suffixes. |
| `scripts/installer.nsi`, `scripts/bundle-mac.sh` | `/DFLAVOR=base` skips the sidecars and `agent\`; writes `flavor` into the marker; the NSIS uninstaller removes `${DATADIR}\components`. |
| `.github/workflows/release.yml` | Per-platform component packaging (macOS per arch in the `build-macos` matrix); base and full packages; sign and upload `components.json` and `component-*`; SHA256SUMS covers them; release notes list the base download first. |
| `.github/workflows/publish-clip.yml` (new, manual) | One-time upload of `component-clip-b33cedf.tar` to `components-clip-b33cedf`. |
| `AGENTS.md` | The bundled assistant may be installed on demand (required by the CLAUDE.md rule for `crates/agent-runtime`). |
| `docs/dev/SETUP.md`, `docs/dev/agent-runtime.md` | Mirror recipe; where Pi now lives. |
| `qrate-site` (branch `site`) | Download links point to the base flavor; hide `component-*` in the releases listing. Separate commit on that branch. |

## 5. Implementation steps (each shippable on `main`)

0. **Measure.** One `cargo bloat --release -p app --crates -n 30` with the cargo slot, recorded here.
   Say which exact PDFium and ffmpeg versions to pin. No code.
1. **Download source.** `updater::Source` + `DOWNLOAD_SOURCE_KEY`, the reader, the update feed and
   the plugin catalog through it, and the settings field. This is the roadmap's `main` item and
   useful on its own.
2. **Updater refactor.** `verify_payload*`, `download_verified` (cancel, resume, `file://`).
   `fetch_and_stage` uses them. No behavior change; existing tests are the check.
3. **`components` core + CI assets.** The crate without `jobs.rs`, the new scripts, and `release.yml`
   publishing `components.json` and component assets next to the unchanged full bundle. Nothing in
   the app calls it yet, and the next tag proves the pipeline.

   **Done (2026-09-26).** `crates/components/src/lib.rs`, `scripts/package-component.sh`,
   `scripts/build-components-manifest.sh`, `scripts/fetch-clip-weights.sh`, and the component
   steps in `release.yml`. A dev-key manifest built by the scripts was installed by the crate end to
   end. Deviations from § 3 and § 4:
   - The API is a `Store` over an explicit root (`verify`, `fetch_manifest`, `receipt`, `locate`,
     `install`, `remove`, `sweep`), and `generation()` stays a free function. `root()` moves to
     `jobs.rs`, which can call `settings::data_dir()`; the core does not depend on `settings` or
     `gpui`. `locate` does not cache: the step 4 callers cache per `generation()`, as the lookup
     table says. `ComponentId::label` and `Found` come with the steps that use them.
   - No `pending-removal.json`. The receipt is the only state: `remove` deletes it first, then moves
     the folder to `.trash-<n>`. When Windows refuses (a loaded PDFium, a running Pi), it returns
     `AfterRestart`, and `sweep` deletes every folder that no receipt names.
   - `app` ranges use plain version order and comparison operators only. SemVer's own rule, that
     a pre-release only matches a range naming its x.y.z, would make each beta update download its
     components again. The manifest script writes `>=X.Y.0-0, <X.(Y+1).0-0` for PDFium and Pi and
     `>=X.Y.0-0` for ffmpeg and CLIP, so a PDFium or Pi pin moves only in a minor release.
   - CLIP follows § 7, not the fixed `components-clip-b33cedf` release: the release job fetches the
     pinned weights on the runner (or, if Hugging Face fails, an earlier release's copy, checked
     against the same pins) and attaches `component-clip-b33cedf-any-any.tar` to every release, so
     `publish-clip.yml` is not needed. The Hugging Face origin (revision, sizes, SHA-256) is compiled
     into `components` and needs no manifest; `ClipSource` under the setting `clip_source`
     (`hugging-face` by default, or `github`) picks which goes first, and the other is the
     fallback. Hugging Face downloads do not go through the download source.
   - Downloads are content-addressed, `downloads/<sha256>.partial`; `sweep` keeps a partial for a
     week. Component versions come from the fetch scripts' pins: `chromium-7881`,
     `n8.1.2-50-g1a748fe2cd`, `0.84.2-ext.0.2.1` (Pi and the qrate extension), `b33cedf`.
   - `updater::client` and `updater::read_bytes` are public, for `fetch_manifest`.
   - The release job verifies the new manifest's signature with `openssl pkeyutl -verify` inside
     `build-components-manifest.sh`, and `QRATE_COMPONENTS_REQUIRE` in `release.yml` fails the
     release if any id and platform is missing. `SHA256SUMS.txt` covers the component archives.
   - `build-update-manifest.sh` is unchanged: nothing new ends in `-setup.exe` until step 6.
4. **Lookup order + jobs + CLIP.** The preview, agent-runtime and visual changes, `jobs.rs`, the CLIP
   migration, `sweep`. Full bundles behave exactly as before (beside the exe wins). Visual search now
   downloads from GitHub.

   **Done (2026-09-26).** `crates/components/src/jobs.rs`; lookup order in `preview` (`pdf.rs`,
   `media.rs`, the asset key in `lib.rs`), `agent-runtime` and `table::visual`; `components::init`
   in `main`. Deviations from § 3 and § 4:
   - `jobs.rs` exposes `store()` (the app's `Store` over `<data dir>/components`, used off the
     gpui thread by the tiers) instead of a separate `root()`. `source(cx)` moved here from
     `app::update_check::download_source`.
   - `State` is `Missing { download }`, `Downloading { received, total }`, `Installing`,
     `Installed { version }`, `UpdateRequired` and `Failed`. `Bundled`, `System` and `Unavailable`
     come with the UI in step 5, which is the first reader that needs them; so do `refresh` and
     the automatic reinstall after an app update. The manifest is read by the first install.
   - `components::remember(slot, generation, find)` is the per-generation cache both tiers use:
     ffmpeg keeps its answer (hit or miss) per generation, PDFium keeps a bind forever and a miss
     per generation. The PDF page count is no longer written to the disk cache when PDFium is
     missing, so it is counted again after an install. An OS-thumbnailer picture already in the
     disk cache stays until the file or the cache changes.
   - `agent_runtime::init` observes `Components` itself (a dependency from `components` to
     `agent-runtime` would be a cycle) and sets or removes `AgentRuntime` when `generation()` moves.
   - CLIP: `visual_search::installed`, `download`, `MODEL_DIR` and the pins are deleted, so the
     pins live only in `components`. Adoption checks SHA-256, not just sizes, runs in the
     background after `init`, deletes a legacy folder that does not match, and removes the empty
     `models` folder. `table::visual` mirrors `state(Clip)` into its `Status`; "Remove model…"
     goes through `table::remove_visual_model`, which drops the loaded model and calls
     `components::remove`.
5. **UI.** Settings ▸ Components, first-use banners, agent panel state. The onboarding crate can call
   the API from here on.

   **Done (2026-09-26).** `crates/app/src/app_settings/components_page.rs`,
   `crates/workspace/src/component_banner.rs`, and the states in `jobs.rs`. Deviations from § 3:
   - `State` gains `Bundled`, `System` and `Unavailable(reason)`. `components` cannot see the tiers
     (they depend on it), so each tier registers a finder with `found_by(id, fn() -> Option<Found>, cx)`:
     `app::main` for `preview::pdfium_found` and `preview::ffmpeg_found`, `agent_runtime::init` for
     Pi. A finder answers only for a copy qrate did not install. Order in `state`: a running job,
     a failure, `Bundled`, the receipt (`Installed`/`UpdateRequired`), `System`, then the manifest.
     A system copy never hides an installed one, so Remove stays reachable.
   - `refresh(cx)` reads the manifest in the background. Nothing reads it at startup: a banner calls
     it when it has no size to show, and the Settings page each time it opens ("Check again" too).
     A manifest without the component, or one that cannot be read and has no cached copy, makes it
     `Unavailable`; on macOS ffmpeg's reason points at Homebrew. CLIP is never `Unavailable`.
   - After an app update, `init` reinstalls every `UpdateRequired` component in the background when
     automatic updates are on. `automatic_updates(cx)` moved here from `app::update_check`.
     `Store::install` first compares the receipt with the manifest: the same version and SHA-256
     only rewrites the receipt's `app` range, with no download. That holds because
     `package-component.sh` now packs reproducibly (sorted entries, zero mtimes and owners, modes
     reduced to 755/644, no gzip timestamp), so unchanged inputs give the same archive each release.
   - `ComponentId::label` is the name the UI shows. The sizes use `preview::file_size` (binary
     units), not the decimal MB of § 3's examples.
   - Banners: the viewer shows one at the top and reopens the file once the part lands, so a PDF's
     page count is read again; the details panel shows it where a recording's transport goes;
     the agent panel shows it under the status line and starts Pi once `AgentRuntime` appears.
     `preview::missing(path)` says which part a file needs. "Not now" is per session
     (`NotNow` global); the agent panel has none, since the banner is its only content.
   - Settings ▸ Components is one entity (`ComponentRows`) that observes `Components`, since the
     Settings window re-renders only on settings changes. It also holds the `clip_source` dropdown.
     The old "Visual search model" item under Table ▸ Previews is gone.
   - Sizes on disk are not shown: the receipt keeps the download size, and walking the folder on
     every render is not worth it.
6. **Base bundle.** `Flavor` in marker/manifest, base packages in `release.yml`, update-manifest
   ordering, site links, ASNT-103 runtime check. This is the step that shrinks the default download,
   and it can wait until 4 and 5 have been through one beta.

`feat/file-integrity` also changes `preview`. Rebase whichever branch lands second (roadmap § Order).

## 6. Test plan

`components` (unit, temp dirs, test key via `verify_payload_with`):
- Accepts a signed manifest. Rejects tampering, a wrong key, an unknown schema, an update envelope
  (no `kind`), `app_version` ≠ running version, and a non-GitHub signed URL.
- Asset selection by os/arch and `any`. Unknown ids ignored. `VersionReq` filtering.
- End-to-end install from a `file://` mirror: receipt written, `locate` returns the path,
  `generation` bumps. Size overflow, hash mismatch, `..`, symlink and oversize expansion are each
  refused with the previous version still in place.
- Cancel mid-download leaves no receipt and a resumable `.partial`. A resumed download completes and verifies.
- `remove`: `Removed`, and `AfterRestart` with a held handle (Windows). `sweep` finishes it.
- CLIP adoption moves a size-valid legacy dir and writes a receipt. It ignores an incomplete one.
- `Source::parse`: https ok, http non-loopback refused, `file://` ok. Rewrites for site paths and GitHub paths. 404 on the mirror falls back to the original.

`components::jobs` (`#[gpui::test]`, explicit imports, no `use super::*`): two `install` calls share
one job; state goes `Missing → Installing → Installed`; `cancel` returns to `Missing`; the observer fires.

`updater`: the existing suite unchanged; `verify_payload_with` on a non-update payload; `artifact_for`
with flavors, plus an old manifest with no `flavor` read as full.

`preview` and `agent-runtime`: factor the candidate order into a pure fn
(`candidates(exe_dir, installed) -> Vec<PathBuf>`) and assert beside, then components, then system.
The existing binary-dependent PDF and video tests keep their two-way meaning (CLAUDE.md § Preview binaries).

CI: the release job checks `components.json` with `openssl pkeyutl -verify`, and checks that every
matrix platform has an asset for each id it publishes. The base-bundle jobs assert the sidecars are
**absent**, and the full ones assert them present (ASNT-103).

Manual before step 6 ships: on Windows, install the base NSIS, open a PDF and a video, install both
from the banners, update qrate from the previous base build, remove PDFium while a PDF is open, and
repeat with `QRATE_DOWNLOAD_SOURCE` pointing at a local folder.

## 7. Decisions (2026-09-26)

- **PDFium is optional.** It leaves the base bundle and installs on the first PDF opened, like ffmpeg.
- **ffmpeg:** pinned BtbN LGPL builds for Windows and Linux. macOS keeps the system ffmpeg
  (`brew install ffmpeg`) for now.
- **CLIP weights:** every release's workflow downloads the pinned weights on the runner and attaches
  them to the release, as a fail-safe if Hugging Face disappears. Hugging Face stays the default
  source, the release asset is the automatic fallback, and a setting lets the user pick either.
- **Update feed:** updates read `update-manifest.json` from the GitHub release tag too, so the site
  leaves the update path. The download source setting covers updates, the plugin catalog and
  components alike.
- Questions 4 to 10 take the proposals below.

## 8. Open questions (as asked)

1. **PDFium in base?** It is about 2.6 MB of a 75 MB installer, and PDF is the most common archival
   format. Recommendation: keep it in the base bundle and still publish it as a component (repairs,
   MSI without it). The brief lists it as optional. Which way?
2. **ffmpeg source and license.** Today it is the gyan.dev GPL "essentials" build, Windows only. For
   macOS and Linux components we need a pinned, redistributable build. qrate only decodes, so LGPL
   builds (BtbN for Windows and Linux) would do. macOS has no obvious GitHub-hosted source. Build it
   in CI, or keep "brew install" there?
3. **CLIP on GitHub.** Re-host the weights under a fixed release (proposed), or keep Hugging Face and
   accept that one component bypasses the download source? Confirm the weights' license allows
   redistribution.
4. **Which formats get a base flavor?** Proposal: NSIS, dmg and tar.gz in both flavors. Portable zip
   and MSI full only, since they serve offline and managed installs.
5. **Per-release re-upload** of PDFium, ffmpeg and Pi (simple, about 90 MB per platform per release),
   or point `components.json` at the first release that carried an unchanged asset?
6. **Trim Pi's docs, examples and assets** (about 4.8 MB raw) from the component? This needs a check
   that Pi does not read them at runtime.
7. **"Not now"**: session only (proposed), or also a persistent "Don't ask again"?
8. **Automatic component updates** after an app update when automatic updates are on (proposed), or always ask?
9. **Mirror 404 fallback to GitHub**: acceptable, or should a set download source be strict (for networks where GitHub is blocked, the fallback only adds a delay)?
10. **macOS signing later**: a hardened-runtime app cannot load an unsigned dylib from Application
    Support without the `disable-library-validation` entitlement. That is not a problem while builds
    are unsigned. Decide before notarization.
11. **Site dependence.** Updates and the plugin catalog still come through `qrate.dvnl.work`. Only
    components go straight to GitHub. Is that split acceptable, or should the update feed also read
    `update-manifest.json` from the release tag?

## 9. Handoff (2026-09-26)

Steps 0 to 4 are on `main` (bf9d9b7, f34267b, fa991f5, 8e3f918). Step 5 is on
`claude/clever-euler-ic4wpg`, waiting for review. The next session picks up here:

1. **Step 5, check by hand.** Nothing here was run in a window: on a build without PDFium and
   ffmpeg beside it, open a PDF and a video in the viewer and the details panel, install both from
   the banners, cancel one midway, open the agent panel without Pi, and use Settings ▸ Components.
   The onboarding crate (branch `onboarding`) calls `components::install`, `state`, `cancel` and
   `observe_global`; it now also sees `Bundled`, `System` and `Unavailable`.
2. **Step 6, base bundle.** Wait until steps 4 and 5 have been through one beta.
3. **Known gap from step 4.** A thumbnail the OS made while PDFium or ffmpeg was missing stays in
   the disk cache after an install until the file or the cache changes.
4. **Then** run `./scripts/ci.sh` and cut a pre-release with the `cut-release` skill (suggested
   `0.6.0-beta.1`; confirm the version with the maintainer). That tag is the first real run of the
   component packaging and the CLIP release copy in `release.yml`. Do not bump `qrate-export`'s
   version for a pre-release.
