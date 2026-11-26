# Web support plan — qrate in the browser

Status: plan only. Target: full parity with the desktop app; projects are opened and saved as
`.qrate` files through the browser's file APIs. Tracking issue: #157 (milestones #158–#167).

## Context

The question was whether qrate can run in a browser tab. **Yes, and the hard part already exists:**
`gpui-pre-platform 0.3.3` (already in `Cargo.lock`) pulls in `gpui-pre-web` whenever
`target_family = "wasm"`. It is a real `Platform` that draws through wgpu (WebGPU, falling back to
WebGL2) and handles keyboard, mouse, IME, clipboard, cursors, touch and `fetch`-based HTTP.
`gpui_platform::application()` already chooses it on wasm, so the call in `crates/app/src/main.rs`
does not change. A wasm/SQLite path is also proven in this repo: `crates/qrate-export/src/wasm.rs`
loads a `.qrate` into in-memory SQLite with `deserialize_read_exact`.

What the web backend can't do (from reading its source, `gpui-pre-web/src/platform.rs`):
- **One top-level window only.** Popup, floating and dialog windows return errors, and a closed
  window can't be reopened.
- `prompt_for_paths` / `prompt_for_new_path` return errors: no native file dialogs.
- `write_credentials` returns an error. `quit` and `restart` do nothing. Menus are not native.
- **The font database starts empty.** Fonts have to be registered before the first window opens.
- Multithreading (the default `multithreaded` feature) needs `SharedArrayBuffer`. That means a
  nightly toolchain with `+atomics` and `-Zbuild-std`, and the page must be served with COOP/COEP
  headers. `single_threaded_web()` exists, but then background work blocks the UI.

So the work is almost all on qrate's side. Every crate assumes a filesystem, OS threads, blocking
HTTP, subprocesses and several windows. Measured in this checkout:
- ~600 `std::fs` call sites, concentrated in table, preview, components, updater, plugin-package,
  file-ingest and project-wizard.
- 6 `std::thread::spawn` calls (settings/db.rs, settings/project.rs, app/instance_handoff.rs,
  agent-runtime, updater).
- `reqwest::blocking` in checks, data-exchange, plugin-host, plugin-package, spellcheck and updater.
  The blocking client doesn't exist on wasm, so these are compile errors, not runtime ones.
- 7 `open_window` sites: app main/about/plugin_marketplace, project-wizard launcher/wizard,
  workspace pop_out.
- Native C/C++ code or processes: mlua with vendored Luau (plugin-host), pdfium plus an ffmpeg
  subprocess (preview), alacritty_terminal plus a PTY (agent-runtime), keyring, single-instance,
  sysinfo, notify.

`table` sits at the centre and depends on preview, plugin-host, visual-search, notify and
file-ingest. So nothing visible runs in a browser until that dependency graph compiles for
`wasm32-unknown-unknown`.

## Design principles

1. **Gate on the target, not on features.** The browser is a compile target, so use
   `[target.'cfg(not(target_family = "wasm"))'.dependencies]` and `#[cfg(target_family = "wasm")]`.
   A `web` cargo feature would be a second axis that can drift.
2. **Change a shared seam once instead of `cfg`-ing every call site.** Threads go to gpui's
   executor, HTTP goes async, and file access goes through one project-root abstraction. Each of
   these then works on both targets, and the native build keeps its current behaviour.
3. **One window becomes "views in one window".** On web, each extra window becomes a gpui-component
   `Dialog` or a workspace view. Native keeps its windows. The decision lives in `window-wrapper`'s
   registry, so callers don't branch.
4. Native CI stays green after every phase. Add `cargo check --target wasm32-unknown-unknown` as a
   CI step from Phase 1 on, so wasm support can't quietly break.

## Phases

### Phase 0 — toolchain spike (de-risk before touching crates)
- New crate `crates/web`: a `cdylib` with a `#[wasm_bindgen(start)]` that calls
  `gpui_platform::web_init()`, opens one gpui-component `Root`, and renders a static table.
- Fonts: embed the UI fonts (include_bytes) and call `cx.text_system().add_fonts(...)` before
  `open_window`. Check whether `gpui-component-assets` already ships them.
- Toolchain: pin a nightly in `crates/web/rust-toolchain.toml`. Use
  `RUSTFLAGS="-C target-feature=+atomics,+bulk-memory"` and `-Zbuild-std=std,panic_abort`, then
  `wasm-bindgen --target web`. Add `scripts/web.sh` (build plus a local server that sends
  `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`).
- Decide: multithreaded (needs nightly and the headers) or single-threaded (stable). The
  recommendation is multithreaded, because diagnostics, imports and visual search run in background
  tasks.

