//! Every embedded file's vector, kept on disk between launches.
//!
//! Keyed by path and stamped with the file's size and modification time, so an edited or replaced
//! file is embedded again and an untouched one never is.
//!
//! ponytail: entries for deleted files are never dropped, at 2 KB each. Prune on save if a
//! long-lived index grows noticeably.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"qrvidx01";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    len: u64,
    modified: Duration,
}

impl Stamp {
    pub fn of(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        Some(Self {
            len: meta.len(),
            modified: meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?,
        })
    }
}

#[derive(Default)]
pub struct Index {
    model: String,
    entries: HashMap<PathBuf, (Stamp, Vec<f32>)>,
}

impl Index {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_owned(),
            entries: HashMap::new(),
        }
    }

    /// The index saved at `file`, or an empty one when there is none, it is unreadable, or it was
    /// built by a different model.
    pub fn load(file: &Path, model: &str) -> Self {
        match read(file) {
            Ok(index) if index.model == model => index,
            Ok(_) => {
                log::info!("the visual search index was built by another model, rebuilding it");
                Self::new(model)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::new(model),
            Err(err) => {
                log::warn!("could not read the visual search index, rebuilding it: {err}");
                Self::new(model)
            }
        }
    }

    /// Written beside `file` and renamed over it, so a crash mid-write leaves the previous index.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        let partial = file.with_extension("part");
        let mut out = BufWriter::new(fs::File::create(&partial)?);
        out.write_all(MAGIC)?;
        write_bytes(&mut out, self.model.as_bytes())?;
        for (path, (stamp, vector)) in &self.entries {
            let Some(path) = path.to_str() else {
                continue;
            };
            write_bytes(&mut out, path.as_bytes())?;
            out.write_all(&stamp.len.to_le_bytes())?;
            out.write_all(&stamp.modified.as_secs().to_le_bytes())?;
            out.write_all(&stamp.modified.subsec_nanos().to_le_bytes())?;
            out.write_all(&(vector.len() as u32).to_le_bytes())?;
            for value in vector {
                out.write_all(&value.to_le_bytes())?;
            }
        }
        out.into_inner()
            .map_err(|err| err.into_error())?
            .sync_all()?;
        fs::rename(partial, file)
    }

    pub fn is_current(&self, path: &Path, stamp: Stamp) -> bool {
        self.entries
            .get(path)
            .is_some_and(|(kept, _)| *kept == stamp)
    }

    pub fn insert(&mut self, path: PathBuf, stamp: Stamp, vector: Vec<f32>) {
        self.entries.insert(path, (stamp, vector));
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn vector(&self, path: &Path) -> Option<&[f32]> {
        self.entries.get(path).map(|(_, vector)| vector.as_slice())
    }
}

fn read(file: &Path) -> io::Result<Index> {
    let mut input = BufReader::new(fs::File::open(file)?);
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an index"));
    }
    let model = String::from_utf8(read_bytes(&mut input)?)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut index = Index::new(&model);
    loop {
        let path = match read_bytes(&mut input) {
            Ok(path) => path,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(index),
            Err(err) => return Err(err),
        };
        let path = String::from_utf8(path)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let len = u64::from_le_bytes(read_array(&mut input)?);
        let secs = u64::from_le_bytes(read_array(&mut input)?);
        let nanos = u32::from_le_bytes(read_array(&mut input)?);
        let dim = u32::from_le_bytes(read_array(&mut input)?) as usize;
        let vector = (0..dim)
            .map(|_| read_array(&mut input).map(f32::from_le_bytes))
            .collect::<io::Result<_>>()?;
        let stamp = Stamp {
            len,
            modified: Duration::new(secs, nanos),
        };
        index.insert(path.into(), stamp, vector);
    }
}

fn write_bytes(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    out.write_all(&(bytes.len() as u32).to_le_bytes())?;
    out.write_all(bytes)
}

fn read_bytes(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let len = u32::from_le_bytes(read_array(input)?) as usize;
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_array<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_forgets_another_models_vectors() {
        let dir = std::env::temp_dir().join("qrate-visual-index-test");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("index.bin");
        let photo = dir.join("photo.jpg");
        fs::write(&photo, b"pixels").unwrap();
        let stamp = Stamp::of(&photo).unwrap();

        let mut index = Index::new("clip-a");
        index.insert(photo.clone(), stamp, vec![0.6, 0.8]);
        index.save(&file).unwrap();

        let loaded = Index::load(&file, "clip-a");
        assert!(loaded.is_current(&photo, stamp));
        assert_eq!(loaded.vector(&photo), Some(&[0.6, 0.8][..]));

        fs::write(&photo, b"a different, longer set of pixels").unwrap();
        assert!(
            !loaded.is_current(&photo, Stamp::of(&photo).unwrap()),
            "an edited file is stale"
        );
        assert!(Index::load(&file, "clip-b").vector(&photo).is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
