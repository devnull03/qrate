//! Compression module for high-performance thumbnail generation
//!
//! This module handles:
//! - Background thumbnail generation using rayon for parallelization
//! - SIMD-accelerated image decoding (zune-jpeg) and resizing (fast_image_resize)
//! - WebP encoding for optimized thumbnails
//! - SQLite-based thumbnail cache stored in .project.qrate folder
//! - Progress tracking for status bar integration

pub mod cache;
pub mod commands;
pub mod pipeline;
pub mod processor;

/// Default thumbnail height in pixels (maintains aspect ratio)
pub const DEFAULT_THUMBNAIL_HEIGHT: u32 = 300;

/// WebP quality for lossy encoding (0-100)
pub const WEBP_QUALITY: f32 = 80.0;

/// Supported image extensions for thumbnail generation
pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif"];

/// Check if a file extension is supported for thumbnail generation
pub fn is_supported_extension(ext: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}
