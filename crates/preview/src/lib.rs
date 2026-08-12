//! Turning a row's linked file into something the UI can draw.
//!
//! Every view that shows a file — the Details panel's frame, the gallery's cards, the fullscreen
//! viewer — asks the same two questions: *can this be rendered?* and *what do we draw when it
//! can't?* Those answers used to live in `table::photos` next to the filename resolver, which put
//! a rendering concern inside the spreadsheet crate. They live here instead, so the decoders that
//! follow (PDF, video, camera RAW — each with a native dependency of its own) never reach `table`.
//!
//! Path resolution stays in [`table::photos`]: that is about matching a cell to a file on disk,
//! which is a different job from decoding whatever the match turned out to be.

mod audio;
pub mod cache;
mod embedded;
mod media;
mod native;
mod pdf;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Asset, Global, ImageCacheError, ImageSource, IntoElement as _, ObjectFit,
    ParentElement as _, RenderImage, Styled as _, StyledImage as _, Window, div, img,
};
use gpui_component::{ActiveTheme, Icon, IconName};
use image::Frame;

/// Longest edge, in pixels, of a gallery card's thumbnail. Cards are 168px wide, so this still has
/// headroom on a 2× display while costing a sixteenth of a full scan's pixels.
pub const CARD: u32 = 512;

/// Longest edge for the Details panel's preview, whose pane the user can drag to 600px tall.
pub const PANE: u32 = 1024;

/// No size cap: whatever the file's own resolution is. What the fullscreen viewer asks for, since
/// it zooms to 8× and a downscaled copy would go soft exactly when someone is looking closely.
///
/// For a format gpui decodes itself this hands over the original file untouched — see [`source`].
/// For the rest it is an uncapped run down the ladder, never written to the disk cache (it would
/// be a second copy of the original), so it lives only in the memory budget while it is on screen.
pub const FULL: u32 = 0;

/// What a tier that must be told a size renders at when asked for [`FULL`]. A PDF page has no
/// natural pixel size, so "no cap" still needs a number; this is generous enough to zoom into.
const FULL_FALLBACK: u32 = 2048;

/// How much decoded image data may stay resident. gpui's asset cache never evicts on its own, so
/// without a ceiling a scroll through a large collection retains every thumbnail it passes.
///
/// ponytail: one global budget, FIFO, and a linear scan of the held entries per drawn card. It
/// only has to exceed what is on screen at once — a gallery shows a few dozen cards — so eviction
/// never touches a visible card and true LRU buys nothing. The scan is a few thousand path
/// comparisons a frame at this budget; index the deque by key if the budget is ever raised much.
const BUDGET: usize = 192 * 1024 * 1024;

/// Formats the `image` crate decodes for us directly — gpui's own list minus AVIF, which
/// `Img::extensions()` advertises but Zed compiles out (its decoder would drag in a C dav1d
/// build). AVIF is handled a tier further down instead.
fn is_raster(extension: &str) -> bool {
    matches!(
        extension,
        "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "webp"
            | "bmp"
            | "ico"
            | "tif"
            | "tiff"
            | "tga"
            | "dds"
            | "hdr"
            | "exr"
            | "pbm"
            | "pam"
            | "ppm"
            | "pgm"
            | "ff"
            | "farbfeld"
            | "qoi"
    )
}

/// Whether we can draw this file's contents rather than an icon standing in for it.
///
/// Extension-only and cheap on purpose: this runs for every visible row, and it also decides
/// whether the fullscreen button and the gallery's double-click do anything, so it must answer
/// without touching the disk. It is a claim about the *format*, not a promise about the file —
/// a truncated JPEG still passes here and falls back to the icon when the decode fails.
pub fn can_preview(path: &Path) -> bool {
    let Some(extension) = extension(path) else {
        return false;
    };
    let extension = extension.as_str();
    is_raster(extension)
        || extension == "svg"
        || pdf::handles(extension)
        || embedded::handles(extension)
        || media::handles(extension)
        || audio::handles(extension)
        || native::handles(extension)
}

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

