pub mod cache;
pub mod commands;
pub mod pipeline;
pub mod processor;

pub const DEFAULT_THUMBNAIL_HEIGHT: u32 = 600;
pub const WEBP_QUALITY: f32 = 90.0;
pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif"];

pub fn is_supported_extension(ext: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}
