//! A note card's picture of its region: that part of the page, cut from the capped copy a card or
//! the Details pane already decodes, and held only in memory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, Asset, ImageCacheError, ImageSource, RenderImage, Window};
use image::Frame;

/// The longest edge a crop is cut to: twice the largest card that draws one.
const EDGE: u32 = 112;
/// How much of the page's shorter side a pin's crop shows around it.
const AROUND_PIN: f32 = 0.15;
const SCALE: f32 = 10_000.;

/// File, page, the area in ten-thousandths of the upright page, and the decoder generation.
type Key = (PathBuf, usize, [u16; 4], u64);

pub struct Crop;

impl Asset for Crop {
    type Source = Key;
    type Output = Option<Arc<RenderImage>>;

    fn load(
        (path, page, area, _): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let executor = cx.background_executor().clone();
        async move { executor.spawn(async move { cut(&path, page, area) }).await }
    }
}

fn cut(path: &Path, page: usize, [x, y, w, h]: [u16; 4]) -> Option<Arc<RenderImage>> {
    let pixels = crate::thumbnail_pixels(path, crate::PANE, page)?;
    let (width, height) = (pixels.width() as f32, pixels.height() as f32);
    let (left, top) = (f32::from(x) / SCALE * width, f32::from(y) / SCALE * height);
    let (mut l, mut t, mut r, mut b) = (
        left,
        top,
        left + f32::from(w) / SCALE * width,
        top + f32::from(h) / SCALE * height,
    );
    if w == 0 && h == 0 {
        let half = width.min(height) * AROUND_PIN / 2.;
        (l, t, r, b) = (l - half, t - half, r + half, b + half);
    }
    let (l, t) = (l.clamp(0., width - 1.), t.clamp(0., height - 1.));
    let (r, b) = (r.clamp(l + 1., width), b.clamp(t + 1., height));
    let cut = image::imageops::crop_imm(
        &pixels,
        l as u32,
        t as u32,
        (r - l).max(1.) as u32,
        (b - t).max(1.) as u32,
    )
    .to_image();
    let mut bgra = crate::downscale(cut.into(), EDGE);
    for px in bgra.pixels_mut() {
        px.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(bgra)])))
}

/// What a card draws for the part of `path`'s `page` that `area` covers — x, y, width and height
/// in ten-thousandths of the upright page, zero-sized for a pin. Fails, so `with_fallback` takes
/// over, when the file can't be read and was not cut earlier this session.
pub fn crop(path: &Path, page: usize, area: [u16; 4]) -> ImageSource {
    let generation = crate::extension(path).map_or(0, |extension| crate::generation(&extension));
    let key = (path.to_path_buf(), page, area, generation);
    ImageSource::Custom(Arc::new(move |window: &mut Window, cx: &mut App| {
        let loaded = window.use_asset::<Crop>(&key, cx)?;
        Some(loaded.ok_or_else(|| {
            ImageCacheError::Other(Arc::new(anyhow::anyhow!("no crop of {}", key.0.display())))
        }))
    }))
}

#[cfg(test)]
mod tests {
    use crate::crop::cut;

    /// A region is cut from where it sits on the page, and a pin from a square around it.
    #[test]
    fn a_crop_is_the_part_of_the_page_the_region_covers() {
        let path = std::env::temp_dir().join("qrate-crop.png");
        let mut page = image::RgbaImage::new(200, 100);
        for (x, _, px) in page.enumerate_pixels_mut() {
            *px = image::Rgba([if x < 100 { 255 } else { 0 }, 0, 0, 255]);
        }
        page.save(&path).unwrap();

        let right_half = cut(&path, 0, [5000, 0, 5000, 10_000]).unwrap();
        let size = right_half.size(0);
        assert_eq!((size.width.0, size.height.0), (100, 100));
        // BGRA: red sits in the third byte, and the right half has none.
        assert_eq!(right_half.as_bytes(0).unwrap()[2], 0);

        let pin = cut(&path, 0, [2500, 5000, 0, 0]).unwrap();
        let size = pin.size(0);
        assert_eq!((size.width.0, size.height.0), (15, 15));
        assert!(
            pin.as_bytes(0).unwrap()[2] > 250,
            "the pin sits in the red half"
        );
    }
}