/// Placeholder icon for a file we can't render inline. `gpui_component`'s bundled icon set has
/// no media glyphs (no camera/film/music), so these are the nearest stand-ins available —
/// swap in custom SVGs via `Icon::path` if the set ever grows.
///
/// ponytail: four buckets, extension-keyed. Add a real mime crate only if the icon actually
/// needs to be right for files with no/wrong extension.
pub fn placeholder_icon(path: Option<&Path>) -> IconName {
    match path.and_then(extension).as_deref() {
        Some("pdf" | "epub" | "doc" | "docx" | "txt" | "md") => IconName::BookOpen,
        Some("mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "aiff") => IconName::Play,
        Some("mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v") => IconName::Frame,
        _ => IconName::File,
    }
}

/// A file decoded and scaled down to `max_edge`, ready to draw.
///
/// Keyed by `(path, max_edge)` and loaded through gpui's own asset system, which already gives us
/// the three things this needs: the work happens off the main thread, two cards asking for the
/// same file share one decode, and the view is re-rendered when it lands. What it does not give us
/// is eviction — see [`retain`].
pub struct Preview;

impl Asset for Preview {
    /// File, size cap, and which page — the last only ever non-zero for a multi-page document.
    type Source = (PathBuf, u32, usize);
    type Output = Option<Arc<RenderImage>>;

    fn load(
        (path, max_edge, page): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let executor = cx.background_executor().clone();
        async move {
            executor
                .spawn(async move { render(&path, max_edge, page) })
                .await
        }
    }
}

/// How many pages this file has. One for everything that is not a document, so a caller can show
/// page controls whenever this is greater than one without knowing which tier answered.
pub fn page_count(path: &Path) -> usize {
    match extension(path) {
        Some(extension) if pdf::handles(&extension) => pdf::page_count(path).unwrap_or(1),
        _ => 1,
    }
}

/// Decode `path`, shrink it to fit `max_edge`, and hand back something gpui can draw. Runs on a
/// background thread; `None` for anything that won't decode, which the caller turns into the icon.
fn render(path: &Path, max_edge: u32, page: usize) -> Option<Arc<RenderImage>> {
    // A full-size rendering is not cached: it would be a second copy of the original file on disk,
    // and it is wanted once, while someone is looking at it.
    let key = (max_edge != FULL)
        .then(|| cache::key(path, max_edge, page))
        .flatten();
    let scaled = match key.as_deref().and_then(cache::read) {
        Some(cached) => cached,
        None => {
            let scaled = downscale(decode(path, max_edge, page)?, max_edge);
            if let Some(key) = &key {
                cache::write(key, &scaled);
            }
            scaled
        }
    };

    // `RenderImage` is documented as BGRA and gpui only swaps inside its own decode path, so an
    // image built by hand has to arrive already swapped or every preview draws blue-for-red.
    let mut bgra = scaled;
    for px in bgra.pixels_mut() {
        px.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(bgra)])))
}

/// The ladder: each tier is asked in turn and the first one that produces pixels wins.
///
/// Order is by confidence, not by cost. A tier that understands the format specifically comes
/// before one that guesses, and the OS is asked last because what it can answer depends on which
/// applications happen to be installed — two archivists must not see different previews of the
/// same file while any deterministic tier could have handled it.
///
/// Falling *through* matters as much as the order: a `.heic` with no embedded preview drops to
/// ffmpeg, and a `.psd` nothing else claims still reaches the OS.
fn decode(path: &Path, max_edge: u32, page: usize) -> Option<image::DynamicImage> {
    let extension = extension(path).unwrap_or_default();
    // Tiers that rasterise rather than decode have to be given a number even when the caller
    // asked for no cap; the ones that decode an existing image ignore this entirely.
    let target = if max_edge == FULL {
        FULL_FALLBACK
    } else {
        max_edge
    };

    if is_raster(&extension)
        && let Some(image) = raster(path)
    {
        return Some(image);
    }
    if pdf::handles(&extension)
        && let Some(image) = pdf::decode(path, target, page)
    {
        return Some(image);
    }
    if embedded::handles(&extension)
        && let Some(image) = embedded::decode(path, &extension)
    {
        return Some(image);
    }
    if media::handles(&extension)
        && let Some(image) = media::decode(path, target)
    {
        return Some(image);
    }
    if audio::handles(&extension)
        && let Some(image) = audio::cover(path)
    {
        return Some(image);
    }
    native::thumbnail(path, target)
}

/// Formats `image` handles itself. Decoded by content rather than by extension — matching gpui,
/// and matching what [`can_preview`] promises: a `.jpg` that is really a PNG still opens.
fn raster(path: &Path) -> Option<image::DynamicImage> {
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .map_err(|err| log::warn!("could not decode {}: {err}", path.display()))
        .ok()
}

