//! High-performance image processing pipeline using SIMD-accelerated libraries.

use std::path::Path;

use fast_image_resize::{
    images::Image as FirImage, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
};
use image::{GenericImageView, ImageReader};
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use super::{DEFAULT_THUMBNAIL_HEIGHT, WEBP_QUALITY};

#[derive(Debug)]
pub struct PipelineResult {
    pub webp_data: Vec<u8>,
    pub original_width: u32,
    pub original_height: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
}

#[derive(Debug)]
pub enum PipelineError {
    Io(std::io::Error),
    Decode(String),
    Resize(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::Io(e) => write!(f, "IO error: {}", e),
            PipelineError::Decode(e) => write!(f, "Decode error: {}", e),
            PipelineError::Resize(e) => write!(f, "Resize error: {}", e),
        }
    }
}

impl std::error::Error for PipelineError {}

impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self {
        PipelineError::Io(e)
    }
}

pub struct ThumbnailPipeline {
    target_height: u32,
    webp_quality: f32,
}

impl Default for ThumbnailPipeline {
    fn default() -> Self {
        Self::new(DEFAULT_THUMBNAIL_HEIGHT, WEBP_QUALITY)
    }
}

impl ThumbnailPipeline {
    pub fn new(target_height: u32, webp_quality: f32) -> Self {
        Self {
            target_height,
            webp_quality,
        }
    }

    pub fn process(&self, path: &Path) -> Result<PipelineResult, PipelineError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let (pixels, width, height, has_alpha) = self.decode(path, &ext)?;
        let (target_width, target_height) = self.calculate_dimensions(width, height);

        let (final_width, final_height) = if height <= self.target_height {
            (width, height)
        } else {
            (target_width, target_height)
        };

        let resized = if final_width == width && final_height == height {
            pixels
        } else {
            self.resize(&pixels, width, height, final_width, final_height, has_alpha)?
        };

        let webp_data = self.encode_webp(&resized, final_width, final_height, has_alpha)?;

        Ok(PipelineResult {
            webp_data,
            original_width: width,
            original_height: height,
            thumb_width: final_width,
            thumb_height: final_height,
        })
    }

    fn decode(&self, path: &Path, ext: &str) -> Result<(Vec<u8>, u32, u32, bool), PipelineError> {
        match ext {
            "jpg" | "jpeg" => self.decode_jpeg(path),
            "png" => self.decode_png(path),
            _ => self.decode_generic(path),
        }
    }

    fn decode_jpeg(&self, path: &Path) -> Result<(Vec<u8>, u32, u32, bool), PipelineError> {
        let data = std::fs::read(path)?;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut decoder = JpegDecoder::new_with_options(&data, options);

        let pixels = decoder
            .decode()
            .map_err(|e| PipelineError::Decode(format!("JPEG decode failed: {:?}", e)))?;

        let info = decoder
            .info()
            .ok_or_else(|| PipelineError::Decode("Failed to get JPEG info".to_string()))?;

        Ok((pixels, info.width as u32, info.height as u32, false))
    }

    fn decode_png(&self, path: &Path) -> Result<(Vec<u8>, u32, u32, bool), PipelineError> {
        let file = std::fs::File::open(path)?;
        let decoder = png::Decoder::new(file);
        let mut reader = decoder
            .read_info()
            .map_err(|e| PipelineError::Decode(format!("PNG decode failed: {}", e)))?;

        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| PipelineError::Decode(format!("PNG frame read failed: {}", e)))?;

        let has_alpha = matches!(
            info.color_type,
            png::ColorType::Rgba | png::ColorType::GrayscaleAlpha
        );

        let pixels = match info.color_type {
            png::ColorType::Rgb | png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::Grayscale => buf[..info.buffer_size()]
                .iter()
                .flat_map(|&g| [g, g, g])
                .collect(),
            png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()]
                .chunks(2)
                .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
                .collect(),
            png::ColorType::Indexed => return self.decode_generic(path),
        };

        Ok((pixels, info.width, info.height, has_alpha))
    }

    fn decode_generic(&self, path: &Path) -> Result<(Vec<u8>, u32, u32, bool), PipelineError> {
        let img = ImageReader::open(path)
            .map_err(PipelineError::Io)?
            .decode()
            .map_err(|e| PipelineError::Decode(format!("Image decode failed: {}", e)))?;

        let (width, height) = img.dimensions();
        let has_alpha = img.color().has_alpha();

        let pixels = if has_alpha {
            img.to_rgba8().into_raw()
        } else {
            img.to_rgb8().into_raw()
        };

        Ok((pixels, width, height, has_alpha))
    }

    fn calculate_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
        if height <= self.target_height {
            return (width, height);
        }

        let ratio = self.target_height as f64 / height as f64;
        let target_width = ((width as f64 * ratio).round() as u32).max(1);

        (target_width, self.target_height.max(1))
    }

    fn resize(
        &self,
        pixels: &[u8],
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
        has_alpha: bool,
    ) -> Result<Vec<u8>, PipelineError> {
        let pixel_type = if has_alpha {
            PixelType::U8x4
        } else {
            PixelType::U8x3
        };

        let src_image = FirImage::from_vec_u8(src_width, src_height, pixels.to_vec(), pixel_type)
            .map_err(|e| {
            PipelineError::Resize(format!("Failed to create source image: {}", e))
        })?;

        let mut dst_image = FirImage::new(dst_width, dst_height, pixel_type);
        let mut resizer = Resizer::new();
        let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));

        resizer
            .resize(&src_image, &mut dst_image, Some(&options))
            .map_err(|e| PipelineError::Resize(format!("Resize failed: {}", e)))?;

        Ok(dst_image.into_vec())
    }

    fn encode_webp(
        &self,
        pixels: &[u8],
        width: u32,
        height: u32,
        has_alpha: bool,
    ) -> Result<Vec<u8>, PipelineError> {
        let encoder = if has_alpha {
            webp::Encoder::from_rgba(pixels, width, height)
        } else {
            webp::Encoder::from_rgb(pixels, width, height)
        };

        Ok(encoder.encode(self.webp_quality).to_vec())
    }
}

pub fn process_image(path: &Path) -> Result<PipelineResult, PipelineError> {
    ThumbnailPipeline::default().process(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_dimensions() {
        let pipeline = ThumbnailPipeline::new(300, 80.0);

        let (w, h) = pipeline.calculate_dimensions(1920, 1080);
        assert_eq!(h, 300);
        assert!(w < 1920);

        let (w, h) = pipeline.calculate_dimensions(200, 150);
        assert_eq!(w, 200);
        assert_eq!(h, 150);

        let (w, h) = pipeline.calculate_dimensions(1000, 1000);
        assert_eq!(h, 300);
        assert_eq!(w, 300);
    }
}