**M0 result (done — #158).** `crates/web` renders a gpui-component view in Chromium, with
background threads and on the WebGL2 fallback. `./scripts/web.sh --serve` builds it and serves it.
What the spike settled:
- **Nightly is required, not optional.** `gpui-pre-platform` depends on `gpui-pre-web` with its
  default `multithreaded` feature, and `wasm_thread` uses `#![feature]`. A stable build doesn't
  compile at all. So the threading decision is multithreaded.
- `+atomics` alone does not produce shared memory. The linker also needs `--shared-memory
  --import-memory --max-memory` and the TLS exports plus `__heap_base`/`__data_end`. Without those,
  wasm-bindgen can't prepare the module for threads (see `scripts/web.sh`).
- Use `Application::run_embedded` and hold the `ApplicationHandle`. On the web, `run` returns
  immediately and the app is dropped ("app was released").
- On wasm, `gpui-kit-assets::Assets` downloads icons from a CDN, and it is a struct with a
  constructor rather than a unit struct. `crates/app/src/assets.rs` must account for both in M1,
  and M9 should vendor the icons.
- Fonts: IBM Plex Sans (OFL), converted from the npm package's WOFF2 to TTF, lives in
  `crates/web/fonts`.

### Phase 1 — make the graph compile for wasm
- **Threads:** replace the `std::thread::spawn` calls with `cx.background_spawn` (or the
  `BackgroundExecutor` passed in). `settings/src/db.rs:184` and `settings/src/project.rs:827` are
  the ones on the web path.
- **HTTP:** move the six `reqwest::blocking` users to async `reqwest`, which compiles to `fetch` on
  wasm, and await it inside background tasks. Representative sites:
  `checks/src/authority/mod.rs:665`, `data-exchange/src/google.rs:80`, `data-exchange/src/sheet.rs:110`,
  `spellcheck/src/catalogue.rs:274`, `plugin-package/src/lib.rs:298`, `plugin-host/src/plugin.rs:994`.
- **Native-only deps** move under `cfg(not(target_family="wasm"))`: keyring, single-instance,
  sysinfo, simplelog, os_info (app); notify (table); alacritty_terminal (agent-runtime); the updater
  `client` feature. Each module that uses them gets a web counterpart in the later phases. Until
  then it compiles to a stub that turns the UI control off.
- **Logging:** on wasm, `crates/app/src/logging.rs` uses `gpui_web::init_logging` (the console).
  Copy Debug Info keeps working from an in-memory ring buffer.
- Gate: `cargo check --target wasm32-unknown-unknown -p table -p workspace -p app`.

**M1 result (done — #159).** `cargo +nightly wasm-check -p app -p web` passes, and CI runs it
as the `wasm` job (`scripts/ci.sh` too, when nightly is installed). The nightly flags live in
`.cargo/config.toml` under `[target.wasm32-unknown-unknown]`, with `wasm-check`/`wasm-build`
aliases. What it settled:
- **HTTP goes through gpui's `HttpClient`, not async reqwest.** Async reqwest needs a tokio
  reactor, which gpui's executors are not, so it would have broken desktop HTTP. The new `fetch`
  crate does one round trip over `cx.http_client()`; `app` installs Zed's `reqwest_client` on the
  desktop, and the browser brings `fetch`. data-exchange, spellcheck, checks and plugin-package
  moved to it, which made their call chains async.
- Two crates keep blocking reqwest on the desktop only: `plugin-host` (Lua's `http.get` cannot
  yield) and `updater` (whose `web.rs` refuses every download, so `components` compiles unchanged).
- Native-only code gets a same-surface browser stand-in rather than `cfg` at every caller:
  `agent-runtime/src/web.rs` (no PTY), `plugin-host/src/web.rs` (no Luau; plugins load as a
  "desktop only" failure), `updater/src/web.rs`. PDFium does not bind in the browser yet (M5).
- The settings writers run on the background executor instead of `std::thread`.
- The workspace `zip` is deflate-only, so no C codecs reach the wasm build.

### Phase 2 — storage: open and save `.qrate` files
- `settings/src/project.rs:275/287` and `settings/src/db.rs:120` open by path. Add a web branch that
  takes bytes: `Connection::open_in_memory` plus `deserialize_read_exact`, as in
  `qrate-export/src/wasm.rs`. Saving does `serialize` and then writes to a
  `FileSystemFileHandle` (`showSaveFilePicker` / `createWritable`). Where the File System Access
  API is missing (Firefox, Safari), saving falls back to a download.
- Autosave keeps the in-memory DB and flushes to the handle with a debounce. The audited `table`
  entry points don't change, because they still commit to SQLite.
- `AppSettings` (user-wide) goes to `localStorage`. `dirs::*` returns `None` on wasm, so route the
  callers in `settings` to a web store.
- Recent projects: keep the `FileSystemFileHandle`s in IndexedDB. On reopen, call
  `requestPermission`.
- File pickers: a small `web_sys` helper in `crates/web` (or in `window-wrapper`) for
  `showOpenFilePicker`, `showDirectoryPicker` and `showSaveFilePicker`. The callers in
  `project-wizard/src/launcher.rs` and `wizard.rs` that use `cx.prompt_for_paths` go through it on
  wasm.

### Phase 3 — single-window shell
- `window-wrapper` registry: on wasm, "open window X" mounts X as a full-window view or a `Dialog`
  inside the one `Root`. Affected: launcher, wizard, about, plugin marketplace, settings, pop-out
  viewer.
- Native menu bar → an in-window menu (gpui-component title-bar menu) on wasm. The keybindings
  stay as they are.
- Quit, restart and the update flow are hidden on wasm. The `beforeunload` prompt turns on while
  there are unsaved changes.

### Phase 4 — files, folders and watching
- Folder-backed projects use `showDirectoryPicker` (Chromium only). Put a `ProjectRoot` seam in
  `file-ingest` (list, read, write and rename against either a `PathBuf` or a
  `FileSystemDirectoryHandle`). `table` (photos, relink, files panel) and `project-wizard` (files
  step) then call `ProjectRoot` instead of `std::fs`. This is the largest mechanical change. Do it
  crate by crate, with native tests as the regression net.
- `table/src/panel/watch.rs` (notify): on web, use `FileSystemObserver` where it exists. Otherwise
  rescan when the window regains focus.
- Linked files outside the project root can't be reached from a browser. Show them as unreachable
  instead of erroring.

### Phase 5 — previews and media (`crates/preview`)
- Images: the `image` crate is pure Rust and works as-is.
- PDF: pdfium-render supports wasm by binding to PDFium's wasm build, loaded from JS. Ship
  `pdfium.wasm` beside the bundle. The "decline cleanly when the binary is missing" behaviour still
  holds.
- Video: the ffmpeg subprocess becomes an `HTMLVideoElement` plus a canvas for thumbnails and
  frames (WebCodecs where available).
- Audio: rodio/cpal with cpal's `wasm-bindgen` (WebAudio) backend. symphonia is pure Rust.
- The thumbnail cache moves to OPFS on wasm.

### Phase 6 — plugins (highest risk; spike first)
- mlua's vendored Luau is C++ and does not build for `wasm32-unknown-unknown`.
- Spike A: compile Luau with clang and wasi-libc into the same module.
- Spike B: Luau as a separate Emscripten module in a Worker, with plugin-host's host calls bridged
  through messages. B is more likely to work, but it turns sync host calls into async ones and could
  touch the plugin API.
- If the API visibly changes, the three-repo rule in CLAUDE.md applies: update the template's
  `types/qrate.lua`, the islandora copy, `API_VERSION` and the registry.
- Plugin install (`plugin-package`): store packages in OPFS. Signature checks don't change
  (ed25519 is pure Rust).

### Phase 7 — network features and CORS
- Authority checks, the spellcheck dictionary download, the plugin catalog and visual-search
  weights all do cross-origin fetches. Test each endpoint's CORS. Any endpoint that refuses needs a
  small proxy, for example next to `qrate-site`.
- Visual search: candle builds to wasm (enable SIMD128). Weights are cached in OPFS or Cache
  Storage, and inference runs on a background task. It will be slower than native.
- Google Sheets: switch from the desktop loopback OAuth to a browser redirect/popup flow on the same
  client, with a web OAuth client ID.

### Phase 8 — what cannot run inside a browser tab (parity needs a substitute, not a port)
- **Agent panel / `agent-runtime`:** a PTY and the bundled Pi executable can't run in a tab. A
  substitute would be a remote agent over WebSocket. Until then the panel is hidden on wasm.
- **`qrate agent` CLI / `app_control`:** these control a local process over IPC. There is nothing to
  connect to in a tab. The panel is hidden, so `AGENTS.md` only gains a "desktop only" note.
- **Updater, single-instance handoff, OS keyring:** these don't apply. A redeploy is the update.
  Credentials fall back to session-only storage, and that limitation is shown to the user.

### Build, hosting, CI
- `scripts/web.sh` builds into `dist/` (`index.html`, the wasm, the JS glue, `pdfium.wasm`, fonts).
  Host it on the `qrate-site` branch, which needs the COOP/COEP headers.
- CI gets a `wasm32` check job (`.github/workflows/ci.yml`), plus `scripts/ci.sh`.
- Docs: add `docs/web.md`. Follow CLAUDE.md process for the work itself: one Notion task per phase,
  each mirrored to a GitHub issue.

## Verification
- From Phase 1 on: `cargo check --target wasm32-unknown-unknown -p <crate>` on the touched crates,
  and `./scripts/ci.sh` before each PR so native doesn't regress.
- End-to-end: `scripts/web.sh` serves the build, then the pre-installed Playwright Chromium loads it
  (with `--enable-unsafe-webgpu`, or `WebBackendPreference` forced to WebGL2) and checks:
  - the window renders (screenshot);
  - opening a fixture `.qrate`, editing a cell and saving produces bytes that the native
    `settings::project` opens with the edit and its History entry;
  - an image, a PDF and an audio preview each render.
- Each phase's definition of done includes the matching Playwright check.