/// Shrink to fit `max_edge`, preserving aspect. Never enlarges — a 200px source stays 200px rather
/// than being blown up to a soft 512.
///
/// `Triangle` rather than `Lanczos3`: on a 40-megapixel scan the sharper filter costs noticeably
/// more, and the result is written to disk once and reused, so the difference lands on the user's
/// first look at every file rather than being amortised.
fn downscale(image: image::DynamicImage, max_edge: u32) -> image::RgbaImage {
    let (w, h) = (image.width(), image.height());
    if max_edge == FULL || w.max(h) <= max_edge {
        return image.to_rgba8();
    }
    image
        .resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
        .to_rgba8()
}

/// Everything currently decoded, oldest first, and what it costs.
///
/// Holding the `Arc` here is deliberate: releasing an image needs the handle itself, not just its
/// cache slot, because the GPU-side texture is freed by `App::drop_image` rather than by the last
/// `Arc` going away.
#[derive(Default)]
struct Live {
    entries: VecDeque<((PathBuf, u32, usize), Arc<RenderImage>)>,
    bytes: usize,
}

impl Global for Live {}

/// Account for a freshly-loaded image and release older ones until the total is back under
/// `budget`. Called on every frame a preview is drawn, so re-registering an image already tracked
/// has to be free.
///
/// `budget` is a parameter rather than reading [`BUDGET`] directly so a test can drive eviction
/// with a handful of small images instead of allocating its way past the real ceiling.
fn retain(
    source: &(PathBuf, u32, usize),
    image: &Arc<RenderImage>,
    budget: usize,
    window: &mut Window,
    cx: &mut App,
) {
    // ponytail: frame 0 only, so an animated GIF is undercounted. Costs accuracy on a format the
    // budget already tolerates; revisit if animations become common in collections.
    let cost = image.as_bytes(0).map_or(0, <[u8]>::len);
    let live = cx.default_global::<Live>();
    if live.entries.iter().any(|(key, _)| key == source) {
        return;
    }
    live.entries.push_back((source.clone(), image.clone()));
    live.bytes += cost;

    // Never evict the entry just added, or a single oversized image would thrash forever.
    while cx.global::<Live>().bytes > budget && cx.global::<Live>().entries.len() > 1 {
        let live = cx.global_mut::<Live>();
        let Some((key, image)) = live.entries.pop_front() else {
            break;
        };
        live.bytes = live
            .bytes
            .saturating_sub(image.as_bytes(0).map_or(0, <[u8]>::len));
        cx.remove_asset::<Preview>(&key);
        cx.drop_image(image, Some(window));
    }
}

/// The file's contents fit to whatever box the caller gives it, or a type icon when there is
/// nothing to draw — no path, an undecodable one, or a decode that fails at paint time.
///
/// Callers supply the surrounding chrome (the Details panel's bordered frame and its action
/// buttons, a gallery card's caption and selection ring); this is only the picture. It centres
/// itself, so it works both as the sole child of a plain frame and inside a parent that already
/// centres.
///
/// Returns `AnyElement` (erased), not `impl IntoElement` — this is shared across several `Render`
/// impls, and gpui's chained builder calls produce deeply nested generic types; propagating that
/// concrete type into every caller overflows rustc's stack during type-checking instead of just
/// hitting a slow compile.
pub fn thumb(path: Option<&Path>, max_edge: u32, cx: &App) -> AnyElement {
    // Also `img()`'s decode-failure fallback, so a file that passes `can_preview` but turns out to
    // be truncated still lands on the icon. Captures by value because the `'static` fallback
    // closure can't borrow `cx` or the path.
    let placeholder = {
        let color = cx.theme().muted_foreground;
        let icon = placeholder_icon(path);
        move || {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(color)
                // `IconName` isn't `Copy`, and `with_fallback` wants `Fn` (it may re-render) —
                // so clone per call rather than move the captured icon out.
                .child(Icon::new(icon.clone()).size_6())
                .into_any_element()
        }
    };

    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .map(|frame| match path.filter(|path| can_preview(path)) {
            // gpui's `img` stamps the element with the *image's* aspect ratio, so a `size_full`
            // img ignores the frame shape and `object_fit` has nothing to letterbox. Size it by
            // its intrinsic ratio under `max_w/h_full` (where that aspect logic applies) so it
            // shrinks to fit — bars and all.
            Some(path) => frame.child(
                // Page one: a card or a details pane shows what the file *is*, and paging through
                // a document is the viewer's job.
                img(source(path, max_edge, 0))
                    .max_w_full()
                    .max_h_full()
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(placeholder),
            ),
            None => frame.child(placeholder()),
        })
        .into_any_element()
}

