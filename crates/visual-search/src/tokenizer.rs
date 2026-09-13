//! CLIP's byte-level BPE, read from the model's own `tokenizer.json`.
//!
//! ponytail: skips the NFC normalisation step, which only changes text with decomposed accents.
//! Add `unicode-normalization` if queries in such text rank oddly.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use regex::Regex;

const START: u32 = 49406;
const END: u32 = 49407;
/// The text encoder's position embeddings stop here, start and end tokens included.
const CONTEXT: usize = 77;

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    ranks: HashMap<(String, String), usize>,
    split: Regex,
    bytes: [char; 256],
}

impl Tokenizer {
    pub fn load(file: &Path) -> Result<Self> {
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)?;
        let model = &json["model"];
        let vocab = serde_json::from_value(model["vocab"].clone()).context("tokenizer vocab")?;
        let ranks = model["merges"]
            .as_array()
            .context("tokenizer merges")?
            .iter()
            .filter_map(|merge| merge.as_str()?.split_once(' '))
            .enumerate()
            .map(|(rank, (a, b))| ((a.to_owned(), b.to_owned()), rank))
            .collect();
        Ok(Self {
            vocab,
            ranks,
            split: Regex::new(r"'s|'t|'re|'ve|'m|'ll|'d|\p{L}+|\p{N}|[^\s\p{L}\p{N}]+")?,
            bytes: byte_chars(),
        })
    }

    pub fn encode(&self, text: &str) -> Vec<u32> {
        let text = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let mut ids = vec![START];
        for word in self.split.find_iter(&text) {
            let symbols = word
                .as_str()
                .bytes()
                .map(|b| self.bytes[b as usize].to_string());
            ids.extend(
                self.merge(symbols.collect())
                    .iter()
                    .map(|symbol| self.vocab.get(symbol).copied().unwrap_or(END)),
            );
        }
        ids.truncate(CONTEXT - 1);
        ids.push(END);
        ids
    }

    /// Apply merges lowest rank first until none apply, marking the word's end the way the
    /// vocabulary does.
    fn merge(&self, mut symbols: Vec<String>) -> Vec<String> {
        if let Some(last) = symbols.last_mut() {
            last.push_str("</w>");
        }
        loop {
            let best = symbols
                .windows(2)
                .enumerate()
                .filter_map(|(at, pair)| {
                    let rank = self.ranks.get(&(pair[0].clone(), pair[1].clone()))?;
                    Some((*rank, at))
                })
                .min();
            let Some((_, at)) = best else {
                return symbols;
            };
            let right = symbols.remove(at + 1);
            symbols[at].push_str(&right);
        }
    }
}

/// GPT-2's reversible byte-to-character table: printable bytes stand for themselves, the rest are
/// shifted past 255 so every byte has a visible, vocabulary-safe character.
fn byte_chars() -> [char; 256] {
    let mut table = ['\0'; 256];
    let mut shifted = 0;
    for byte in 0..=255u8 {
        table[byte as usize] = match byte {
            b'!'..=b'~' | 0xA1..=0xAC | 0xAE..=0xFF => char::from(byte),
            _ => {
                shifted += 1;
                char::from_u32(255 + shifted).unwrap_or('\0')
            }
        };
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unprintable_bytes_move_past_latin_one() {
        let table = byte_chars();
        assert_eq!(table[b'a' as usize], 'a');
        assert_eq!(table[b' ' as usize], '\u{120}', "GPT-2 writes a space as Ġ");
    }

    #[test]
    fn encodes_like_the_reference_tokenizer() {
        let Some(file) = dirs::data_local_dir()
            .map(|d| d.join("qrate/models/clip-vit-base-patch32/tokenizer.json"))
            .filter(|file| file.is_file())
        else {
            eprintln!("skipping the tokenizer check: CLIP weights are not installed");
            return;
        };
        let tokenizer = Tokenizer::load(&file).unwrap();
        assert_eq!(
            tokenizer.encode("A  photo of a CAT"),
            vec![START, 320, 1125, 539, 320, 2368, END]
        );
    }
}
