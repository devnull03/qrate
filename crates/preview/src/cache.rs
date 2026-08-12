//! The on-disk half of the preview cache: downscaled copies of files that are expensive to decode.
//!
//! Decoding a 40-megapixel scan to show a 512px card is the cost worth avoiding, and it is the
//! same cost on every launch — so the result is written next to the user's other caches rather
//! than rebuilt each session. Tropy does the same thing with WebP variants.
//!
//! Entries are keyed by content identity (path, size, mtime) as well as by the requested edge, so
//! a re-scanned file simply misses and a stale entry is never served. Nothing invalidates or
//! prunes: a miss is cheap and orphans are inert, so the only cleanup is the user asking for it.

use std::fs;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};

use image::RgbaImage;

/// Where the downscaled copies live — the platform's own cache location, so an OS cleanup tool
/// treats them as what they are. `None` if it can't be created, which degrades to decoding from
/// source every time rather than failing the preview.
pub fn dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("qrate").join("thumbnails");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Identity of one cached rendering. The file's length and mtime are in the hash, so editing or
/// replacing a source file misses rather than serving the old picture — which is why nothing here
/// needs an invalidation pass.
///
/// `DefaultHasher` rather than a cryptographic digest: this names a cache entry, it does not
/// authenticate one. A collision costs a wrong thumbnail, not a security property.
pub fn key(path: &Path, max_edge: u32, page: usize) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok()?.hash(&mut hasher);
    max_edge.hash(&mut hasher);
    page.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

/// The cached rendering for `key`, if one was written. Decoded by magic bytes, so the entry needs
/// no extension and [`write`] is free to choose its encoding per image.
pub fn read(key: &str) -> Option<RgbaImage> {
    let bytes = fs::read(dir()?.join(key)).ok()?;
    Some(image::load_from_memory(&bytes).ok()?.to_rgba8())
}

/// Store `image` as `key`. Failure is logged and ignored — an unwritable cache directory should
/// cost speed, never a preview.
///
/// JPEG unless the image actually uses its alpha channel, which is the difference between roughly
/// 40 KB and 400 KB per entry; across a ten-thousand-row collection that is the difference between
/// a cache the user never notices and one they do.
pub fn write(key: &str, image: &RgbaImage) {
    let Some(dir) = dir() else {
        return;
    };
    let transparent = image.pixels().any(|px| px.0[3] < u8::MAX);
    let mut bytes = Vec::new();
    let encoded = if transparent {
        image.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
    } else {
        image::DynamicImage::ImageRgba8(image.clone())
            .to_rgb8()
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
    };
    let written = encoded
        .map_err(|err| err.to_string())
        .and_then(|()| fs::write(dir.join(key), &bytes).map_err(|err| err.to_string()));
    if let Err(err) = written {
        log::warn!("could not cache a preview thumbnail, previews will be slower: {err}");
    }
}

/// Delete every cached rendering. Returns how many entries went, for the message the caller shows.
pub fn clear() -> std::io::Result<usize> {
    let Some(dir) = dir() else {
        return Ok(0);
    };
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::RgbaImage;

    use crate::cache;

    fn sample(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample/photos")
            .join(name)
            .canonicalize()
            .expect("sample/photos present in repo")
    }

    /// The key has to change when the source file changes, or a re-scanned page keeps showing the
    /// old picture forever — the failure this scheme exists to prevent.
    #[test]
    fn the_key_tracks_the_file_and_the_requested_size() {
        let path = sample("1.jpg");
        let base = cache::key(&path, 512, 0).expect("sample file is readable");
        assert_eq!(
            base,
            cache::key(&path, 512, 0).unwrap(),
            "stable across calls"
        );
        assert_ne!(
            base,
            cache::key(&path, 1024, 0).unwrap(),
            "size is in the key"
        );
        // Without this, every page of a PDF would overwrite the same entry.
        assert_ne!(
            base,
            cache::key(&path, 512, 1).unwrap(),
            "page is in the key"
        );
        assert!(cache::key(std::path::Path::new("/nonexistent/x.jpg"), 512, 0).is_none());

        // Same bytes at a different path is a different entry: the path is part of the identity.
        let copy = std::env::temp_dir().join("qrate-cache-key-probe.jpg");
        std::fs::copy(&path, &copy).unwrap();
        assert_ne!(base, cache::key(&copy, 512, 0).unwrap());
        let _ = std::fs::remove_file(&copy);
    }

    /// A round trip has to preserve the pixels, including through the JPEG branch — an entry that
    /// decodes to the wrong colours would be worse than a cache miss.
    #[test]
    fn round_trips_opaque_and_transparent_images() {
        // Flat colours, so JPEG's lossy encoding still compares equal within a wide tolerance.
        let opaque = RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 60, 255]));
        cache::write("qrate-test-opaque", &opaque);
        let back = cache::read("qrate-test-opaque").expect("just written");
        assert_eq!(back.dimensions(), (8, 8));
        let px = back.get_pixel(4, 4).0;
        assert!(px[0].abs_diff(200) < 12 && px[1].abs_diff(40) < 12 && px[2].abs_diff(60) < 12);
        assert_eq!(px[3], 255);

        // Alpha must survive, which is the whole reason for the PNG branch.
        let mut transparent = RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        transparent.put_pixel(0, 0, image::Rgba([10, 20, 30, 0]));
        cache::write("qrate-test-alpha", &transparent);
        let back = cache::read("qrate-test-alpha").expect("just written");
        assert_eq!(back.get_pixel(0, 0).0[3], 0, "alpha preserved");
        assert_eq!(back.get_pixel(1, 1).0, [10, 20, 30, 255]);

        assert!(cache::read("qrate-test-never-written").is_none());
    }
}