/// Where the pixels come from for one previewable file.
///
/// gpui's own loader gets the file whenever it can read it and nothing has to be shrunk, so what
/// the fullscreen viewer draws is the original: animation intact, no round trip through our decode
/// and BGRA swap, no second interpretation of a file gpui already understands.
///
/// - **SVG at every size.** It is the one format [`can_preview`] accepts that the `image` crate
///   cannot decode — gpui rasterises it through the `resvg` it already vendors — and vector files
///   are small enough that neither the downscale nor the disk cache would earn its keep.
/// - **Raster at [`FULL`] only**, i.e. the viewer. A card or a details pane wants the capped,
///   cached copy; there is nothing to cap here.
///
/// Everything else goes through [`Preview`] as a custom source — the formats gpui cannot read at
/// all (PDF, RAW, video, audio artwork, whatever only the OS can thumbnail), and every capped
/// size. That has to be `Custom` rather than a plain path: `Custom` is the only variant whose
/// loader runs with a `Window`, which is what both [`Window::use_asset`] and the eviction in
/// [`retain`] need.
///
/// ponytail: the handed-over file lands in gpui's asset cache, which [`retain`] cannot evict —
/// so a session spent opening one large scan after another keeps every one of them. Acceptable
/// while the viewer shows one at a time; give the viewer an explicit `drop_image` on close if it
/// ever shows up in a memory profile.
pub fn source(path: &Path, max_edge: u32, page: usize) -> ImageSource {
    let extension = extension(path).unwrap_or_default();
    if extension == "svg" || (max_edge == FULL && page == 0 && is_raster(&extension)) {
        return ImageSource::Resource(path.to_path_buf().into());
    }
    let key = (path.to_path_buf(), max_edge, page);
    ImageSource::Custom(Arc::new(move |window: &mut Window, cx: &mut App| {
        // `None` while the decode is still running, which leaves the frame empty rather than
        // flashing the icon; gpui re-renders the view when the task lands.
        let loaded = window.use_asset::<Preview>(&key, cx)?;
        let Some(image) = loaded else {
            // Decoded to nothing. Reported as an error so `with_fallback` shows the type icon,
            // which is the honest answer for a file we cannot draw.
            return Some(Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                "no preview available for {}",
                key.0.display()
            )))));
        };
        retain(&key, &image, BUDGET, window, cx);
        Some(Ok(image))
    }))
}

#[cfg(test)]
mod tests {
    // No `use super::*`: chain-globbing `gpui::*` shadows the built-in `#[test]` and recurses (see CLAUDE.md).
    use std::path::{Path, PathBuf};

    use gpui::{Context, IntoElement, Render, TestAppContext, Window};
    use gpui_component::{IconName, IconNamed as _};

    use crate::{can_preview, placeholder_icon, thumb};

    /// Which loader a file is routed to, and it has to be gpui's own for an image the viewer is
    /// showing — that is what makes the fullscreen view the *original* file rather than our
    /// re-decoded copy, animation and all. Nothing else observes the difference, so without this
    /// the rule could be inverted and every other test would still pass.
    #[test]
    fn the_viewer_gets_the_original_file_and_everything_else_gets_the_ladder() {
        use gpui::ImageSource;

        use crate::{CARD, FULL, source};

        let native = |p: &str, max_edge, page| {
            matches!(
                source(Path::new(p), max_edge, page),
                ImageSource::Resource(_)
            )
        };

        assert!(native("/f/scan.jpg", FULL, 0), "the viewer's own case");
        assert!(
            native("/f/anim.gif", FULL, 0),
            "a single frame would kill it"
        );
        // SVG has no other loader: the `image` crate cannot decode it at any size.
        assert!(native("/f/logo.svg", CARD, 0));
        assert!(native("/f/logo.svg", FULL, 0));

        // Capped sizes stay on the ladder — a card wants the shrunk, disk-cached copy.
        assert!(!native("/f/scan.jpg", CARD, 0));
        // Formats gpui cannot read at all, at any size.
        assert!(!native("/f/scan.pdf", FULL, 0));
        assert!(!native("/f/shot.cr2", FULL, 0));
        assert!(!native("/f/photo.avif", FULL, 0), "gpui compiles AVIF out");
        // A page past the first only exists for formats the ladder renders anyway; handing over
        // the whole file would silently show page one.
        assert!(!native("/f/scan.tif", FULL, 3));
    }

