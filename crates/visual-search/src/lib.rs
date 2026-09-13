//! Searching a collection by what its images show.
//!
//! CLIP turns an image and a sentence into vectors in the same space, so "a sawmill by the water"
//! lands near photographs of one. Every linked file's thumbnail is embedded once into an [`Index`]
//! on disk; a query is embedded as text, or taken from another image for "find similar", and rows
//! are ranked by how close their vectors are.
//!
//! The model runs on the CPU through candle, which is pure Rust. Its weights are not part of the
//! install: [`download`] fetches a pinned revision and checks it against the published checksum.

mod index;
mod tokenizer;

use std::fs;
use std::io::{Read as _, Write as _};
use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::clip::{ClipConfig, ClipModel, div_l2_norm};
use image::{DynamicImage, RgbaImage, imageops::FilterType};
use sha2::{Digest as _, Sha256};

pub use index::{Index, Stamp};

/// Which weights an index was built with. Vectors from different models are not comparable, so an
/// index recorded under another name is discarded rather than searched.
pub const MODEL: &str = "openai/clip-vit-base-patch32@b33cedf";

/// Folder name for the weights under qrate's models directory.
pub const MODEL_DIR: &str = "clip-vit-base-patch32";

const REVISION: &str = "b33cedfd0df4e43b8238760678fcc89e1a0d38b3";

struct File {
    name: &'static str,
    size: u64,
    sha256: &'static str,
}

const FILES: [File; 2] = [
    File {
        name: "tokenizer.json",
        size: 2_224_041,
        sha256: "b556ac8c99757ffb677208af34bc8c6721572114111a6e0aaf5fa69ff0b8d842",
    },
    File {
        name: "model.safetensors",
        size: 605_157_884,
        sha256: "99d28a652e6ec46629ab7047a0ac82c69b1fe11e0ce672c43af65d3a9a3fc05d",
    },
];

/// Bytes [`download`] fetches in total, for the install prompt.
pub const DOWNLOAD_SIZE: u64 = FILES[0].size + FILES[1].size;

/// Whether the weights are in `dir`. Checks sizes, not checksums: [`download`] only renames a file
/// into place after its checksum matched.
pub fn installed(dir: &Path) -> bool {
    FILES
        .iter()
        .all(|file| fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.size))
}

/// Fetch the pinned weights into `dir`, calling `progress` with bytes done so far. A file is written
/// beside its final name and moved into place only once its SHA-256 matches, so an interrupted or
/// tampered download never looks installed.
pub fn download(dir: &Path, progress: &dyn Fn(u64)) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    // No overall timeout: the weights are 600 MB and a slow connection is not a failure.
    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let mut done = 0;
    for file in &FILES {
        let target = dir.join(file.name);
        if fs::metadata(&target).is_ok_and(|meta| meta.len() == file.size) {
            done += file.size;
            progress(done);
            continue;
        }
        let url = format!(
            "https://huggingface.co/openai/clip-vit-base-patch32/resolve/{REVISION}/{}",
            file.name
        );
        let mut response = client.get(&url).send()?.error_for_status()?;
        let partial = target.with_extension("part");
        let mut out = fs::File::create(&partial)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 1 << 16];
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            done += read as u64;
            progress(done);
        }
        drop(out);
        let digest = format!("{:x}", hasher.finalize());
        if digest != file.sha256 {
            let _ = fs::remove_file(&partial);
        }
        ensure!(
            digest == file.sha256,
            "the downloaded {} did not match its published checksum",
            file.name
        );
        fs::rename(&partial, &target)?;
    }
    Ok(())
}

/// The loaded model. Holds about 600 MB, memory-mapped from the weights file.
pub struct Clip {
    model: ClipModel,
    tokenizer: tokenizer::Tokenizer,
}

/// The normalisation CLIP was trained with, per RGB channel.
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

impl Clip {
    pub fn load(dir: &Path) -> Result<Self> {
        let tokenizer = tokenizer::Tokenizer::load(&dir.join("tokenizer.json"))?;
        // SAFETY: the weights file is only ever replaced by rename, never modified in place, so the
        // mapping cannot change underneath the model.
        let weights = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(
                &[dir.join("model.safetensors")],
                DType::F32,
                &Device::Cpu,
            )?
        };
        let model = ClipModel::new(weights, &ClipConfig::vit_base_patch32())?;
        Ok(Self { model, tokenizer })
    }

    /// One unit-length vector per image, in order.
    pub fn embed_images(&self, images: &[RgbaImage]) -> Result<Vec<Vec<f32>>> {
        let size = ClipConfig::vit_base_patch32().image_size;
        let mut pixels = Vec::with_capacity(images.len() * 3 * size * size);
        for image in images {
            // Fill then centre-crop, as CLIP's own preprocessing does, so nothing is squashed.
            let rgb = DynamicImage::ImageRgba8(image.clone())
                .resize_to_fill(size as u32, size as u32, FilterType::CatmullRom)
                .to_rgb8();
            for channel in 0..3 {
                pixels.extend(
                    rgb.pixels().map(|px| {
                        (f32::from(px.0[channel]) / 255.0 - MEAN[channel]) / STD[channel]
                    }),
                );
            }
        }
        let batch = Tensor::from_vec(pixels, (images.len(), 3, size, size), &Device::Cpu)?;
        Ok(div_l2_norm(&self.model.get_image_features(&batch)?)?.to_vec2()?)
    }

    /// A unit-length vector for a description of what an image shows.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let ids = Tensor::new(self.tokenizer.encode(text), &Device::Cpu)?.unsqueeze(0)?;
        let features = div_l2_norm(&self.model.get_text_features(&ids)?)?;
        Ok(features.squeeze(0)?.to_vec1()?)
    }
}

/// Cosine similarity of two unit-length vectors.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs against the real weights when they are installed. Without them this is the skip path,
    /// not a pass — `download` into the models folder first.
    #[test]
    fn a_description_finds_the_image_it_describes() {
        let Some(dir) = dirs::data_local_dir().map(|d| d.join("qrate/models").join(MODEL_DIR))
        else {
            return;
        };
        if !installed(&dir) {
            eprintln!("skipping the model check: CLIP weights are not installed");
            return;
        }
        let clip = Clip::load(&dir).unwrap();
        let red = RgbaImage::from_pixel(64, 64, image::Rgba([220, 20, 20, 255]));
        let blue = RgbaImage::from_pixel(64, 64, image::Rgba([20, 40, 220, 255]));
        let images = clip.embed_images(&[red, blue]).unwrap();
        assert_eq!(images[0].len(), 512);

        let query = clip.embed_text("a plain red square").unwrap();
        assert!(
            (similarity(&query, &query) - 1.0).abs() < 1e-3,
            "unit length"
        );
        assert!(
            similarity(&query, &images[0]) > similarity(&query, &images[1]),
            "red ranks above blue"
        );
    }
}