    #[test]
    fn placeholder_icon_varies_by_file_type() {
        // `IconName` is macro-generated and implements neither `PartialEq` nor `Debug`, so
        // compare the embedded asset paths it resolves to instead.
        let icon = |p: &str| placeholder_icon(Some(Path::new(p))).path();
        assert_eq!(icon("/f/scan.PDF"), IconName::BookOpen.path());
        assert_eq!(icon("/f/take.mp3"), IconName::Play.path());
        assert_eq!(icon("/f/clip.mov"), IconName::Frame.path());
        assert_eq!(icon("/f/archive.zip"), IconName::File.path());
        assert_eq!(icon("/f/no-extension"), IconName::File.path());
        assert_eq!(placeholder_icon(None).path(), IconName::File.path());
        // The buckets must actually differ — a regression that collapsed them all to `File`
        // would otherwise still pass every assertion above.
        assert_ne!(icon("/f/scan.pdf"), icon("/f/take.mp3"));
    }

    #[test]
    fn only_decodable_images_get_an_inline_preview() {
        assert!(can_preview(Path::new("/f/a.JPG")));
        assert!(can_preview(Path::new("/f/a.png")));
        // Formats gpui decodes that the old `table::photos` allowlist rejected.
        assert!(can_preview(Path::new("/f/a.qoi")));
        assert!(can_preview(Path::new("/f/a.exr")));
        // Each of these is claimed by a tier further down the ladder rather than by `image`.
        assert!(can_preview(Path::new("/f/a.pdf")), "pdfium");
        assert!(
            can_preview(Path::new("/f/a.cr2")),
            "embedded camera preview"
        );
        assert!(can_preview(Path::new("/f/a.mp4")), "ffmpeg");
        assert!(can_preview(Path::new("/f/a.mp3")), "tagged cover art");
        // AVIF reaches ffmpeg rather than `image`, whose decoder Zed compiles out — which is what
        // lets us claim the format without switching on a C dav1d build for the whole workspace.
        assert!(can_preview(Path::new("/f/a.avif")));

        assert!(!can_preview(Path::new("/f/a")), "no extension, no claim");
        assert!(
            !can_preview(Path::new("/f/a.sqlite")),
            "nothing renders a database"
        );
    }

    /// Wraps [`thumb`] in a root `Render` view so a test can actually draw it — `Img`'s real
    /// load/fallback logic runs during layout/paint, not at element construction, so building the
    /// element tree alone (without a window draw) wouldn't exercise it.
    struct ThumbProbe(Option<PathBuf>);

    impl Render for ThumbProbe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            thumb(self.0.as_deref(), crate::CARD, cx)
        }
    }

    fn sample_photo(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample/photos")
            .join(name)
            .canonicalize()
            .expect("sample/photos present in repo")
    }

    #[gpui::test]
    fn renders_a_resolved_image_without_panicking(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = sample_photo("1.jpg");
        cx.add_window_view(|_, _| ThumbProbe(Some(path)));
    }

    #[gpui::test]
    fn falls_back_when_the_resolved_path_is_missing_on_disk(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = PathBuf::from("/nonexistent/qrate-test-image.jpg");
        cx.add_window_view(|_, _| ThumbProbe(Some(path)));
    }

    #[gpui::test]
    fn renders_the_icon_when_there_is_no_path(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|_, _| ThumbProbe(None));
    }

    /// The one that would ship silently. `RenderImage` is BGRA, but every decoder we will add
    /// produces RGBA — get the swap wrong and every preview draws with red and blue exchanged,
    /// which looks plausible enough on a sepia scan to survive a manual check.
    #[test]
    fn decoded_pixels_reach_gpui_as_bgra() {
        let red = std::env::temp_dir().join("qrate-bgra-probe.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .save(&red)
            .unwrap();

        let rendered = crate::render(&red, crate::CARD, 0).expect("a 4x4 png decodes");
        let bytes = rendered.as_bytes(0).expect("one frame");
        assert_eq!(
            &bytes[..4],
            &[0, 0, 255, 255],
            "pure red must arrive as B=0 G=0 R=255, not R=255 G=0 B=0"
        );

        let _ = std::fs::remove_file(&red);
    }

    /// Downscaling is the whole point: an oversized source must not reach the GPU at full size,
    /// and an undersized one must not be blown up.
    #[test]
    fn oversized_images_shrink_and_small_ones_are_left_alone() {
        let big = std::env::temp_dir().join("qrate-downscale-probe.png");
        image::RgbaImage::from_pixel(900, 300, image::Rgba([1, 2, 3, 255]))
            .save(&big)
            .unwrap();
        let rendered = crate::render(&big, 256, 0).expect("decodes");
        let size = rendered.size(0);
        assert_eq!(i32::from(size.width), 256, "longest edge is capped");
        assert_eq!(i32::from(size.height), 85, "aspect ratio preserved");

        let small = std::env::temp_dir().join("qrate-noupscale-probe.png");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([1, 2, 3, 255]))
            .save(&small)
            .unwrap();
        let rendered = crate::render(&small, 512, 0).expect("decodes");
        assert_eq!(i32::from(rendered.size(0).width), 40, "never enlarged");

        let _ = std::fs::remove_file(&big);
        let _ = std::fs::remove_file(&small);
    }

    /// A second look at the same file must come off disk rather than decoding again — the reason
    /// the cache exists. Asserted through the artefact, since the decode itself is not observable.
    #[test]
    fn the_first_decode_writes_an_entry_the_next_one_can_read() {
        let path = std::env::temp_dir().join("qrate-cache-hit-probe.png");
        image::RgbaImage::from_pixel(600, 600, image::Rgba([9, 9, 9, 255]))
            .save(&path)
            .unwrap();
        let key = crate::cache::key(&path, 128, 0).expect("readable");
        let entry = crate::cache::dir().expect("cache dir").join(&key);
        let _ = std::fs::remove_file(&entry);

        crate::render(&path, 128, 0).expect("decodes");
        assert!(entry.is_file(), "the first decode leaves an entry behind");
        assert!(crate::cache::read(&key).is_some(), "and it reads back");

        // Touching the source has to miss, or an edited file shows its old thumbnail forever.
        std::thread::sleep(std::time::Duration::from_millis(10));
        image::RgbaImage::from_pixel(600, 600, image::Rgba([7, 7, 7, 255]))
            .save(&path)
            .unwrap();
        assert_ne!(
            crate::cache::key(&path, 128, 0).expect("readable"),
            key,
            "a rewritten source is a different entry"
        );

        let _ = std::fs::remove_file(&entry);
        let _ = std::fs::remove_file(&path);
    }

    /// The reason this phase exists. gpui's asset cache never evicts, so without this the gallery
    /// keeps every thumbnail it scrolls past — the memory climbs for as long as the user browses.
    /// Driven with a tiny budget rather than the real 192 MB so it stays a unit test.
    #[gpui::test]
    fn scrolling_past_more_images_than_fit_releases_the_oldest(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_probe, cx) = cx.add_window_view(|_, _| ThumbProbe(None));

        // 32×32 RGBA = 4096 bytes each; a budget of three lets the fourth push the first out.
        let image = || {
            std::sync::Arc::new(gpui::RenderImage::new([image::Frame::new(
                image::RgbaImage::new(32, 32),
            )]))
        };
        let budget = 4096 * 3;

        cx.update(|window, cx| {
            for n in 0..8 {
                let key = (PathBuf::from(format!("/f/{n}.png")), crate::CARD, 0);
                crate::retain(&key, &image(), budget, window, cx);
            }
            let live = cx.global::<crate::Live>();
            assert!(
                live.bytes <= budget,
                "held {} bytes against a {budget} budget",
                live.bytes
            );
            assert_eq!(live.entries.len(), 3, "older images were released");
            assert_eq!(
                live.entries.front().map(|(key, _)| key.0.clone()),
                Some(PathBuf::from("/f/5.png")),
                "the survivors are the most recent, oldest first"
            );

            // Re-registering something already held must not double-count it: this runs on every
            // frame a card is drawn, so a leak here would evict the whole cache within seconds.
            let held = live.entries.back().expect("non-empty").clone();
            let before = cx.global::<crate::Live>().bytes;
            crate::retain(&held.0, &held.1, budget, window, cx);
            assert_eq!(cx.global::<crate::Live>().bytes, before);
            assert_eq!(cx.global::<crate::Live>().entries.len(), 3);
        });
    }

    #[test]
    fn undecodable_files_render_nothing_rather_than_panicking() {
        let junk = std::env::temp_dir().join("qrate-junk-probe.jpg");
        std::fs::write(&junk, b"this is not an image").unwrap();
        assert!(crate::render(&junk, crate::CARD, 0).is_none());
        assert!(crate::render(Path::new("/nonexistent/x.png"), crate::CARD, 0).is_none());
        let _ = std::fs::remove_file(&junk);
    }
}
